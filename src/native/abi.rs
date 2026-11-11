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
#[path = "abi_resume_tests.rs"]
mod resume_tests;
#[cfg(test)]
#[path = "abi_tests.rs"]
mod tests;

pub struct Context<'a> {
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
    fn local(&self, slot: usize) -> R<usize> {
        let frame = self.frame.as_ref().ok_or("E_NATIVE: missing ABI frame")?;
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
        if c.frame.is_some() {
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
        let frame = c.frame.as_ref().ok_or("E_NATIVE: missing ABI frame")?;
        let destination = c.storage.values().local_storage(frame, slot)?;
        let root = c.storage.values().require_storage(&destination)?;
        if c.storage.values()[root].is_some() {
            return Err("E_NATIVE: occupied constructor destination".into());
        }
        let name = unsafe { text(name, len)? };
        let values = c.take_many(unsafe { slice(args, count) })?;
        c.host.enter(name)?;
        let mut child = Context {
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
            host.enter(entry.display_name)?;
            let mut context = Context {
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
            context.host.leave();
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
