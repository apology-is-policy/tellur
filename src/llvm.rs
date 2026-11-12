//! Embedded LLVM and LLD (single-binary backend arc, station `llvm`).
//!
//! LLVM IR is parsed, linked, verified, optimized with the standard new-pass-manager
//! pipeline (which includes the coroutine passes at every level) and emitted as an
//! object in-process. LLD links in-process through `lld_shim.cpp`.
use llvm_sys::{
    analysis::{LLVMVerifierFailureAction, LLVMVerifyModule},
    core::*,
    ir_reader::LLVMParseIRInContext2,
    linker::LLVMLinkModules2,
    prelude::*,
    target::*,
    target_machine::*,
    transforms::pass_builder::*,
};
use std::{
    ffi::{CStr, CString, c_char, c_int},
    path::Path,
    ptr,
    sync::{Mutex, Once},
};

unsafe extern "C" {
    fn tellur_lld_link(argc: c_int, argv: *const *const c_char, can_run_again: *mut c_int)
    -> c_int;
}

/// The LLVM version built into this compiler.
pub fn version() -> (u32, u32, u32) {
    let (mut major, mut minor, mut patch) = (0, 0, 0);
    unsafe { LLVMGetVersion(&mut major, &mut minor, &mut patch) };
    (major, minor, patch)
}

fn message(raw: *mut c_char) -> String {
    if raw.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { LLVMDisposeMessage(raw) };
    text
}

/// Compiles textual LLVM IR modules into one object file for `triple`.
pub fn compile(modules: &[(String, String)], triple: &str, opt: u8) -> Result<Vec<u8>, String> {
    static TARGETS: Once = Once::new();
    TARGETS.call_once(|| unsafe {
        LLVM_InitializeAllTargetInfos();
        LLVM_InitializeAllTargets();
        LLVM_InitializeAllTargetMCs();
        LLVM_InitializeAllAsmPrinters();
    });
    unsafe {
        let context = LLVMContextCreate();
        let result = compile_in(context, modules, triple, opt);
        LLVMContextDispose(context);
        result
    }
}

unsafe fn parse(
    context: LLVMContextRef,
    name: &str,
    source: &str,
) -> Result<LLVMModuleRef, String> {
    let name = CString::new(name).unwrap();
    let buffer = unsafe {
        LLVMCreateMemoryBufferWithMemoryRangeCopy(
            source.as_ptr().cast(),
            source.len(),
            name.as_ptr(),
        )
    };
    let mut module = ptr::null_mut();
    let mut error = ptr::null_mut();
    let failed = unsafe { LLVMParseIRInContext2(context, buffer, &mut module, &mut error) };
    unsafe { LLVMDisposeMemoryBuffer(buffer) };
    if failed != 0 {
        return Err(format!("{}: {}", name.to_string_lossy(), message(error)));
    }
    Ok(module)
}

unsafe fn compile_in(
    context: LLVMContextRef,
    modules: &[(String, String)],
    triple: &str,
    opt: u8,
) -> Result<Vec<u8>, String> {
    let triple_c = CString::new(triple).unwrap();
    let mut target = ptr::null_mut();
    let mut error = ptr::null_mut();
    if unsafe { LLVMGetTargetFromTriple(triple_c.as_ptr(), &mut target, &mut error) } != 0 {
        return Err(message(error));
    }
    let empty = CString::new("").unwrap();
    let level = match opt {
        0 => LLVMCodeGenOptLevel::LLVMCodeGenLevelNone,
        1 => LLVMCodeGenOptLevel::LLVMCodeGenLevelLess,
        2 => LLVMCodeGenOptLevel::LLVMCodeGenLevelDefault,
        _ => LLVMCodeGenOptLevel::LLVMCodeGenLevelAggressive,
    };
    let machine = unsafe {
        LLVMCreateTargetMachine(
            target,
            triple_c.as_ptr(),
            empty.as_ptr(),
            empty.as_ptr(),
            level,
            LLVMRelocMode::LLVMRelocPIC,
            LLVMCodeModel::LLVMCodeModelDefault,
        )
    };
    let result = unsafe { link_and_emit(context, modules, machine, &triple_c, opt) };
    unsafe { LLVMDisposeTargetMachine(machine) };
    result
}

unsafe fn link_and_emit(
    context: LLVMContextRef,
    modules: &[(String, String)],
    machine: LLVMTargetMachineRef,
    triple: &CStr,
    opt: u8,
) -> Result<Vec<u8>, String> {
    // Every module is compiled for the one target, with the machine's layout.
    let layout = unsafe { LLVMCreateTargetDataLayout(machine) };
    let layout_text = message(unsafe { LLVMCopyStringRepOfTargetData(layout) });
    unsafe { LLVMDisposeTargetData(layout) };
    let layout_c = CString::new(layout_text).unwrap();
    let mut module: LLVMModuleRef = ptr::null_mut();
    for (name, source) in modules {
        let next = unsafe { parse(context, name, source)? };
        unsafe {
            LLVMSetTarget(next, triple.as_ptr());
            LLVMSetDataLayout(next, layout_c.as_ptr());
        }
        if module.is_null() {
            module = next;
        } else if unsafe { LLVMLinkModules2(module, next) } != 0 {
            // `next` is consumed either way.
            unsafe { LLVMDisposeModule(module) };
            return Err(format!("linking {name} failed"));
        }
    }
    if module.is_null() {
        return Err("no IR modules".into());
    }
    let result = unsafe { emit_with(module, machine, opt) };
    unsafe { LLVMDisposeModule(module) };
    result
}

unsafe fn emit_with(
    module: LLVMModuleRef,
    machine: LLVMTargetMachineRef,
    opt: u8,
) -> Result<Vec<u8>, String> {
    unsafe {
        let mut error = ptr::null_mut();
        if LLVMVerifyModule(
            module,
            LLVMVerifierFailureAction::LLVMReturnStatusAction,
            &mut error,
        ) != 0
        {
            return Err(format!("invalid IR: {}", message(error)));
        }
        LLVMDisposeMessage(error);
        // The new pass manager's default pipelines include the coroutine passes at
        // every level, O0 included.
        let pipeline = CString::new(format!("default<O{}>", opt.min(3))).unwrap();
        let options = LLVMCreatePassBuilderOptions();
        LLVMPassBuilderOptionsSetVerifyEach(options, 1);
        let failure = LLVMRunPasses(module, pipeline.as_ptr(), machine, options);
        LLVMDisposePassBuilderOptions(options);
        if !failure.is_null() {
            return Err(message(llvm_sys::error::LLVMGetErrorMessage(failure)));
        }
        let mut buffer = ptr::null_mut();
        let mut error = ptr::null_mut();
        if LLVMTargetMachineEmitToMemoryBuffer(
            machine,
            module,
            LLVMCodeGenFileType::LLVMObjectFile,
            &mut error,
            &mut buffer,
        ) != 0
        {
            return Err(message(error));
        }
        let start = LLVMGetBufferStart(buffer) as *const u8;
        let bytes = std::slice::from_raw_parts(start, LLVMGetBufferSize(buffer)).to_vec();
        LLVMDisposeMemoryBuffer(buffer);
        Ok(bytes)
    }
}

/// Runs the embedded LLD. `args[0]` selects the driver: `ld64.lld`, `ld.lld` or
/// `lld-link`. LLD permits sequential re-entry but uses process-global driver
/// state. Serialize the entire call, including driver cleanup in the C shim.
pub fn link(args: &[String]) -> Result<(), String> {
    static LINK: Mutex<()> = Mutex::new(());
    let _guard = LINK.lock().map_err(|_| "LLD invocation lock is poisoned")?;
    let owned: Vec<CString> = args
        .iter()
        .map(|a| CString::new(a.as_str()).unwrap())
        .collect();
    let argv: Vec<*const c_char> = owned.iter().map(|a| a.as_ptr()).collect();
    let mut again = 0;
    let code = unsafe { tellur_lld_link(argv.len() as c_int, argv.as_ptr(), &mut again) };
    if code == 0 {
        Ok(())
    } else {
        Err(format!("LLD failed with status {code}"))
    }
}

/// The host's object/executable triple.
pub fn host_triple() -> String {
    message(unsafe { LLVMGetDefaultTargetTriple() })
}

/// The macOS deployment floor of generated executables.
pub const MACOS_MINIMUM: &str = "13.0";

/// Development smoke path: compile IR files and link a host executable. On macOS
/// this still uses the SDK's `libSystem` stub; the bundled stub follows.
pub fn smoke(inputs: &[&Path], output: &Path, opt: u8) -> Result<(), String> {
    let modules = inputs
        .iter()
        .map(|p| {
            std::fs::read_to_string(p)
                .map(|s| (p.display().to_string(), s))
                .map_err(|e| format!("{}: {e}", p.display()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let triple = format!("arm64-apple-macosx{MACOS_MINIMUM}.0");
    let object = compile(&modules, &triple, opt)?;
    let object_path = output.with_extension("o");
    std::fs::write(&object_path, object).map_err(|e| e.to_string())?;
    let sdk = std::process::Command::new("xcrun")
        .arg("--show-sdk-path")
        .output()
        .map_err(|e| e.to_string())?;
    let sdk = String::from_utf8_lossy(&sdk.stdout).trim().to_string();
    link(&[
        "ld64.lld".into(),
        "-arch".into(),
        "arm64".into(),
        "-platform_version".into(),
        "macos".into(),
        MACOS_MINIMUM.into(),
        MACOS_MINIMUM.into(),
        "-syslibroot".into(),
        sdk,
        "-lSystem".into(),
        "-o".into(),
        output.display().to_string(),
        object_path.display().to_string(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn concurrent_compilers_and_linkers_keep_independent_executables() {
        use std::sync::{Arc, Barrier};
        let directory =
            std::env::temp_dir().join(format!("tellur-concurrent-llvm-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let barrier = Arc::new(Barrier::new(8));
        let workers = (0..8)
            .map(|id| {
                let directory = directory.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    for opt in [0, 2] {
                        let input = directory.join(format!("{id}-{opt}.ll"));
                        let output = directory.join(format!("{id}-{opt}"));
                        std::fs::write(
                            &input,
                            format!("define i32 @main() {{ ret i32 {} }}", id + 1),
                        )
                        .unwrap();
                        barrier.wait();
                        smoke(&[&input], &output, opt).unwrap();
                        let status = std::process::Command::new(&output).status().unwrap();
                        assert_eq!(status.code(), Some(id + 1));
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
