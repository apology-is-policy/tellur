//! Private versioned C ABI for checked, synchronous LLVM programs.
//!
//! Only integers, byte spans, opaque context pointers and function pointers cross
//! this boundary. Rust values never do. Handles belong to one invocation, are
//! consumed once, and are never reused. Zero is invalid. The first failure is
//! sticky: later calls cannot perform effects or replace its diagnostic.
//!
//! Safety: callers are compiler-generated code. Context pointers must refer to
//! the active invocation; spans/arrays must be readable for their stated length
//! and may be null only when empty. Output payload spans must be writable for their
//! stated length. Callbacks must not retain these pointers.
use super::*;
#[cfg(test)]
#[path = "abi_arena_tests.rs"]
mod arena_tests;
#[cfg(test)]
#[path = "abi_cold_tests.rs"]
mod cold_tests;
#[cfg(test)]
#[path = "abi_host_tests.rs"]
mod host_tests;
#[cfg(test)]
#[path = "abi_program_tests.rs"]
mod program_tests;
#[cfg(test)]
#[path = "abi_resume_tests.rs"]
mod resume_tests;
#[cfg(test)]
#[path = "abi_set_tests.rs"]
mod set_tests;
#[cfg(test)]
#[path = "abi_spawn_tests.rs"]
mod spawn_tests;
#[cfg(test)]
#[path = "abi_tests.rs"]
mod tests;

pub struct Context<'a> {
    // Existing Machine owns this activation's slots; the ABI borrows its identity
    // only during the selected callback. RetainedState never stores this borrow.
    selected_frame: Option<&'a frames::Frame>,
    // Only borrowed during a callback; never parked in RetainedState.
    selected_activation: Option<&'a execution::Activation>,
    program: Option<ProgramFrame>,
    request: Option<RequestedInvocation>,
    physical_arena: Option<&'a physical_frames::Arena>,
    host: &'a mut Host,
    storage: FrameStorage<'a>,
    frame: Option<frames::Frame>,
    destination: Option<frames::Storage>,
    values: Vec<Option<V>>,
    outcomes: Vec<Option<(String, Vec<V>)>>,
    value_base: u64,
    outcome_base: u64,
    error: Option<String>,
    result: Option<Completion>,
    lexical: Vec<LexicalBoundary>,
    next_lexical: u64,
}
// A lexical scope shares its control's frame. Checked local identities are
// resolved before entering the host scope, and only the selected top scope ends.
struct LexicalBoundary {
    id: u64,
    state: BoundaryState,
    retained: Vec<usize>,
    retire: Vec<usize>,
    cleanup: Vec<usize>,
    fault: usize,
}
// Temporary handles remain invocation-local. Source locals have stable addresses
// in one arena shared by the synchronous call tree, just as in checked execution.
enum FrameStorage<'a> {
    Owned(Frames),
    Borrowed(&'a mut Frames),
}
impl FrameStorage<'_> {
    fn values(&self) -> &Frames {
        match self {
            Self::Owned(values) => values,
            Self::Borrowed(values) => values,
        }
    }
    fn values_mut(&mut self) -> &mut Frames {
        match self {
            Self::Owned(values) => values,
            Self::Borrowed(values) => values,
        }
    }
}
impl Context<'_> {
    fn frame_ref(&self) -> R<&frames::Frame> {
        self.selected_frame
            .or(self.frame.as_ref())
            .ok_or_else(|| "E_NATIVE: missing ABI frame".into())
    }
    fn local(&self, slot: usize) -> R<usize> {
        let frame = self.frame_ref()?;
        let values = self.storage.values();
        values.require_storage(&values.local_storage(frame, slot)?)
    }
    fn finish(&mut self) -> R<Completion> {
        if !self.lexical.is_empty() {
            self.error
                .get_or_insert_with(|| "E_NATIVE: unfinished ABI lexical boundary".into());
            while let Some(scope) = self.lexical.last() {
                let id = scope.id;
                if let Err(error) = self.end_lexical(id) {
                    self.error.get_or_insert(error);
                    break;
                }
            }
        }
        let result = match self.error.take() {
            Some(error) => Err(error),
            None => self
                .result
                .take()
                .ok_or_else(|| "E_NATIVE: compiled ABI control fell through".into()),
        };
        let retired = match self.frame.take() {
            Some(frame) => self.storage.values_mut().retire(&frame),
            None => Ok(()),
        };
        // Retire storage even after failure, but preserve the original diagnostic.
        let result = result?;
        retired?;
        Ok(result)
    }
    // Only the inner callback error is eligible for source-fault handover.
    // Admission/retained-layout failures still finalize the scope but bypass cleanup.
    fn boundary_call(
        &mut self,
        callback: Callback,
        name: &str,
        values: Vec<V>,
        retained: &[usize],
        handover: &mut Vec<V>,
    ) -> R<R<Completion>> {
        self.host.enter(name)?;
        let mut child = Context {
            selected_frame: None,
            selected_activation: None,
            program: None,
            physical_arena: None,
            request: None,
            host: &mut *self.host,
            storage: FrameStorage::Borrowed(self.storage.values_mut()),
            frame: None,
            destination: None,
            values: vec![],
            outcomes: vec![],
            value_base: 0,
            outcome_base: 0,
            error: None,
            result: None,
            lexical: vec![],
            next_lexical: 0,
        };
        let arguments: Vec<_> = values.into_iter().map(|v| child.put(v)).collect();
        unsafe { callback(&mut child, arguments.as_ptr(), arguments.len()) };
        // Resolve the whole retained set before taking any owner. Failed work's
        // borrowers and ordinary locals retire before the contingency is entered.
        let captured: R<Vec<V>> = (|| {
            let roots = retained
                .iter()
                .map(|slot| child.local(*slot))
                .collect::<R<Vec<_>>>()?;
            if child.error.is_some() && child.host.source_faulted() {
                if roots
                    .iter()
                    .any(|root| child.storage.values()[*root].is_none())
                {
                    return Err("E_NATIVE: retained boundary parameter unavailable".into());
                }
                Ok(roots
                    .into_iter()
                    .map(|root| child.storage.values_mut()[root].take().unwrap())
                    .collect())
            } else {
                Ok(vec![])
            }
        })();
        let result = child.finish(); // Always retire even if capture validation failed.
        *handover = captured?;
        if result.is_ok() {
            child.host.leave();
        }
        Ok(result)
    }
    fn require_lexical(&self, id: u64) -> R<()> {
        if self.lexical.last().is_some_and(|scope| scope.id == id) {
            Ok(())
        } else {
            Err("E_NATIVE: ABI lexical boundary is not selected".into())
        }
    }
    fn abandon_temporaries(&mut self) -> R<()> {
        for value in &mut self.values {
            *value = None;
        }
        for outcome in &mut self.outcomes {
            *outcome = None;
        }
        self.retire_temporaries()
    }
    fn catch_lexical(&mut self, id: u64) -> R<bool> {
        self.require_lexical(id)?;
        let message = self
            .error
            .as_ref()
            .ok_or("E_NATIVE: lexical handover without error")?
            .clone();
        if !self.host.source_faulted() {
            return Ok(false);
        }
        if self.lexical.last().unwrap().state.fault_message.is_some() {
            return Err("E_NATIVE: repeated ABI lexical handover".into());
        }
        self.abandon_temporaries()?;
        let scope = self.lexical.last_mut().unwrap();
        let keep = self.host.resources.owned_tokens(
            scope
                .retained
                .iter()
                .filter_map(|id| self.storage.values()[*id].as_ref()),
        )?;
        for id in &scope.retire {
            self.storage.values_mut()[*id] = None;
        }
        let report = self
            .host
            .boundary_fault(&mut scope.state, &message, &keep)?;
        let (message, stack, sites) = report.source_fields();
        let strings = |xs: Vec<String>| V::Construct {
            ty: "vector<String>".into(),
            name: "vector".into(),
            fields: xs.into_iter().map(V::String).collect(),
        };
        self.storage.values_mut()[scope.fault] = Some(V::Construct {
            ty: "fault_report".into(),
            name: "fault_report".into(),
            fields: vec![V::String(message), strings(stack), strings(sites)],
        });
        self.host.enter_contingency(&mut scope.state)?;
        self.error = None;
        Ok(true)
    }
    fn end_lexical(&mut self, id: u64) -> R<()> {
        self.require_lexical(id)?;
        // Failure may strand temporary borrowers/owners; retire those before
        // resource finalization. Successful outward jumps still own their args.
        if self.error.is_some() {
            self.abandon_temporaries()?;
        }
        let mut scope = self.lexical.pop().unwrap();
        for id in scope.cleanup {
            self.storage.values_mut()[id] = None;
        }
        let result: R<()> = self.error.clone().map_or(Ok(()), Err);
        self.host.finish_boundary(&mut scope.state, &result)
    }
    fn put(&mut self, value: V) -> u64 {
        let Some(handle) = self
            .value_base
            .checked_add(self.values.len() as u64)
            .and_then(|n| n.checked_add(1))
        else {
            self.error
                .get_or_insert_with(|| "E_NATIVE: ABI value handle space exhausted".into());
            return 0;
        };
        self.values.push(Some(value));
        handle
    }
    fn require_outcome_room(&self) -> R<()> {
        self.outcome_base
            .checked_add(self.outcomes.len() as u64)
            .and_then(|n| n.checked_add(1))
            .map(|_| ())
            .ok_or_else(|| "E_NATIVE: ABI outcome handle space exhausted".into())
    }
    fn put_outcome(&mut self, outcome: (String, Vec<V>)) -> u64 {
        let Some(handle) = self
            .outcome_base
            .checked_add(self.outcomes.len() as u64)
            .and_then(|n| n.checked_add(1))
        else {
            self.error
                .get_or_insert_with(|| "E_NATIVE: ABI outcome handle space exhausted".into());
            return 0;
        };
        self.outcomes.push(Some(outcome));
        handle
    }
    // A checked instruction consumes every temporary it creates. Source owners
    // and references already live in Frames; no handle may cross this boundary.
    // Retain capacity, but advance the identity bases so old handles stay stale.
    fn retire_temporaries(&mut self) -> R<()> {
        if self.values.iter().any(Option::is_some) || self.outcomes.iter().any(Option::is_some) {
            return Err("E_NATIVE: live ABI temporary at instruction boundary".into());
        }
        let values = self
            .value_base
            .checked_add(self.values.len() as u64)
            .ok_or("E_NATIVE: ABI value handle space exhausted")?;
        let outcomes = self
            .outcome_base
            .checked_add(self.outcomes.len() as u64)
            .ok_or("E_NATIVE: ABI outcome handle space exhausted")?;
        self.values.clear();
        self.outcomes.clear();
        self.value_base = values;
        self.outcome_base = outcomes;
        Ok(())
    }
    fn value_index(&self, handle: u64) -> R<usize> {
        let index = handle
            .checked_sub(self.value_base)
            .and_then(|n| n.checked_sub(1))
            .and_then(|n| usize::try_from(n).ok());
        index
            .filter(|&i| self.values.get(i).is_some_and(Option::is_some))
            .ok_or_else(|| "E_NATIVE: invalid or consumed ABI value handle".into())
    }
    fn get(&self, handle: u64) -> R<&V> {
        Ok(self.values[self.value_index(handle)?].as_ref().unwrap())
    }
    fn outcome(&self, handle: u64) -> R<&(String, Vec<V>)> {
        handle
            .checked_sub(self.outcome_base)
            .and_then(|n| n.checked_sub(1))
            .and_then(|n| usize::try_from(n).ok())
            .and_then(|i| self.outcomes.get(i))
            .and_then(Option::as_ref)
            .ok_or_else(|| "E_NATIVE: invalid or consumed ABI outcome handle".into())
    }
    fn take_many(&mut self, handles: &[u64]) -> R<Vec<V>> {
        let mut seen = BTreeSet::new();
        for &handle in handles {
            self.get(handle)?;
            if !seen.insert(handle) {
                return Err("E_NATIVE: duplicate ABI value transfer".into());
            }
        }
        Ok(handles
            .iter()
            .map(|&h| {
                let index = self.value_index(h).unwrap();
                self.values[index].take().unwrap()
            })
            .collect())
    }
    fn defer_control(
        &mut self,
        producer: &'static execution::Control,
        ty: &str,
        handles: &[u64],
    ) -> R<u64> {
        let activation = self
            .selected_activation
            .ok_or("E_NATIVE: defer requires a selected ABI invocation")?;
        if producer.constructor
            || handles.len() != producer.parameters.len()
            || !ty.starts_with("$task<")
            || !ty.ends_with('>')
        {
            return Err("E_NATIVE: invalid ABI cold task descriptor".into());
        }
        self.value_base
            .checked_add(self.values.len() as u64)
            .and_then(|n| n.checked_add(1))
            .ok_or("E_NATIVE: ABI value handle space exhausted")?;
        let indices = handles
            .iter()
            .map(|h| self.value_index(*h))
            .collect::<R<Vec<_>>>()?;
        let mut arguments = self.take_many(handles)?;
        match activation.defer_task(self.host, producer, ty, &mut arguments) {
            Ok(task) => Ok(self.put(task)),
            Err(error) => {
                // Registry admission rejects before moving arguments. Preserve
                // every capture in its original temporary handle on failure.
                for (index, value) in indices.into_iter().zip(arguments) {
                    self.values[index] = Some(value);
                }
                Err(error)
            }
        }
    }
    fn take(&mut self, handle: u64) -> R<V> {
        Ok(self.take_many(&[handle])?.pop().unwrap())
    }
    fn construct(&mut self, ty: &str, name: &str, handles: &[u64]) -> R<u64> {
        let collection = collection_support::Collection::parse(ty);
        let expected = if let Some(collection) = &collection {
            if name != collection.name() {
                return Err("E_NATIVE: invalid ABI collection constructor".into());
            }
            collection
                .fields(handles.len())
                .ok_or("E_NATIVE: ABI collection field count mismatch")?
        } else {
            fields(&self.host.manifest, ty, name)
                .map_err(|e| format!("E_NATIVE: invalid ABI constructor: {e}"))?
        };
        if expected.len() != handles.len() {
            return Err("E_NATIVE: ABI constructor field count mismatch".into());
        }
        for (&handle, ty) in handles.iter().zip(&expected) {
            validate(self.get(handle)?, ty, &self.host.manifest)
                .map_err(|e| format!("E_NATIVE: invalid ABI constructor payload: {e}"))?;
        }
        if matches!(collection, Some(collection_support::Collection::Map(..))) {
            let values = handles
                .iter()
                .map(|h| self.get(*h).cloned())
                .collect::<R<Vec<_>>>()?;
            if !collection_support::sorted_map(&values) {
                return Err("E_NATIVE: ABI map keys must be unique and sorted".into());
            }
        }
        let fields = self.take_many(handles)?;
        Ok(self.put(V::Construct {
            ty: ty.into(),
            name: name.into(),
            fields,
        }))
    }
    fn perform<T: Default>(&mut self, f: impl FnOnce(&mut Self) -> R<T>) -> T {
        if self.error.is_some() {
            return T::default();
        }
        match f(self) {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error);
                T::default()
            }
        }
    }
}
unsafe fn slice<'a, T>(data: *const T, len: usize) -> &'a [T] {
    if len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }
    }
}
unsafe fn text<'a>(data: *const u8, len: usize) -> R<&'a str> {
    std::str::from_utf8(unsafe { slice(data, len) })
        .map_err(|_| "E_NATIVE: invalid UTF-8 in ABI text".into())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_ok(ctx: *mut Context<'_>) -> i32 {
    i32::from(unsafe { &*ctx }.error.is_none())
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_frame(ctx: *mut Context<'_>, count: usize) {
    unsafe { &mut *ctx }.perform(|c| {
        if c.frame.is_some() || c.selected_frame.is_some() {
            return Err("E_NATIVE: duplicate ABI frame admission".into());
        }
        c.frame = Some(c.storage.values_mut().enter(count)?);
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_local(
    ctx: *mut Context<'_>,
    slot: usize,
    path: *const usize,
    count: usize,
    moving: u32,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let root = c.local(slot)?;
        let path = unsafe { slice(path, count) };
        let value = match moving {
            0 => read_place(c.storage.values(), root, path, false)?,
            1 => take_place(c.storage.values_mut(), root, path, false)?,
            _ => return Err("E_NATIVE: invalid ABI local transfer mode".into()),
        };
        Ok(c.put(value))
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_store(
    ctx: *mut Context<'_>,
    slot: usize,
    path: *const usize,
    count: usize,
    value: u64,
) {
    unsafe { &mut *ctx }.perform(|c| {
        let root = c.local(slot)?;
        let path = unsafe { slice(path, count) };
        c.get(value)?;
        // A root may be uninitialized or moved; a field needs a valid container.
        // Validate both sides before consuming the temporary owner.
        if !path.is_empty() {
            mutable_place(c.storage.values_mut(), root, path)?;
        }
        let value = c.take(value)?;
        assign_place(c.storage.values_mut(), root, path, false, value)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_release_local(ctx: *mut Context<'_>, slot: usize) {
    unsafe { &mut *ctx }.perform(|c| {
        let root = c.local(slot)?;
        c.storage.values_mut()[root] = None;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_reference(
    ctx: *mut Context<'_>,
    slot: usize,
    path: *const usize,
    count: usize,
    indirect: u32,
    ty: *const u8,
    len: usize,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let root = c.local(slot)?;
        let ty = unsafe { text(ty, len)? };
        if indirect > 1 || !ty.starts_with('&') {
            return Err("E_NATIVE: invalid ABI reference descriptor".into());
        }
        let value = reference(
            c.storage.values(),
            root,
            unsafe { slice(path, count) },
            indirect != 0,
            ty,
        )?;
        Ok(c.put(value))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_deref(
    ctx: *mut Context<'_>,
    slot: usize,
    path: *const usize,
    count: usize,
    moving: u32,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let root = c.local(slot)?;
        let path = unsafe { slice(path, count) };
        let value = match moving {
            0 => read_place(c.storage.values(), root, path, true)?,
            1 => take_place(c.storage.values_mut(), root, path, true)?,
            _ => return Err("E_NATIVE: invalid ABI indirect transfer mode".into()),
        };
        Ok(c.put(value))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_assign_through(
    ctx: *mut Context<'_>,
    slot: usize,
    path: *const usize,
    count: usize,
    value: u64,
) {
    unsafe { &mut *ctx }.perform(|c| {
        let root = c.local(slot)?;
        c.get(value)?;
        let (root, fields) = address(
            c.storage.values(),
            root,
            unsafe { slice(path, count) },
            true,
        )?;
        // Resolve and validate the target before moving the RHS, exactly as for
        // direct fields. Address formation does not grant source move permission.
        if fields.is_empty() {
            c.storage
                .values()
                .get(root)
                .ok_or("E_NATIVE: invalid indirect assignment")?;
        } else {
            mutable_place(c.storage.values_mut(), root, &fields)?;
        }
        let value = c.take(value)?;
        assign_place(c.storage.values_mut(), root, &fields, false, value)
    })
}

/// Metadata words must have module lifetime, not be a stack temporary. Each
/// descriptor is [kind, fresh, (local, flags, path_length, path...)...], with
/// kind 0=partial, 1=complete, 2=incoming and flags indirect/guard/opaque in bits
/// 0/1/2. Kinds 3=guarded and 4=carrier require fresh=0, followed by an
/// origin-word count, those origin words, then respectively:
/// [leaf, holder, path_length, path...] or
/// [holder, leaf_count, (leaf, path_length, path...)...].
/// No Rust layout crosses the ABI. Symbolic paths are never dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_evidence(
    ctx: *mut Context<'_>,
    value: u64,
    data: *const usize,
    count: usize,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        use execution::reference_evidence::{
            InputLeaf, Origin, attach_carrier_input, attach_checked, attach_guarded_input,
            inherit_checked,
        };
        if !matches!(c.get(value)?, V::Reference { .. }) {
            return Err("E_NATIVE: ABI evidence requires a reference".into());
        }
        // Safety: compiled descriptors are immutable LLVM globals, available for
        // the complete program lifetime, as required by this entry point.
        let words: &'static [usize] = unsafe { slice(data, count) };
        let [kind, fresh, rest @ ..] = words else {
            return Err("E_NATIVE: truncated ABI reference evidence".into());
        };
        if *kind > 4
            || *fresh > 1
            || (*kind >= 2 && *fresh != 0)
            || (*kind == 2 && !rest.is_empty())
        {
            return Err("E_NATIVE: invalid ABI reference evidence mode".into());
        }
        let (mut rest, extra) = if *kind >= 3 {
            let [length, rest @ ..] = rest else {
                return Err("E_NATIVE: missing ABI carrier origin length".into());
            };
            if *length > rest.len() {
                return Err("E_NATIVE: truncated ABI carrier origins".into());
            }
            rest.split_at(*length)
        } else {
            (rest, &[][..])
        };
        let mut origins = vec![];
        while !rest.is_empty() {
            let [slot, flags, count, fields @ ..] = rest else {
                return Err("E_NATIVE: truncated ABI reference origin".into());
            };
            if *flags > 7 || *count > fields.len() {
                return Err("E_NATIVE: invalid ABI reference origin".into());
            }
            origins.push(Origin {
                root: c.local(*slot)?,
                fields: &fields[..*count],
                indirect: flags & 1 != 0,
                guard: flags & 2 != 0,
                opaque: flags & 4 != 0,
            });
            rest = &fields[*count..];
        }
        // Decode and resolve every local before consuming the operand. Actual
        // holder/leaf relationships remain checked by the existing metadata helpers.
        let guarded = if *kind == 3 {
            let [leaf, holder, length, fields @ ..] = extra else {
                return Err("E_NATIVE: truncated ABI guarded input".into());
            };
            if *length != fields.len() {
                return Err("E_NATIVE: invalid ABI guarded input path".into());
            }
            Some((c.local(*leaf)?, c.local(*holder)?, fields))
        } else {
            None
        };
        let carrier = if *kind == 4 {
            let [holder, length, rest @ ..] = extra else {
                return Err("E_NATIVE: truncated ABI carrier input".into());
            };
            let holder = c.local(*holder)?;
            let mut rest = rest;
            let mut leaves = vec![];
            // Parse by available words rather than trusting an allocation count.
            while !rest.is_empty() {
                let [leaf, length, fields @ ..] = rest else {
                    return Err("E_NATIVE: truncated ABI carrier leaf".into());
                };
                if *length > fields.len() {
                    return Err("E_NATIVE: invalid ABI carrier leaf path".into());
                }
                leaves.push(InputLeaf {
                    slot: c.local(*leaf)?,
                    fields: &fields[..*length],
                });
                rest = &fields[*length..];
            }
            if leaves.len() != *length {
                return Err("E_NATIVE: invalid ABI carrier leaf count".into());
            }
            Some((holder, leaves))
        } else {
            None
        };
        let value = c.take(value)?;
        let value = if let Some((leaf, holder, fields)) = guarded {
            attach_guarded_input(c.storage.values(), value, &origins, leaf, holder, fields)
        } else if let Some((holder, leaves)) = carrier {
            attach_carrier_input(c.storage.values(), value, &origins, holder, &leaves)
        } else if *kind == 2 {
            inherit_checked(c.storage.values(), value)
        } else {
            attach_checked(c.storage.values(), value, &origins, *kind == 1, *fresh != 0)
        };
        Ok(c.put(value))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_borrowed_variant(ctx: *mut Context<'_>, value: u64) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        c.require_outcome_room()?;
        if !matches!(c.get(value)?, V::Reference { .. }) {
            return Err("E_NATIVE: ABI borrowed decision requires a reference".into());
        }
        let value = c.take(value)?;
        let outcome = borrowed_variant(c.storage.values(), value)?;
        Ok(c.put_outcome(outcome))
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_value(
    ctx: *mut Context<'_>,
    tag: u32,
    number: u64,
    data: *const u8,
    len: usize,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let value = match tag {
            0 => V::Unit,
            1 if number <= 1 => V::Bool(number != 0),
            2 if number <= u32::MAX as u64 => V::U32(number as u32),
            3 => V::String(unsafe { text(data, len)? }.into()),
            4 => V::Bytes(unsafe { slice(data, len) }.to_vec()),
            _ => return Err("E_NATIVE: invalid ABI scalar literal".into()),
        };
        Ok(c.put(value))
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_copy(ctx: *mut Context<'_>, handle: u64) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let value = c.get(handle)?.clone();
        Ok(c.put(value))
    })
}
/// Record fields arrive in declaration order after evaluation in source order.
/// Validate the complete shape and handle batch before transferring any owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_record(
    ctx: *mut Context<'_>,
    ty: *const u8,
    len: usize,
    args: *const u64,
    count: usize,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let ty = unsafe { text(ty, len)? };
        c.host.manifest["types"]
            .as_array()
            .and_then(|types| {
                types
                    .iter()
                    .find(|t| t["name"] == ty && t["kind"] == "record")
            })
            .ok_or("E_NATIVE: invalid ABI record type")?;
        c.construct(ty, ty, unsafe { slice(args, count) })
    })
}

/// A tagged value uses the same opaque ownership transfer as an ordinary record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_construct(
    ctx: *mut Context<'_>,
    ty: *const u8,
    ty_len: usize,
    name: *const u8,
    name_len: usize,
    args: *const u64,
    count: usize,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        c.construct(
            unsafe { text(ty, ty_len)? },
            unsafe { text(name, name_len)? },
            unsafe { slice(args, count) },
        )
    })
}

/// Consume a checked value decision into the same one-use outcome table as calls.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_variant(ctx: *mut Context<'_>, handle: u64) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        c.require_outcome_room()?;
        let value = c.get(handle)?;
        if !matches!(value, V::Bool(_) | V::Construct { .. }) {
            return Err("E_NATIVE: invalid ABI decision value".into());
        }
        validate(value, value.ty(), &c.host.manifest)
            .map_err(|e| format!("E_NATIVE: invalid ABI decision payload: {e}"))?;
        let outcome = variant(c.take(handle)?)?;
        Ok(c.put_outcome(outcome))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_field(
    ctx: *mut Context<'_>,
    handle: u64,
    path: *const usize,
    count: usize,
    moving: u32,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let root = c.value_index(handle)?;
        let path = unsafe { slice(path, count) };
        let value = match moving {
            0 => read_place(&c.values, root, path, false)?,
            1 => take_place(&mut c.values, root, path, false)?,
            _ => return Err("E_NATIVE: invalid ABI field transfer mode".into()),
        };
        Ok(c.put(value))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_assign(
    ctx: *mut Context<'_>,
    handle: u64,
    path: *const usize,
    count: usize,
    value: u64,
) {
    unsafe { &mut *ctx }.perform(|c| {
        let root = c.value_index(handle)?;
        c.get(value)?;
        if handle == value {
            return Err("E_NATIVE: aliased ABI assignment transfer".into());
        }
        let path = unsafe { slice(path, count) };
        // Validate the destination before consuming the RHS. No value is cloned.
        mutable_place(&mut c.values, root, path)?;
        let value = c.take(value)?;
        assign_place(&mut c.values, root, path, false, value)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_project(
    ctx: *mut Context<'_>,
    handle: u64,
    index: usize,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let root = c.value_index(handle)?;
        mutable_place(&mut c.values, root, &[index])?;
        let value = project_owned(c.take(handle)?, index)?;
        Ok(c.put(value))
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_drop(ctx: *mut Context<'_>, handle: u64) {
    unsafe { &mut *ctx }.perform(|c| {
        c.take(handle)?;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_release(ctx: *mut Context<'_>, handle: u64) {
    if handle != 0 {
        unsafe { tellur_rt_v1_drop(ctx, handle) }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_bool(ctx: *mut Context<'_>, handle: u64) -> i32 {
    unsafe { &mut *ctx }.perform(|c| Ok(i32::from(boolean(c.take(handle)?)?)))
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_binary(
    ctx: *mut Context<'_>,
    op: *const u8,
    len: usize,
    left: u64,
    right: u64,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let op = unsafe { text(op, len)? };
        let mut args = c.take_many(&[left, right])?.into_iter();
        let value = binary(op, args.next().unwrap(), args.next().unwrap())?;
        Ok(c.put(value))
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_pure(
    ctx: *mut Context<'_>,
    name: *const u8,
    len: usize,
    args: *const u64,
    count: usize,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let name = unsafe { text(name, len)? };
        let args = c.take_many(unsafe { slice(args, count) })?;
        Ok(c.put(pure(name, args)?))
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_tick(
    ctx: *mut Context<'_>,
    site: usize,
    line: usize,
    column: usize,
) {
    unsafe { &mut *ctx }.perform(|c| {
        c.host.at(site);
        c.retire_temporaries()?;
        c.host.tick(line, column)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_emit(ctx: *mut Context<'_>, value: u64) {
    unsafe { &mut *ctx }.perform(|c| {
        let value = c.take(value)?;
        c.host.emit(value)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_fault(ctx: *mut Context<'_>, value: u64) {
    unsafe { &mut *ctx }.perform::<()>(|c| {
        let value = c.take(value)?;
        c.host.fault(value)
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_complete(
    ctx: *mut Context<'_>,
    name: *const u8,
    len: usize,
    args: *const u64,
    count: usize,
) {
    unsafe { &mut *ctx }.perform(|c| {
        if c.result.is_some() {
            return Err("E_NATIVE: duplicate ABI completion".into());
        }
        if c.destination.is_some() {
            return Err("E_NATIVE: constructor requires destination completion".into());
        }
        let name = unsafe { text(name, len)? }.to_owned();
        let args = c.take_many(unsafe { slice(args, count) })?;
        c.result = Some(Completion::Outcome((name, args)));
        Ok(())
    })
}

/// Publish only a fully initialized value. The destination identifies the caller's
/// live allocation, never an offset that could alias a retired constructor frame.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_constructed(ctx: *mut Context<'_>, value: u64) {
    unsafe { &mut *ctx }.perform(|c| {
        if c.result.is_some() {
            return Err("E_NATIVE: duplicate ABI completion".into());
        }
        let destination = c
            .destination
            .as_ref()
            .ok_or("E_NATIVE: missing constructor destination")?;
        let root = c.storage.values().require_storage(destination)?;
        if c.storage.values()[root].is_some() {
            return Err("E_NATIVE: occupied constructor destination".into());
        }
        let value = c.take(value)?;
        c.storage.values_mut()[root] = Some(value);
        c.result = Some(Completion::Constructed);
        Ok(())
    })
}

/// Private frontend-frame callback. Only the first call receives argument handles.
/// Context and argument pointers are valid for this callback only; the program
/// frame is caller-owned stable storage whose lifetime covers the retained root.
/// Return 1 for a source yield, 2 for completion, or 0 after recording an error.
pub type ResumeCallback = unsafe extern "C" fn(*mut Context<'_>, *mut u8, *const u64, usize) -> u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeStatus {
    Yielded,
    Complete,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResumePhase {
    Cold,
    Yielded,
    Complete,
    Failed,
    Retired,
}

// Unlike Context, this record owns everything it retains. Host and Frames are
// reborrowed only while a callback or retirement operation is actually selected.
struct RetainedState {
    frame: Option<frames::Frame>,
    destination: Option<frames::Storage>,
    values: Vec<Option<V>>,
    outcomes: Vec<Option<(String, Vec<V>)>>,
    value_base: u64,
    outcome_base: u64,
    error: Option<String>,
    result: Option<Completion>,
    lexical: Vec<LexicalBoundary>,
    next_lexical: u64,
}
impl RetainedState {
    fn select<'a>(self, host: &'a mut Host, values: &'a mut Frames) -> Context<'a> {
        Context {
            selected_frame: None,
            selected_activation: None,
            program: None,
            physical_arena: None,
            request: None,
            host,
            storage: FrameStorage::Borrowed(values),
            frame: self.frame,
            destination: self.destination,
            values: self.values,
            outcomes: self.outcomes,
            value_base: self.value_base,
            outcome_base: self.outcome_base,
            error: self.error,
            result: self.result,
            lexical: self.lexical,
            next_lexical: self.next_lexical,
        }
    }
    fn park(context: Context<'_>) -> Self {
        debug_assert!(matches!(context.storage, FrameStorage::Borrowed(_)));
        Self {
            frame: context.frame,
            destination: context.destination,
            values: context.values,
            outcomes: context.outcomes,
            value_base: context.value_base,
            outcome_base: context.outcome_base,
            error: context.error,
            result: context.result,
            lexical: context.lexical,
            next_lexical: context.next_lexical,
        }
    }
}

/// An independent private ABI root, not a source task or an awaitable handle.
/// It reuses Host context/scope selection and Frames identity/retirement. Source
/// scheduling, direct-child adoption and checked-IR emission are separate layers.
/// The Rust lifetime pins the caller-owned typed program frame without allocating
/// it here. Every admitted root must be completed or stopped before it is dropped.
pub struct RetainedRoot<'frame> {
    program_frame: *mut u8,
    callback: ResumeCallback,
    name: String,
    state: Option<RetainedState>,
    arguments: Vec<u64>,
    host: HostContext,
    root: ContextRoot,
    phase: ResumePhase,
    failure: Option<fault::Report>,
    _frame: std::marker::PhantomData<&'frame mut ()>,
}
impl<'frame> RetainedRoot<'frame> {
    /// Safety: callback must interpret the program frame as T and obey every ABI
    /// pointer/span contract. It must not unwind or retain Context/argument pointers.
    /// Slot count and argument types are supplied by checked control metadata.
    pub unsafe fn new<T>(
        host: &mut Host,
        frames: &mut Frames,
        name: &str,
        slots: usize,
        arguments: Vec<V>,
        program_frame: &'frame mut T,
        callback: ResumeCallback,
    ) -> R<Self> {
        // All recoverable checks precede capture transfer and live frame admission.
        host.resources.result_tokens(&arguments)?;
        let prepared = frames.prepare(slots)?;
        let (root, context) = host.prepare_context(&arguments)?;
        let frame = frames.commit(prepared).expect("prevalidated ABI frame");
        let handles = (1..=arguments.len() as u64).collect();
        Ok(Self {
            program_frame: (program_frame as *mut T).cast(),
            callback,
            name: name.to_owned(),
            state: Some(RetainedState {
                frame: Some(frame),
                destination: None,
                values: arguments.into_iter().map(Some).collect(),
                outcomes: vec![],
                value_base: 0,
                outcome_base: 0,
                error: None,
                result: None,
                lexical: vec![],
                next_lexical: 0,
            }),
            arguments: handles,
            host: context,
            root,
            phase: ResumePhase::Cold,
            failure: None,
            _frame: std::marker::PhantomData,
        })
    }
    fn require(&self, host: &Host, frames: &Frames) -> R<()> {
        if self.phase == ResumePhase::Retired {
            return Err("E_NATIVE: ABI root already retired".into());
        }
        host.require_context(&self.host, &self.root)?;
        let frame = self
            .state
            .as_ref()
            .and_then(|s| s.frame.as_ref())
            .ok_or("E_NATIVE: missing retained ABI frame")?;
        frames.frame_values(frame)?;
        Ok(())
    }
    /// One selected callback. Rejected identities/phases perform no source work;
    /// the caller's host context is restored after yield, completion or error.
    pub fn resume(
        &mut self,
        host: &mut Host,
        frames: &mut Frames,
        program_frame: *mut u8,
    ) -> R<ResumeStatus> {
        self.require(host, frames)?;
        if program_frame != self.program_frame {
            return Err("E_NATIVE: ABI program frame identity mismatch".into());
        }
        if !matches!(self.phase, ResumePhase::Cold | ResumePhase::Yielded) {
            return Err("E_NATIVE: ABI root cannot resume after completion".into());
        }
        host.exchange_context(&mut self.host)?;
        let mut context = self.state.take().unwrap().select(host, frames);
        let cold = self.phase == ResumePhase::Cold;
        if cold {
            context.error = context.host.enter(&self.name).err();
        }
        let arguments = if cold { self.arguments.as_slice() } else { &[] };
        let tag = if context.error.is_none() {
            unsafe {
                (self.callback)(
                    &mut context,
                    self.program_frame,
                    arguments.as_ptr(),
                    arguments.len(),
                )
            }
        } else {
            0
        };
        let status = if context.error.is_some() {
            ResumeStatus::Failed
        } else if tag == 1 && context.result.is_none() {
            ResumeStatus::Yielded
        } else if tag == 2
            && matches!(context.result, Some(Completion::Outcome(_)))
            && context.lexical.is_empty()
        {
            ResumeStatus::Complete
        } else {
            context.error = Some("E_NATIVE: malformed ABI resume step".into());
            ResumeStatus::Failed
        };
        self.state = Some(RetainedState::park(context));
        // No callback may retire the independent root or the saved caller scope.
        host.exchange_context(&mut self.host)?;
        self.arguments.clear();
        self.phase = match status {
            ResumeStatus::Yielded => ResumePhase::Yielded,
            ResumeStatus::Complete => ResumePhase::Complete,
            ResumeStatus::Failed => ResumePhase::Failed,
        };
        Ok(status)
    }
    /// Consume a completed root once. Failed callbacks keep their original report;
    /// all state retires even when outcome/resource validation rejects a callback.
    pub fn finish(&mut self, host: &mut Host, frames: &mut Frames) -> R<Completion> {
        self.require(host, frames)?;
        if !matches!(self.phase, ResumePhase::Complete | ResumePhase::Failed) {
            return Err("E_NATIVE: ABI root has not completed".into());
        }
        self.retire(host, frames, false)
    }
    /// Administrative stop: never resumes cold/suspended work and produces no
    /// declared outcome. This is cleanup machinery, not cooperative cancellation.
    pub fn stop(&mut self, host: &mut Host, frames: &mut Frames) -> R<()> {
        self.require(host, frames)?;
        self.retire(host, frames, true).map(|_| ())
    }
    pub fn failure(&self) -> Option<&fault::Report> {
        self.failure.as_ref()
    }
    fn retire(&mut self, host: &mut Host, frames: &mut Frames, stopped: bool) -> R<Completion> {
        host.exchange_context(&mut self.host)?;
        let mut context = self.state.take().unwrap().select(host, frames);
        let had_error = context.error.is_some();
        if stopped {
            context
                .error
                .get_or_insert_with(|| "E_NATIVE: ABI root stopped".into());
        }
        // Borrowed temporaries/completions must disappear before source slots and
        // resource owners. A stopped completed result is not adopted by the caller.
        if context.error.is_some() {
            context.values.clear();
            context.outcomes.clear();
            context.result = None;
        }
        let mut result = context.finish();
        if let Err(error) = &result {
            if !stopped || had_error {
                self.failure = Some(context.host.activations.failure(
                    &self.name,
                    fault::Phase::Execution,
                    error,
                ));
            }
        }
        let resource_result = if result.is_err() || stopped {
            context.host.finish_context(&mut self.root, true)
        } else {
            Ok(()) // Outgoing owners transfer from the parked root below.
        };
        drop(context);
        host.exchange_context(&mut self.host)?;
        if result.is_ok() && !stopped {
            let fields = match result.as_ref().unwrap() {
                Completion::Outcome((_, fields)) => fields,
                _ => unreachable!("resume validated normal completion"),
            };
            if let Err(error) = host.adopt_context_result(&self.host, &mut self.root, None, fields)
            {
                // The callback left an unreturned owner. Settle its root instead of
                // leaking it or exposing a partially validated normal completion.
                host.exchange_context(&mut self.host)?;
                self.failure = Some(host.activations.failure(
                    &self.name,
                    fault::Phase::ResultValidation,
                    &error,
                ));
                result = Err(error);
                let cleanup = host.finish_context(&mut self.root, true);
                host.exchange_context(&mut self.host)?;
                cleanup?;
            } else {
                // Preserve the producer activation until result validation has
                // succeeded, so a rejected result retains its original context.
                self.host.tracker.leave();
            }
        }
        self.phase = ResumePhase::Retired;
        resource_result?;
        if stopped {
            Ok(Completion::Settled)
        } else {
            result
        }
    }
}

/// Private runtime descriptor for a typed LLVM resume frame. This Rust metadata
/// stays inside the runtime; generated code sees only Context and integer handles.
/// Each callback executes one checked transition and returns 0 error, 1 yield,
/// 2 normal completion, 3 direct await, 4 continue without a source handoff, or
/// 5 run an inline child on the current invocation stack, 6 spawn without a source
/// handoff, 7 consume a started child through the source join policy, or 8 request
/// cancellation without a source handoff or consuming the started value, or
/// 9 start a retained async host operation, 10 spawn into a task set, or
/// 11 await the earliest completed member of a nonempty set.
/// Initial arguments arrive once; after child delivery the span contains one
/// outcome handle. Other callbacks receive an empty span.
pub struct InvocationLayout {
    pub control: &'static execution::Control,
    pub callback: ResumeCallback,
    pub size: usize,
    pub align: usize,
    pub prefix: usize,
    pub children: &'static [InlineChild],
}
pub struct InlineChild {
    pub offset: usize,
    pub layout: &'static InvocationLayout,
}
impl InvocationLayout {
    fn validate(&self) -> R<()> {
        if self.size == 0
            || !self.align.is_power_of_two()
            || self.prefix > self.size
            || self.control.constructor
            || !self.control.retained.is_empty()
            || !std::ptr::fn_addr_eq(self.control.resume, resume_invocation as fn(_, _, _) -> _)
            || self.control.parameters.iter().enumerate().any(|(i, slot)| {
                *slot >= self.control.slots || self.control.parameters[..i].contains(slot)
            })
        {
            return Err("E_NATIVE: invalid retained ABI invocation layout".into());
        }
        for (i, child) in self.children.iter().enumerate() {
            let end = child
                .offset
                .checked_add(child.layout.size)
                .ok_or("E_NATIVE: inline ABI frame extent overflow")?;
            if child.layout.size == 0
                || !child.layout.align.is_power_of_two()
                || child.offset < self.prefix
                || end > self.size
                || child.layout.align > self.align
                || child.offset % child.layout.align != 0
                || self.children[..i].iter().any(|other| {
                    child.offset < other.offset.saturating_add(other.layout.size)
                        && other.offset < end
                })
            {
                return Err("E_NATIVE: invalid inline ABI child layout".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
struct ProgramFrame {
    address: *mut u8,
    layout: &'static InvocationLayout,
}
impl ProgramFrame {
    fn child(self, index: usize) -> R<Self> {
        self.layout.validate()?;
        let child = self
            .layout
            .children
            .get(index)
            .ok_or("E_NATIVE: unknown inline ABI child")?;
        child.layout.validate()?;
        Ok(Self {
            // Layout preflight proves the full child lies in the stable root frame.
            address: unsafe { self.address.add(child.offset) },
            layout: child.layout,
        })
    }
}
// Runtime-owned physical storage for spawned ABI frames. Slots are recycled only
// after the last invocation pin (including inline descendants) retires. Dynamic
// source values and task metadata remain in the existing runtime stores.
mod physical_frames {
    use super::R;
    use std::{alloc::Layout, cell::RefCell, collections::BTreeMap, ptr::NonNull, rc::Rc};

    const CHUNK_BYTES: usize = 4096;
    #[derive(Clone, Default)]
    pub(super) struct Arena(Rc<RefCell<State>>);
    #[derive(Default)]
    struct State {
        chunks: Vec<Chunk>,
        slots: Vec<Slot>,
        free: BTreeMap<(usize, usize), Vec<usize>>,
    }
    struct Chunk {
        address: NonNull<u8>,
        layout: Layout,
        used: usize,
    }
    impl Drop for Chunk {
        fn drop(&mut self) {
            unsafe { std::alloc::dealloc(self.address.as_ptr(), self.layout) };
        }
    }
    struct Slot {
        address: NonNull<u8>,
        layout: Layout,
        pins: usize,
    }
    pub(super) struct Pin {
        arena: Arena,
        slot: usize,
    }
    impl Arena {
        pub(super) fn allocate(&self, size: usize, align: usize) -> R<Pin> {
            let layout = Layout::from_size_align(size, align)
                .map_err(|_| "E_NATIVE: invalid physical frame allocation".to_owned())?;
            if size == 0 {
                return Err("E_NATIVE: physical frame allocation is empty".into());
            }
            let mut state = self.0.borrow_mut();
            let key = (size, align);
            let slot = if let Some(slot) = state.free.get_mut(&key).and_then(Vec::pop) {
                assert_eq!(state.slots[slot].pins, 0);
                state.slots[slot].pins = 1;
                slot
            } else {
                let mut address = None;
                for chunk in state.chunks.iter_mut().rev() {
                    if chunk.layout.align() < align {
                        continue;
                    }
                    let Some(offset) = chunk.used.checked_add(align - 1).map(|n| n & !(align - 1))
                    else {
                        continue;
                    };
                    if offset <= chunk.layout.size() && size <= chunk.layout.size() - offset {
                        address = Some(unsafe {
                            NonNull::new_unchecked(chunk.address.as_ptr().add(offset))
                        });
                        chunk.used = offset + size;
                        break;
                    }
                }
                let address = match address {
                    Some(address) => address,
                    None => {
                        let chunk_layout = Layout::from_size_align(CHUNK_BYTES.max(size), align)
                            .map_err(|_| {
                                "E_NATIVE: physical arena chunk is too large".to_owned()
                            })?;
                        let address = NonNull::new(unsafe { std::alloc::alloc(chunk_layout) })
                            .ok_or("E_NATIVE: physical arena allocation failed")?;
                        state.chunks.push(Chunk {
                            address,
                            layout: chunk_layout,
                            used: size,
                        });
                        address
                    }
                };
                let index = state.slots.len();
                state.slots.push(Slot {
                    address,
                    layout,
                    pins: 1,
                });
                index
            };
            // Generated frames contain a program counter, facts and inline child
            // frames; their initial state is all zero. No source V lives here.
            unsafe { std::ptr::write_bytes(state.slots[slot].address.as_ptr(), 0, size) };
            Ok(Pin {
                arena: self.clone(),
                slot,
            })
        }
        #[cfg(test)]
        pub(super) fn counts(&self) -> (usize, usize, usize) {
            let state = self.0.borrow();
            (
                state.chunks.len(),
                state.slots.len(),
                state.slots.iter().map(|s| s.pins).sum(),
            )
        }
    }
    impl Pin {
        pub(super) fn address(&self) -> *mut u8 {
            self.arena.0.borrow().slots[self.slot].address.as_ptr()
        }
        pub(super) fn retain(&self) -> R<Self> {
            let mut state = self.arena.0.borrow_mut();
            let slot = &mut state.slots[self.slot];
            let pins = slot
                .pins
                .checked_add(1)
                .ok_or("E_NATIVE: physical frame pin space exhausted")?;
            assert_ne!(slot.pins, 0);
            slot.pins = pins;
            Ok(Self {
                arena: self.arena.clone(),
                slot: self.slot,
            })
        }
    }
    impl Drop for Pin {
        fn drop(&mut self) {
            let mut state = self.arena.0.borrow_mut();
            let slot = &mut state.slots[self.slot];
            assert_ne!(slot.pins, 0);
            slot.pins -= 1;
            if slot.pins == 0 {
                let key = (slot.layout.size(), slot.layout.align());
                state.free.entry(key).or_default().push(self.slot);
            }
        }
    }
}

enum InvocationArguments {
    Direct(Vec<V>),
    Cold(Option<V>),
}
#[derive(Clone, Copy)]
struct HostLeaf {
    operation: &'static str,
    cancellable: bool,
}
enum RequestedInvocation {
    SpawnInto {
        child: Invocation,
        arguments: Vec<V>,
        set: V,
        slot: usize,
        cold_type: &'static str,
        started_type: &'static str,
    },
    Next {
        set: V,
        producer: &'static execution::Control,
    },
    Host {
        leaf: HostLeaf,
        arguments: Vec<V>,
    },
    Inline {
        handoff: bool,
        child: ProgramFrame,
        arguments: InvocationArguments,
    },
    Spawn {
        child: Invocation,
        arguments: Vec<V>,
        slot: usize,
        cold_type: &'static str,
        started_type: &'static str,
    },
    Join {
        value: V,
        producer: &'static execution::Control,
    },
    Cancel {
        value: V,
        producer: &'static execution::Control,
        slot: usize,
    },
}
impl RequestedInvocation {
    fn tag(&self) -> u32 {
        match self {
            Self::Inline { handoff: true, .. } => 3,
            Self::Inline { handoff: false, .. } => 5,
            Self::Spawn { .. } => 6,
            Self::Join { .. } => 7,
            Self::Cancel { .. } => 8,
            Self::Host { .. } => 9,
            Self::SpawnInto { .. } => 10,
            Self::Next { .. } => 11,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InvocationPhase {
    Cold,
    Active,
    Awaiting,
    Complete,
    Failed,
}
/// Owned ABI data attached to an existing runtime activation. Machine/Contexts
/// retain all invocation, domain, task, source scheduling and retirement authority.
/// Physical storage may be caller-owned or leased from the runtime arena; neither
/// form creates an independent task or fault-domain ownership root.
pub(crate) struct Invocation {
    program: ProgramFrame,
    state: Option<RetainedState>,
    phase: InvocationPhase,
    delivered: Vec<u64>,
    call: Option<ProgramFrame>,
    // Keep storage after retained values and pending children in drop order.
    storage: Option<physical_frames::Pin>,
    call_storage: Option<physical_frames::Pin>,
    arena: physical_frames::Arena,
}
impl std::fmt::Debug for Invocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Invocation")
            .field("control", &self.program.layout.control.name)
            .field("phase", &self.phase)
            .finish_non_exhaustive()
    }
}
impl Invocation {
    /// Safety: the callback interprets an all-zero physical frame with this layout.
    /// Storage remains pinned through every inline descendant and is recycled only
    /// after those invocations retire. This private entry does not schedule a task.
    unsafe fn in_arena(
        arena: &physical_frames::Arena,
        layout: &'static InvocationLayout,
    ) -> R<Self> {
        layout.validate()?;
        let storage = arena.allocate(layout.size, layout.align)?;
        let program = ProgramFrame {
            address: storage.address(),
            layout,
        };
        Ok(Self::with_storage(program, Some(storage), arena.clone()))
    }
    fn with_storage(
        program: ProgramFrame,
        storage: Option<physical_frames::Pin>,
        arena: physical_frames::Arena,
    ) -> Self {
        Self::at_in(program, storage, arena)
    }
    /// Safety: callbacks obey the ABI contract; physical storage must remain stable
    /// and exclusively controlled by this session until all invocations retire.
    pub(crate) unsafe fn new<T>(layout: &'static InvocationLayout, program: &mut T) -> R<Self> {
        unsafe {
            Self::buffer(
                layout,
                (program as *mut T).cast(),
                std::mem::size_of::<T>(),
                std::mem::align_of::<T>(),
            )
        }
    }
    /// Safety: the writable physical extent is exclusively owned and remains
    /// stable until its execution session retires. A generated entry owns it.
    pub(crate) unsafe fn buffer(
        layout: &'static InvocationLayout,
        address: *mut u8,
        size: usize,
        align: usize,
    ) -> R<Self> {
        layout.validate()?;
        if address.is_null()
            || size < layout.size
            || align < layout.align
            || !align.is_power_of_two()
            || address as usize % align != 0
        {
            return Err("E_NATIVE: ABI physical frame does not match its layout".into());
        }
        Ok(Self::at(ProgramFrame { address, layout }))
    }
    pub(crate) fn take_call(&mut self, control: &'static execution::Control) -> R<Self> {
        let child = self
            .call
            .as_ref()
            .ok_or("E_NATIVE: missing ABI run child")?;
        if self.phase != InvocationPhase::Awaiting || !std::ptr::eq(child.layout.control, control) {
            return Err("E_NATIVE: ABI run child identity or phase differs".into());
        }
        Ok(Self::with_storage(
            self.call.take().unwrap(),
            self.call_storage.take(),
            self.arena.clone(),
        ))
    }
    fn at(program: ProgramFrame) -> Self {
        Self::at_in(program, None, physical_frames::Arena::default())
    }
    fn at_in(
        program: ProgramFrame,
        storage: Option<physical_frames::Pin>,
        arena: physical_frames::Arena,
    ) -> Self {
        Self {
            program,
            state: Some(RetainedState {
                frame: None,
                destination: None,
                values: vec![],
                outcomes: vec![],
                value_base: 0,
                outcome_base: 0,
                error: None,
                result: None,
                lexical: vec![],
                next_lexical: 0,
            }),
            phase: InvocationPhase::Cold,
            delivered: vec![],
            call: None,
            storage,
            call_storage: None,
            arena,
        }
    }
    fn step(
        &mut self,
        host: &mut Host,
        values: &mut Frames,
        activation: &execution::Activation,
    ) -> R<execution::Step> {
        let frame = activation.abi_frame(host, values)?;
        if !std::ptr::eq(activation.abi_control(), self.program.layout.control)
            || !matches!(self.phase, InvocationPhase::Cold | InvocationPhase::Active)
        {
            return Err("E_NATIVE: retained ABI invocation identity or phase differs".into());
        }
        let cold = self.phase == InvocationPhase::Cold;
        let parameters = self.program.layout.control.parameters;
        if cold
            && parameters
                .iter()
                .any(|slot| values[frame.base() + slot].is_none())
        {
            return Err("E_NATIVE: missing admitted ABI argument".into());
        }
        let mut context = self.state.take().unwrap().select(host, values);
        context.selected_frame = Some(frame);
        context.selected_activation = Some(activation);
        context.program = Some(self.program);
        context.physical_arena = Some(&self.arena);
        let arguments = if cold {
            parameters
                .iter()
                .map(|slot| {
                    let value = context.storage.values_mut()[frame.base() + slot]
                        .take()
                        .unwrap();
                    context.put(value)
                })
                .collect::<Vec<_>>()
        } else {
            std::mem::take(&mut self.delivered)
        };
        let tag = unsafe {
            (self.program.layout.callback)(
                &mut context,
                self.program.address,
                arguments.as_ptr(),
                arguments.len(),
            )
        };
        let request = context.request.take();
        let mut result = if let Some(error) = &context.error {
            Err(error.clone())
        } else if !context.lexical.is_empty() {
            Err("E_NATIVE: retained ABI has unfinished lexical work".into())
        } else {
            match (
                tag,
                request.as_ref().map(RequestedInvocation::tag),
                context.result.is_some(),
            ) {
                (1, None, false) => Ok(execution::Step::Yield),
                (4, None, false) => Ok(execution::Step::Continue),
                (3, Some(3), false)
                | (5, Some(5), false)
                | (6, Some(6), false)
                | (7, Some(7), false)
                | (8, Some(8), false)
                | (9, Some(9), false)
                | (10, Some(10), false)
                | (11, Some(11), false) => Ok(execution::Step::Continue), // Queue after callback borrows end.
                (2, None, true) => match context.result.take().unwrap() {
                    value @ Completion::Outcome(_) => Ok(execution::Step::Complete(value)),
                    _ => Err("E_NATIVE: retained ABI requires a normal completion".into()),
                },
                _ => Err("E_NATIVE: malformed retained ABI step".into()),
            }
        };
        self.state = Some(RetainedState::park(context));
        self.phase = if result.is_err() {
            InvocationPhase::Failed
        } else if tag == 2 {
            InvocationPhase::Complete
        } else {
            InvocationPhase::Active
        };
        if result.is_ok()
            && let Some(request) = request
        {
            result = (|| match request {
                RequestedInvocation::SpawnInto {
                    child,
                    mut arguments,
                    set,
                    slot,
                    cold_type,
                    started_type,
                } => {
                    let producer = child.program.layout.control;
                    let mut cold =
                        Some(activation.defer_task(host, producer, cold_type, &mut arguments)?);
                    let mut task = activation.queue_started(host, producer, &mut cold)?;
                    task.abi = Some(child);
                    Ok(execution::Step::SpawnInto {
                        task,
                        set,
                        slot,
                        ty: started_type,
                    })
                }
                RequestedInvocation::Next { set, producer } => {
                    self.phase = InvocationPhase::Awaiting;
                    Ok(execution::Step::Next {
                        set,
                        producer,
                        deliver: deliver_invocation,
                    })
                }
                RequestedInvocation::Host { leaf, arguments } => {
                    self.phase = InvocationPhase::Awaiting;
                    Ok(execution::Step::Host {
                        operation: leaf.operation,
                        arguments,
                        deliver: deliver_host_invocation,
                        cancellable: leaf.cancellable,
                    })
                }
                RequestedInvocation::Spawn {
                    child,
                    mut arguments,
                    slot,
                    cold_type,
                    started_type,
                } => {
                    let producer = child.program.layout.control;
                    let mut cold =
                        Some(activation.defer_task(host, producer, cold_type, &mut arguments)?);
                    let mut task = activation.queue_started(host, producer, &mut cold)?;
                    task.abi = Some(child);
                    Ok(execution::Step::Spawn {
                        task,
                        slot,
                        ty: started_type,
                    })
                }
                RequestedInvocation::Cancel {
                    value,
                    producer,
                    slot,
                } => {
                    activation.cancel_tasks(host, producer, &value)?;
                    values[frame.base() + slot] = Some(value);
                    Ok(execution::Step::Cancel)
                }
                RequestedInvocation::Join { value, producer } => {
                    self.phase = InvocationPhase::Awaiting;
                    Ok(execution::Step::Join {
                        value,
                        producer,
                        deliver: deliver_invocation,
                    })
                }
                RequestedInvocation::Inline {
                    handoff,
                    child,
                    mut arguments,
                } => {
                    let storage = self
                        .storage
                        .as_ref()
                        .map(physical_frames::Pin::retain)
                        .transpose()?;
                    if handoff {
                        // Queue only after callback borrows end; existing task/domain
                        // authority controls admission and all subsequent handoffs.
                        let mut task =
                            match &mut arguments {
                                InvocationArguments::Direct(arguments) => activation
                                    .queue_invocation(host, child.layout.control, arguments)?,
                                InvocationArguments::Cold(value) => {
                                    activation.queue_task(host, child.layout.control, value)?
                                }
                            };
                        task.abi = Some(Self::with_storage(child, storage, self.arena.clone()));
                        self.phase = InvocationPhase::Awaiting;
                        Ok(execution::Step::Await {
                            task,
                            deliver: deliver_invocation,
                        })
                    } else {
                        match arguments {
                            InvocationArguments::Direct(arguments) => {
                                self.call = Some(child);
                                self.call_storage = storage;
                                self.phase = InvocationPhase::Awaiting;
                                Ok(execution::Step::Call {
                                    control: child.layout.control,
                                    arguments,
                                    destination: None,
                                    deliver: deliver_invocation,
                                })
                            }
                            InvocationArguments::Cold(_) => {
                                Err("E_NATIVE: cold task requires an await handoff".into())
                            }
                        }
                    }
                }
            })();
            if result.is_err() {
                self.phase = InvocationPhase::Failed;
            }
        }
        result
    }
}
/// Runtime trampoline; the selected Machine already owns context/domain authority.
pub fn resume_invocation(
    host: &mut Host,
    values: &mut Frames,
    activation: &mut execution::Activation,
) -> R<execution::Step> {
    activation.abi_frame(host, values)?;
    let mut invocation = activation
        .abi
        .take()
        .ok_or("E_NATIVE: missing ABI invocation state")?;
    let result = invocation.step(host, values, activation);
    activation.abi = Some(invocation);
    result
}
fn deliver_host_invocation(
    activation: &mut execution::Activation,
    values: &mut Frames,
    outcome: String,
    fields: Vec<V>,
) -> R<()> {
    deliver_invocation(activation, values, Completion::Outcome((outcome, fields)))
}
fn deliver_invocation(
    activation: &mut execution::Activation,
    values: &mut Frames,
    completion: Completion,
) -> R<()> {
    // Machine has checked the original child/invocation/domain and adopted owners.
    // Child frames have retired; host results have passed runtime publication.
    // No source tick or generated callback occurs here.
    values.frame_values(activation.abi_frame_for_delivery())?;
    let invocation = activation
        .abi
        .as_mut()
        .ok_or("E_NATIVE: missing ABI delivery state")?;
    if invocation.phase != InvocationPhase::Awaiting || !invocation.delivered.is_empty() {
        return Err("E_NATIVE: duplicate or unexpected ABI child delivery".into());
    }
    let state = invocation.state.as_mut().unwrap();
    let outcome = completion.outcome()?;
    let handle = state.outcome_base + state.outcomes.len() as u64 + 1;
    // Capacity was checked before the await; the suspended caller cannot allocate.
    state.outcomes.push(Some(outcome));
    invocation.delivered.push(handle);
    invocation.phase = InvocationPhase::Active;
    Ok(())
}
/// Request one direct by-value child. The descriptor fixes its offset/layout;
/// generated code cannot redirect delivery to an unrelated frame or invocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_await(
    ctx: *mut Context<'_>,
    child: usize,
    args: *const u64,
    count: usize,
) {
    unsafe { request_invocation(ctx, child, args, count, true) }
}
/// Run an inline compiled child on the current invocation stack. Only a child
/// yield/await hands off; the call itself uses the existing Step::Call policy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_run_child(
    ctx: *mut Context<'_>,
    child: usize,
    args: *const u64,
    count: usize,
) {
    unsafe { request_invocation(ctx, child, args, count, false) }
}
unsafe fn request_invocation(
    ctx: *mut Context<'_>,
    child: usize,
    args: *const u64,
    count: usize,
    handoff: bool,
) {
    unsafe { &mut *ctx }.perform(|c| {
        if c.request.is_some() || c.result.is_some() {
            return Err("E_NATIVE: duplicate ABI continuation request".into());
        }
        let program = c
            .program
            .ok_or("E_NATIVE: await requires a selected ABI invocation")?
            .child(child)?;
        if count != program.layout.control.parameters.len() {
            return Err("E_NATIVE: invalid ABI child arity".into());
        }
        c.require_outcome_room()?;
        let arguments = c.take_many(unsafe { slice(args, count) })?;
        c.request = Some(RequestedInvocation::Inline {
            handoff,
            child: program,
            arguments: InvocationArguments::Direct(arguments),
        });
        Ok(())
    })
}

/// Capture a task using its original selected invocation and registered producer.
/// This creates an obligation/value; it neither starts nor allocates a child frame.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_defer(
    ctx: *mut Context<'_>,
    producer: usize,
    ty: *const u8,
    len: usize,
    args: *const u64,
    count: usize,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let registry = COMPILED_PROGRAM
            .get()
            .ok_or("E_NATIVE: missing compiled module registry")?;
        let layout = registry.layout(producer)?;
        c.defer_control(layout.control, unsafe { text(ty, len)? }, unsafe {
            slice(args, count)
        })
    })
}
/// Consume the existing cold task at an explicitly selected await site. Validate
/// original host/owner/producer/wait and physical child before taking its handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_await_task(ctx: *mut Context<'_>, child: usize, value: u64) {
    unsafe { &mut *ctx }.perform(|c| {
        if c.request.is_some() || c.result.is_some() {
            return Err("E_NATIVE: duplicate ABI continuation request".into());
        }
        let program = c
            .program
            .ok_or("E_NATIVE: cold await requires a selected ABI invocation")?
            .child(child)?;
        let activation = c
            .selected_activation
            .ok_or("E_NATIVE: cold await requires an activation")?;
        c.require_outcome_room()?;
        activation.abi_validate_cold_task(c.host, program.layout.control, c.get(value)?)?;
        let value = c.take(value)?;
        c.request = Some(RequestedInvocation::Inline {
            handoff: true,
            child: program,
            arguments: InvocationArguments::Cold(Some(value)),
        });
        Ok(())
    });
}

impl Context<'_> {
    fn empty_task_set(
        &mut self,
        producer: &'static execution::Control,
        set_type: &str,
        started_type: &str,
    ) -> R<u64> {
        let activation = self
            .selected_activation
            .ok_or("E_NATIVE: task set requires a selected ABI invocation")?;
        if self.program.is_none() {
            return Err("E_NATIVE: task set requires an ABI program".into());
        }
        let value = V::Construct {
            ty: set_type.into(),
            name: "tasks".into(),
            fields: vec![],
        };
        activation.abi_validate_task_set(
            self.host,
            producer,
            set_type,
            started_type,
            &value,
            false,
        )?;
        self.value_base
            .checked_add(self.values.len() as u64)
            .and_then(|n| n.checked_add(1))
            .ok_or("E_NATIVE: ABI value handle space exhausted")?;
        Ok(self.put(value))
    }
    fn spawn_into_control(
        &mut self,
        layout: &'static InvocationLayout,
        cold_type: &'static str,
        started_type: &'static str,
        set_type: &str,
        slot: usize,
        set: u64,
        handles: &[u64],
    ) -> R<()> {
        if self.request.is_some() || self.result.is_some() {
            return Err("E_NATIVE: duplicate ABI continuation request".into());
        }
        let activation = self
            .selected_activation
            .ok_or("E_NATIVE: spawn requires a selected ABI invocation")?;
        if self.program.is_none()
            || self.storage.values()[self.local(slot)?].is_some()
            || handles.len() != layout.control.parameters.len()
        {
            return Err("E_NATIVE: invalid ABI set spawn destination or arity".into());
        }
        activation.abi_validate_task_set(
            self.host,
            layout.control,
            set_type,
            started_type,
            self.get(set)?,
            false,
        )?;
        let mut seen = BTreeSet::from([set]);
        for &handle in handles {
            self.get(handle)?;
            if !seen.insert(handle) {
                return Err("E_NATIVE: duplicate ABI value transfer".into());
            }
        }
        let arena = self
            .physical_arena
            .ok_or("E_NATIVE: missing physical frame arena")?;
        let child = unsafe { Invocation::in_arena(arena, layout)? };
        let arguments = self.take_many(handles)?;
        let set = self.take(set)?;
        self.request = Some(RequestedInvocation::SpawnInto {
            child,
            arguments,
            set,
            slot,
            cold_type,
            started_type,
        });
        Ok(())
    }
    fn next_control(
        &mut self,
        producer: &'static execution::Control,
        set_type: &str,
        started_type: &str,
        handle: u64,
    ) -> R<bool> {
        if self.request.is_some() || self.result.is_some() {
            return Err("E_NATIVE: duplicate ABI continuation request".into());
        }
        let activation = self
            .selected_activation
            .ok_or("E_NATIVE: next requires a selected ABI invocation")?;
        if self.program.is_none() {
            return Err("E_NATIVE: next requires an ABI program".into());
        }
        let empty = activation.abi_validate_task_set(
            self.host,
            producer,
            set_type,
            started_type,
            self.get(handle)?,
            true,
        )?;
        if empty {
            self.take(handle)?;
            return Ok(false);
        }
        self.require_outcome_room()?;
        let set = self.take(handle)?;
        self.request = Some(RequestedInvocation::Next { set, producer });
        Ok(true)
    }
    fn cancel_set_control(
        &mut self,
        producer: &'static execution::Control,
        set_type: &str,
        started_type: &str,
        slot: usize,
        handle: u64,
    ) -> R<()> {
        if self.request.is_some() || self.result.is_some() {
            return Err("E_NATIVE: duplicate ABI continuation request".into());
        }
        let activation = self
            .selected_activation
            .ok_or("E_NATIVE: cancellation requires a selected ABI invocation")?;
        if self.program.is_none() || self.storage.values()[self.local(slot)?].is_some() {
            return Err("E_NATIVE: invalid ABI cancellation destination".into());
        }
        activation.abi_validate_task_set(
            self.host,
            producer,
            set_type,
            started_type,
            self.get(handle)?,
            false,
        )?;
        let value = self.take(handle)?;
        self.request = Some(RequestedInvocation::Cancel {
            value,
            producer,
            slot,
        });
        Ok(())
    }
    fn host_control(
        &mut self,
        producer: &'static execution::Control,
        leaf: HostLeaf,
        handles: &[u64],
    ) -> R<()> {
        if self.request.is_some() || self.result.is_some() {
            return Err("E_NATIVE: duplicate ABI continuation request".into());
        }
        let selected = self
            .selected_activation
            .ok_or("E_NATIVE: host operation requires a selected ABI invocation")?;
        if !std::ptr::eq(selected.abi_control(), producer)
            || self
                .program
                .is_none_or(|p| !std::ptr::eq(p.layout.control, producer))
            || handles.len() != producer.parameters.len()
        {
            return Err("E_NATIVE: host operation producer or arity differs".into());
        }
        let declaration = self.host.declared_primitive(leaf.operation)?;
        if declaration["async"] != true
            || declaration["returns"] != true
            || declaration["parameters"].as_array().map(Vec::len) != Some(handles.len())
        {
            return Err("E_NATIVE: invalid async host operation descriptor".into());
        }
        self.require_outcome_room()?;
        let arguments = self.take_many(handles)?;
        self.request = Some(RequestedInvocation::Host { leaf, arguments });
        Ok(())
    }
    fn spawn_control(
        &mut self,
        layout: &'static InvocationLayout,
        cold_type: &'static str,
        started_type: &'static str,
        slot: usize,
        handles: &[u64],
    ) -> R<()> {
        if self.request.is_some() || self.result.is_some() {
            return Err("E_NATIVE: duplicate ABI continuation request".into());
        }
        if self.selected_activation.is_none() || self.program.is_none() {
            return Err("E_NATIVE: spawn requires a selected ABI invocation".into());
        }
        let address = self.local(slot)?;
        if self.storage.values()[address].is_some()
            || handles.len() != layout.control.parameters.len()
        {
            return Err("E_NATIVE: invalid ABI spawn slot or arity".into());
        }
        // Validate captures before allocating; admission errors leave handles intact.
        let mut seen = BTreeSet::new();
        for handle in handles {
            self.get(*handle)?;
            if !seen.insert(*handle) {
                return Err("E_NATIVE: duplicate ABI value transfer".into());
            }
        }
        let arena = self
            .physical_arena
            .ok_or("E_NATIVE: missing physical frame arena")?;
        let child = unsafe { Invocation::in_arena(arena, layout)? };
        let arguments = self.take_many(handles)?;
        self.request = Some(RequestedInvocation::Spawn {
            child,
            arguments,
            slot,
            cold_type,
            started_type,
        });
        Ok(())
    }
    fn cancel_control(
        &mut self,
        producer: &'static execution::Control,
        slot: usize,
        handle: u64,
    ) -> R<()> {
        if self.request.is_some() || self.result.is_some() {
            return Err("E_NATIVE: duplicate ABI continuation request".into());
        }
        let activation = self
            .selected_activation
            .ok_or("E_NATIVE: cancellation requires a selected ABI invocation")?;
        if self.program.is_none() || self.storage.values()[self.local(slot)?].is_some() {
            return Err("E_NATIVE: invalid ABI cancellation destination".into());
        }
        // A request allocates no completion wait and preserves the same task value.
        activation.abi_validate_cancellation(self.host, producer, self.get(handle)?)?;
        let value = self.take(handle)?;
        self.request = Some(RequestedInvocation::Cancel {
            value,
            producer,
            slot,
        });
        Ok(())
    }
    fn join_control(&mut self, producer: &'static execution::Control, handle: u64) -> R<()> {
        if self.request.is_some() || self.result.is_some() {
            return Err("E_NATIVE: duplicate ABI continuation request".into());
        }
        let activation = self
            .selected_activation
            .ok_or("E_NATIVE: join requires a selected ABI invocation")?;
        self.require_outcome_room()?;
        activation.abi_validate_started_task(self.host, producer, self.get(handle)?)?;
        let value = self.take(handle)?;
        self.request = Some(RequestedInvocation::Join { value, producer });
        Ok(())
    }
}
/// Queue an arena-owned child; the source driver immediately reselects its parent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_spawn(
    ctx: *mut Context<'_>,
    producer: usize,
    slot: usize,
    args: *const u64,
    count: usize,
) {
    unsafe { &mut *ctx }.perform(|c| {
        let registry = COMPILED_PROGRAM
            .get()
            .ok_or("E_NATIVE: missing compiled module registry")?;
        let layout = registry.layout(producer)?;
        if count != layout.control.parameters.len() {
            return Err("E_NATIVE: invalid ABI spawn arity".into());
        }
        let metadata = &registry.compiled[producer];
        c.spawn_control(
            layout,
            &metadata.cold_type,
            &metadata.started_type,
            slot,
            unsafe { slice(args, count) },
        )
    });
}
/// Consume the original source handle; existing claim/join delivery owns settlement.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_join(ctx: *mut Context<'_>, producer: usize, handle: u64) {
    unsafe { &mut *ctx }.perform(|c| {
        let registry = COMPILED_PROGRAM
            .get()
            .ok_or("E_NATIVE: missing compiled module registry")?;
        c.join_control(registry.layout(producer)?.control, handle)
    });
}

/// Request cancellation and restore the same started value to its checked local.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_cancel(
    ctx: *mut Context<'_>,
    producer: usize,
    slot: usize,
    handle: u64,
) {
    unsafe { &mut *ctx }.perform(|c| {
        let registry = COMPILED_PROGRAM
            .get()
            .ok_or("E_NATIVE: missing compiled module registry")?;
        c.cancel_control(registry.layout(producer)?.control, slot, handle)
    });
}

/// Suspend a generated host wrapper through the original runtime host-operation path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_host(
    ctx: *mut Context<'_>,
    producer: usize,
    args: *const u64,
    count: usize,
) {
    unsafe { &mut *ctx }.perform(|c| {
        let registry = COMPILED_PROGRAM
            .get()
            .ok_or("E_NATIVE: missing compiled module registry")?;
        let layout = registry.layout(producer)?;
        let leaf = registry.compiled[producer]
            .host
            .ok_or("E_NATIVE: compiled control is not an async host wrapper")?;
        if count != layout.control.parameters.len() {
            return Err("E_NATIVE: invalid ABI host argument count".into());
        }
        c.host_control(layout.control, leaf, unsafe { slice(args, count) })
    });
}

/// Construct an empty exact-producer set; this allocates no source task.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_task_set(ctx: *mut Context<'_>, producer: usize) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let registry = COMPILED_PROGRAM
            .get()
            .ok_or("E_NATIVE: missing compiled module registry")?;
        let control = registry.layout(producer)?.control;
        let metadata = &registry.compiled[producer];
        c.empty_task_set(control, &metadata.set_type, &metadata.started_type)
    })
}
/// Queue a member without a source handoff, preserving the original set value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_spawn_into(
    ctx: *mut Context<'_>,
    producer: usize,
    slot: usize,
    set: u64,
    args: *const u64,
    count: usize,
) {
    unsafe { &mut *ctx }.perform(|c| {
        let registry = COMPILED_PROGRAM
            .get()
            .ok_or("E_NATIVE: missing compiled module registry")?;
        let layout = registry.layout(producer)?;
        if count != layout.control.parameters.len() {
            return Err("E_NATIVE: invalid ABI set spawn arity".into());
        }
        let metadata = &registry.compiled[producer];
        c.spawn_into_control(
            layout,
            &metadata.cold_type,
            &metadata.started_type,
            &metadata.set_type,
            slot,
            set,
            unsafe { slice(args, count) },
        )
    });
}
/// Returns one for a parked Next request, zero for an immediate empty selection.
/// Callers check the sticky error before interpreting the empty return value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_next(ctx: *mut Context<'_>, producer: usize, set: u64) -> u8 {
    unsafe { &mut *ctx }.perform(|c| {
        let registry = COMPILED_PROGRAM
            .get()
            .ok_or("E_NATIVE: missing compiled module registry")?;
        let control = registry.layout(producer)?.control;
        let metadata = &registry.compiled[producer];
        c.next_control(control, &metadata.set_type, &metadata.started_type, set)
            .map(u8::from)
    })
}
/// Flag current members and restore the set; future insertions are independent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_cancel_set(
    ctx: *mut Context<'_>,
    producer: usize,
    slot: usize,
    set: u64,
) {
    unsafe { &mut *ctx }.perform(|c| {
        let registry = COMPILED_PROGRAM
            .get()
            .ok_or("E_NATIVE: missing compiled module registry")?;
        let control = registry.layout(producer)?.control;
        let metadata = &registry.compiled[producer];
        c.cancel_set_control(
            control,
            &metadata.set_type,
            &metadata.started_type,
            slot,
            set,
        )
    });
}

/// C-safe immutable descriptor emitted by the compiler. Every span points into
/// module-static storage. Rust Control/InvocationLayout values never cross C ABI.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CompiledControl {
    // Internal identity is distinct from the human-readable activation name.
    pub producer: *const u8,
    pub producer_len: usize,
    pub name: *const u8,
    pub name_len: usize,
    pub slots: usize,
    pub parameters: *const usize,
    pub parameter_count: usize,
    pub frame_size: usize,
    pub frame_align: usize,
    pub frame_prefix: usize,
    pub callback: Option<ResumeCallback>,
    pub children: *const CompiledChild,
    pub child_count: usize,
}
// Generated descriptors and their pointed-to spans are immutable module data.
// Constructing them is harmless; registration requires that lifetime contract.
unsafe impl Sync for CompiledControl {}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CompiledChild {
    pub control: usize,
    pub offset: usize,
}
struct CompiledLayout {
    host: Option<HostLeaf>,
    cold_type: String,
    started_type: String,
    set_type: String,
    callback: ResumeCallback,
    size: usize,
    align: usize,
    prefix: usize,
    children: Vec<CompiledChild>,
}
/// One generated module owns one process-lifetime registry. Repeated launch uses
/// the same immutable descriptors. OnceLock owns all allocations; no per-launch
/// leaked tables, forged static references, or user-program state lives here.
struct CompiledRegistry {
    identity: (usize, usize),
    initialized: std::sync::OnceLock<()>,
    controls: Vec<execution::Control>,
    compiled: Vec<CompiledLayout>,
    children: Vec<std::sync::OnceLock<Vec<InlineChild>>>,
    layouts: Vec<std::sync::OnceLock<InvocationLayout>>,
}
static COMPILED_PROGRAM: std::sync::OnceLock<CompiledRegistry> = std::sync::OnceLock::new();
impl CompiledRegistry {
    /// Safety: the table and all spans are readable, immutable module-static data.
    /// Callbacks obey ResumeCallback and interpret their declared physical frame.
    /// A registry belongs to a single module; the module cannot unload while used.
    unsafe fn register(
        registry: &'static std::sync::OnceLock<Self>,
        abi: u32,
        table: *const CompiledControl,
        count: usize,
    ) -> R<&'static Self> {
        if abi != 1
            || count == 0
            || count > 65536
            || table.is_null()
            || table as usize % std::mem::align_of::<CompiledControl>() != 0
        {
            return Err("E_NATIVE: incompatible compiled ABI control table".into());
        }
        let identity = (table as usize, count);
        if let Some(registered) = registry.get() {
            return if registered.identity == identity {
                registered.initialize();
                Ok(registered)
            } else {
                Err("E_NATIVE: compiled ABI module identity differs".into())
            };
        }
        let table = unsafe { slice(table, count) };
        let mut controls = Vec::with_capacity(count);
        let mut compiled = Vec::with_capacity(count);
        for raw in table {
            if raw.producer_len == 0
                || raw.producer.is_null()
                || raw.producer_len > isize::MAX as usize
                || raw.name_len == 0
                || raw.name.is_null()
                || raw.name_len > isize::MAX as usize
                || raw.slots > isize::MAX as usize / std::mem::size_of::<Option<V>>()
                || raw.parameter_count > raw.slots
                || (raw.parameter_count != 0
                    && (raw.parameters.is_null()
                        || raw.parameters as usize % std::mem::align_of::<usize>() != 0))
                || raw.frame_size == 0
                || raw.frame_size > isize::MAX as usize
                || !raw.frame_align.is_power_of_two()
                || raw.frame_align > raw.frame_size
                || raw.frame_size % raw.frame_align != 0
                || raw.frame_prefix > raw.frame_size
                || raw.child_count > 65536
                || (raw.child_count != 0
                    && (raw.children.is_null()
                        || raw.children as usize % std::mem::align_of::<CompiledChild>() != 0))
            {
                return Err("E_NATIVE: invalid compiled ABI descriptor".into());
            }
            let callback = raw
                .callback
                .ok_or("E_NATIVE: missing compiled ABI callback")?;
            let name = unsafe { text(raw.name, raw.name_len)? };
            let producer: &'static str = unsafe { text(raw.producer, raw.producer_len)? };
            let host = producer
                .strip_suffix("$host$cancel")
                .map(|operation| HostLeaf {
                    operation,
                    cancellable: true,
                })
                .or_else(|| {
                    producer.strip_suffix("$host").map(|operation| HostLeaf {
                        operation,
                        cancellable: false,
                    })
                });
            if host.is_some_and(|leaf| leaf.operation.is_empty()) {
                return Err("E_NATIVE: empty compiled host operation".into());
            }
            let parameters: &'static [usize] =
                unsafe { slice(raw.parameters, raw.parameter_count) };
            if parameters
                .iter()
                .enumerate()
                .any(|(i, slot)| *slot >= raw.slots || parameters[..i].contains(slot))
            {
                return Err("E_NATIVE: invalid compiled ABI parameter layout".into());
            }
            let children = unsafe { slice(raw.children, raw.child_count) }.to_vec();
            controls.push(execution::Control {
                name,
                slots: raw.slots,
                atoms: 0,
                parameters,
                retained: &[],
                constructor: false,
                access: None,
                resume: resume_invocation,
            });
            compiled.push(CompiledLayout {
                host,
                cold_type: format!("$task<{producer}>"),
                started_type: format!("$started<{producer}>"),
                set_type: format!("$tasks<{producer}>"),
                callback,
                size: raw.frame_size,
                align: raw.frame_align,
                prefix: raw.frame_prefix,
                children,
            });
        }
        // Check every edge and overlap before publishing any runtime identity.
        // Each child has positive size. A finite by-value graph cannot be cyclic.
        for parent in &compiled {
            for (i, child) in parent.children.iter().enumerate() {
                let target = compiled
                    .get(child.control)
                    .ok_or("E_NATIVE: unknown compiled ABI child control")?;
                let end = child
                    .offset
                    .checked_add(target.size)
                    .ok_or("E_NATIVE: compiled ABI child extent overflow")?;
                if child.offset < parent.prefix
                    || end > parent.size
                    || target.align > parent.align
                    || child.offset % target.align != 0
                    || parent.children[..i].iter().any(|other| {
                        let other_size = compiled[other.control].size;
                        child.offset < other.offset.saturating_add(other_size) && other.offset < end
                    })
                {
                    return Err("E_NATIVE: invalid compiled ABI child extent".into());
                }
            }
        }
        // Iterative topological validation avoids recursive stack growth from
        // untrusted counts, including an equal-size/self-embedding zero prefix.
        let mut incoming = vec![0usize; count];
        for parent in &compiled {
            for child in &parent.children {
                incoming[child.control] += 1;
            }
        }
        let mut ready = incoming
            .iter()
            .enumerate()
            .filter_map(|(i, n)| (*n == 0).then_some(i))
            .collect::<Vec<_>>();
        let mut visited = 0;
        while let Some(index) = ready.pop() {
            visited += 1;
            for child in &compiled[index].children {
                incoming[child.control] -= 1;
                if incoming[child.control] == 0 {
                    ready.push(child.control);
                }
            }
        }
        if visited != count {
            return Err("E_NATIVE: cyclic compiled ABI by-value frames".into());
        }
        let candidate = Self {
            identity,
            initialized: std::sync::OnceLock::new(),
            controls,
            compiled,
            children: (0..count).map(|_| std::sync::OnceLock::new()).collect(),
            layouts: (0..count).map(|_| std::sync::OnceLock::new()).collect(),
        };
        // A racing registration may win. Its owned metadata remains authoritative;
        // the losing candidate is dropped normally, with no leaked allocation.
        let _ = registry.set(candidate);
        let result = registry.get().unwrap();
        if result.identity != identity {
            return Err("E_NATIVE: compiled ABI module identity differs".into());
        }
        result.initialize();
        Ok(result)
    }
    fn initialize(&'static self) {
        self.initialized.get_or_init(|| self.initialize_once());
    }
    fn initialize_once(&'static self) {
        // Bottom-up construction borrows actual process-lifetime owned cells.
        // No transmute or temporary reference is needed for Rust's static tables.
        let mut pending = vec![0usize; self.compiled.len()];
        let mut parents = vec![vec![]; self.compiled.len()];
        for (index, layout) in self.compiled.iter().enumerate() {
            pending[index] = layout.children.len();
            for child in &layout.children {
                parents[child.control].push(index);
            }
        }
        let mut ready = pending
            .iter()
            .enumerate()
            .filter_map(|(i, n)| (*n == 0).then_some(i))
            .collect::<Vec<_>>();
        while let Some(index) = ready.pop() {
            let compiled = &self.compiled[index];
            let children = self.children[index].get_or_init(|| {
                compiled
                    .children
                    .iter()
                    .map(|child| InlineChild {
                        offset: child.offset,
                        layout: self.layouts[child.control].get().unwrap(),
                    })
                    .collect()
            });
            self.layouts[index].get_or_init(|| InvocationLayout {
                control: &self.controls[index],
                callback: compiled.callback,
                size: compiled.size,
                align: compiled.align,
                prefix: compiled.prefix,
                children,
            });
            for parent in &parents[index] {
                pending[*parent] -= 1;
                if pending[*parent] == 0 {
                    ready.push(*parent);
                }
            }
        }
    }
    fn layout(&'static self, index: usize) -> R<&'static InvocationLayout> {
        self.layouts
            .get(index)
            .and_then(std::sync::OnceLock::get)
            .ok_or_else(|| "E_NATIVE: unknown compiled ABI control".into())
    }
}

/// Run a generated entry's typed frame through the ordinary source driver.
/// Safety: descriptors are immutable module-static data; program storage is
/// writable, aligned and exclusively owned for the entire call. No pointer to
/// program storage survives this function, even when execution fails.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_run_entry(
    ctx: *mut Context<'_>,
    table: *const CompiledControl,
    controls: usize,
    index: usize,
    program: *mut u8,
    size: usize,
    align: usize,
    args: *const u64,
    count: usize,
    quota: usize,
) {
    unsafe { &mut *ctx }.perform(|c| {
        if quota == 0 {
            return Err("E_NATIVE: compiled entry requires a positive host quota".into());
        }
        if c.frame.is_some()
            || c.selected_frame.is_some()
            || c.program.is_some()
            || c.result.is_some()
            || c.request.is_some()
            || !c.lexical.is_empty()
        {
            return Err("E_NATIVE: compiled entry requires an unused launch context".into());
        }
        let registry =
            unsafe { CompiledRegistry::register(&COMPILED_PROGRAM, 1, table, controls)? };
        let layout = registry.layout(index)?;
        // Validate physical storage and argument count before consuming handles.
        let invocation = unsafe { Invocation::buffer(layout, program, size, align)? };
        if count != layout.control.parameters.len() {
            return Err("E_NATIVE: invalid compiled entry arity".into());
        }
        let args = c.take_many(unsafe { slice(args, count) })?;
        let result =
            unsafe { execution::run_abi_invocation(c.host, args, layout, invocation, quota)? };
        c.result = Some(result);
        Ok(())
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_invalid_resume(ctx: *mut Context<'_>) {
    unsafe { &mut *ctx }.perform::<()>(|_| Err("E_NATIVE: invalid compiled resume state".into()));
}

pub type Callback = unsafe extern "C" fn(*mut Context<'_>, *const u64, usize);

/// Collection operations use the same stable arena and element-reference
/// evidence as generated Rust. Check the entire handle batch before mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_collection(
    ctx: *mut Context<'_>,
    name: *const u8,
    len: usize,
    args: *const u64,
    count: usize,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        c.require_outcome_room()?;
        let name = unsafe { text(name, len)? };
        let args = c.take_many(unsafe { slice(args, count) })?;
        let outcome = collection(name, args, c.storage.values_mut())?;
        Ok(c.put_outcome(outcome))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_length(ctx: *mut Context<'_>, handle: u64) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let value = c.take(handle)?;
        let length = collection_length(value, c.storage.values_mut())?;
        Ok(c.put(length))
    })
}

/// Synchronous host calls retain the existing runtime's live/scripted
/// dispatch, tracing and failures. Borrowed parameters are already lowered to
/// checked value reads; consuming this temporary handle does not consume the
/// source owner. They do not create a control activation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_primitive(
    ctx: *mut Context<'_>,
    name: *const u8,
    len: usize,
    args: *const u64,
    count: usize,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        c.require_outcome_room()?;
        let name = unsafe { text(name, len)? };
        let declaration = c.host.declared_primitive(name)?;
        let parameters = declaration["parameters"]
            .as_array()
            .ok_or("E_NATIVE: invalid ABI primitive parameters")?;
        if declaration["async"] == true
            || declaration["rust"].is_object()
            || declaration["returns"] != true
        {
            return Err("E_NATIVE: primitive requires unsupported ABI invocation mode".into());
        }
        if parameters.len() != count {
            return Err("E_NATIVE: ABI primitive argument count mismatch".into());
        }
        let handles = unsafe { slice(args, count) };
        for (&handle, parameter) in handles.iter().zip(parameters) {
            let ty = parameter["type"]
                .as_str()
                .ok_or("E_NATIVE: invalid ABI parameter type")?;
            validate(c.get(handle)?, ty, &c.host.manifest)?;
        }
        let arguments = c.take_many(handles)?;
        let outcome = c.host.primitive(name, arguments)?;
        Ok(c.put_outcome(outcome))
    })
}

/// A synchronous child borrows the Host only while its compiled callback runs.
/// Failed callbacks retain their activation chain for the launcher's fault report.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_call(
    ctx: *mut Context<'_>,
    callback: Callback,
    name: *const u8,
    len: usize,
    args: *const u64,
    count: usize,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        c.require_outcome_room()?;
        let name = unsafe { text(name, len)? };
        let values = c.take_many(unsafe { slice(args, count) })?;
        let outcome = {
            c.host.enter(name)?;
            let mut child = Context {
                selected_frame: None,
                selected_activation: None,
                program: None,
                physical_arena: None,
                request: None,
                host: &mut *c.host,
                storage: FrameStorage::Borrowed(c.storage.values_mut()),
                frame: None,
                destination: None,
                values: vec![],
                outcomes: vec![],
                value_base: 0,
                outcome_base: 0,
                error: None,
                result: None,
                lexical: vec![],
                next_lexical: 0,
            };
            let arguments: Vec<u64> = values.into_iter().map(|v| child.put(v)).collect();
            unsafe { callback(&mut child, arguments.as_ptr(), arguments.len()) };
            let result = child.finish()?.outcome()?;
            child.host.leave();
            result
        };
        Ok(c.put_outcome(outcome))
    })
}

/// Constructor calls bypass outcome packaging and deliver into a checked caller
/// local. Destination and the whole argument batch are checked before transfer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_construct_call(
    ctx: *mut Context<'_>,
    callback: Callback,
    name: *const u8,
    len: usize,
    args: *const u64,
    count: usize,
    slot: usize,
) {
    unsafe { &mut *ctx }.perform(|c| {
        let frame = c.frame_ref()?;
        let destination = c.storage.values().local_storage(frame, slot)?;
        let root = c.storage.values().require_storage(&destination)?;
        if c.storage.values()[root].is_some() {
            return Err("E_NATIVE: occupied constructor destination".into());
        }
        let name = unsafe { text(name, len)? };
        let values = c.take_many(unsafe { slice(args, count) })?;
        c.host.enter(name)?;
        let mut child = Context {
            selected_frame: None,
            selected_activation: None,
            program: None,
            physical_arena: None,
            request: None,
            host: &mut *c.host,
            storage: FrameStorage::Borrowed(c.storage.values_mut()),
            frame: None,
            destination: Some(destination.clone()),
            values: vec![],
            outcomes: vec![],
            value_base: 0,
            outcome_base: 0,
            error: None,
            result: None,
            lexical: vec![],
            next_lexical: 0,
        };
        let arguments: Vec<u64> = values.into_iter().map(|v| child.put(v)).collect();
        unsafe { callback(&mut child, arguments.as_ptr(), arguments.len()) };
        if let Err(error) = child.finish().and_then(Completion::constructed) {
            // A malformed callback might fail after delivery. Revoke that value,
            // retain the original failure and leave the activation chain intact.
            if let Ok(root) = child.storage.values().require_storage(&destination) {
                child.storage.values_mut()[root] = None;
            }
            return Err(error);
        }
        child.host.leave();
        Ok(())
    })
}

/// Enter a lexical scope over the current frame. The packed layout contains
/// retained, abandoned, work-local and handler-local indices, in that order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_lexical_begin(
    ctx: *mut Context<'_>,
    name: *const u8,
    len: usize,
    layout: *const usize,
    retained: usize,
    retire: usize,
    work: usize,
    handler: usize,
    fault: usize,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        if c.program.is_some() {
            return Err("E_NATIVE: retained ABI lexical boundaries are not admitted".into());
        }
        let name = unsafe { text(name, len)? };
        if c.result.is_some() {
            return Err("E_NATIVE: lexical boundary after completion".into());
        }
        c.retire_temporaries()?;
        let id = c
            .next_lexical
            .checked_add(1)
            .ok_or("E_NATIVE: ABI boundary identity exhausted")?;
        let count = [retained, retire, work, handler]
            .into_iter()
            .try_fold(0usize, |a, b| a.checked_add(b))
            .ok_or("E_NATIVE: ABI boundary layout overflow")?;
        let layout = unsafe { slice(layout, count) };
        let roots = layout
            .iter()
            .map(|id| c.local(*id))
            .collect::<R<Vec<_>>>()?;
        let fault = c.local(fault)?;
        let (retained, rest) = roots.split_at(retained);
        let (retire, rest) = rest.split_at(retire);
        let (work, handler) = rest.split_at(work);
        for group in [retained, retire, work, handler] {
            if group.iter().collect::<BTreeSet<_>>().len() != group.len() {
                return Err("E_NATIVE: duplicate ABI lexical slot".into());
            }
        }
        if retained
            .iter()
            .any(|id| retire.contains(id) || work.contains(id) || handler.contains(id))
            || !handler.contains(&fault)
        {
            return Err("E_NATIVE: invalid ABI lexical slot roles".into());
        }
        let state = c.host.begin_boundary(
            name,
            retained
                .iter()
                .chain(retire)
                .filter_map(|id| c.storage.values()[*id].as_ref()),
            true,
        )?;
        c.lexical.push(LexicalBoundary {
            id,
            state,
            retained: retained.to_vec(),
            retire: retire.to_vec(),
            cleanup: work.iter().chain(handler).copied().collect(),
            fault,
        });
        c.next_lexical = id;
        Ok(id)
    })
}
/// This is an explicit error transfer; ordinary ABI effects remain sticky.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_lexical_catch(ctx: *mut Context<'_>, id: u64) -> u32 {
    let c = unsafe { &mut *ctx };
    match c.catch_lexical(id) {
        Ok(handled) => u32::from(handled),
        Err(error) => {
            c.error = Some(error);
            0
        }
    }
}
/// Finalization must run even on the error channel, without re-entering cleanup.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_lexical_end(ctx: *mut Context<'_>, id: u64) {
    let c = unsafe { &mut *ctx };
    if let Err(error) = c.end_lexical(id) {
        c.error.get_or_insert(error);
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_process(ctx: *mut Context<'_>, status: u32) {
    unsafe { &mut *ctx }.perform(|c| {
        if c.result.is_some() || c.destination.is_some() || status > 255 {
            return Err("E_NATIVE: invalid ABI process completion".into());
        }
        c.result = Some(Completion::Process(status as u8));
        Ok(())
    })
}

/// Administrative completion is distinct from an ordinary outcome or constructor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_settled(ctx: *mut Context<'_>) {
    unsafe { &mut *ctx }.perform(|c| {
        if c.result.is_some() || c.destination.is_some() {
            return Err("E_NATIVE: invalid ABI settlement".into());
        }
        c.result = Some(Completion::Settled);
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_escalate(ctx: *mut Context<'_>) {
    unsafe { &mut *ctx }.perform::<()>(|c| c.host.escalate())
}

/// Named synchronous isolation uses the existing host lifecycle. No outcome or
/// fault value is returned to the supervisor. Only a source fault enters cleanup;
/// fuel, driver and internal errors retain their original error channel.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_supervise(
    ctx: *mut Context<'_>,
    work: Callback,
    handler: Callback,
    name: *const u8,
    len: usize,
    handler_name: *const u8,
    handler_len: usize,
    args: *const u64,
    count: usize,
    retained: *const usize,
    retained_count: usize,
) {
    unsafe { &mut *ctx }.perform(|c| {
        let name = unsafe { text(name, len)? };
        let handler_name = unsafe { text(handler_name, handler_len)? };
        let retained_slots = unsafe { slice(retained, retained_count) };
        if retained_count > count
            || retained_slots.iter().collect::<BTreeSet<_>>().len() != retained_count
        {
            return Err("E_NATIVE: invalid ABI boundary retained set".into());
        }
        let values = c.take_many(unsafe { slice(args, count) })?;
        let mut state = c.host.begin_boundary(name, &values, false)?;
        let mut retained = vec![];
        let attempted = c.boundary_call(work, name, values, retained_slots, &mut retained);
        let result = (|| match attempted? {
            Err(message) if c.host.source_faulted() => {
                let keep = c.host.resources.owned_tokens(&retained)?;
                c.host.boundary_fault(&mut state, &message, &keep)?;
                retained.push(V::String(
                    message.strip_prefix("E_FAULT: ").unwrap_or(&message).into(),
                ));
                c.host.enter_contingency(&mut state)?;
                c.boundary_call(handler, handler_name, retained, &[], &mut vec![])?
                    .and_then(Completion::settled)
            }
            other => other.and_then(Completion::settled),
        })();
        // Finalize once on both channels, including errors while entering cleanup.
        c.host.finish_boundary(&mut state, &result)?;
        result
    })
}

/// A nonmatching arm does not consume the outcome. A matching arm validates its
/// entire shape before consuming it or publishing any payload handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_route(
    ctx: *mut Context<'_>,
    outcome: u64,
    name: *const u8,
    len: usize,
    payload: *mut u64,
    count: usize,
) -> i32 {
    unsafe { &mut *ctx }.perform(|c| {
        let expected = unsafe { text(name, len)? };
        let (name, fields) = c.outcome(outcome)?;
        if name != expected {
            return Ok(0);
        }
        if fields.len() != count {
            return Err("E_NATIVE: ABI outcome payload count mismatch".into());
        }
        c.value_base
            .checked_add(c.values.len() as u64)
            .and_then(|n| n.checked_add(count as u64))
            .ok_or("E_NATIVE: ABI value handle space exhausted")?;
        let (_, fields) = c.outcomes[(outcome - c.outcome_base - 1) as usize]
            .take()
            .unwrap();
        for (i, value) in fields.into_iter().enumerate() {
            unsafe { payload.add(i).write(c.put(value)) };
        }
        Ok(1)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_unmatched(ctx: *mut Context<'_>, outcome: u64) {
    unsafe { &mut *ctx }.perform::<()>(|c| {
        c.outcome(outcome)?;
        Err("E_NATIVE: unmatched declared outcome".into())
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryInfo {
    name: String,
    display: String,
    arguments: Vec<String>,
    exits: Vec<(String, u8)>,
    #[serde(default)]
    retained: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramInfo {
    abi: u32,
    manifest: String,
    entry: String,
    sites: String,
    entries: Vec<EntryInfo>,
}
unsafe fn launch_program(
    data: *const u8,
    len: usize,
    callbacks: *const Callback,
    count: usize,
) -> R<u8> {
    let info: ProgramInfo = serde_json::from_str(unsafe { text(data, len)? })
        .map_err(|e| format!("E_NATIVE: invalid ABI program: {e}"))?;
    if info.abi != 1 || info.entries.len() != count {
        return Err("E_NATIVE: incompatible ABI program table".into());
    }
    let callbacks = unsafe { slice(callbacks, count) };
    let codes: Vec<Vec<(&str, u8)>> = info
        .entries
        .iter()
        .map(|e| e.exits.iter().map(|(n, c)| (n.as_str(), *c)).collect())
        .collect();
    let entries: Vec<Entry<'_>> = info
        .entries
        .iter()
        .zip(&codes)
        .map(|(e, c)| Entry {
            name: &e.name,
            display_name: &e.display,
            arguments: e.arguments.clone(),
            exit_codes: c,
            run: |_, _| Err("E_NATIVE: ABI entry requires its compiled callback".into()),
        })
        .collect();
    launch_with(
        &info.manifest,
        &info.entry,
        &entries,
        None,
        None,
        || Ok(None),
        &info.sites,
        |entry, host, args| {
            let index = entries.iter().position(|e| std::ptr::eq(e, entry)).unwrap();
            let retained = info.entries[index].retained;
            if !retained {
                host.enter(entry.display_name)?;
            }
            let mut context = Context {
                selected_frame: None,
                selected_activation: None,
                program: None,
                physical_arena: None,
                request: None,
                host,
                storage: FrameStorage::Owned(Frames::default()),
                frame: None,
                destination: None,
                values: vec![],
                outcomes: vec![],
                value_base: 0,
                outcome_base: 0,
                error: None,
                result: None,
                lexical: vec![],
                next_lexical: 0,
            };
            let args: Vec<u64> = args.into_iter().map(|v| context.put(v)).collect();
            unsafe { callbacks[index](&mut context, args.as_ptr(), args.len()) };
            let result = context.finish()?;
            if !retained {
                context.host.leave();
            }
            Ok(result)
        },
    )
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_launch(
    data: *const u8,
    len: usize,
    callbacks: *const Callback,
    count: usize,
) -> i32 {
    match unsafe { launch_program(data, len, callbacks, count) } {
        Ok(code) => code.into(),
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}
