//! Checked graph -> explicit Rust control flow -> verified LLVM IR -> native code.
//! Rust is a portable implementation bridge; the emitted program has no graph interpreter.
use crate::{Diagnostic, Result, Span, ir::*};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
};
const SUPPORT: &str = include_str!("native/runtime.rs");
const CARGO: &str = include_str!("native/Cargo.template");
const LOCK: &str = include_str!("native/Cargo.lock");
fn error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_BACKEND", Span::default(), message)
}
fn cargo_command() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|p| p.join(".cargo/bin/cargo"))
            .filter(|p| p.is_file())
            .map(|p| p.into_os_string())
            .unwrap_or_else(|| "cargo".into())
    }))
}
fn string(s: &str) -> String {
    format!("{s:?}")
}
fn value(v: &Value) -> String {
    match v {
        Value::Reference { .. } => unreachable!("references are never embedded literals"),
        Value::Bool(b) => format!("V::Bool({b})"),
        Value::U32(n) => format!("V::U32({n})"),
        Value::Unit => "V::Unit".into(),
        Value::String(s) => format!("V::String({}.into())", string(s)),
        Value::Bytes(b) => format!("V::Bytes(vec!{b:?})"),
        Value::Opaque { ty, token } => format!(
            "V::Opaque{{ty:{}.into(),token:{}.into()}}",
            string(ty),
            string(token)
        ),
        Value::Construct { ty, name, fields } => format!(
            "V::Construct{{ty:{}.into(),name:{}.into(),fields:vec![{}]}}",
            string(ty),
            string(name),
            fields.iter().map(value).collect::<Vec<_>>().join(",")
        ),
    }
}
fn expression(e: &Expr) -> String {
    match e {
        Expr::Length(e) => format!(
            "{{ let value={}; collection_length(value,v)? }}",
            expression(e)
        ),
        Expr::Value(v) => value(v),
        Expr::Pure(name, args) => format!("pure({}, {})?", string(name), expressions(args)),
        Expr::Reference(p, ty) => format!(
            "reference(&v, base + {}, &{:?}, {}, {})?",
            p.root,
            p.fields,
            p.indirect,
            string(ty)
        ),
        Expr::Move(p) => format!(
            "take_place(&mut v, base + {}, &{:?}, {})?",
            p.root, p.fields, p.indirect
        ),
        Expr::Place(p) => format!(
            "read_place(&v, base + {}, &{:?}, {})?",
            p.root, p.fields, p.indirect
        ),
        Expr::Project(e, i) => format!("project(&({}), &[{i}])?", expression(e)),
        Expr::Construct(t, n, args) => format!(
            "V::Construct{{ty:{}.into(),name:{}.into(),fields:{}}}",
            string(t),
            string(n),
            expressions(args)
        ),
        Expr::Record(ty, fields) => {
            let assignments = fields
                .iter()
                .map(|(i, e)| format!("let value={}; fields[{i}]=value;", expression(e)))
                .collect::<String>();
            format!(
                "{{let mut fields=vec![V::Unit;{}];{assignments}V::Construct{{ty:{}.into(),name:{}.into(),fields}}}}",
                fields.len(),
                string(ty),
                string(ty)
            )
        }
        Expr::Not(e) => format!("V::Bool(!boolean({})?)", expression(e)),
        Expr::Binary(op, a, b) if op == "&&" || op == "||" => format!(
            "V::Bool(boolean({})? {op} boolean({})?)",
            expression(a),
            expression(b)
        ),
        Expr::Binary(op, a, b) => format!(
            "binary({}, {}, {})?",
            string(op),
            expression(a),
            expression(b)
        ),
    }
}
fn expressions(args: &[Expr]) -> String {
    format!(
        "vec![{}]",
        args.iter().map(expression).collect::<Vec<_>>().join(",")
    )
}
fn guard(g: &Guard) -> String {
    match g {
        Guard::True => "true".into(),
        Guard::Atom(id) => format!("a[{id}]"),
        Guard::And(x, y) => format!("({} && {})", guard(x), guard(y)),
        Guard::Or(x, y) => format!("({} || {})", guard(x), guard(y)),
    }
}
struct Generator<'a> {
    boundaries: BTreeMap<&'a str, (usize, usize)>,
    display_names: BTreeMap<&'a str, String>,
    names: BTreeMap<&'a str, usize>,
    code: String,
    slots: Vec<Id>,
}
impl Generator<'_> {
    fn instructions(&mut self, code: &[Instruction]) {
        for i in code {
            let p = &i.provenance;
            let _ = writeln!(
                self.code,
                "// {}:{}:{} definition {} calls {:?}\nhost.at({});\nhost.tick({}, {})?;\nif {} {{",
                p.region,
                p.span.line,
                p.span.column,
                p.definition,
                p.call_sites,
                i.id,
                p.span.line,
                p.span.column,
                guard(&i.guard)
            );
            match &i.action {
                Action::Terminate => {
                    self.code.push_str("return Ok(Completion::Settled);\n");
                }
                Action::Escalate => {
                    self.code.push_str("return host.escalate();\n");
                }
                Action::Supervise { boundary, args } => {
                    let (work, handler) = self.boundaries[boundary.as_str()];
                    let _ = writeln!(
                        self.code,
                        "let args={}; host.supervise(&mut v, args, region_{work}, region_{handler}, {})?;",
                        expressions(args),
                        string(&self.display_names[boundary.as_str()])
                    );
                }
                Action::Bind(id, e) => {
                    let _ = writeln!(
                        self.code,
                        "let x = {}; v[base + {id}] = Some(x);",
                        expression(e)
                    );
                }
                Action::Assign(p, e) => {
                    let _ = writeln!(
                        self.code,
                        "let x = {}; assign_place(&mut v, base + {}, &{:?}, {}, x)?;",
                        expression(e),
                        p.root,
                        p.fields,
                        p.indirect
                    );
                }
                Action::Drop(e) => {
                    let _ = writeln!(self.code, "drop({});", expression(e));
                }
                Action::Release(ids) => {
                    for id in ids {
                        let _ = writeln!(self.code, "v[base + {id}] = None;");
                    }
                }
                Action::Emit(e) => {
                    let _ = writeln!(self.code, "host.emit({})?;", expression(e));
                }
                Action::Fault(e) => {
                    let _ = writeln!(
                        self.code,
                        "let message = {}; return host.fault(message);",
                        expression(e)
                    );
                }
                Action::Exit(name, args) => {
                    let _ = writeln!(self.code, "let fields = {};", expressions(args));
                    for id in &self.slots {
                        let _ = writeln!(self.code, "v[base + {id}] = None;");
                    }
                    let _ = writeln!(
                        self.code,
                        "return Ok(Completion::Outcome(({}.into(), fields)));",
                        string(name)
                    );
                }
                Action::Again { target, args } => {
                    let _ = writeln!(
                        self.code,
                        "next_{target} = {}; continue 'loop_{target};",
                        expressions(args)
                    );
                }
                Action::Loop {
                    id,
                    parameters,
                    local_slots,
                    local_atoms,
                    body,
                } => {
                    let _ = writeln!(
                        self.code,
                        "let mut next_{id}:Vec<V> = {};\n'loop_{id}: loop {{",
                        expressions(
                            &parameters
                                .iter()
                                .map(|(_, e)| e.clone())
                                .collect::<Vec<_>>()
                        )
                    );
                    for s in local_slots {
                        let _ = writeln!(self.code, "v[base + {s}] = None;");
                    }
                    for atom in local_atoms {
                        let _ = writeln!(self.code, "a[{atom}] = false;");
                    }
                    let _ = writeln!(
                        self.code,
                        "let mut initial_{id} = std::mem::take(&mut next_{id}).into_iter();"
                    );
                    for (s, _) in parameters {
                        let _ = writeln!(self.code, "v[base + {s}] = initial_{id}.next();");
                    }
                    self.instructions(body);
                    let _ = writeln!(
                        self.code,
                        "return Err(\"E_NATIVE: checked loop fell through\".into());\n}}"
                    );
                }
                Action::Complete {
                    atom,
                    bindings,
                    unwind,
                    release,
                } => {
                    let _ = writeln!(
                        self.code,
                        "let mut fields:std::vec::IntoIter<V> = {}.into_iter();",
                        expressions(&bindings.iter().map(|(_, e)| e.clone()).collect::<Vec<_>>())
                    );
                    for (id, _) in bindings {
                        let _ = writeln!(self.code, "v[base + {id}] = fields.next();");
                    }
                    for id in release {
                        let _ = writeln!(self.code, "v[base + {id}] = None;");
                    }
                    let _ = writeln!(self.code, "a[{atom}] = true;");
                    if let Some(target) = unwind {
                        let _ = writeln!(self.code, "break 'loop_{target};");
                    }
                }
                Action::Decide {
                    operation,
                    args,
                    arms,
                } => {
                    let call = match operation {
                        Operation::Collection(name) => format!(
                            "{{ let args={}; collection({},args,v)? }}",
                            expressions(args),
                            string(name)
                        ),
                        Operation::Variant(e) => format!("variant({})?", expression(e)),
                        Operation::BorrowedVariant(e) => {
                            format!("borrowed_variant(&v, {})?", expression(e))
                        }
                        Operation::Primitive(name) => {
                            format!("host.primitive({}, {})?", string(name), expressions(args))
                        }
                        Operation::Region(name) => format!(
                            "{{ let args = {}; region_{}(host, &mut v, args, &mut vec![])?.outcome()? }}",
                            expressions(args),
                            self.names[name.as_str()]
                        ),
                    };
                    let _ = writeln!(
                        self.code,
                        "let (outcome, fields) = {call};\nmatch outcome.as_str() {{"
                    );
                    for arm in arms {
                        let _ = writeln!(
                            self.code,
                            "{} => {{ a[{}] = true; let mut fields = fields.into_iter();",
                            string(&arm.outcome),
                            arm.atom
                        );
                        for id in &arm.slots {
                            let _ = writeln!(self.code, "v[base + {id}] = fields.next();");
                        }
                        self.code.push_str("},\n");
                    }
                    self.code.push_str(
                        "_ => return Err(\"E_NATIVE: unmatched declared outcome\".into()),\n}\n",
                    );
                }
            }
            self.code.push_str("}\n");
        }
    }
}
#[derive(Debug)]
pub struct NativeBuild {
    pub executable: PathBuf,
    pub llvm: PathBuf,
    pub generated: PathBuf,
}
impl CheckedProgram {
    pub fn native_source(&self, entry: &str) -> Result<String> {
        self.verify()?;
        if self.region(entry).is_none() {
            return Err(error(format!("unknown entry region {entry}")));
        }
        let launch_entries = self.executable_entries(entry)?;
        let names: BTreeMap<_, _> = self
            .regions()
            .enumerate()
            .map(|(i, r)| (r.name.as_str(), i))
            .collect();
        let mut g = Generator {
            display_names: self
                .regions()
                .map(|r| (r.name.as_str(), self.runtime_name(&r.name)))
                .collect(),
            boundaries: self
                .regions()
                .filter_map(|r| {
                    r.boundary.as_ref().map(|b| {
                        (
                            r.name.as_str(),
                            (names[r.name.as_str()], names[b.handler.as_str()]),
                        )
                    })
                })
                .collect(),
            slots: vec![],
            names: self
                .regions()
                .enumerate()
                .map(|(i, r)| (r.name.as_str(), i))
                .collect(),
            code: String::from("#![allow(unused,unreachable_code,unused_labels)]\nmod support {\n"),
        };
        g.code.push_str("mod collection_support {\n");
        g.code.push_str(include_str!("collection_support.rs"));
        g.code.push_str("\n}\n");
        g.code.push_str("mod fault {\n");
        g.code.push_str(include_str!("fault.rs"));
        g.code.push_str("\n}\n");
        g.code.push_str(SUPPORT);
        g.code.push_str("\n}\nuse support::*;\n");
        g.code.push_str(&crate::rust_host::source(self.manifest())?);
        for r in self.regions() {
            g.slots = r.slots.keys().copied().collect();
            let size = r
                .slots
                .keys()
                .chain(r.atoms.keys())
                .max()
                .map(|id| id + 1)
                .unwrap_or(0);
            let _ = writeln!(
                g.code,
                "fn region_{}(host:&mut Host, mut v:&mut Vec<Option<V>>, args:Vec<V>, handover:&mut Vec<V>)->R<Completion> {{\nhost.enter({})?;\nlet base=v.len(); v.resize(base + {size}, None);\nlet result=(|| {{\nlet mut a = vec![false; {size}];\nlet mut args = args.into_iter();",
                g.names[r.name.as_str()],
                string(&self.runtime_name(&r.name))
            );
            for p in &r.parameters {
                let _ = writeln!(g.code, "v[base + {}] = args.next();", p.id);
            }
            g.instructions(&r.code);
            g.code
                .push_str("Err(\"E_NATIVE: checked region fell through\".into())\n})();\n");
            if let Some(b) = &r.boundary {
                g.code
                    .push_str("if result.is_err() && host.source_faulted() {\n");
                for id in &b.retained {
                    let _ = writeln!(
                        g.code,
                        "handover.push(v[base + {id}].take().ok_or(\"E_NATIVE: retained boundary parameter unavailable\")?);"
                    );
                }
                g.code.push_str("}\n");
            }
            g.code.push_str("if result.is_err() { host.release_frame(&v[base..]); } v.truncate(base); if result.is_ok() { host.leave(); } result\n}\n");
        }
        for r in self.regions() {
            let id = g.names[r.name.as_str()];
            if r.boundary.is_some()
                || r.contingency
                || r.parameters.iter().any(|p| p.ty.starts_with('&'))
            {
                let _ = writeln!(
                    g.code,
                    "fn entry_{id}(_: &mut Host, _: Vec<V>)->R<Outcome> {{ Err(\"E_ENTRY: entry controls cannot require borrowed arguments\".into()) }}"
                );
            } else {
                let _ = writeln!(
                    g.code,
                    "fn entry_{id}(host: &mut Host, args: Vec<V>)->R<Outcome> {{ let mut v=Vec::new(); region_{id}(host, &mut v, args, &mut vec![])?.outcome() }}",
                );
            }
        }
        let manifest = serde_json::to_string(self.manifest()).map_err(|e| error(e.to_string()))?;
        let _ = writeln!(
            g.code,
            "fn main() -> std::process::ExitCode {{\nlet entries:Vec<Entry> = vec!["
        );
        for r in self.regions() {
            let Some(codes) = launch_entries.get(&r.name) else {
                continue;
            };
            let _ = writeln!(
                g.code,
                "Entry {{ name: {}, display_name: {}, run: entry_{}, arguments: vec![{}], exit_codes: &[{}] }},",
                string(&r.name),
                string(&self.runtime_name(&r.name)),
                g.names[r.name.as_str()],
                r.parameters
                    .iter()
                    .map(|p| format!("{}.into()", string(&p.ty)))
                    .collect::<Vec<_>>()
                    .join(","),
                codes
                    .iter()
                    .map(|(name, code)| format!("({}, {code})", string(name)))
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
        let _ = writeln!(
            g.code,
            "];\nmatch launch({}, {}, &entries, Some(rust_dispatch), {}) {{ Ok(code) => std::process::ExitCode::from(code), Err(error) => {{ eprintln!(\"{{error}}\"); std::process::ExitCode::FAILURE }} }}\n}}",
            string(&manifest),
            string(entry),
            string(
                &serde_json::to_string(&self.runtime_sites()).map_err(|e| error(e.to_string()))?
            )
        );
        Ok(g.code)
    }
    fn native_project(
        &self,
        entry: &str,
        output: &Path,
    ) -> Result<(PathBuf, PathBuf, PathBuf, String)> {
        let output = if output.is_absolute() {
            output.to_owned()
        } else {
            std::env::current_dir()
                .map_err(|e| error(e.to_string()))?
                .join(output)
        };
        let directory = PathBuf::from(format!("{}.tellur-build", output.display()));
        let mut hasher = std::hash::DefaultHasher::new();
        directory.hash(&mut hasher);
        let package = format!("path_generated_{:016x}", hasher.finish());
        let source = self.native_source(entry)?;
        fs::create_dir_all(directory.join("src")).map_err(|e| error(e.to_string()))?;
        let generated = directory.join("src/main.rs");
        fs::write(&generated, source).map_err(|e| error(e.to_string()))?;
        fs::write(
            directory.join("Cargo.toml"),
            crate::rust_host::cargo(self.manifest(), CARGO).replace("path_generated", &package),
        )
        .map_err(|e| error(e.to_string()))?;
        Ok((output, directory, generated, package))
    }
    /// Dependency resolution is an explicit action; normal builds always use --locked.
    pub fn lock_native(&self, entry: &str, output: &Path) -> Result<PathBuf> {
        let config = self
            .manifest()
            .rust
            .as_ref()
            .ok_or_else(|| error("lock-native requires a Rust adapter configuration"))?;
        let (_, directory, _, package) = self.native_project(entry, output)?;
        let result = cargo_command()
            .arg("generate-lockfile")
            .arg("--manifest-path")
            .arg(directory.join("Cargo.toml"))
            .output()
            .map_err(|e| error(format!("could not start Cargo: {e}")))?;
        if !result.status.success() {
            return Err(error(format!(
                "dependency resolution failed: {}",
                String::from_utf8_lossy(&result.stderr)
            )));
        }
        let lock =
            fs::read_to_string(directory.join("Cargo.lock")).map_err(|e| error(e.to_string()))?;
        let lock = lock.replace(
            &format!("name = {package:?}"),
            "name = \"tellur_adapter_lock\"",
        );
        fs::write(&config.lockfile, lock)
            .map_err(|e| error(format!("cannot save {}: {e}", config.lockfile.display())))?;
        Ok(config.lockfile.clone())
    }
    pub fn build_native(&self, entry: &str, output: &Path, opt: u8) -> Result<NativeBuild> {
        if opt > 3 {
            return Err(error("optimization level must be 0..3"));
        }
        let (output, directory, generated, package) = self.native_project(entry, output)?;
        let lock = if let Some(config) = &self.manifest().rust {
            let lock=fs::read_to_string(&config.lockfile).map_err(|e|error(format!("cannot read {}: {e}; run tellurc lock-native explicitly to resolve dependencies",config.lockfile.display())))?;
            if !lock.contains("name = \"tellur_adapter_lock\"") {
                return Err(error(
                    "invalid adapter lockfile; regenerate with tellurc lock-native",
                ));
            }
            lock.replace(
                "name = \"tellur_adapter_lock\"",
                &format!("name = {package:?}"),
            )
        } else {
            LOCK.replace("path_generated", &package)
        };
        fs::write(directory.join("Cargo.lock"), lock).map_err(|e| error(e.to_string()))?;
        let llvm = PathBuf::from(format!("{}.ll", output.display()));
        let target = std::env::var_os("TELLUR_NATIVE_TARGET_DIR")
            .or_else(|| std::env::var_os("PATH_NATIVE_TARGET_DIR"))
            .map(PathBuf::from)
            .unwrap_or_else(|| output.parent().unwrap().join(".tellur-cache"));
        let result = cargo_command()
            .arg("rustc")
            .arg("--locked")
            .arg("--manifest-path")
            .arg(directory.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(target)
            .arg("--message-format=json-render-diagnostics")
            .arg("--")
            .arg("-C")
            .arg(format!("opt-level={opt}"))
            .arg("-C")
            .arg("codegen-units=1")
            .arg("-C")
            .arg("debuginfo=0")
            .arg("-C")
            .arg("passes=verify")
            .arg(format!("--emit=link,llvm-ir={}", llvm.display()))
            .output()
            .map_err(|e| error(format!("could not start Cargo/Rust toolchain: {e}")))?;
        let messages = String::from_utf8_lossy(&result.stdout);
        if !result.status.success() {
            let diagnostics = messages
                .lines()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .filter_map(|v| v["message"]["rendered"].as_str().map(str::to_owned))
                .collect::<String>();
            return Err(error(format!(
                "native build failed:\n{}\n{diagnostics}",
                String::from_utf8_lossy(&result.stderr)
            )));
        }
        let executable = messages
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find_map(|v| v["executable"].as_str().map(PathBuf::from))
            .ok_or_else(|| error("toolchain did not report an executable"))?;
        fs::copy(executable, &output).map_err(|e| error(e.to_string()))?;
        Ok(NativeBuild {
            executable: output,
            llvm,
            generated,
        })
    }
}
