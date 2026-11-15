//! Private owned operation bridge. Version 2 adapter tables can register starts.
//! LLVM source admission follows typed code generation and readiness integration.
use super::*;
use std::{ffi::c_void, time::Duration};

pub type OperationPoll =
    for<'a> unsafe extern "C" fn(*mut c_void, *mut AdapterContext<'a>, *const u64, usize);

/// The adapter lends a writable descriptor; a cleared state pointer means the
/// runtime adopted it. The adapter allocates and destroys the pointed-to state.
/// Its poll and drop callbacks must remain callable throughout that lifetime.
#[repr(C)]
pub struct OperationDescriptor {
    pub version: u32,
    pub flags: u32,
    pub state: *mut c_void,
    pub poll: Option<OperationPoll>,
    pub drop: Option<ResourceDrop>,
}

/// Flags: 1 = delay, 2 = readable, 4 = writable. At least one is required; the
/// descriptor interests are exclusive. Unused fields must be zero. No Duration,
/// Rust enum, trait object or borrowed runtime storage crosses this protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WaitDescriptor {
    pub version: u32,
    pub flags: u32,
    pub seconds: u64,
    pub nanoseconds: u32,
    pub descriptor: i32,
}

pub(super) struct ForeignState {
    state: *mut c_void,
    poll: OperationPoll,
    drop: ResourceDrop,
}
impl Drop for ForeignState {
    fn drop(&mut self) {
        unsafe { (self.drop)(self.state) };
    }
}

#[derive(Default)]
pub(super) enum Progress {
    #[default]
    Synchronous,
    Starting(Option<ForeignState>),
    Polling(Option<WaitDescriptor>),
}
impl Progress {
    pub(super) fn finished(&self) -> bool {
        matches!(self, Self::Starting(Some(_)) | Self::Polling(Some(_)))
    }
}

pub struct PolledBinding {
    binding: AdapterBinding,
}
impl PolledBinding {
    pub fn new(manifest: &Json, operation: &str, start: AdapterDescriptor) -> R<Self> {
        let binding = AdapterBinding::prepare(manifest, operation, start, true)?;
        if binding.signature["contingent"] == true || binding.signature["returns"] == false {
            return Err(adapter_error(
                "polled binding requires returning ordinary execution",
            ));
        }
        let releases = |ty: &str| {
            manifest["types"].as_array().is_some_and(|types| {
                types
                    .iter()
                    .any(|t| t["name"] == ty && t["fault"] == "release")
            })
        };
        for parameter in binding.signature["parameters"].as_array().unwrap() {
            let ty = parameter["type"].as_str().unwrap();
            if parameter["mode"] == "borrow" && !resource_type(manifest, ty) {
                return Err(adapter_error("polled borrows require bound resources"));
            }
            if parameter["mode"] == "consume" && resource_type(manifest, ty) && !releases(ty) {
                return Err(adapter_error(
                    "polled resource consumption requires fault release",
                ));
            }
            if parameter["mode"] == "value" && !matches!(ty, "Unit" | "Bool" | "U32") {
                return Err(adapter_error("polled owned data requires consume mode"));
            }
        }
        for outcome in binding.signature["outcomes"].as_array().unwrap() {
            if outcome["name"] == "cancelled" {
                return Err(adapter_error(
                    "cancelled is reserved for runtime cancellation",
                ));
            }
            for field in outcome["fields"].as_array().unwrap() {
                let ty = field["type"].as_str().unwrap();
                if resource_type(manifest, ty) && !releases(ty) {
                    return Err(adapter_error(
                        "polled resource results require fault release",
                    ));
                }
            }
        }
        Ok(Self { binding })
    }

    /// Preflight the whole batch before transfer. The resulting wrapper retains
    /// only owned foreign state and original shared supplier tokens, never a Rust
    /// reference or callback context. Every poll creates fresh checked handles.
    pub fn start(
        &self,
        arguments: &mut Vec<V>,
        resources: &mut Resources,
        owner: &Arc<()>,
    ) -> R<Box<dyn HostPoll>> {
        let identity = Arc::clone(&resources.identity);
        let parameters = self.binding.signature["parameters"].as_array().unwrap();
        let shared = parameters
            .iter()
            .zip(arguments.iter())
            .filter(|(p, _)| p["mode"] == "borrow")
            .map(|(_, v)| v.clone())
            .collect();
        let mut poll_binding = self.binding.clone();
        poll_binding.signature["parameters"] = Json::Array(
            parameters
                .iter()
                .filter(|p| p["mode"] == "borrow")
                .cloned()
                .collect(),
        );
        let (mut context, handles) = self.binding.context(arguments, Some((resources, owner)))?;
        context.progress = Progress::Starting(None);
        unsafe { (self.binding.callback)(&mut context, handles.as_ptr(), handles.len()) };
        if let Some(error) = context.error.take() {
            return Err(error);
        }
        let Progress::Starting(Some(state)) = std::mem::take(&mut context.progress) else {
            return Err(adapter_error("start returned without an operation"));
        };
        Ok(Box::new(ForeignOperation {
            state,
            binding: poll_binding,
            shared,
            identity,
            owner: Arc::clone(owner),
            terminal: false,
        }))
    }
}

struct ForeignOperation {
    state: ForeignState,
    binding: AdapterBinding,
    shared: Vec<V>,
    identity: Arc<()>,
    owner: Arc<()>,
    terminal: bool,
}
impl HostPoll for ForeignOperation {
    fn poll(&mut self, resources: &mut Resources) -> R<HostProgress> {
        if !Arc::ptr_eq(&self.identity, &resources.identity) {
            return Err(adapter_error("operation belongs to another host"));
        }
        if self.terminal {
            return Err(adapter_error("polled a finished operation"));
        }
        let result = (|| {
            let mut arguments = self.shared.clone();
            let (mut context, handles) = self
                .binding
                .context(&mut arguments, Some((resources, &self.owner)))?;
            context.progress = Progress::Polling(None);
            unsafe {
                (self.state.poll)(
                    self.state.state,
                    &mut context,
                    handles.as_ptr(),
                    handles.len(),
                )
            };
            if let Some(error) = context.error.take() {
                return Err(error);
            }
            if let Some(outcome) = context.result.take() {
                return Ok(HostProgress::Ready(outcome));
            }
            let Progress::Polling(Some(wait)) = std::mem::take(&mut context.progress) else {
                return Err(adapter_error("poll returned without progress"));
            };
            Ok(HostProgress::Pending {
                delay: (wait.flags & 1 != 0).then(|| Duration::new(wait.seconds, wait.nanoseconds)),
                descriptor: (wait.flags & 6 != 0).then_some(Descriptor {
                    fd: wait.descriptor,
                    writable: wait.flags & 4 != 0,
                }),
            })
        })();
        if !matches!(result, Ok(HostProgress::Pending { .. })) {
            self.terminal = true;
        }
        // Original HostSource first deregisters readiness, then drops this state,
        // then delivers the outcome or retires suppliers. Never destroy it here.
        result
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_operation_publish(
    c: *mut AdapterContext,
    descriptor: *mut OperationDescriptor,
) {
    let c = unsafe { &mut *c };
    // Adopt valid state even after an earlier callback error. The caller observes
    // the cleared pointer; the local guard destroys any rejected publication.
    let state = (|| {
        unsafe { adapter_span(descriptor, 1)? };
        let descriptor = unsafe { &mut *descriptor };
        if descriptor.version != 1
            || descriptor.flags != 0
            || descriptor.state.is_null()
            || descriptor.poll.is_none()
            || descriptor.drop.is_none()
        {
            return Err(adapter_error("invalid operation descriptor"));
        }
        Ok(ForeignState {
            state: std::mem::replace(&mut descriptor.state, std::ptr::null_mut()),
            poll: descriptor.poll.unwrap(),
            drop: descriptor.drop.unwrap(),
        })
    })();
    c.perform(move |c| {
        let state = state?;
        let Progress::Starting(slot) = &mut c.progress else {
            return Err(adapter_error(
                "operation publication requires a start callback",
            ));
        };
        *slot = Some(state);
        Ok(())
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_pending(
    c: *mut AdapterContext,
    descriptor: *const WaitDescriptor,
) {
    unsafe { &mut *c }.perform(|c| {
        let wait = unsafe { adapter_span(descriptor, 1)? }[0];
        if wait.version != 1
            || wait.flags == 0
            || wait.flags & !7 != 0
            || wait.flags & 6 == 6
            || wait.nanoseconds >= 1_000_000_000
            || (wait.flags & 1 == 0 && (wait.seconds != 0 || wait.nanoseconds != 0))
            || (wait.flags & 6 == 0 && wait.descriptor != 0)
            || (wait.flags & 6 != 0 && wait.descriptor < 0)
        {
            return Err(adapter_error("invalid pending wait descriptor"));
        }
        let Progress::Polling(slot) = &mut c.progress else {
            return Err(adapter_error("pending requires a poll callback"));
        };
        *slot = Some(wait);
        Ok(())
    });
}

#[cfg(test)]
#[path = "abi_adapter_polled_tests.rs"]
mod tests;
