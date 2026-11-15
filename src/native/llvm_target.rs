//! Link a generated object with the runtime and startup files for its target.
use super::*;

struct Input {
    name: &'static str,
    bytes: &'static [u8],
}

impl Input {
    fn extract(&self, cache: &Path) -> Result<String> {
        let path = cache.join(self.name);
        unpack(&path, self.bytes)?;
        Ok(path.display().to_string())
    }
}

#[allow(dead_code)] // A source build may package only one linker flavor.
enum LinkPack {
    Darwin,
    Windows {
        start: &'static [Input],
        libraries: &'static [Input],
        end: &'static Input,
    },
    Musl {
        start: &'static [Input],
        libraries: &'static [Input],
        end: &'static [Input],
    },
}

pub(super) struct TargetPack {
    pub triple: &'static str,
    plain: Input,
    polled: Input,
    link: LinkPack,
}

static PACKS: &[TargetPack] = include!(concat!(env!("OUT_DIR"), "/target-packs.rs"));

pub(super) fn native_triple() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-musl"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-musl"),
        _ => None,
    }
}

pub(super) fn select(requested: Option<&str>) -> Result<&'static TargetPack> {
    let triple = requested
        .or_else(|| native_triple())
        .ok_or_else(|| error("no native LLVM target pack for this host"))?;
    PACKS.iter().find(|p| p.triple == triple).ok_or_else(|| {
        let available = PACKS
            .iter()
            .map(|p| p.triple)
            .collect::<Vec<_>>()
            .join(", ");
        error(format!(
            "target pack {triple} is unavailable in this compiler (embedded targets: {available})"
        ))
    })
}

impl TargetPack {
    pub fn llvm_triple(&self) -> String {
        match self.link {
            LinkPack::Darwin => format!("arm64-apple-macosx{}.0", crate::llvm::MACOS_MINIMUM),
            LinkPack::Musl { .. } | LinkPack::Windows { .. } => self.triple.to_owned(),
        }
    }

    pub fn link(
        &self,
        cache: &Path,
        object: &Path,
        output: &Path,
        adapter: Option<PathBuf>,
        prebuilt_adapter: bool,
        polled: bool,
    ) -> Result<()> {
        let cache = cache.join("tellur-runtime-pack");
        let runtime = if polled { &self.polled } else { &self.plain }.extract(&cache)?;
        let output = output.display().to_string();
        let obj = object.display().to_string();
        let args = match self.link {
            LinkPack::Darwin => {
                let mut args = vec![
                    "ld64.lld".into(),
                    "-arch".into(),
                    "arm64".into(),
                    "-platform_version".into(),
                    "macos".into(),
                    crate::llvm::MACOS_MINIMUM.into(),
                    crate::llvm::MACOS_MINIMUM.into(),
                    "-o".into(),
                    output,
                    obj,
                    runtime,
                ];
                if let Some(adapter) = &adapter {
                    args.push(adapter.display().to_string());
                }
                if adapter.is_some() && !prebuilt_adapter {
                    // A Rust source adapter may import additional system symbols.
                    let sdk = Command::new("xcrun")
                        .arg("--show-sdk-path")
                        .output()
                        .map_err(|e| {
                            error(format!("cannot locate macOS SDK for Rust adapter: {e}"))
                        })?;
                    if !sdk.status.success() {
                        return Err(error("cannot locate macOS SDK for Rust adapter"));
                    }
                    args.extend([
                        "-syslibroot".into(),
                        String::from_utf8_lossy(&sdk.stdout).trim().into(),
                        "-lSystem".into(),
                    ]);
                } else {
                    let stub = object.with_file_name("libSystem.tbd");
                    unpack(
                        &stub,
                        include_bytes!("targets/aarch64-apple-darwin/libSystem.tbd"),
                    )?;
                    args.extend(["-Z".into(), stub.display().to_string()]);
                }
                args
            }
            LinkPack::Windows {
                start,
                libraries,
                end,
            } => {
                let mut args = vec![
                    "lld-link".into(),
                    "-lldmingw".into(),
                    "/nodefaultlib".into(),
                    "/opt:ref".into(),
                    "/machine:x64".into(),
                    "/subsystem:console".into(),
                    "/entry:mainCRTStartup".into(),
                    format!("/out:{output}"),
                ];
                for file in start {
                    args.push(file.extract(&cache)?);
                }
                args.extend([obj, runtime]);
                if let Some(path) = adapter {
                    args.push(path.display().to_string());
                }
                for file in libraries {
                    args.push(file.extract(&cache)?);
                }
                args.push(end.extract(&cache)?);
                args
            }
            LinkPack::Musl {
                start,
                libraries,
                end,
            } => {
                let mut args = vec![
                    "ld.lld".into(),
                    "-static".into(),
                    "--gc-sections".into(),
                    "-z".into(),
                    "noexecstack".into(),
                    "-o".into(),
                    output,
                ];
                for file in start {
                    args.push(file.extract(&cache)?);
                }
                args.extend([obj, "--start-group".into(), runtime]);
                if let Some(path) = adapter {
                    args.push(path.display().to_string());
                }
                for file in libraries {
                    args.push(file.extract(&cache)?);
                }
                args.push("--end-group".into());
                for file in end {
                    args.push(file.extract(&cache)?);
                }
                args
            }
        };
        crate::llvm::link(&args).map_err(error)
    }
}
