//! Build a minimal Windows console executable through the compiler's embedded LLVM and LLD.
use std::{error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let directory = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("expected output directory")?,
    );
    fs::create_dir_all(&directory)?;
    std::env::set_current_dir(directory)?;

    let ir = r#"
        declare dllimport void @ExitProcess(i32) noreturn
        define void @entry() noreturn {
            call void @ExitProcess(i32 42)
            unreachable
        }
    "#;
    let object = tellur::llvm::compile(
        &[("exit.ll".into(), ir.into())],
        "x86_64-pc-windows-msvc",
        2,
    )?;
    fs::write("exit.obj", object)?;
    fs::write(
        "kernel32.def",
        "LIBRARY KERNEL32.dll\nEXPORTS\n    ExitProcess\n",
    )?;
    link(&[
        "lld-link",
        "/lib",
        "/def:kernel32.def",
        "/machine:x64",
        "/out:kernel32.lib",
    ])?;
    link(&[
        "lld-link",
        "/entry:entry",
        "/subsystem:console",
        "/nodefaultlib",
        "/machine:x64",
        "/out:exit.exe",
        "exit.obj",
        "kernel32.lib",
    ])?;
    Ok(())
}

fn link(args: &[&str]) -> Result<(), String> {
    tellur::llvm::link(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}
