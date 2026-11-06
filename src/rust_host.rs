//! Source-level Rust adapters. These are trusted host mechanisms, not a stable ABI.
use crate::{
    Diagnostic, Result, Span,
    collection_support::Collection,
    manifest::{Manifest, Mode, TypeDef},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::PathBuf,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub source: PathBuf,
    pub lockfile: PathBuf,
    /// Explicit child modules embedded inside rust_host, alongside source.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "crate::launch::unique_map"
    )]
    pub modules: BTreeMap<String, PathBuf>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
    /// Tellur opaque must-complete type -> public type in the adapter module.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "crate::launch::unique_map"
    )]
    pub resources: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub version: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default = "enabled")]
    pub default_features: bool,
}
fn enabled() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub execution: Execution,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Execution {
    Immediate,
    Blocking,
    /// An async host operation: started once, then polled until it completes.
    Poll,
}
fn error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_RUST_ADAPTER", Span::default(), message)
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s != "_"
        && !["self", "Self", "super", "crate"].contains(&s)
        && s.bytes()
            .enumerate()
            .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
}
fn supported(t: &str) -> bool {
    match t {
        "Bool" | "U32" | "String" | "Bytes" | "Unit" => true,
        _ => match Collection::parse(t) {
            Some(Collection::Vector(t)) => supported(&t),
            Some(Collection::Map(k, v)) => {
                matches!(k.as_str(), "String" | "U32" | "Bool") && supported(&v)
            }
            _ => false,
        },
    }
}
pub(crate) fn validate(manifest: &Manifest) -> Result<()> {
    if let Some(config) = &manifest.rust {
        if config.source.as_os_str().is_empty()
            || config.lockfile.as_os_str().is_empty()
            || config.source == config.lockfile
        {
            return Err(error(
                "Rust source and lockfile must be distinct nonempty paths",
            ));
        }
        for (name, path) in &config.modules {
            if !identifier(name) || path.as_os_str().is_empty() || path == &config.lockfile {
                return Err(error(format!(
                    "Rust module `{name}` requires a Rust-compatible identifier and a nonempty source path distinct from the lockfile"
                )));
            }
        }
        for (name, d) in &config.dependencies {
            if !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
                || name.is_empty()
                || ["serde", "serde_json", "tellur_adapter_lock", "tellur_mio"]
                    .contains(&name.as_str())
                || name.starts_with("path_generated_")
            {
                return Err(error(format!(
                    "invalid or reserved Rust dependency `{name}`; serde and serde_json are already provided"
                )));
            }
            if d.version.is_empty()
                || !d.version.is_ascii()
                || d.version.chars().any(char::is_control)
                || d.features
                    .iter()
                    .any(|f| f.is_empty() || !f.is_ascii() || f.chars().any(char::is_control))
            {
                return Err(error(format!(
                    "invalid version or feature for Rust dependency `{name}`"
                )));
            }
        }
        for (name, rust_type) in &config.resources {
            if !identifier(name)
                || !identifier(rust_type)
                || !matches!(
                    manifest.type_def(name),
                    Some(TypeDef::Opaque { linear: true, .. })
                )
            {
                return Err(error(format!(
                    "Rust resource `{name}` requires an opaque linear manifest type and a Rust-compatible adapter type identifier"
                )));
            }
        }
    }
    for p in manifest.primitives.iter().filter(|p| p.rust.is_some()) {
        if manifest.rust.is_none() {
            return Err(error(
                "Rust-bound primitives require a top-level rust configuration",
            ));
        }
        if !identifier(&p.name) || p.outcomes.iter().any(|o| !identifier(&o.name)) {
            return Err(error(
                "Rust-bound operation and outcome names require Rust-compatible ASCII identifiers",
            ));
        }
        if p.pure {
            return Err(error(
                "Rust adapters cannot declare pure expression evaluation",
            ));
        }
        let polled = matches!(p.rust.as_ref().unwrap().execution, Execution::Poll);
        if polled != p.asynchronous {
            return Err(error(format!(
                "Rust adapter `{}`: async host operations use `\"execution\": \"poll\"`; synchronous primitives use `immediate` or `blocking`",
                p.name
            )));
        }
        for ty in p.parameters.iter().map(|p| p.ty.as_str()).chain(
            p.outcomes
                .iter()
                .flat_map(|o| o.fields.iter().map(|f| f.ty.as_str())),
        ) {
            if !supported(ty) && !manifest.rust.as_ref().unwrap().resources.contains_key(ty) {
                return Err(error(format!(
                    "Rust adapter `{}` does not support `{ty}`; use owned scalar/string/bytes/vector/map data, call-duration shared borrows, or a directly bound opaque resource",
                    p.name
                )));
            }
        }
    }
    Ok(())
}
fn rust_type(t: &str, resources: &BTreeMap<String, String>) -> String {
    if let Some(name) = resources.get(t) {
        return format!("crate::rust_host::r#{name}");
    }
    match t {
        "Bool" => "bool".into(),
        "U32" => "u32".into(),
        "String" => "String".into(),
        "Bytes" => "Vec<u8>".into(),
        "Unit" => "()".into(),
        _ => match Collection::parse(t).unwrap() {
            Collection::Vector(t) => format!("Vec<{}>", rust_type(&t, resources)),
            Collection::Map(k, v) => format!(
                "std::collections::BTreeMap<{},{}>",
                rust_type(&k, resources),
                rust_type(&v, resources)
            ),
            _ => unreachable!(),
        },
    }
}
fn collect_type(t: &str, all: &mut BTreeSet<String>) {
    all.insert(t.into());
    if let Some(c) = Collection::parse(t) {
        for t in c.types() {
            collect_type(t, all);
        }
    }
}
/// Generate Rust enums and exact function-pointer checks from the same manifest
/// that checks Tellur callers. No arbitrary dynamic wire-value API reaches wrappers.
pub(crate) fn source(manifest: &Manifest) -> Result<String> {
    let Some(config) = &manifest.rust else {
        return Ok("fn rust_dispatch(_: &mut Resources, _: &str, _: Vec<V>) -> R<Outcome> { Err(\"E_HOST: no Rust adapter\".into()) }\nfn rust_start(_: &mut Resources, _: &str, _: Vec<V>) -> R<Box<dyn HostPoll>> { Err(\"E_HOST: no Rust adapter\".into()) }\n".to_string() + NO_READINESS);
    };
    if !config.source.is_absolute()
        || !config.lockfile.is_absolute()
        || config.modules.values().any(|path| !path.is_absolute())
    {
        return Err(error(
            "native adapter paths are unresolved; load the manifest with Manifest::from_json_at",
        ));
    }
    let source = std::fs::read_to_string(&config.source).map_err(|e| {
        error(format!(
            "cannot read Rust adapter {}: {e}",
            config.source.display()
        ))
    })?;
    let mut code = String::from("#[allow(non_camel_case_types)]\npub mod rust_api {\n");
    let resources = &config.resources;
    let mut all = BTreeSet::new();
    for p in manifest.primitives.iter().filter(|p| p.rust.is_some()) {
        writeln!(code, "pub enum {}_outcome {{", p.name).unwrap();
        for o in &p.outcomes {
            if o.fields.is_empty() {
                writeln!(code, "r#{},", o.name).unwrap();
            } else {
                writeln!(
                    code,
                    "r#{}({}),",
                    o.name,
                    o.fields
                        .iter()
                        .map(|f| rust_type(&f.ty, resources))
                        .collect::<Vec<_>>()
                        .join(",")
                )
                .unwrap();
            }
            for f in &o.fields {
                collect_type(&f.ty, &mut all);
            }
        }
        code.push_str("}\n");
        for f in &p.parameters {
            collect_type(&f.ty, &mut all);
        }
        if p.asynchronous {
            let borrowed = p
                .parameters
                .iter()
                .filter(|f| f.mode == Mode::Borrow)
                .map(|f| format!("{}: &{}", f.name, rust_type(&f.ty, resources)))
                .collect::<Vec<_>>();
            writeln!(
                code,
                "/// A started `{0}` operation, polled with its borrowed resources until it completes.\npub trait {0}_operation {{ fn poll(&mut self{1}) -> Poll<{0}_outcome>; }}",
                p.name,
                borrowed.iter().map(|b| format!(", {b}")).collect::<String>()
            )
            .unwrap();
        }
    }
    if manifest
        .primitives
        .iter()
        .any(|p| p.asynchronous && p.rust.is_some())
    {
        code.push_str(POLL_API);
    }
    code.push_str("}\nmod rust_host {\n");
    code.push_str(&source);
    for (name, path) in &config.modules {
        let source = std::fs::read_to_string(path).map_err(|e| {
            error(format!(
                "cannot read Rust adapter module `{name}` at {}: {e}",
                path.display()
            ))
        })?;
        write!(code, "\nmod r#{name} {{\n{source}\n}}\n").unwrap();
    }
    code.push_str("\n}\n");
    all.retain(|t| !resources.contains_key(t));
    let ids: BTreeMap<_, _> = all
        .iter()
        .enumerate()
        .map(|(i, t)| (t.clone(), i))
        .collect();
    for t in &all {
        let id = ids[t];
        let ty = rust_type(t, resources);
        let (decode, encode) = match t.as_str() {
            "Bool" | "U32" | "String" | "Bytes" => (
                format!(
                    "match value {{ V::{t}(v)=>Ok(v), _=>Err(\"E_HOST: adapter input type mismatch\".into()) }}"
                ),
                format!("V::{t}(value)"),
            ),
            "Unit" => (
                "match value { V::Unit=>Ok(()), _=>Err(\"E_HOST: expected unit\".into()) }".into(),
                "V::Unit".into(),
            ),
            _ => match Collection::parse(t).unwrap() {
                Collection::Vector(inner) => (
                    format!(
                        "match value {{ V::Construct{{fields,..}}=>fields.into_iter().map(decode_{}).collect(), _=>Err(\"E_HOST: expected vector\".into()) }}",
                        ids[&inner]
                    ),
                    format!(
                        "V::Construct{{ty:{t:?}.into(),name:\"vector\".into(),fields:value.into_iter().map(encode_{}).collect()}}",
                        ids[&inner]
                    ),
                ),
                Collection::Map(k, v) => (
                    format!(
                        "match value {{ V::Construct{{fields,..}}=>{{ let mut fields=fields.into_iter();let mut map=std::collections::BTreeMap::new();while let Some(k)=fields.next() {{let v=fields.next().ok_or(\"E_HOST: malformed map\")?;map.insert(decode_{}(k)?,decode_{}(v)?);}}Ok(map)}}, _=>Err(\"E_HOST: expected map\".into()) }}",
                        ids[&k], ids[&v]
                    ),
                    format!(
                        "V::Construct{{ty:{t:?}.into(),name:\"map\".into(),fields:value.into_iter().flat_map(|(k,v)|[encode_{}(k),encode_{}(v)]).collect()}}",
                        ids[&k], ids[&v]
                    ),
                ),
                _ => unreachable!(),
            },
        };
        writeln!(code,"fn decode_{id}(value:V)->R<{ty}>{{{decode}}}\nfn encode_{id}(value:{ty})->V{{{encode}}}").unwrap();
    }
    code.push_str("fn rust_dispatch(resources:&mut Resources,name:&str,args:Vec<V>)->R<Outcome>{let mut args=args.into_iter();match name{\n");
    for p in manifest
        .primitives
        .iter()
        .filter(|p| p.rust.is_some() && !p.asynchronous)
    {
        writeln!(code, "{:?}=>{{", p.name).unwrap();
        for i in 0..p.parameters.len() {
            writeln!(
                code,
                "let a{i}=args.next().ok_or(\"E_HOST: missing adapter argument\")?;"
            )
            .unwrap();
        }
        code.push_str(
            "if args.next().is_some(){return Err(\"E_HOST: extra adapter argument\".into());}\n",
        );
        writeln!(
            code,
            "resources.validate_call(&[{}])?;",
            p.parameters
                .iter()
                .enumerate()
                .filter(|(_, f)| resources.contains_key(&f.ty))
                .map(|(i, f)| format!("(&a{i},{:?},{})", f.ty, f.mode == Mode::Consume))
                .collect::<Vec<_>>()
                .join(",")
        )
        .unwrap();
        // Decode data and take owners before creating shared arena references.
        for (i, f) in p.parameters.iter().enumerate() {
            if resources.contains_key(&f.ty) {
                if f.mode != Mode::Borrow {
                    writeln!(
                        code,
                        "let p{i}=resources.take::<{}>(&a{i},{:?})?;",
                        rust_type(&f.ty, resources),
                        f.ty
                    )
                    .unwrap();
                }
            } else {
                writeln!(code, "let p{i}=decode_{}(a{i})?;", ids[&f.ty]).unwrap();
            }
        }
        for (i, f) in p.parameters.iter().enumerate() {
            if resources.contains_key(&f.ty) && f.mode == Mode::Borrow {
                writeln!(
                    code,
                    "let p{i}=resources.get::<{}>(&a{i},{:?})?;",
                    rust_type(&f.ty, resources),
                    f.ty
                )
                .unwrap();
            }
        }
        let types = p
            .parameters
            .iter()
            .map(|p| {
                if p.mode != Mode::Borrow {
                    return rust_type(&p.ty, resources);
                }
                match p.ty.as_str() {
                    "String" => "&str".into(),
                    "Bytes" => "&[u8]".into(),
                    _ => match Collection::parse(&p.ty) {
                        Some(Collection::Vector(t)) => format!("&[{}]", rust_type(&t, resources)),
                        _ => format!("&{}", rust_type(&p.ty, resources)),
                    },
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let args = p
            .parameters
            .iter()
            .enumerate()
            .map(|(i, p)| {
                format!(
                    "{}p{i}",
                    if p.mode == Mode::Borrow && !resources.contains_key(&p.ty) {
                        "&"
                    } else {
                        ""
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        writeln!(
            code,
            "let call:fn({types})->rust_api::{}_outcome=rust_host::r#{};match call({args}){{",
            p.name, p.name
        )
        .unwrap();
        for o in &p.outcomes {
            let pattern = if o.fields.is_empty() {
                String::new()
            } else {
                format!(
                    "({})",
                    (0..o.fields.len())
                        .map(|i| format!("v{i}"))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            writeln!(
                code,
                "rust_api::{}_outcome::r#{}{pattern}=>Ok(({:?}.into(),vec![{}])),",
                p.name,
                o.name,
                o.name,
                o.fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| if resources.contains_key(&f.ty) {
                        format!("resources.insert({:?},v{i})?", f.ty)
                    } else {
                        format!("encode_{}(v{i})", ids[&f.ty])
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
            .unwrap();
        }
        code.push_str("}},\n");
    }
    code.push_str("_=>Err(format!(\"E_HOST: unbound Rust operation {name}\")),}}\n");
    start_source(manifest, resources, &ids, &mut code);
    if manifest
        .primitives
        .iter()
        .any(|p| p.asynchronous && p.rust.is_some())
    {
        code.push_str(MIO_READINESS);
    } else {
        code.push_str(NO_READINESS);
    }
    Ok(code)
}
/// Readiness vocabulary shared by every polled async host operation.
const POLL_API: &str = r#"/// One poll of a started async host operation.
pub enum Poll<T> { Ready(T), Pending(Wait) }
/// When the runtime may poll a pending operation again: after a delay, when a
/// descriptor becomes readable, or whichever comes first. It never polls earlier.
/// Readability is edge-triggered: report it only after a nonblocking read
/// returned `WouldBlock`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wait { pub(crate) retry_after: Option<std::time::Duration>, pub(crate) readable: Option<std::os::fd::RawFd> }
impl Wait {
    pub fn after(delay: std::time::Duration) -> Self { Self { retry_after: Some(delay), readable: None } }
    pub fn readable(source: &impl std::os::fd::AsRawFd) -> Self { Self { retry_after: None, readable: Some(source.as_raw_fd()) } }
    pub fn or_after(self, delay: std::time::Duration) -> Self { Self { retry_after: Some(delay), ..self } }
}
"#;
const NO_READINESS: &str =
    "fn rust_readiness() -> R<Option<Box<dyn ReadinessBackend>>> { Ok(None) }\n";
/// The internal readiness backend for polled adapters; Mio stays out of the adapter API.
const MIO_READINESS: &str = r#"struct MioReadiness { poll: tellur_mio::Poll, events: tellur_mio::Events }
impl ReadinessBackend for MioReadiness {
    fn register(&mut self, token: u64, fd: std::os::fd::RawFd) -> R<()> {
        self.poll.registry().register(&mut tellur_mio::unix::SourceFd(&fd), tellur_mio::Token(token as usize), tellur_mio::Interest::READABLE)
            .map_err(|e| format!("E_HOST: readiness registration failed: {e}"))
    }
    fn deregister(&mut self, fd: std::os::fd::RawFd) -> R<()> {
        self.poll.registry().deregister(&mut tellur_mio::unix::SourceFd(&fd))
            .map_err(|e| format!("E_HOST: readiness deregistration failed: {e}"))
    }
    fn wait(&mut self, timeout: Option<std::time::Duration>) -> R<Vec<u64>> {
        loop {
            match self.poll.poll(&mut self.events, timeout) {
                Ok(()) => return Ok(self.events.iter().map(|e| e.token().0 as u64).collect()),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("E_HOST: readiness wait failed: {e}")),
            }
        }
    }
}
fn rust_readiness() -> R<Option<Box<dyn ReadinessBackend>>> {
    let poll = tellur_mio::Poll::new().map_err(|e| format!("E_HOST: readiness backend failed: {e}"))?;
    Ok(Some(Box::new(MioReadiness { poll, events: tellur_mio::Events::with_capacity(64) })))
}
"#;
/// Start async host operations and adapt their typed polls to the runtime.
fn start_source(
    manifest: &Manifest,
    resources: &BTreeMap<String, String>,
    ids: &BTreeMap<String, usize>,
    code: &mut String,
) {
    let operations = manifest
        .primitives
        .iter()
        .filter(|p| p.rust.is_some() && p.asynchronous)
        .collect::<Vec<_>>();
    code.push_str("fn rust_start(resources:&mut Resources,name:&str,args:Vec<V>)->R<Box<dyn HostPoll>>{let mut args=args.into_iter();match name{\n");
    for p in &operations {
        writeln!(code, "{:?}=>{{", p.name).unwrap();
        for i in 0..p.parameters.len() {
            writeln!(
                code,
                "let a{i}=args.next().ok_or(\"E_HOST: missing adapter argument\")?;"
            )
            .unwrap();
        }
        code.push_str(
            "if args.next().is_some(){return Err(\"E_HOST: extra adapter argument\".into());}\n",
        );
        let borrowed = p
            .parameters
            .iter()
            .enumerate()
            .filter(|(_, f)| f.mode == Mode::Borrow)
            .collect::<Vec<_>>();
        writeln!(
            code,
            "resources.validate_call(&[{}])?;",
            borrowed
                .iter()
                .map(|(i, f)| format!("(&a{i},{:?},false)", f.ty))
                .collect::<Vec<_>>()
                .join(",")
        )
        .unwrap();
        for (i, f) in p.parameters.iter().enumerate() {
            if f.mode == Mode::Borrow {
                writeln!(
                    code,
                    "let p{i}=resources.get::<{}>(&a{i},{:?})?;",
                    rust_type(&f.ty, resources),
                    f.ty
                )
                .unwrap();
            } else {
                writeln!(code, "let p{i}=decode_{}(a{i})?;", ids[&f.ty]).unwrap();
            }
        }
        let types = p
            .parameters
            .iter()
            .map(|f| {
                let ty = rust_type(&f.ty, resources);
                if f.mode == Mode::Borrow {
                    format!("&{ty}")
                } else {
                    ty
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let args = (0..p.parameters.len())
            .map(|i| format!("p{i}"))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(
            code,
            "let start:fn({types})->Box<dyn rust_api::{0}_operation>=rust_host::r#{0};let operation=start({args});Ok(Box::new({0}_glue{{operation{1}}}))}},",
            p.name,
            borrowed
                .iter()
                .map(|(i, _)| format!(",a{i}"))
                .collect::<String>()
        )
        .unwrap();
    }
    code.push_str("_=>Err(format!(\"E_HOST: unbound async Rust operation {name}\")),}}\n");
    for p in &operations {
        let borrowed = p
            .parameters
            .iter()
            .enumerate()
            .filter(|(_, f)| f.mode == Mode::Borrow)
            .collect::<Vec<_>>();
        writeln!(
            code,
            "#[allow(non_camel_case_types)]\nstruct {}_glue{{operation:Box<dyn rust_api::{}_operation>{}}}",
            p.name,
            p.name,
            borrowed
                .iter()
                .map(|(i, _)| format!(",a{i}:V"))
                .collect::<String>()
        )
        .unwrap();
        writeln!(
            code,
            "impl HostPoll for {}_glue{{fn poll(&mut self,resources:&mut Resources)->R<HostProgress>{{",
            p.name
        )
        .unwrap();
        for (i, f) in &borrowed {
            writeln!(
                code,
                "let p{i}=resources.get::<{}>(&self.a{i},{:?})?;",
                rust_type(&f.ty, resources),
                f.ty
            )
            .unwrap();
        }
        writeln!(
            code,
            "match self.operation.poll({}){{rust_api::Poll::Pending(wait)=>Ok(HostProgress::Pending{{delay:wait.retry_after,readable:wait.readable}}),rust_api::Poll::Ready(outcome)=>Ok(HostProgress::Ready(match outcome{{",
            borrowed
                .iter()
                .map(|(i, _)| format!("p{i}"))
                .collect::<Vec<_>>()
                .join(",")
        )
        .unwrap();
        for o in &p.outcomes {
            let pattern = if o.fields.is_empty() {
                String::new()
            } else {
                format!(
                    "({})",
                    (0..o.fields.len())
                        .map(|i| format!("v{i}"))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            writeln!(
                code,
                "rust_api::{}_outcome::r#{}{pattern}=>({:?}.into(),vec![{}]),",
                p.name,
                o.name,
                o.name,
                o.fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| format!("encode_{}(v{i})", ids[&f.ty]))
                    .collect::<Vec<_>>()
                    .join(",")
            )
            .unwrap();
        }
        code.push_str("}))}}}\n");
    }
}
pub(crate) fn cargo(manifest: &Manifest, template: &str) -> String {
    let mut dependencies = String::new();
    if manifest
        .primitives
        .iter()
        .any(|p| p.asynchronous && p.rust.is_some())
    {
        dependencies.push_str(
            "tellur_mio = { package = \"mio\", version = \"1\", features = [\"os-poll\", \"os-ext\"] }\n",
        );
    }
    if let Some(config) = &manifest.rust {
        for (name, d) in &config.dependencies {
            writeln!(
                dependencies,
                "{} = {{ version = {}, features = {}, default-features = {} }}",
                serde_json::to_string(name).unwrap(),
                serde_json::to_string(&d.version).unwrap(),
                serde_json::to_string(&d.features).unwrap(),
                d.default_features
            )
            .unwrap();
        }
    }
    template.replace(
        "\n# Each output",
        &format!("\n{dependencies}\n# Each output"),
    )
}
