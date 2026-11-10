//! Private versioned C ABI for checked, synchronous LLVM programs.
//!
//! Only integers, byte spans, opaque context pointers and function pointers cross
//! this boundary. Rust values never do. Handles belong to one invocation, are
//! consumed once, and are never reused. Zero is invalid. The first failure is
//! sticky: later calls cannot perform effects or replace its diagnostic.
//!
//! Safety: callers are compiler-generated code. Context pointers must refer to
//! the active invocation; spans/arrays must be readable for their stated length
//! and may be null only when empty. Callbacks must not retain these pointers.
use super::*;
#[cfg(test)]
#[path = "abi_tests.rs"]
mod tests;

pub struct Context<'a> {
    host: &'a mut Host,
    values: Vec<Option<V>>,
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
