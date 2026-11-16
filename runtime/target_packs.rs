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
const WINDOWS: &str = "x86_64-pc-windows-gnu";
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
        println!("cargo:rustc-cfg=tellur_native_llvm");
        prepare(&out, DARWIN, None, &mut descriptors);
    }
    let host_cpu = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let host_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    for triple in LINUX {
        let native = host_os == "linux" && triple.split('-').next() == Some(host_cpu.as_str());
        if native {
            println!("cargo:rustc-cfg=tellur_native_llvm");
        }
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
    let native_windows = host_os == "windows" && host_cpu == "x86_64";
    if native_windows || cfg!(feature = "windows-gnu-pack") {
        if native_windows {
            println!("cargo:rustc-cfg=tellur_native_llvm");
        }
        println!("cargo:rerun-if-env-changed=TELLUR_WINDOWS_SYSROOT");
        println!("cargo:rerun-if-env-changed=TELLUR_WINDOWS_DLLTOOL");
        let root = PathBuf::from(
            env::var_os("TELLUR_WINDOWS_SYSROOT")
                .expect("set TELLUR_WINDOWS_SYSROOT to the prepared Windows GNU Rust sysroot"),
        );
        prepare(&out, WINDOWS, Some(&root), &mut descriptors);
    }
    let native_thylacine = host_os == "thylacine" && host_cpu == "aarch64";
    if native_thylacine || cfg!(feature = "thylacine-pack") {
        if native_thylacine {
            println!("cargo:rustc-cfg=tellur_native_llvm");
        }
        println!("cargo:rerun-if-env-changed=TELLUR_THYLACINE_PACK");
        let pack = PathBuf::from(env::var_os("TELLUR_THYLACINE_PACK").expect(
            "set TELLUR_THYLACINE_PACK to the prepared Thylacine runtime and pouch link inputs",
        ));
        thylacine_pack(
            &pack.canonicalize().expect("locate Thylacine pack"),
            &mut descriptors,
        );
    }
    descriptors.push(']');
    fs::write(out.join("target-packs.rs"), descriptors).unwrap();
}

fn prepare(out: &Path, triple: &str, sysroot: Option<&Path>, descriptors: &mut String) {
    let pack = out.join(triple);
    fs::create_dir_all(&pack).unwrap();
    let link = if let Some(sysroot) = sysroot {
        if triple == WINDOWS {
            windows_link(&pack, sysroot)
        } else {
            musl_link(&pack, triple, sysroot)
        }
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
            if triple == WINDOWS {
                flags.push("\x1f-Cdlltool=");
                flags.push(windows_dlltool());
            }
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

fn thylacine_pack(pack: &Path, descriptors: &mut String) {
    let file = |name: &str| {
        let path = pack.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let (stem, ext) = name.rsplit_once('.').unwrap();
        input(&path, stem, ext)
    };
    let files = |names: &[&str]| {
        names
            .iter()
            .map(|name| file(name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let start = files(&["crt1.o", "crti.o", "entry.o"]);
    let libraries = files(&["libunwind.a", "libc.a", "libclang_rt.builtins.a"]);
    let end = file("crtn.o");
    let plain = file("runtime-plain.a");
    let polled = file("runtime-polled.a");
    let dl = file("libdl.a");
    let pthread = file("libpthread.a");
    writeln!(descriptors, "TargetPack {{ triple: \"aarch64-unknown-thylacine\", plain: {plain}, polled: {polled}, link: LinkPack::Pouch {{ start: &[{start}], libraries: &[{libraries}], end: &{end}, dl: {dl}, pthread: {pthread} }} }},").unwrap();
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

fn windows_dlltool() -> PathBuf {
    env::var_os("TELLUR_WINDOWS_DLLTOOL")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("LLVM_SYS_221_PREFIX").unwrap()).join("bin/llvm-dlltool")
        })
}

fn windows_link(pack: &Path, sysroot: &Path) -> String {
    let rust_lib = sysroot.join("lib/rustlib").join(WINDOWS).join("lib");
    let crt = rust_lib.join("self-contained");
    let file = |dir: &Path, name: &str| {
        let path = dir.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let (stem, ext) = name.rsplit_once('.').unwrap();
        input(&path, stem, ext)
    };
    let mut libraries: Vec<_> = [
        "libmingw32.a",
        "libmingwex.a",
        "libmoldname.a",
        "libmsvcrt.a",
        "libgcc.a",
        "libgcc_eh.a",
    ]
    .iter()
    .map(|name| file(&crt, name))
    .collect();
    for dll in ["kernel32", "ntdll", "userenv", "ws2_32"] {
        let definition = Path::new("src/native/targets")
            .join(WINDOWS)
            .join(format!("{dll}.def"));
        let library = pack.join(format!("{dll}.lib"));
        println!("cargo:rerun-if-changed={}", definition.display());
        let status = Command::new(windows_dlltool())
            .args(["-m", "i386:x86-64", "-d"])
            .arg(&definition)
            .arg("-l")
            .arg(&library)
            .status()
            .expect("start llvm-dlltool");
        assert!(
            status.success(),
            "building Windows import library for {dll}"
        );
        libraries.push(input(&library, dll, "lib"));
    }
    format!(
        "LinkPack::Windows {{ start: &[{}, {}], libraries: &[{}], end: &{} }}",
        file(&crt, "crt2.o"),
        file(&rust_lib, "rsbegin.o"),
        libraries.join(", "),
        file(&rust_lib, "rsend.o")
    )
}
