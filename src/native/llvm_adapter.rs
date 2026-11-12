//! Packaging and C table emission for the admitted synchronous source adapters.
use super::*;
use crate::{manifest::Manifest, rust_host::static_adapter};

pub(super) fn table(manifest: &Manifest, emitter: &mut Emitter) {
    emitter.frame_types.push_str("declare i32 @tellur_rt_v2_launch(ptr, i64, ptr, i64, ptr)\n%adapter_descriptor = type { i32, i32, ptr }\n%adapter_operation = type { ptr, i64, %adapter_descriptor }\n%adapter_table = type { i32, i32, ptr, i64 }\n");
    let mut operations = Vec::new();
    for (index, primitive) in manifest
        .primitives
        .iter()
        .filter(|p| p.rust.is_some())
        .enumerate()
    {
        let symbol = static_adapter::symbol(index);
        writeln!(emitter.frame_types, "declare void @{symbol}(ptr, ptr, i64)").unwrap();
        let (name, len) = emitter.text(&primitive.name);
        operations.push(format!("%adapter_operation {{ ptr {name}, i64 {len}, %adapter_descriptor {{ i32 1, i32 0, ptr @{symbol} }} }}"));
    }
    let count = operations.len();
    writeln!(emitter.globals, "@adapter_operations = private constant [{count} x %adapter_operation] [{}]\n@adapter_table = private constant %adapter_table {{ i32 1, i32 0, ptr @adapter_operations, i64 {count} }}", operations.join(", ")).unwrap();
}

pub(super) fn library(manifest: &Manifest, target: &Path, opt: u8) -> Result<PathBuf> {
    let source = static_adapter::source(manifest)?;
    let config = manifest.rust.as_ref().unwrap();
    let lock = fs::read_to_string(&config.lockfile).map_err(|e| {
        error(format!(
            "cannot read adapter lock {}: {e}; run lock-native first",
            config.lockfile.display()
        ))
    })?;
    if !lock.contains("name = \"tellur_adapter_lock\"") {
        return Err(error(
            "adapter lock is not a lock-native lock; run lock-native first",
        ));
    }
    let cargo = crate::rust_host::cargo(manifest, CARGO);
    // Ask the same toolchain selected for Cargo, retaining rustup's current-directory
    // selection and explicit RUSTC. Cargo still checks freshness and --locked on
    // every invocation; an existing archive is not sufficient evidence of validity.
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| {
        Path::new(cargo_command().get_program())
            .parent()
            .map(|p| p.join("rustc"))
            .filter(|p| p.is_file())
            .map(|p| p.into_os_string())
            .unwrap_or_else(|| "rustc".into())
    });
    let identity = Command::new(&rustc)
        .arg("-vV")
        .output()
        .map_err(|e| error(format!("cannot identify adapter Rust toolchain: {e}")))?;
    if !identity.status.success() {
        return Err(error(format!(
            "cannot identify adapter Rust toolchain: {}",
            String::from_utf8_lossy(&identity.stderr)
        )));
    }
    let triple = "aarch64-apple-darwin";
    let mut hasher = std::hash::DefaultHasher::new();
    (
        "tellur-source-adapter-c-abi-v1",
        &source,
        &cargo,
        &lock,
        serde_json::to_string(manifest).map_err(|e| error(e.to_string()))?,
        triple,
        opt,
        &rustc,
        &identity.stdout,
    )
        .hash(&mut hasher);
    for name in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTUP_TOOLCHAIN",
    ] {
        (name, std::env::var_os(name)).hash(&mut hasher);
    }
    let hash = format!("{:016x}", hasher.finish());
    let package = format!("tellur_adapter_{hash}");
    let cargo = format!(
        "{}\n[lib]\npath = \"lib.rs\"\ncrate-type = [\"staticlib\"]\n",
        cargo.replace(
            "name = \"path_generated\"",
            &format!("name = \"{package}\"")
        )
    );
    let lock = lock.replace(
        "name = \"tellur_adapter_lock\"",
        &format!("name = \"{package}\""),
    );
    let directory = target.join("tellur-adapters").join(hash);
    fs::create_dir_all(&directory).map_err(|e| error(e.to_string()))?;
    for (name, contents) in [
        ("lib.rs", source),
        ("Cargo.toml", cargo),
        ("Cargo.lock", lock),
    ] {
        let file = directory.join(name);
        if fs::read_to_string(&file).ok().as_deref() != Some(contents.as_str()) {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let temporary = directory.join(format!(
                "{name}.{}.{}.partial",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::write(&temporary, contents).map_err(|e| error(e.to_string()))?;
            fs::rename(temporary, file).map_err(|e| error(e.to_string()))?;
        }
    }
    let build = target.join("adapter-build");
    let result = cargo_command()
        .env("RUSTC", rustc)
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("CARGO_PROFILE_DEV_OPT_LEVEL", opt.to_string())
        .args([
            "build",
            "--lib",
            "--locked",
            "--target",
            triple,
            "--manifest-path",
        ])
        .arg(directory.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&build)
        .output()
        .map_err(|e| error(format!("could not build Rust adapter: {e}")))?;
    if !result.status.success() {
        return Err(error(format!(
            "Rust adapter build failed: {}",
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(build.join(triple).join(format!("debug/lib{package}.a")))
}
