//! Run a compiled program with the pouch spawn interface and inherit its exit status.
use std::{
    ffi::{CString, c_char, c_int, c_void},
    io,
    os::unix::{ffi::OsStrExt, process::ExitStatusExt},
    path::Path,
    process::ExitStatus,
    ptr,
};

unsafe extern "C" {
    fn posix_spawn(
        pid: *mut c_int,
        path: *const c_char,
        actions: *const c_void,
        attributes: *const c_void,
        argv: *const *const c_char,
        envp: *const *const c_char,
    ) -> c_int;
    fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
}

pub(super) fn run(path: &Path, args: &[String]) -> io::Result<ExitStatus> {
    let path = CString::new(path.as_os_str().as_bytes())?;
    let arguments: Vec<_> = args
        .iter()
        .map(|arg| CString::new(arg.as_bytes()))
        .collect::<Result<_, _>>()?;
    let environment: Vec<_> = std::env::vars_os()
        .map(|(mut key, value)| {
            key.push("=");
            key.push(value);
            CString::new(key.as_os_str().as_bytes())
        })
        .collect::<Result<_, _>>()?;
    let argv: Vec<_> = std::iter::once(path.as_ptr())
        .chain(arguments.iter().map(|s| s.as_ptr()))
        .chain(std::iter::once(ptr::null()))
        .collect();
    let envp: Vec<_> = environment
        .iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(ptr::null()))
        .collect();

    let mut pid = 0;
    // Both pointer arrays and their C strings live through the spawn call.
    let err = unsafe {
        posix_spawn(
            &mut pid,
            path.as_ptr(),
            ptr::null(),
            ptr::null(),
            argv.as_ptr(),
            envp.as_ptr(),
        )
    };
    if err != 0 {
        return Err(io::Error::from_raw_os_error(err));
    }
    loop {
        let mut status = 0;
        if unsafe { waitpid(pid, &mut status, 0) } == pid {
            return Ok(ExitStatus::from_raw(status));
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}
