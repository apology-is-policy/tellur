//! Build the embedded linker and native runtime pack for LLVM-enabled compilers.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/llvm/lld_shim.cpp");
    println!("cargo:rerun-if-env-changed=LLVM_SYS_221_PREFIX");
    #[cfg(feature = "llvm")]
    {
        llvm::build();
        if std::env::var("TARGET").as_deref() == Ok("aarch64-apple-darwin") {
            darwin_runtime::build();
        }
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

#[cfg(feature = "llvm")]
mod darwin_runtime {
    use std::{
        env, fs,
        hash::{DefaultHasher, Hash, Hasher},
        path::PathBuf,
        process::Command,
    };

    pub fn build() {
        for path in [
            "runtime/Cargo.toml",
            "runtime/Cargo.lock",
            "runtime/lib.rs",
            "src/native",
            "src/collection_support.rs",
            "src/fault.rs",
            "src/task_support.rs",
        ] {
            println!("cargo:rerun-if-changed={path}");
        }
        let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
        // A separate target directory keeps the child Cargo off the compiler's build lock.
        let target_dir = out.join("runtime-build");
        let target = env::var("TARGET").unwrap();
        for profile in ["plain", "polled"] {
            let mut cmd = Command::new(env::var_os("CARGO").unwrap());
            cmd.args([
                "build",
                "--locked",
                "--release",
                "--manifest-path",
                "runtime/Cargo.toml",
                "--target",
                &target,
                "--target-dir",
            ])
            .arg(&target_dir)
            .env("CARGO_INCREMENTAL", "0")
            .env("MACOSX_DEPLOYMENT_TARGET", "13.0");
            if profile == "polled" {
                cmd.args(["--features", "polled"]);
            }
            let result = cmd.output().expect("start Cargo for the Tellur runtime");
            assert!(
                result.status.success(),
                "building {profile} runtime: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let bytes = fs::read(
                target_dir
                    .join(&target)
                    .join("release/libtellur_runtime_abi.a"),
            )
            .unwrap();
            let mut hash = DefaultHasher::new();
            bytes.hash(&mut hash);
            println!(
                "cargo:rustc-env=TELLUR_RUNTIME_{}_ID={:016x}",
                profile.to_uppercase(),
                hash.finish()
            );
            fs::write(out.join(format!("runtime-{profile}.a")), bytes).unwrap();
        }
    }
}
