//! Builds the in-process LLD shim when the `llvm` feature is enabled.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/llvm/lld_shim.cpp");
    println!("cargo:rerun-if-env-changed=LLVM_SYS_221_PREFIX");
    #[cfg(feature = "llvm")]
    llvm::build();
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
        cc::Build::new()
            .cpp(true)
            .std("c++17")
            .include(config(&prefix, "--includedir"))
            .file("src/llvm/lld_shim.cpp")
            .warnings(false)
            .compile("tellur_lld_shim");
        println!(
            "cargo:rustc-link-search=native={}",
            config(&prefix, "--libdir")
        );
        for lib in ["lldMachO", "lldELF", "lldCOFF", "lldCommon"] {
            println!("cargo:rustc-link-lib=static={lib}");
        }
    }
}
