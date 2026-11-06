//! Checked graph -> explicit Rust control flow -> verified LLVM IR -> native code.
//! Rust is a portable implementation bridge; the emitted program has no graph interpreter.
use crate::{Diagnostic, Result, Span, ir::*};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
};
const SUPPORT: &str = include_str!("native/runtime.rs");
const EXECUTION: &str = include_str!("native/execution.rs");
const CONTEXTS: &str = include_str!("native/contexts.rs");
const CARGO: &str = include_str!("native/Cargo.template");
const LOCK: &str = include_str!("native/Cargo.lock");
#[path = "native/access.rs"]
mod access;
#[cfg(test)]
#[path = "native/boundary_handoff_resource_tests.rs"]
mod boundary_handoff_resource_tests;
#[path = "native/continuations.rs"]
mod continuations;
#[cfg(test)]
#[path = "native/pending_operation_tests.rs"]
mod pending_operation_tests;
#[cfg(test)]
#[path = "native/resume_model.rs"]
mod resume_model;
#[cfg(test)]
#[path = "native/source_scheduler_tests.rs"]
mod source_scheduler_tests;
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
        Value::Reference { .. } | Value::Task { .. } => {
            unreachable!("references and tasks are never embedded literals")
        }
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
/// Storage is local to an activation; checked IDs remain global identities for
/// provenance, expansion certificates and control targets. Values and decisions
/// have independent dense layouts. References store absolute value-slot offsets.
#[derive(Default)]
struct FrameLayout {
    slots: BTreeMap<Id, usize>,
    atoms: BTreeMap<Id, usize>,
}
impl FrameLayout {
    fn new(region: &Region) -> Self {
        Self {
            slots: region
                .slots
                .keys()
                .enumerate()
                .map(|(i, id)| (*id, i))
                .collect(),
            atoms: region
                .atoms
                .keys()
                .enumerate()
                .map(|(i, id)| (*id, i))
                .collect(),
        }
    }
    fn slots(&self, ids: &[Id]) -> Vec<usize> {
        ids.iter().map(|id| self.slots[id]).collect()
    }
}
fn expression(e: &Expr, layout: &FrameLayout) -> String {
    match e {
        Expr::WithReferenceEvidence(value, evidence) => {
            if matches!(evidence, ReferenceEvidence::Incoming) {
                return format!(
                    "{{let reference_value={}; execution::reference_evidence::inherit_checked(v,reference_value)}}",
                    expression(value, layout)
                );
            }
            let origins = evidence.origins();
            let complete = evidence.is_complete();
            let fresh = matches!(value.as_ref(), Expr::Reference(p, _) if !p.indirect);
            let origins = origins.iter().map(|o| format!(
                "execution::reference_evidence::Origin{{root:base+{},fields:&{:?},indirect:{},guard:{},opaque:{}}}",
                layout.slots[&o.place.root], o.place.fields, o.place.indirect, o.guard, o.opaque
            )).collect::<Vec<_>>().join(",");
            if let ReferenceEvidence::CarrierInput { holder, leaves, .. } = evidence {
                let leaves = leaves
                    .iter()
                    .map(|l| {
                        format!(
                            "execution::reference_evidence::InputLeaf{{slot:base+{},fields:&{:?}}}",
                            layout.slots[&l.leaf], l.fields
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                return format!(
                    "{{let reference_value={}; execution::reference_evidence::attach_carrier_input(v,reference_value,&[{origins}],base+{},&[{leaves}])}}",
                    expression(value, layout),
                    layout.slots[holder]
                );
            }
            if let ReferenceEvidence::GuardedInput { leaf, holder, .. } = evidence {
                return format!(
                    "{{let reference_value={}; execution::reference_evidence::attach_guarded_input(v,reference_value,&[{origins}],base+{},base+{},&{:?})}}",
                    expression(value, layout),
                    layout.slots[leaf],
                    layout.slots[&holder.root],
                    holder.fields
                );
            }
            format!(
                "{{let reference_value={}; execution::reference_evidence::attach_checked(v,reference_value,&[{origins}],{complete},{fresh})}}",
                expression(value, layout)
            )
        }
        Expr::Length(e) => format!(
            "{{ let value={}; collection_length(value,v)? }}",
            expression(e, layout)
        ),
        Expr::Value(v) => value(v),
        Expr::Pure(name, args) => format!("pure({}, {})?", string(name), expressions(args, layout)),
        Expr::Reference(p, ty) => format!(
            "reference(&v, base + {}, &{:?}, {}, {})?",
            layout.slots[&p.root],
            p.fields,
            p.indirect,
            string(ty)
        ),
        Expr::Move(p) => format!(
            "take_place(&mut v, base + {}, &{:?}, {})?",
            layout.slots[&p.root], p.fields, p.indirect
        ),
        Expr::Place(p) => format!(
            "read_place(&v, base + {}, &{:?}, {})?",
            layout.slots[&p.root], p.fields, p.indirect
        ),
        Expr::Project(e, i) => format!("project_owned({}, {i})?", expression(e, layout)),
        Expr::Construct(t, n, args) => format!(
            "V::Construct{{ty:{}.into(),name:{}.into(),fields:{}}}",
            string(t),
            string(n),
            expressions(args, layout)
        ),
        Expr::Record(ty, fields) => {
            let assignments = fields
                .iter()
                .map(|(i, e)| format!("let value={}; fields[{i}]=value;", expression(e, layout)))
                .collect::<String>();
            format!(
                "{{let mut fields=vec![V::Unit;{}];{assignments}V::Construct{{ty:{}.into(),name:{}.into(),fields}}}}",
                fields.len(),
                string(ty),
                string(ty)
            )
        }
        Expr::Not(e) => format!("V::Bool(!boolean({})?)", expression(e, layout)),
        Expr::Binary(op, a, b) if op == "&&" || op == "||" => format!(
            "V::Bool(boolean({})? {op} boolean({})?)",
            expression(a, layout),
            expression(b, layout)
        ),
        Expr::Binary(op, a, b) => format!(
            "binary({}, {}, {})?",
            string(op),
            expression(a, layout),
            expression(b, layout)
        ),
    }
}
fn expressions(args: &[Expr], layout: &FrameLayout) -> String {
    format!(
        "vec![{}]",
        args.iter()
            .map(|e| expression(e, layout))
            .collect::<Vec<_>>()
            .join(",")
    )
}
fn guard(g: &Guard, layout: &FrameLayout) -> String {
    match g {
        Guard::True => "true".into(),
        Guard::Atom(id) => format!("a[{}]", layout.atoms[id]),
        Guard::And(x, y) => format!("({} && {})", guard(x, layout), guard(y, layout)),
        Guard::Or(x, y) => format!("({} || {})", guard(x, layout), guard(y, layout)),
    }
}
struct Generator<'a> {
    constructors: BTreeSet<&'a str>,
    constructing: bool,
    boundaries: BTreeMap<&'a str, (usize, usize)>,
    display_names: BTreeMap<&'a str, String>,
    names: BTreeMap<&'a str, usize>,
    code: String,
    layout: FrameLayout,
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
                guard(&i.guard, &self.layout)
            );
            match &i.action {
                Action::Boundary {
                    body,
                    handler,
                    retained,
                    retire,
                    work_slots,
                    handler_slots,
                    fault_slot,
                } => {
                    let name = format!("{}$boundary@{}", p.region, p.span.line);
                    let _ = writeln!(
                        self.code,
                        "let boundary_flow=host.lexical(v, &mut a, base, {}, &{:?}, &{:?}, &{:?}, &{:?}, {}, |host, mut v, mut a| {{",
                        string(&name),
                        self.layout.slots(retained),
                        self.layout.slots(retire),
                        self.layout.slots(work_slots),
                        self.layout.slots(handler_slots),
                        self.layout.slots[fault_slot]
                    );
                    self.instructions(body);
                    self.code
                        .push_str("Ok(Completion::Fallthrough)\n}, |host, mut v, mut a| {\n");
                    self.instructions(handler);
                    self.code.push_str("Err(\"E_NATIVE: contingency fell through\".into())\n})?;\nmatch boundary_flow {Completion::Fallthrough=>{}, other=>return Ok(other)}\n");
                }
                Action::Recover { target, args } => {
                    let _ = writeln!(
                        self.code,
                        "return Ok(Completion::Recover({target}, {}));",
                        expressions(args, &self.layout)
                    );
                }
                Action::TerminateProcess(status) => {
                    let _ = writeln!(self.code, "return Ok(Completion::Process({status}));");
                }
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
                        expressions(args, &self.layout),
                        string(&self.display_names[boundary.as_str()])
                    );
                }
                Action::Bind(id, e) => {
                    let id = self.layout.slots[id];
                    let _ = writeln!(
                        self.code,
                        "let x = {}; v[base + {id}] = Some(x);",
                        expression(e, &self.layout)
                    );
                }
                Action::Assign(p, e) => {
                    let _ = writeln!(
                        self.code,
                        "let x = {}; assign_place(&mut v, base + {}, &{:?}, {}, x)?;",
                        expression(e, &self.layout),
                        self.layout.slots[&p.root],
                        p.fields,
                        p.indirect
                    );
                }
                Action::Drop(e) => {
                    let _ = writeln!(self.code, "drop({});", expression(e, &self.layout));
                }
                Action::Release(ids) => {
                    for id in ids {
                        let id = self.layout.slots[id];
                        let _ = writeln!(self.code, "v[base + {id}] = None;");
                    }
                }
                Action::Emit(e) => {
                    let _ = writeln!(self.code, "host.emit({})?;", expression(e, &self.layout));
                }
                Action::Fault(e) => {
                    let _ = writeln!(
                        self.code,
                        "let message = {}; return host.fault(message);",
                        expression(e, &self.layout)
                    );
                }
                Action::Exit(name, args) if self.constructing => {
                    debug_assert_eq!(name, crate::constructors::COMPLETED);
                    debug_assert_eq!(args.len(), 1);
                    let _ = writeln!(
                        self.code,
                        "let constructed = {};",
                        expression(&args[0], &self.layout)
                    );
                    for id in self.layout.slots.values() {
                        let _ = writeln!(self.code, "v[base + {id}] = None;");
                    }
                    self.code.push_str("v[destination] = Some(constructed);\nreturn Ok(Completion::Constructed);\n");
                }
                Action::Exit(name, args) => {
                    let _ = writeln!(
                        self.code,
                        "let fields = {};",
                        expressions(args, &self.layout)
                    );
                    for id in self.layout.slots.values() {
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
                        "return Ok(Completion::Again({target}, {}));",
                        expressions(args, &self.layout)
                    );
                }
                Action::Loop {
                    supervised: _,
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
                                .collect::<Vec<_>>(),
                            &self.layout
                        )
                    );
                    for s in local_slots {
                        let s = self.layout.slots[s];
                        let _ = writeln!(self.code, "v[base + {s}] = None;");
                    }
                    for atom in local_atoms {
                        let atom = self.layout.atoms[atom];
                        let _ = writeln!(self.code, "a[{atom}] = false;");
                    }
                    let _ = writeln!(
                        self.code,
                        "let mut initial_{id} = std::mem::take(&mut next_{id}).into_iter();"
                    );
                    for (s, _) in parameters {
                        let s = self.layout.slots[s];
                        let _ = writeln!(self.code, "v[base + {s}] = initial_{id}.next();");
                    }
                    self.code.push_str("let loop_flow=(|| -> R<Completion> {\n");
                    self.instructions(body);
                    self.code
                        .push_str("Err(\"E_NATIVE: checked loop fell through\".into())\n})();\n");
                    let _ = writeln!(
                        self.code,
                        "match loop_flow? {{Completion::Again(target,args) if target=={id}=>{{next_{id}=args;continue 'loop_{id};}}, Completion::Recover(target,args) if target=={id}=>{{"
                    );
                    for s in local_slots {
                        let s = self.layout.slots[s];
                        let _ = writeln!(
                            self.code,
                            "host.release_frame(std::slice::from_ref(&v[base+{s}]));"
                        );
                    }
                    let _ = writeln!(
                        self.code,
                        "next_{id}=args;continue 'loop_{id};}},Completion::Complete(target) if target=={id}=>break 'loop_{id},other=>return Ok(other)}}\n}}"
                    );
                }
                Action::Complete {
                    atom,
                    bindings,
                    unwind,
                    release,
                } => {
                    if let [(id, value)] = bindings.as_slice() {
                        let id = self.layout.slots[id];
                        let _ = writeln!(
                            self.code,
                            "let value = {}; v[base + {id}] = Some(value);",
                            expression(value, &self.layout)
                        );
                    } else if !bindings.is_empty() {
                        let _ = writeln!(
                            self.code,
                            "let mut fields:std::vec::IntoIter<V> = {}.into_iter();",
                            expressions(
                                &bindings.iter().map(|(_, e)| e.clone()).collect::<Vec<_>>(),
                                &self.layout
                            )
                        );
                        for (id, _) in bindings {
                            let id = self.layout.slots[id];
                            let _ = writeln!(self.code, "v[base + {id}] = fields.next();");
                        }
                    }
                    for id in release {
                        let id = self.layout.slots[id];
                        let _ = writeln!(self.code, "v[base + {id}] = None;");
                    }
                    let atom = self.layout.atoms[atom];
                    let _ = writeln!(self.code, "a[{atom}] = true;");
                    if let Some(target) = unwind {
                        let _ = writeln!(self.code, "return Ok(Completion::Complete({target}));");
                    }
                }
                Action::Decide {
                    operation,
                    args,
                    arms,
                } => {
                    if let Operation::Region(name) = operation
                        && self.constructors.contains(name.as_str())
                    {
                        debug_assert_eq!(arms.len(), 1);
                        debug_assert_eq!(arms[0].slots.len(), 1);
                        let destination = self.layout.slots[&arms[0].slots[0]];
                        let atom = self.layout.atoms[&arms[0].atom];
                        let _ = writeln!(
                            self.code,
                            "let args = {}; region_{}(host, &mut v, args, &mut vec![], base + {destination})?.constructed()?;\na[{atom}] = true;",
                            expressions(args, &self.layout),
                            self.names[name.as_str()]
                        );
                        self.code.push_str("}\n");
                        continue;
                    }
                    let call = match operation {
                        Operation::Spawn(_) | Operation::Join(_) | Operation::Yield => {
                            unreachable!("source scheduling uses compiled continuations")
                        }
                        Operation::Defer(name) => format!(
                            "(String::from({}), vec![V::Task {{binding: None, ty: {}.into(), operation: {}.into(), arguments: {}}}])",
                            string("$pending"),
                            string(&crate::manifest::task_type(name)),
                            string(name),
                            expressions(args, &self.layout)
                        ),
                        Operation::AwaitTask(name) => format!(
                            "{{ let args = await_arguments({}, {})?; region_{}(host, &mut v, args, &mut vec![])?.outcome()? }}",
                            expressions(args, &self.layout),
                            string(name),
                            self.names[name.as_str()]
                        ),
                        Operation::Collection(name) => format!(
                            "{{ let args={}; collection({},args,v)? }}",
                            expressions(args, &self.layout),
                            string(name)
                        ),
                        Operation::Variant(e) => {
                            format!("variant({})?", expression(e, &self.layout))
                        }
                        Operation::BorrowedVariant(e) => {
                            format!("borrowed_variant(&v, {})?", expression(e, &self.layout))
                        }
                        Operation::Primitive(name) => {
                            format!(
                                "host.primitive({}, {})?",
                                string(name),
                                expressions(args, &self.layout)
                            )
                        }
                        Operation::Region(name) | Operation::Run(name) | Operation::Await(name) => {
                            format!(
                                "{{ let args = {}; region_{}(host, &mut v, args, &mut vec![])?.outcome()? }}",
                                expressions(args, &self.layout),
                                self.names[name.as_str()]
                            )
                        }
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
                            self.layout.atoms[&arm.atom]
                        );
                        for id in &arm.slots {
                            let id = self.layout.slots[id];
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
        let constructors = self
            .regions()
            .filter(|r| {
                r.is_constructor()
                    && r.exits.len() == 1
                    && r.exits[0].name == crate::constructors::COMPLETED
                    && r.exits[0].fields.len() == 1
            })
            .map(|r| r.name.as_str())
            .collect();
        let mut g = Generator {
            constructors,
            constructing: false,
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
            layout: FrameLayout::default(),
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
        g.code.push_str("mod frames {\n");
        g.code.push_str(include_str!("native/frames.rs"));
        g.code.push_str("\n}\n");
        g.code.push_str("mod tasks {\n");
        g.code.push_str(include_str!("native/tasks.rs"));
        g.code.push_str("\nmod protocol {\n");
        g.code.push_str(include_str!("task_support.rs"));
        g.code.push_str("\n}\n}\n");
        g.code.push_str("pub mod execution {\n");
        g.code.push_str(EXECUTION);
        g.code.push_str("\npub mod access_resolution {\n");
        g.code.push_str(include_str!("native/access_resolution.rs"));
        g.code.push_str("\n}\npub mod dependencies {\n");
        g.code.push_str(include_str!("native/dependencies.rs"));
        g.code.push_str("\n}\npub mod reference_evidence {\n");
        g.code
            .push_str(include_str!("native/reference_evidence.rs"));
        g.code.push_str("\n}\n");
        g.code.push_str("\nmod task_payload {\n");
        g.code.push_str(include_str!("native/task_payload.rs"));
        g.code.push_str("\n}\nmod domains {\n");
        g.code.push_str(include_str!("native/domains.rs"));
        g.code.push_str("\n}\nmod contexts {\n");
        g.code.push_str(CONTEXTS);
        g.code.push_str("\nmod source_scheduler {\n");
        g.code.push_str(include_str!("native/source_scheduler.rs"));
        g.code.push_str("\n}\n");
        g.code.push_str("\nmod started_tasks {\n");
        g.code.push_str(include_str!("native/started_tasks.rs"));
        g.code.push_str("\n}\n");
        g.code.push_str("\nmod borrower_coverage {\n");
        g.code.push_str(include_str!("native/borrower_coverage.rs"));
        g.code.push_str("\n}\n");
        g.code.push_str("\n}\n}\n");
        g.code.push_str(SUPPORT);
        g.code
            .push_str("\n}\nuse support::*; use support::execution::*;\n");
        g.code.push_str(&crate::rust_host::source(self.manifest())?);
        g.code.push_str(&continuations::source(self));
        fn schedules(code: &[Instruction]) -> bool {
            code.iter().any(|i| match &i.action {
                Action::Decide {
                    operation: Operation::Spawn(_) | Operation::Join(_) | Operation::Yield,
                    ..
                } => true,
                Action::Loop { body, .. } => schedules(body),
                Action::Boundary { body, handler, .. } => schedules(body) || schedules(handler),
                _ => false,
            })
        }
        let driver = if self.regions().any(|r| schedules(&r.code)) {
            "run_source_entry"
        } else {
            "run_entry"
        };
        for r in self.regions() {
            let id = g.names[r.name.as_str()];
            if r.boundary.is_some()
                || r.is_constructor()
                || r.contingency
                || r.parameters.iter().any(|p| p.ty.starts_with('&'))
            {
                let _ = writeln!(
                    g.code,
                    "fn entry_{id}(_: &mut Host, _: Vec<V>)->R<Completion> {{ Err(\"E_ENTRY: entry controls cannot require borrowed arguments\".into()) }}"
                );
            } else {
                let _ = writeln!(
                    g.code,
                    "fn entry_{id}(host:&mut Host,args:Vec<V>)->R<Completion>{{{driver}(host,args,&MODEL_{id})}}",
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
            "];\nmatch launch({}, {}, &entries, Some(rust_dispatch), Some(rust_start), {}) {{ Ok(code) => std::process::ExitCode::from(code), Err(error) => {{ eprintln!(\"{{error}}\"); std::process::ExitCode::FAILURE }} }}\n}}",
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
        self.finish_native_build(self.native_project(entry, output)?, opt)
    }
    fn finish_native_build(
        &self,
        project: (PathBuf, PathBuf, PathBuf, String),
        opt: u8,
    ) -> Result<NativeBuild> {
        let (output, directory, generated, package) = project;
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

#[cfg(test)]
#[path = "native/http_receive_tests.rs"]
mod http_receive_tests;

#[cfg(all(test, unix))]
#[path = "native/http_live_receive_tests.rs"]
mod http_live_receive_tests;
