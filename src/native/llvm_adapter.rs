//! Packaging and C table emission for typed source adapters.
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
        let flags = u32::from(primitive.asynchronous);
        operations.push(format!("%adapter_operation {{ ptr {name}, i64 {len}, %adapter_descriptor {{ i32 1, i32 {flags}, ptr @{symbol} }} }}"));
    }
    let count = operations.len();
    let version = if crate::rust_host::has_polled_adapters(manifest) {
        2
    } else {
        1
    };
    writeln!(emitter.globals, "@adapter_operations = private constant [{count} x %adapter_operation] [{}]\n@adapter_table = private constant %adapter_table {{ i32 {version}, i32 0, ptr @adapter_operations, i64 {count} }}", operations.join(", ")).unwrap();
}

/// A single snapshot feeds both the locked Cargo build and the portable pack identity.
pub(super) struct Inputs {
    source: String,
    cargo: String,
    lock: String,
}

pub(super) fn inputs(manifest: &Manifest) -> Result<Inputs> {
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
    Ok(Inputs {
        source,
        cargo,
        lock,
    })
}

pub(super) fn library(
    manifest: &Manifest,
    inputs: Inputs,
    target: &Path,
    opt: u8,
    triple: &str,
) -> Result<PathBuf> {
    let Inputs {
        source,
        cargo,
        lock,
    } = inputs;
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
    // The custom Rust sysroot identifies its crates by the JSON target spec.
    // Keep the Tellur triple for the pack identity and Cargo output directory.
    let mut cargo_build = cargo_command();
    cargo_build.arg("build");
    let mut hasher = std::hash::DefaultHasher::new();
    let mut rust_target = PathBuf::from(triple);
    if triple == "aarch64-unknown-thylacine"
        && let Some(spec) = std::env::var_os("TELLUR_THYLACINE_TARGET_SPEC")
    {
        rust_target = std::path::absolute(spec).map_err(|e| error(e.to_string()))?;
        if rust_target.file_name() != Some(std::ffi::OsStr::new("aarch64-unknown-thylacine.json")) {
            return Err(error(
                "expected aarch64-unknown-thylacine.json for the Thylacine Rust target",
            ));
        }
        fs::read(&rust_target)
            .map_err(|e| error(format!("cannot read {}: {e}", rust_target.display())))?
            .hash(&mut hasher);
        cargo_build.arg("-Zjson-target-spec");
    }
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
    let result = cargo_build
        .env("RUSTC", rustc)
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("CARGO_PROFILE_DEV_OPT_LEVEL", opt.to_string())
        .args(["--lib", "--locked", "--target"])
        .arg(rust_target)
        .arg("--manifest-path")
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

// Fixed-width digests followed by archive bytes. The magic versions the envelope.
const PACK_MAGIC: &[u8] = b"TellurAdapterPack/v1\n";

impl Inputs {
    fn identity(&self, manifest: &Manifest, triple: &str) -> Result<[u8; 32]> {
        // Source-module type names embed absolute paths. Only manifest resources
        // and bound primitives cross the adapter ABI.
        let resources = &manifest.rust.as_ref().unwrap().resources;
        let types: Vec<_> = manifest
            .types
            .iter()
            .filter(|t| resources.contains_key(t.name()))
            .collect();
        let primitives: Vec<_> = manifest
            .primitives
            .iter()
            .filter(|p| p.rust.is_some())
            .collect();
        let contract = serde_json::to_vec(&(
            env!("CARGO_PKG_VERSION"),
            triple,
            types,
            primitives,
            crate::llvm::sha256(self.source.as_bytes()),
            crate::llvm::sha256(self.cargo.as_bytes()),
            crate::llvm::sha256(self.lock.as_bytes()),
        ))
        .map_err(|e| error(e.to_string()))?;
        Ok(crate::llvm::sha256(&contract))
    }
}

pub(super) fn pack(
    manifest: &Manifest,
    output: &Path,
    target: &Path,
    opt: u8,
    triple: &str,
) -> Result<()> {
    let inputs = inputs(manifest)?;
    let identity = inputs.identity(manifest, triple)?;
    let archive = library(manifest, inputs, target, opt, triple)?;
    let payload =
        fs::read(&archive).map_err(|e| error(format!("cannot read {}: {e}", archive.display())))?;
    let mut packed = Vec::with_capacity(PACK_MAGIC.len() + 64 + payload.len());
    packed.extend_from_slice(PACK_MAGIC);
    packed.extend_from_slice(&identity);
    packed.extend_from_slice(&crate::llvm::sha256(&payload));
    packed.extend_from_slice(&payload);
    unpack(output, &packed)
}

pub(super) fn open_pack(
    manifest: &Manifest,
    file: &Path,
    target: &Path,
    triple: &str,
) -> Result<PathBuf> {
    let identity = inputs(manifest)?.identity(manifest, triple)?;
    let bytes = fs::read(file)
        .map_err(|e| error(format!("cannot read adapter pack {}: {e}", file.display())))?;
    let body = bytes
        .strip_prefix(PACK_MAGIC)
        .ok_or_else(|| error("unknown adapter pack format"))?;
    let (header, payload) = body
        .split_at_checked(64)
        .ok_or_else(|| error("truncated adapter pack"))?;
    if header[..32] != identity {
        return Err(error(
            "adapter pack does not match compiler version, target, adapter sources, contract or lockfile",
        ));
    }
    let digest = crate::llvm::sha256(payload);
    if header[32..] != digest || !payload.starts_with(b"!<arch>\n") {
        return Err(error("damaged adapter archive in pack"));
    }
    use std::fmt::Write;
    let name = digest.iter().fold(String::new(), |mut name, byte| {
        write!(name, "{byte:02x}").unwrap();
        name
    });
    let archive = target
        .join("tellur-adapter-packs")
        .join(format!("{name}.a"));
    unpack(&archive, payload)?;
    Ok(archive)
}
