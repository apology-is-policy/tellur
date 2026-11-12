//! Private synchronous foreign-adapter invocation and resource lifecycle protocol.
//!
//! The callback borrows an opaque context only until it returns. Spans must be
//! readable/writable for their declared extent; null is valid only for empty spans.
//! Output spans must not alias runtime storage or any readable input span.
//! Runtime values, Rust containers, allocators and unwinding never cross the ABI.
//! Foreign panics retain their fatal behavior. There is no panic-to-fault mapping.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

pub type AdapterCallback = for<'a> unsafe extern "C" fn(*mut AdapterContext<'a>, *const u64, usize);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AdapterDescriptor {
    pub version: u32,
    pub flags: u32,
    pub call: Option<AdapterCallback>,
}

/// Owned, immutable contract for one operation; no mutable global dispatch state.
/// The launch registry owns this contract; generated static libraries are separate.
pub struct AdapterBinding {
    manifest: Json,
    signature: Json,
    callback: AdapterCallback,
}

fn adapter_error(message: &str) -> String {
    format!("E_HOST: foreign adapter {message}")
}
fn data_type(ty: &str) -> bool {
    match ty {
        "Unit" | "Bool" | "U32" | "String" | "Bytes" => true,
        _ => match collection_support::Collection::parse(ty) {
            Some(collection_support::Collection::Vector(inner)) => data_type(&inner),
            Some(collection_support::Collection::Map(key, value)) => {
                matches!(key.as_str(), "Bool" | "U32" | "String") && data_type(&value)
            }
            _ => false,
        },
    }
}
fn resource_type(manifest: &Json, ty: &str) -> bool {
    manifest["rust"]["resources"][ty]
        .as_str()
        .is_some_and(|s| !s.is_empty())
        && manifest["types"].as_array().is_some_and(|types| {
            types
                .iter()
                .any(|t| t["name"] == ty && t["kind"] == "opaque" && t["linear"] == true)
        })
}
impl AdapterBinding {
    pub fn new(manifest: &Json, operation: &str, descriptor: AdapterDescriptor) -> R<Self> {
        if descriptor.version != 1 || descriptor.flags != 0 || descriptor.call.is_none() {
            return Err(adapter_error("has an incompatible callback descriptor"));
        }
        let signature = manifest["primitives"]
            .as_array()
            .and_then(|ps| ps.iter().find(|p| p["name"] == operation))
            .ok_or_else(|| adapter_error("names an unknown operation"))?;
        if !manifest["rust"].is_object()
            || !matches!(
                signature["rust"]["execution"].as_str(),
                Some("immediate" | "blocking")
            )
            || signature["async"] == true
            || signature["pure"] == true
        {
            return Err(adapter_error("requires a synchronous Rust binding"));
        }
        let parameters = signature["parameters"]
            .as_array()
            .ok_or_else(|| adapter_error("has no parameter contract"))?;
        let outcomes = signature["outcomes"]
            .as_array()
            .ok_or_else(|| adapter_error("has no outcome contract"))?;
        if outcomes.is_empty()
            || parameters.iter().any(|p| {
                let Some(ty) = p["type"].as_str() else {
                    return true;
                };
                if resource_type(manifest, ty) {
                    !matches!(p["mode"].as_str(), Some("borrow" | "consume"))
                } else {
                    !data_type(ty)
                        || !matches!(p["mode"].as_str(), Some("value" | "borrow" | "consume"))
                }
            })
            || outcomes.iter().any(|o| {
                o["name"].as_str().is_none_or(str::is_empty)
                    || o["fields"].as_array().is_none_or(|fs| {
                        fs.iter().any(|f| {
                            !f["type"]
                                .as_str()
                                .is_some_and(|t| data_type(t) || resource_type(manifest, t))
                        })
                    })
            })
        {
            return Err(adapter_error(
                "requires scalar/collection data or directly bound resources",
            ));
        }
        Ok(Self {
            manifest: manifest.clone(),
            signature: signature.clone(),
            callback: descriptor.call.unwrap(),
        })
    }

    /// Validate the complete input batch before taking it or entering foreign code.
    /// The original Host still owns dispatch, script precedence, events and faults.
    pub fn invoke(&self, arguments: &mut Vec<V>) -> R<Outcome> {
        self.invoke_with_resources(arguments, None)
    }

    fn invoke_with_resources(
        &self,
        arguments: &mut Vec<V>,
        resources: Option<(&mut Resources, &Arc<()>)>,
    ) -> R<Outcome> {
        let types = self.signature["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["type"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        validate_args(arguments, &types, &self.manifest)?;
        let parameters = self.signature["parameters"].as_array().unwrap();
        let resource_inputs = parameters
            .iter()
            .zip(arguments.iter())
            .filter_map(|(p, v)| {
                let ty = p["type"].as_str().unwrap();
                resource_type(&self.manifest, ty).then_some((v, ty, p["mode"] == "consume"))
            })
            .collect::<Vec<_>>();
        let resource_results = self.signature["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| {
                o["fields"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|f| resource_type(&self.manifest, f["type"].as_str().unwrap()))
            });
        if let Some((resources, owner)) = resources.as_ref() {
            resources.validate_call(&resource_inputs)?;
            for (value, ty, _) in &resource_inputs {
                let resource = resources.get::<ForeignResource>(value, ty)?;
                if !Arc::ptr_eq(owner, &resource.owner) {
                    return Err(adapter_error("resource belongs to another adapter owner"));
                }
            }
        } else if !resource_inputs.is_empty() || resource_results {
            return Err(adapter_error(
                "resource call requires registered launch storage",
            ));
        }
        let handles = reserve_handles(arguments.len())?;
        let owned = parameters
            .iter()
            .zip(&handles)
            .filter_map(|(p, h)| {
                (p["mode"] == "consume"
                    && resource_type(&self.manifest, p["type"].as_str().unwrap()))
                .then_some(*h)
            })
            .collect();
        let (resources, owner) = match resources {
            Some((resources, owner)) => (Some(resources), Some(Arc::clone(owner))),
            None => (None, None),
        };
        let mut context = AdapterContext {
            manifest: self.manifest.clone(),
            signature: self.signature.clone(),
            values: handles
                .iter()
                .copied()
                .zip(std::mem::take(arguments))
                .collect(),
            result: None,
            error: None,
            resources,
            owner,
            owned,
        };
        unsafe { (self.callback)(&mut context, handles.as_ptr(), handles.len()) };
        if let Some(error) = context.error.take() {
            return Err(error);
        }
        context
            .result
            .take()
            .ok_or_else(|| adapter_error("returned without an outcome"))
    }
}

/// The caller lends table/name storage only during registration. Function pointers
/// must remain callable until every invocation using this table has completed.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AdapterOperation {
    pub name: *const u8,
    pub name_len: usize,
    pub callback: AdapterDescriptor,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AdapterTable {
    pub version: u32,
    pub flags: u32,
    pub operations: *const AdapterOperation,
    pub count: usize,
}

/// Immutable launch-local bindings. No borrowed table spans survive preparation.
pub struct AdapterRegistry {
    manifest: Json,
    bindings: BTreeMap<String, AdapterBinding>,
    owner: Arc<()>,
}
impl AdapterRegistry {
    pub unsafe fn prepare(manifest: &Json, table: *const AdapterTable) -> R<Arc<Self>> {
        let table = unsafe { adapter_span(table, 1)? }[0];
        if table.version != 1 || table.flags != 0 {
            return Err(adapter_error("has an incompatible operation table"));
        }
        let operations = unsafe { adapter_span(table.operations, table.count)? };
        let mut bindings = BTreeMap::new();
        for operation in operations {
            let name = unsafe { adapter_text(operation.name, operation.name_len)? };
            if name.is_empty() || bindings.contains_key(name) {
                return Err(adapter_error("has an empty or duplicate operation name"));
            }
            let binding = AdapterBinding::new(manifest, name, operation.callback)?;
            bindings.insert(name.to_owned(), binding);
        }
        let primitives = manifest["primitives"]
            .as_array()
            .ok_or_else(|| adapter_error("has no operation contract"))?;
        let mut expected = BTreeSet::new();
        for primitive in primitives.iter().filter(|p| p["rust"].is_object()) {
            let name = primitive["name"]
                .as_str()
                .filter(|n| !n.is_empty())
                .ok_or_else(|| adapter_error("has an invalid operation name"))?;
            if !expected.insert(name) || !bindings.contains_key(name) {
                return Err(adapter_error("requires every Rust operation exactly once"));
            }
        }
        if bindings.len() != expected.len() {
            return Err(adapter_error("contains an unbound operation"));
        }
        Ok(Arc::new(Self {
            manifest: manifest.clone(),
            bindings,
            owner: Arc::new(()),
        }))
    }

    /// Publication is all-or-nothing and cannot replace another adapter owner.
    pub fn install(self: &Arc<Self>, host: &mut Host) -> R<()> {
        if host.manifest != self.manifest
            || host.resources.foreign_adapters.is_some()
            || host.rust_adapter.is_some()
            || host.rust_start.is_some()
        {
            return Err(adapter_error("cannot replace or mismatch launch bindings"));
        }
        host.resources.foreign_adapters = Some(Arc::clone(self));
        host.rust_adapter = Some(dispatch);
        Ok(())
    }

    pub fn contains(&self, operation: &str) -> bool {
        self.bindings.contains_key(operation)
    }
}
fn dispatch(resources: &mut Resources, operation: &str, mut args: Vec<V>) -> R<Outcome> {
    let registry = resources
        .foreign_adapters
        .clone()
        .ok_or_else(|| adapter_error("has no launch bindings"))?;
    registry
        .bindings
        .get(operation)
        .ok_or_else(|| adapter_error("has no binding for operation"))?
        .invoke_with_resources(&mut args, Some((resources, &registry.owner)))
}

// Identities cannot alias another call, including a concurrent or reentrant call.
// This is only an identity allocator, never a selected callback or launch registry.
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(0);
fn reserve_handles(count: usize) -> R<Vec<u64>> {
    reserve_from(&NEXT_HANDLE, count)
}
fn reserve_from(next: &AtomicU64, count: usize) -> R<Vec<u64>> {
    let count = u64::try_from(count).map_err(|_| adapter_error("handle count overflow"))?;
    let base = next
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            n.checked_add(count)
        })
        .map_err(|_| adapter_error("handle identity exhausted"))?;
    Ok((0..count).map(|i| base + i + 1).collect())
}

pub struct AdapterContext<'a> {
    manifest: Json,
    signature: Json,
    values: BTreeMap<u64, V>,
    result: Option<Outcome>,
    error: Option<String>,
    resources: Option<&'a mut Resources>,
    owner: Option<Arc<()>>,
    // Only these call-local handles own resource tokens. Shared aliases never do.
    owned: BTreeSet<u64>,
}
impl AdapterContext<'_> {
    fn get(&self, handle: u64) -> R<&V> {
        self.values
            .get(&handle)
            .ok_or_else(|| adapter_error("received an unknown or consumed handle"))
    }
    fn batch(&self, handles: &[u64]) -> R<()> {
        let mut seen = std::collections::BTreeSet::new();
        for handle in handles {
            self.get(*handle)?;
            if !seen.insert(*handle) {
                return Err(adapter_error("repeated a transferred handle"));
            }
        }
        Ok(())
    }
    fn take_many(&mut self, handles: &[u64]) -> Vec<V> {
        handles
            .iter()
            .map(|h| {
                self.owned.remove(h);
                self.values.remove(h).unwrap()
            })
            .collect()
    }
    fn put(&mut self, value: V) -> R<u64> {
        let handle = reserve_handles(1)?[0];
        self.values.insert(handle, value);
        Ok(handle)
    }
    fn perform<T: Default>(&mut self, f: impl FnOnce(&mut Self) -> R<T>) -> T {
        if self.error.is_some() {
            return T::default();
        }
        if self.result.is_some() {
            self.error = Some(adapter_error("used a completed invocation"));
            return T::default();
        }
        match f(self) {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e);
                T::default()
            }
        }
    }
    fn resource(&self, handle: u64, ty: &str) -> R<&ForeignResource> {
        if !resource_type(&self.manifest, ty) {
            return Err(adapter_error("unknown resource type"));
        }
        let resources = self
            .resources
            .as_ref()
            .ok_or_else(|| adapter_error("has no resource storage"))?;
        let resource = resources.get::<ForeignResource>(self.get(handle)?, ty)?;
        if !self
            .owner
            .as_ref()
            .is_some_and(|owner| Arc::ptr_eq(owner, &resource.owner))
        {
            return Err(adapter_error("resource belongs to another adapter owner"));
        }
        Ok(resource)
    }
    fn retire(&mut self, value: V) {
        if let V::Opaque { ty, .. } = &value
            && let Some(resources) = self.resources.as_mut()
            && let Ok(resource) = resources.get::<ForeignResource>(&value, ty)
            && self
                .owner
                .as_ref()
                .is_some_and(|owner| Arc::ptr_eq(owner, &resource.owner))
        {
            // Removing the entry drops only the runtime-local wrapper; its foreign
            // allocation is destroyed by the adapter that originally allocated it.
            drop(resources.take::<ForeignResource>(&value, ty));
        }
    }
    fn discard(&mut self, handle: u64) {
        let value = self.values.remove(&handle).unwrap();
        if self.owned.remove(&handle) {
            self.retire(value);
        }
    }
}
impl Drop for AdapterContext<'_> {
    fn drop(&mut self) {
        for handle in std::mem::take(&mut self.owned) {
            if let Some(value) = self.values.remove(&handle) {
                self.retire(value);
            }
        }
        // A late protocol error after completion must not strand selected owners.
        // Successful invocation takes this result before destroying the context.
        if let Some((_, values)) = self.result.take() {
            for value in values {
                self.retire(value);
            }
        }
    }
}
unsafe fn adapter_span<'a, T>(data: *const T, count: usize) -> R<&'a [T]> {
    if !valid_compiled_span(
        data,
        count,
        isize::MAX as usize / std::mem::size_of::<T>().max(1),
    ) {
        return Err(adapter_error("received an invalid span"));
    }
    Ok(unsafe { slice(data, count) })
}
unsafe fn adapter_text<'a>(data: *const u8, count: usize) -> R<&'a str> {
    std::str::from_utf8(unsafe { adapter_span(data, count)? })
        .map_err(|_| adapter_error("received invalid UTF-8"))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_ok(c: *mut AdapterContext) -> u32 {
    u32::from(unsafe { &*c }.error.is_none())
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_fail(
    c: *mut AdapterContext,
    data: *const u8,
    len: usize,
) {
    unsafe { &mut *c }.perform::<()>(|_| Err(adapter_error(unsafe { adapter_text(data, len)? })))
}
/// Tags: Unit0, Bool1, U322, String3, Bytes4, Vector5, Map6.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_kind(c: *mut AdapterContext, handle: u64) -> u32 {
    unsafe { &mut *c }.perform(|c| {
        Ok(match c.get(handle)? {
            V::Unit => 0,
            V::Bool(_) => 1,
            V::U32(_) => 2,
            V::String(_) => 3,
            V::Bytes(_) => 4,
            V::Construct { ty, .. } => match collection_support::Collection::parse(ty) {
                Some(collection_support::Collection::Vector(_)) => 5,
                Some(collection_support::Collection::Map(..)) => 6,
                _ => return Err(adapter_error("received unsupported data")),
            },
            _ => return Err(adapter_error("received unsupported data")),
        })
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_number(
    c: *mut AdapterContext,
    handle: u64,
    tag: u32,
) -> u64 {
    unsafe { &mut *c }.perform(|c| match (tag, c.get(handle)?) {
        (0, V::Unit) => Ok(0),
        (1, V::Bool(v)) => Ok(u64::from(*v)),
        (2, V::U32(v)) => Ok(u64::from(*v)),
        _ => Err(adapter_error("scalar type mismatch")),
    })
}
/// Null/zero requests the length. Otherwise copy all bytes into caller storage;
/// failure writes nothing. No borrowed allocator-owned pointer leaves the runtime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_bytes(
    c: *mut AdapterContext,
    handle: u64,
    tag: u32,
    output: *mut u8,
    capacity: usize,
) -> usize {
    unsafe { &mut *c }.perform(|c| {
        let bytes = match (tag, c.get(handle)?) {
            (3, V::String(s)) => s.as_bytes(),
            (4, V::Bytes(b)) => b,
            _ => return Err(adapter_error("byte type mismatch")),
        };
        if output.is_null() && capacity == 0 {
            return Ok(bytes.len());
        }
        if capacity < bytes.len() || !valid_compiled_span(output, capacity, isize::MAX as usize) {
            return Err(adapter_error("output buffer is too small or invalid"));
        }
        if !bytes.is_empty() {
            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len()) };
        }
        Ok(bytes.len())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_value(
    c: *mut AdapterContext,
    tag: u32,
    number: u64,
    data: *const u8,
    len: usize,
) -> u64 {
    unsafe { &mut *c }.perform(|c| {
        let value = match tag {
            0 if number == 0 && len == 0 => V::Unit,
            1 if number <= 1 && len == 0 => V::Bool(number != 0),
            2 if number <= u32::MAX as u64 && len == 0 => V::U32(number as u32),
            3 if number == 0 => V::String(unsafe { adapter_text(data, len)? }.into()),
            4 if number == 0 => V::Bytes(unsafe { adapter_span(data, len)? }.to_vec()),
            _ => return Err(adapter_error("invalid scalar value")),
        };
        c.put(value)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_length(c: *mut AdapterContext, handle: u64) -> usize {
    unsafe { &mut *c }.perform(|c| match c.get(handle)? {
        V::Construct { fields, .. } => Ok(fields.len()),
        _ => Err(adapter_error("expected collection data")),
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_unpack(
    c: *mut AdapterContext,
    handle: u64,
    output: *mut u64,
    count: usize,
) {
    unsafe { &mut *c }.perform(|c| {
        if !valid_compiled_span(output, count, isize::MAX as usize / 8) {
            return Err(adapter_error("invalid collection output span"));
        }
        match c.get(handle)? {
            V::Construct { fields, .. } if fields.len() == count => (),
            _ => return Err(adapter_error("collection output arity mismatch")),
        }
        let handles = reserve_handles(count)?;
        let V::Construct { fields, .. } = c.values.remove(&handle).unwrap() else {
            unreachable!()
        };
        for (index, (handle, value)) in handles.into_iter().zip(fields).enumerate() {
            c.values.insert(handle, value);
            unsafe { output.add(index).write(handle) };
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_collection(
    c: *mut AdapterContext,
    ty: *const u8,
    len: usize,
    fields: *const u64,
    count: usize,
) -> u64 {
    unsafe { &mut *c }.perform(|c| {
        let ty = unsafe { adapter_text(ty, len)? };
        if !data_type(ty) {
            return Err(adapter_error("unsupported collection type"));
        }
        let collection = collection_support::Collection::parse(ty)
            .ok_or_else(|| adapter_error("expected collection type"))?;
        let handles = unsafe { adapter_span(fields, count)? };
        c.batch(handles)?;
        let value = V::Construct {
            ty: ty.into(),
            name: collection.name().into(),
            fields: handles.iter().map(|h| c.get(*h).unwrap().clone()).collect(),
        };
        validate(&value, ty, &c.manifest)?;
        let handle = reserve_handles(1)?[0];
        let value = V::Construct {
            ty: ty.into(),
            name: collection.name().into(),
            fields: c.take_many(handles),
        };
        c.values.insert(handle, value);
        Ok(handle)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_drop(c: *mut AdapterContext, handle: u64) {
    unsafe { &mut *c }.perform(|c| {
        c.get(handle)?;
        c.discard(handle);
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_complete(
    c: *mut AdapterContext,
    name: *const u8,
    len: usize,
    fields: *const u64,
    count: usize,
) {
    unsafe { &mut *c }.perform(|c| {
        let name = unsafe { adapter_text(name, len)? };
        let handles = unsafe { adapter_span(fields, count)? };
        c.batch(handles)?;
        let fields = c.signature["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == name)
            .and_then(|o| o["fields"].as_array())
            .ok_or_else(|| adapter_error("selected an undeclared outcome"))?;
        if fields.len() != count {
            return Err(adapter_error("outcome arity mismatch"));
        }
        for (field, handle) in fields.iter().zip(handles) {
            let ty = field["type"].as_str().unwrap();
            if resource_type(&c.manifest, ty) {
                c.resource(*handle, ty)?;
                if !c.owned.contains(handle) {
                    return Err(adapter_error("cannot return a shared resource as an owner"));
                }
            }
            validate(c.get(*handle)?, ty, &c.manifest)?;
        }
        c.result = Some((name.into(), c.take_many(handles)));
        Ok(())
    })
}

pub type ResourceDrop = unsafe extern "C" fn(*mut std::ffi::c_void);

/// Allocation and destruction belong to the adapter library. Publication borrows
/// this descriptor mutably and clears `value` exactly when ownership transfers.
/// A non-null value on return remains the caller's responsibility, even on error.
#[repr(C)]
pub struct ResourceDescriptor {
    pub version: u32,
    pub flags: u32,
    pub value: *mut std::ffi::c_void,
    pub drop: Option<ResourceDrop>,
}

// Only this runtime-local wrapper is boxed in Resources. No foreign Rust layout
// is interpreted here, and no Send/Sync requirement is imposed on the allocation.
struct ForeignResource {
    value: *mut std::ffi::c_void,
    drop: ResourceDrop,
    owner: Arc<()>,
}
impl Drop for ForeignResource {
    fn drop(&mut self) {
        if !self.value.is_null() {
            unsafe { (self.drop)(self.value) };
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_resource_publish(
    c: *mut AdapterContext,
    ty: *const u8,
    len: usize,
    descriptor: *mut ResourceDescriptor,
) -> u64 {
    let c = unsafe { &mut *c };
    // Adopt before perform: an already-failed/completed context must also release
    // a valid incoming allocation. The cleared pointer makes this unambiguous.
    let resource = (|| {
        unsafe { adapter_span(descriptor, 1)? };
        let descriptor = unsafe { &mut *descriptor };
        if descriptor.version != 1
            || descriptor.flags != 0
            || descriptor.value.is_null()
            || descriptor.drop.is_none()
        {
            return Err(adapter_error("invalid resource descriptor"));
        }
        let owner = c
            .owner
            .clone()
            .ok_or_else(|| adapter_error("has no resource owner"))?;
        Ok(ForeignResource {
            value: std::mem::replace(&mut descriptor.value, std::ptr::null_mut()),
            drop: descriptor.drop.unwrap(),
            owner,
        })
    })();
    c.perform(move |c| {
        let resource = resource?;
        let ty = unsafe { adapter_text(ty, len)? };
        if !resource_type(&c.manifest, ty) {
            return Err(adapter_error("unknown resource type"));
        }
        let handle = reserve_handles(1)?[0];
        let resources = c
            .resources
            .as_mut()
            .ok_or_else(|| adapter_error("has no resource storage"))?;
        let value = resources.insert(ty.to_owned(), resource)?;
        c.values.insert(handle, value);
        c.owned.insert(handle);
        Ok(handle)
    })
}

/// The returned pointer is a shared view valid only for this callback. Taking
/// owners must precede forming Rust references in a generated adapter callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_resource_borrow(
    c: *mut AdapterContext,
    handle: u64,
    ty: *const u8,
    len: usize,
) -> *const std::ffi::c_void {
    unsafe { &mut *c }.perform(|c| {
        let ty = unsafe { adapter_text(ty, len)? };
        Ok(c.resource(handle, ty)?.value.cast_const())
    })
}

/// On success writes one owned descriptor into caller output storage. On error
/// writes nothing. The adapter must consume or destroy that descriptor exactly
/// once; no reference into the runtime remains after this transfer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_adapter_v1_resource_take(
    c: *mut AdapterContext,
    handle: u64,
    ty: *const u8,
    len: usize,
    output: *mut ResourceDescriptor,
) {
    unsafe { &mut *c }.perform(|c| {
        if !valid_compiled_span(output, 1, 1) {
            return Err(adapter_error("invalid resource output span"));
        }
        let ty = unsafe { adapter_text(ty, len)? };
        c.resource(handle, ty)?;
        if !c.owned.contains(&handle) {
            return Err(adapter_error("cannot consume a shared resource"));
        }
        let value = c.get(handle)?.clone();
        let mut resource = c
            .resources
            .as_mut()
            .unwrap()
            .take::<ForeignResource>(&value, ty)?;
        let descriptor = ResourceDescriptor {
            version: 1,
            flags: 0,
            value: std::mem::replace(&mut resource.value, std::ptr::null_mut()),
            drop: Some(resource.drop),
        };
        c.values.remove(&handle);
        c.owned.remove(&handle);
        unsafe { output.write(descriptor) };
        Ok(())
    })
}

#[cfg(test)]
#[path = "abi_adapter_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "abi_adapter_registry_tests.rs"]
mod registry_tests;

#[cfg(test)]
#[path = "abi_adapter_resource_tests.rs"]
mod resource_tests;
