//! Build-time preparation of the native archives and system files embedded in tellur.
use std::{
    env,
    fmt::Write,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
};

const DARWIN: &str = "aarch64-apple-darwin";
const LINUX: [&str; 2] = ["aarch64-unknown-linux-musl", "x86_64-unknown-linux-musl"];

pub fn build() {
    for path in [
        "runtime/Cargo.toml",
        "runtime/Cargo.lock",
        "runtime/lib.rs",
        "runtime/linux_entry.rs",
        "runtime/target_packs.rs",
        "src/native",
        "src/collection_support.rs",
        "src/fault.rs",
        "src/task_support.rs",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-env-changed=TELLUR_MUSL_SYSROOT");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut descriptors = String::from("&[\n");
    if env::var("TARGET").as_deref() == Ok(DARWIN) {
        prepare(&out, DARWIN, None, &mut descriptors);
    }
    let host_cpu = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    for triple in LINUX {
        let native = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux")
            && triple.split('-').next() == Some(host_cpu.as_str());
        if native || cfg!(feature = "linux-musl-packs") {
            let sysroot = env::var_os("TELLUR_MUSL_SYSROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    let output = Command::new(env::var_os("RUSTC").unwrap())
                        .args(["--print", "sysroot"])
                        .output()
                        .unwrap();
                    assert!(output.status.success(), "cannot locate Rust sysroot");
                    PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
                });
            prepare(&out, triple, Some(&sysroot), &mut descriptors);
        }
    }
    descriptors.push(']');
    fs::write(out.join("target-packs.rs"), descriptors).unwrap();
}

fn prepare(out: &Path, triple: &str, sysroot: Option<&Path>, descriptors: &mut String) {
    let pack = out.join(triple);
    fs::create_dir_all(&pack).unwrap();
    let link = if let Some(sysroot) = sysroot {
        musl_link(&pack, triple, sysroot)
    } else {
        "LinkPack::Darwin".to_owned()
    };
    let target_dir = out.join("runtime-build");
    let mut runtimes = Vec::with_capacity(2);
    for profile in ["plain", "polled"] {
        let mut cargo = Command::new(env::var_os("CARGO").unwrap());
        cargo
            .args([
                "build",
                "--locked",
                "--release",
                "--manifest-path",
                "runtime/Cargo.toml",
                "--target",
                triple,
                "--target-dir",
            ])
            .arg(&target_dir)
            .env("CARGO_INCREMENTAL", "0");
        if let Some(root) = sysroot {
            // Compiler-host flags may name another CPU or SDK. Pass only the target
            // sysroot to the packaged runtime; host build scripts use their own.
            let mut flags = std::ffi::OsString::from("--sysroot\x1f");
            flags.push(root);
            cargo
                .env_remove("RUSTFLAGS")
                .env("CARGO_ENCODED_RUSTFLAGS", flags);
        } else {
            cargo.env("MACOSX_DEPLOYMENT_TARGET", "13.0");
        }
        if profile == "polled" {
            cargo.args(["--features", "polled"]);
        }
        let result = cargo.output().expect("start Cargo for the Tellur runtime");
        assert!(
            result.status.success(),
            "building {triple} {profile} runtime: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let archive = pack.join(format!("runtime-{profile}.a"));
        fs::copy(
            target_dir
                .join(triple)
                .join("release/libtellur_runtime_abi.a"),
            &archive,
        )
        .unwrap();
        runtimes.push(input(&archive, "libtellur_runtime", "a"));
    }
    writeln!(
        descriptors,
        "TargetPack {{ triple: {triple:?}, plain: {}, polled: {}, link: {link} }},",
        runtimes[0], runtimes[1]
    )
    .unwrap();
}

fn input(path: &Path, stem: &str, extension: &str) -> String {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let mut hash = DefaultHasher::new();
    bytes.hash(&mut hash);
    let name = format!("{stem}_{:016x}.{extension}", hash.finish());
    format!("Input {{ name: {name:?}, bytes: include_bytes!({path:?}) }}")
}

fn musl_link(pack: &Path, triple: &str, sysroot: &Path) -> String {
    let crt = sysroot
        .join("lib/rustlib")
        .join(triple)
        .join("lib/self-contained");
    assert!(
        crt.join("crt1.o").is_file(),
        "install the {triple} Rust target or set TELLUR_MUSL_SYSROOT to a matching Rust sysroot"
    );
    let entry = pack.join("tellur-entry.o");
    let status = Command::new(env::var_os("RUSTC").unwrap())
        .args([
            "--edition=2024",
            "--crate-name",
            "tellur_linux_start",
            "--emit=obj",
            "-Copt-level=2",
            "--target",
            triple,
            "--sysroot",
        ])
        .arg(sysroot)
        .arg("runtime/linux_entry.rs")
        .arg("-o")
        .arg(&entry)
        .status()
        .unwrap();
    assert!(status.success(), "building Rust process entry for {triple}");
    let files = |names: &[&str]| -> String {
        names
            .iter()
            .map(|name| {
                let path = crt.join(name);
                println!("cargo:rerun-if-changed={}", path.display());
                let (stem, ext) = name.rsplit_once('.').unwrap();
                input(&path, stem, ext)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "LinkPack::Musl {{ start: &[{}, {}], libraries: &[{}], end: &[{}] }}",
        files(&["crt1.o", "crti.o", "crtbegin.o"]),
        input(&entry, "tellur-entry", "o"),
        files(&["libunwind.a", "libc.a"]),
        files(&["crtend.o", "crtn.o"])
    )
}
