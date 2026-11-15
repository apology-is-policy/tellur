//! Build the embedded linker and native runtime pack for LLVM-enabled compilers.
fn main() {
    println!("cargo:rustc-check-cfg=cfg(tellur_native_llvm)");
    println!("cargo:rustc-check-cfg=cfg(target_os, values(\"thylacine\"))");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/llvm/lld_shim.cpp");
    println!("cargo:rerun-if-env-changed=LLVM_SYS_221_PREFIX");
    #[cfg(feature = "llvm")]
    {
        llvm::build();
        target_packs::build();
    }
}

#[cfg(feature = "llvm")]
mod llvm {
    use std::{path::PathBuf, process::Command};

    fn config(prefix: &PathBuf, arg: &str) -> String {
        let output = Command::new(prefix.join("bin/llvm-config"))
            .arg(arg)
            .output()
            .expect("llvm-config of LLVM_SYS_221_PREFIX");
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    pub fn build() {
        let prefix = PathBuf::from(
            std::env::var("LLVM_SYS_221_PREFIX")
                .expect("set LLVM_SYS_221_PREFIX to the pinned static LLVM 22.1.8 install"),
        );
        let elf_only = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("thylacine")
            && !cfg!(feature = "windows-gnu-pack");
        let mut shim = cc::Build::new();
        if elf_only {
            shim.define("TELLUR_LLD_ELF_ONLY", None);
        }
        shim.cpp(true)
            .std("c++17")
            .include(config(&prefix, "--includedir"))
            .file("src/llvm/lld_shim.cpp")
            .warnings(false)
            .compile("tellur_lld_shim");
        println!(
            "cargo:rustc-link-search=native={}",
            config(&prefix, "--libdir")
        );
        if !elf_only {
            println!("cargo:rustc-link-lib=static=lldMachO");
            println!("cargo:rustc-link-lib=static=lldCOFF");
        }
        for lib in ["lldELF", "lldCommon"] {
            println!("cargo:rustc-link-lib=static={lib}");
        }
    }
}

#[cfg(feature = "llvm")]
#[path = "runtime/target_packs.rs"]
mod target_packs;
