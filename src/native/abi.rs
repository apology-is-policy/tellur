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
#[path = "abi_tests.rs"]
mod tests;

pub struct Context<'a> {
    host: &'a mut Host,
    values: Vec<Option<V>>,
    outcomes: Vec<Option<(String, Vec<V>)>>,
    error: Option<String>,
    result: Option<Completion>,
}
impl Context<'_> {
    fn put(&mut self, value: V) -> u64 {
        self.values.push(Some(value));
        self.values.len() as u64
    }
    fn get(&self, handle: u64) -> R<&V> {
        let index = usize::try_from(handle).ok().and_then(|n| n.checked_sub(1));
        index
            .and_then(|i| self.values.get(i))
            .and_then(Option::as_ref)
            .ok_or_else(|| "E_NATIVE: invalid or consumed ABI value handle".into())
    }
    fn outcome(&self, handle: u64) -> R<&(String, Vec<V>)> {
        usize::try_from(handle)
            .ok()
            .and_then(|n| n.checked_sub(1))
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
            .map(|&h| self.values[h as usize - 1].take().unwrap())
            .collect())
    }
    fn take(&mut self, handle: u64) -> R<V> {
        Ok(self.take_many(&[handle])?.pop().unwrap())
    }
    fn construct(&mut self, ty: &str, name: &str, handles: &[u64]) -> R<u64> {
        let expected = fields(&self.host.manifest, ty, name)
            .map_err(|e| format!("E_NATIVE: invalid ABI constructor: {e}"))?;
        if expected.len() != handles.len() {
            return Err("E_NATIVE: ABI constructor field count mismatch".into());
        }
        for (&handle, ty) in handles.iter().zip(&expected) {
            validate(self.get(handle)?, ty, &self.host.manifest)
                .map_err(|e| format!("E_NATIVE: invalid ABI constructor payload: {e}"))?;
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
        let value = c.get(handle)?;
        if !matches!(value, V::Bool(_) | V::Construct { .. }) {
            return Err("E_NATIVE: invalid ABI decision value".into());
        }
        validate(value, value.ty(), &c.host.manifest)
            .map_err(|e| format!("E_NATIVE: invalid ABI decision payload: {e}"))?;
        let outcome = variant(c.take(handle)?)?;
        c.outcomes.push(Some(outcome));
        Ok(c.outcomes.len() as u64)
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
        c.get(handle)?;
        let path = unsafe { slice(path, count) };
        let root = handle as usize - 1;
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
        c.get(handle)?;
        c.get(value)?;
        if handle == value {
            return Err("E_NATIVE: aliased ABI assignment transfer".into());
        }
        let root = handle as usize - 1;
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
        c.get(handle)?;
        mutable_place(&mut c.values, handle as usize - 1, &[index])?;
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
        let name = unsafe { text(name, len)? }.to_owned();
        let args = c.take_many(unsafe { slice(args, count) })?;
        c.result = Some(Completion::Outcome((name, args)));
        Ok(())
    })
}

pub type Callback = unsafe extern "C" fn(*mut Context<'_>, *const u64, usize);

/// Synchronous owned host calls retain the existing runtime's live/scripted
/// dispatch, tracing and failures. They do not create a control activation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tellur_rt_v1_primitive(
    ctx: *mut Context<'_>,
    name: *const u8,
    len: usize,
    args: *const u64,
    count: usize,
) -> u64 {
    unsafe { &mut *ctx }.perform(|c| {
        let name = unsafe { text(name, len)? };
        let declaration = c.host.declared_primitive(name)?;
        let parameters = declaration["parameters"]
            .as_array()
            .ok_or("E_NATIVE: invalid ABI primitive parameters")?;
        if declaration["async"] == true
            || declaration["rust"].is_object()
            || declaration["returns"] != true
            || parameters.iter().any(|p| p["mode"] == "borrow")
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
        c.outcomes.push(Some(outcome));
        Ok(c.outcomes.len() as u64)
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
        let name = unsafe { text(name, len)? };
        let values = c.take_many(unsafe { slice(args, count) })?;
        let outcome = {
            c.host.enter(name)?;
            let mut child = Context {
                host: &mut *c.host,
                values: vec![],
                outcomes: vec![],
                error: None,
                result: None,
            };
            let arguments: Vec<u64> = values.into_iter().map(|v| child.put(v)).collect();
            unsafe { callback(&mut child, arguments.as_ptr(), arguments.len()) };
            if let Some(error) = child.error.take() {
                return Err(error);
            }
            let result = child
                .result
                .take()
                .ok_or("E_NATIVE: compiled ABI control fell through")?
                .outcome()?;
            child.host.leave();
            result
        };
        c.outcomes.push(Some(outcome));
        Ok(c.outcomes.len() as u64)
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
        let (_, fields) = c.outcomes[outcome as usize - 1].take().unwrap();
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
                values: vec![],
                outcomes: vec![],
                error: None,
                result: None,
            };
            let args: Vec<u64> = args.into_iter().map(|v| context.put(v)).collect();
            unsafe { callbacks[index](&mut context, args.as_ptr(), args.len()) };
            if let Some(error) = context.error {
                return Err(error);
            }
            let result = context
                .result
                .ok_or("E_NATIVE: compiled ABI control fell through")?;
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
