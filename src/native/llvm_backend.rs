//! First direct LLVM emitter: scalar and owned-record synchronous controls and Boolean path facts.
//! Admission is explicit and occurs before writing/building anything. Every value
//! operation goes through the runtime's private C ABI; guards are native branches.
use super::*;

const DECLARATIONS: &str = r#"
declare i32 @tellur_rt_v1_launch(ptr, i64, ptr, i64)
declare i32 @tellur_rt_v1_ok(ptr)
declare i64 @tellur_rt_v1_value(ptr, i32, i64, ptr, i64)
declare i64 @tellur_rt_v1_copy(ptr, i64)
declare i64 @tellur_rt_v1_record(ptr, ptr, i64, ptr, i64)
declare i64 @tellur_rt_v1_field(ptr, i64, ptr, i64, i32)
declare void @tellur_rt_v1_assign(ptr, i64, ptr, i64, i64)
declare i64 @tellur_rt_v1_project(ptr, i64, i64)
declare void @tellur_rt_v1_drop(ptr, i64)
declare void @tellur_rt_v1_release(ptr, i64)
declare i32 @tellur_rt_v1_bool(ptr, i64)
declare i64 @tellur_rt_v1_binary(ptr, ptr, i64, i64, i64)
declare i64 @tellur_rt_v1_pure(ptr, ptr, i64, ptr, i64)
declare void @tellur_rt_v1_tick(ptr, i64, i64, i64)
declare void @tellur_rt_v1_emit(ptr, i64)
declare void @tellur_rt_v1_fault(ptr, i64)
declare void @tellur_rt_v1_complete(ptr, ptr, i64, ptr, i64)
declare i64 @tellur_rt_v1_call(ptr, ptr, ptr, i64, ptr, i64)
declare i32 @tellur_rt_v1_route(ptr, i64, ptr, i64, ptr, i64)
declare void @tellur_rt_v1_unmatched(ptr, i64)
"#;
fn admitted_types(manifest: &crate::manifest::Manifest) -> BTreeSet<String> {
    let mut types: BTreeSet<String> = ["Bool", "U32", "String", "Bytes", "Unit"]
        .into_iter()
        .map(String::from)
        .collect();
    loop {
        let before = types.len();
        for ty in &manifest.types {
            if let crate::manifest::TypeDef::Record { name, fields } = ty {
                if fields.iter().all(|f| types.contains(&f.ty)) {
                    types.insert(name.clone());
                }
            }
        }
        if types.len() == before {
            return types;
        }
    }
}
fn place(p: &Place) -> bool {
    !p.indirect
}
fn expression_supported(e: &Expr, types: &BTreeSet<String>) -> bool {
    match e {
        Expr::Value(v) => matches!(
            v,
            Value::Bool(_) | Value::U32(_) | Value::String(_) | Value::Bytes(_) | Value::Unit
        ),
        Expr::Move(p) | Expr::Place(p) => place(p),
        Expr::Pure(_, args) => args.iter().all(|e| expression_supported(e, types)),
        Expr::Construct(ty, name, args) => {
            types.contains(ty) && name == ty && args.iter().all(|e| expression_supported(e, types))
        }
        Expr::Record(ty, fields) => {
            types.contains(ty) && fields.iter().all(|(_, e)| expression_supported(e, types))
        }
        Expr::Project(e, _) => expression_supported(e, types),
        Expr::Not(e) => expression_supported(e, types),
        Expr::Binary(_, a, b) => expression_supported(a, types) && expression_supported(b, types),
        _ => false,
    }
}
fn supported(i: &Instruction, types: &BTreeSet<String>) -> bool {
    match &i.action {
        Action::Bind(_, e) | Action::Drop(e) | Action::Emit(e) | Action::Fault(e) => {
            expression_supported(e, types)
        }
        Action::Assign(p, e) => place(p) && expression_supported(e, types),
        Action::Release(_) => true,
        Action::Exit(_, args) => args.iter().all(|e| expression_supported(e, types)),
        Action::Complete {
            bindings,
            unwind: None,
            ..
        } => bindings.iter().all(|(_, e)| expression_supported(e, types)),
        Action::Decide {
            operation: Operation::Region(_),
            args,
            ..
        } => args.iter().all(|e| expression_supported(e, types)),
        Action::Decide {
            operation: Operation::Variant(e),
            args,
            arms,
        } => {
            args.is_empty()
                && expression_supported(e, types)
                && arms
                    .iter()
                    .all(|a| matches!(a.outcome.as_str(), "true" | "false") && a.slots.is_empty())
        }
        _ => false,
    }
}
struct Emitter {
    globals: String,
    body: String,
    next: usize,
    layout: FrameLayout,
    controls: BTreeMap<String, (usize, String)>,
}
impl Emitter {
    fn id(&mut self) -> String {
        self.next += 1;
        format!("v{}", self.next)
    }
    fn line(&mut self, line: impl AsRef<str>) {
        self.body.push_str(line.as_ref());
        self.body.push('\n');
    }
    fn value(&mut self, code: impl AsRef<str>) -> String {
        let name = format!("%{}", self.id());
        self.line(format!("{name} = {}", code.as_ref()));
        name
    }
    fn bytes(&mut self, bytes: &[u8]) -> (String, usize) {
        let name = format!("@{}", self.id());
        let data = bytes
            .iter()
            .map(|b| format!("\\{b:02X}"))
            .collect::<String>();
        writeln!(
            self.globals,
            "{name} = private unnamed_addr constant [{} x i8] c\"{data}\"",
            bytes.len()
        )
        .unwrap();
        (name, bytes.len())
    }
    fn text(&mut self, text: &str) -> (String, usize) {
        self.bytes(text.as_bytes())
    }
    fn check(&mut self) {
        let ok = self.value("call i32 @tellur_rt_v1_ok(ptr %ctx)");
        let ok = self.value(format!("icmp ne i32 {ok}, 0"));
        let next = self.id();
        self.line(format!("br i1 {ok}, label %{next}, label %error\n{next}:"));
    }
    fn call(&mut self, code: String) -> String {
        let result = self.value(code);
        self.check();
        result
    }
    fn effect(&mut self, code: String) {
        self.line(code);
        self.check();
    }
    fn literal(&mut self, tag: u32, n: impl std::fmt::Display, data: &[u8]) -> String {
        let (p, len) = self.bytes(data);
        self.call(format!(
            "call i64 @tellur_rt_v1_value(ptr %ctx, i32 {tag}, i64 {n}, ptr {p}, i64 {len})"
        ))
    }
    fn handles(&mut self, values: &[String]) -> String {
        if values.is_empty() {
            return "null".into();
        }
        let array = self.value(format!("alloca [{} x i64]", values.len()));
        for (i, v) in values.iter().enumerate() {
            let p = self.value(format!("getelementptr i64, ptr {array}, i64 {i}"));
            self.line(format!("store i64 {v}, ptr {p}"));
        }
        array
    }
    fn region_call(&mut self, name: &str, args: &[Expr], arms: &[Arm]) {
        let (index, display) = self.controls[name].clone();
        let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
        let args = self.handles(&values);
        let (name, len) = self.text(&display);
        let result = self.call(format!(
            "call i64 @tellur_rt_v1_call(ptr %ctx, ptr @control_{index}, ptr {name}, i64 {len}, ptr {args}, i64 {})",
            values.len()
        ));
        let delivered = self.id();
        for arm in arms {
            let payload = if arm.slots.is_empty() {
                "null".into()
            } else {
                self.value(format!("alloca [{} x i64]", arm.slots.len()))
            };
            let (name, len) = self.text(&arm.outcome);
            let selected = self.call(format!(
                "call i32 @tellur_rt_v1_route(ptr %ctx, i64 {result}, ptr {name}, i64 {len}, ptr {payload}, i64 {})",
                arm.slots.len()
            ));
            let selected = self.value(format!("icmp ne i32 {selected}, 0"));
            let yes = self.id();
            let next = self.id();
            self.line(format!(
                "br i1 {selected}, label %{yes}, label %{next}\n{yes}:"
            ));
            for (i, slot) in arm.slots.iter().enumerate() {
                let ptr = self.value(format!("getelementptr i64, ptr {payload}, i64 {i}"));
                let value = self.value(format!("load i64, ptr {ptr}"));
                self.store(*slot, &value);
            }
            self.line(format!(
                "store i1 true, ptr %a{}\nbr label %{delivered}\n{next}:",
                self.layout.atoms[&arm.atom]
            ));
        }
        self.line(format!(
            "call void @tellur_rt_v1_unmatched(ptr %ctx, i64 {result})\nbr label %error\n{delivered}:"
        ));
    }
    fn record(&mut self, ty: &str, values: &[String]) -> String {
        let args = self.handles(values);
        let (ty, len) = self.text(ty);
        self.call(format!(
            "call i64 @tellur_rt_v1_record(ptr %ctx, ptr {ty}, i64 {len}, ptr {args}, i64 {})",
            values.len()
        ))
    }
    fn boolean(&mut self, value: &str) -> String {
        let v = self.call(format!(
            "call i32 @tellur_rt_v1_bool(ptr %ctx, i64 {value})"
        ));
        self.value(format!("icmp ne i32 {v}, 0"))
    }
    fn expr(&mut self, e: &Expr) -> String {
        match e {
            Expr::Value(v) => match v {
                Value::Bool(b) => self.literal(1, u8::from(*b), b""),
                Value::U32(n) => self.literal(2, n, b""),
                Value::String(s) => self.literal(3, 0, s.as_bytes()),
                Value::Bytes(b) => self.literal(4, 0, b),
                Value::Unit => self.literal(0, 0, b""),
                _ => unreachable!(),
            },
            Expr::Place(p) | Expr::Move(p) => {
                let slot = self.layout.slots[&p.root];
                let v = self.value(format!("load i64, ptr %s{slot}"));
                if !p.fields.is_empty() {
                    let path =
                        self.handles(&p.fields.iter().map(usize::to_string).collect::<Vec<_>>());
                    self.call(format!("call i64 @tellur_rt_v1_field(ptr %ctx, i64 {v}, ptr {path}, i64 {}, i32 {})", p.fields.len(), u8::from(matches!(e, Expr::Move(_)))))
                } else if matches!(e, Expr::Move(_)) {
                    self.line(format!("store i64 0, ptr %s{slot}"));
                    v
                } else {
                    self.call(format!("call i64 @tellur_rt_v1_copy(ptr %ctx, i64 {v})"))
                }
            }
            Expr::Construct(ty, _, args) => {
                let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
                self.record(ty, &values)
            }
            Expr::Record(ty, fields) => {
                let mut values = vec![String::new(); fields.len()];
                for (index, expression) in fields {
                    // Evaluation order is written order, storage order is declaration order.
                    values[*index] = self.expr(expression);
                }
                self.record(ty, &values)
            }
            Expr::Project(e, index) => {
                let value = self.expr(e);
                self.call(format!(
                    "call i64 @tellur_rt_v1_project(ptr %ctx, i64 {value}, i64 {index})"
                ))
            }
            Expr::Pure(name, args) => {
                let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
                let p = self.handles(&values);
                let (name, len) = self.text(name);
                self.call(format!(
                    "call i64 @tellur_rt_v1_pure(ptr %ctx, ptr {name}, i64 {len}, ptr {p}, i64 {})",
                    values.len()
                ))
            }
            Expr::Not(e) => {
                let v = self.expr(e);
                let b = self.boolean(&v);
                let n = self.value(format!("xor i1 {b}, true"));
                let n = self.value(format!("zext i1 {n} to i64"));
                self.literal(1, n, b"")
            }
            Expr::Binary(op, a, b) if op == "&&" || op == "||" => {
                let result = self.value("alloca i1");
                let a = self.expr(a);
                let a = self.boolean(&a);
                let rhs = self.id();
                let skip = self.id();
                let merge = self.id();
                let (yes, no) = if op == "&&" {
                    (&rhs, &skip)
                } else {
                    (&skip, &rhs)
                };
                self.line(format!("br i1 {a}, label %{yes}, label %{no}\n{skip}:\nstore i1 {a}, ptr {result}\nbr label %{merge}\n{rhs}:"));
                let b = self.expr(b);
                let b = self.boolean(&b);
                self.line(format!(
                    "store i1 {b}, ptr {result}\nbr label %{merge}\n{merge}:"
                ));
                let v = self.value(format!("load i1, ptr {result}"));
                let v = self.value(format!("zext i1 {v} to i64"));
                self.literal(1, v, b"")
            }
            Expr::Binary(op, a, b) => {
                let a = self.expr(a);
                let b = self.expr(b);
                let (op, len) = self.text(op);
                self.call(format!(
                    "call i64 @tellur_rt_v1_binary(ptr %ctx, ptr {op}, i64 {len}, i64 {a}, i64 {b})"
                ))
            }
            _ => unreachable!(),
        }
    }
    fn guard(&mut self, guard: &Guard) -> String {
        match guard {
            Guard::True => "true".into(),
            Guard::Atom(id) => self.value(format!("load i1, ptr %a{}", self.layout.atoms[id])),
            Guard::And(a, b) | Guard::Or(a, b) => {
                let x = self.guard(a);
                let y = self.guard(b);
                let op = if matches!(guard, Guard::And(..)) {
                    "and"
                } else {
                    "or"
                };
                self.value(format!("{op} i1 {x}, {y}"))
            }
        }
    }
    fn release(&mut self, id: Id) {
        let slot = self.layout.slots[&id];
        let old = self.value(format!("load i64, ptr %s{slot}"));
        self.effect(format!(
            "call void @tellur_rt_v1_release(ptr %ctx, i64 {old})"
        ));
        self.line(format!("store i64 0, ptr %s{slot}"));
    }
    fn store(&mut self, id: Id, value: &str) {
        self.release(id);
        self.line(format!(
            "store i64 {value}, ptr %s{}",
            self.layout.slots[&id]
        ));
    }
    fn control(&mut self, region: &Region, index: usize) {
        self.layout = FrameLayout::new(region);
        self.line(format!(
            "define void @control_{index}(ptr %ctx, ptr %args, i64 %count) {{\nentry:"
        ));
        for slot in 0..self.layout.slots.len() {
            self.line(format!("%s{slot} = alloca i64\nstore i64 0, ptr %s{slot}"));
        }
        for atom in 0..self.layout.atoms.len() {
            self.line(format!(
                "%a{atom} = alloca i1\nstore i1 false, ptr %a{atom}"
            ));
        }
        for (i, p) in region.parameters.iter().enumerate() {
            let ptr = self.value(format!("getelementptr i64, ptr %args, i64 {i}"));
            let value = self.value(format!("load i64, ptr {ptr}"));
            self.line(format!(
                "store i64 {value}, ptr %s{}",
                self.layout.slots[&p.id]
            ));
        }
        for i in &region.code {
            self.effect(format!(
                "call void @tellur_rt_v1_tick(ptr %ctx, i64 {}, i64 {}, i64 {})",
                i.id, i.provenance.span.line, i.provenance.span.column
            ));
            let guard = self.guard(&i.guard);
            let selected = self.id();
            let next = self.id();
            self.line(format!(
                "br i1 {guard}, label %{selected}, label %{next}\n{selected}:"
            ));
            let mut terminal = false;
            match &i.action {
                Action::Bind(id, e) => {
                    let v = self.expr(e);
                    self.store(*id, &v)
                }
                Action::Assign(p, e) => {
                    let v = self.expr(e);
                    if p.fields.is_empty() {
                        self.store(p.root, &v)
                    } else {
                        let root =
                            self.value(format!("load i64, ptr %s{}", self.layout.slots[&p.root]));
                        let path = self
                            .handles(&p.fields.iter().map(usize::to_string).collect::<Vec<_>>());
                        self.effect(format!("call void @tellur_rt_v1_assign(ptr %ctx, i64 {root}, ptr {path}, i64 {}, i64 {v})", p.fields.len()));
                    }
                }
                Action::Release(ids) => {
                    for id in ids {
                        self.release(*id)
                    }
                }
                Action::Drop(e) | Action::Emit(e) | Action::Fault(e) => {
                    let v = self.expr(e);
                    let op = match &i.action {
                        Action::Drop(_) => "drop",
                        Action::Emit(_) => "emit",
                        _ => "fault",
                    };
                    self.effect(format!("call void @tellur_rt_v1_{op}(ptr %ctx, i64 {v})"));
                    if matches!(&i.action, Action::Fault(_)) {
                        self.line("ret void");
                        terminal = true;
                    }
                }
                Action::Exit(name, args) => {
                    let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
                    let args = self.handles(&values);
                    let (name, len) = self.text(name);
                    self.line(format!("call void @tellur_rt_v1_complete(ptr %ctx, ptr {name}, i64 {len}, ptr {args}, i64 {})\nret void",values.len()));
                    terminal = true;
                }
                Action::Decide {
                    operation: Operation::Region(name),
                    args,
                    arms,
                } => self.region_call(name, args, arms),
                Action::Decide {
                    operation: Operation::Variant(e),
                    arms,
                    ..
                } => {
                    let v = self.expr(e);
                    let b = self.boolean(&v);
                    for arm in arms {
                        let value = if arm.outcome == "true" {
                            b.clone()
                        } else {
                            self.value(format!("xor i1 {b}, true"))
                        };
                        self.line(format!(
                            "store i1 {value}, ptr %a{}",
                            self.layout.atoms[&arm.atom]
                        ));
                    }
                }
                Action::Complete {
                    atom,
                    bindings,
                    release,
                    ..
                } => {
                    let values = bindings
                        .iter()
                        .map(|(id, e)| (*id, self.expr(e)))
                        .collect::<Vec<_>>();
                    for (id, v) in values {
                        self.store(id, &v)
                    }
                    for id in release {
                        self.release(*id)
                    }
                    self.line(format!("store i1 true, ptr %a{}", self.layout.atoms[atom]));
                }
                _ => unreachable!(),
            }
            if !terminal {
                self.line(format!("br label %{next}"));
            }
            self.line(format!("{next}:"));
        }
        self.line("ret void\nerror:\nret void\n}");
    }
}

impl CheckedProgram {
    /// Builds only the admitted synchronous subset. The cached Rust staticlib is
    /// transitional tooling; no per-program Rust is generated or compiled.
    pub fn build_llvm(&self, entry: &str, output: &Path, opt: u8) -> Result<NativeBuild> {
        let source = self.llvm_source(entry)?;
        if std::env::consts::OS != "macos" || std::env::consts::ARCH != "aarch64" {
            return Err(error(
                "LLVM backend currently supports the macOS arm64 target only",
            ));
        }
        let output = if output.is_absolute() {
            output.to_owned()
        } else {
            std::env::current_dir()
                .map_err(|e| error(e.to_string()))?
                .join(output)
        };
        let directory = PathBuf::from(format!("{}.tellur-build", output.display()));
        fs::create_dir_all(&directory).map_err(|e| error(e.to_string()))?;
        let generated = directory.join("program.ll");
        fs::write(&generated, &source).map_err(|e| error(e.to_string()))?;
        let llvm = PathBuf::from(format!("{}.ll", output.display()));
        fs::write(&llvm, &source).map_err(|e| error(e.to_string()))?;
        let runtime = abi_library(&native_target_dir(&output))?;
        let triple = format!("arm64-apple-macosx{}.0", crate::llvm::MACOS_MINIMUM);
        let bytes =
            crate::llvm::compile(&[(generated.display().to_string(), source)], &triple, opt)
                .map_err(error)?;
        let object = directory.join("program.o");
        fs::write(&object, bytes).map_err(|e| error(e.to_string()))?;
        let sdk = Command::new("xcrun")
            .arg("--show-sdk-path")
            .output()
            .map_err(|e| error(e.to_string()))?;
        if !sdk.status.success() {
            return Err(error(
                "cannot find the macOS SDK; bundled link packs follow at station single",
            ));
        }
        crate::llvm::link(&[
            "ld64.lld".into(),
            "-arch".into(),
            "arm64".into(),
            "-platform_version".into(),
            "macos".into(),
            crate::llvm::MACOS_MINIMUM.into(),
            crate::llvm::MACOS_MINIMUM.into(),
            "-syslibroot".into(),
            String::from_utf8_lossy(&sdk.stdout).trim().into(),
            "-lSystem".into(),
            "-o".into(),
            output.display().to_string(),
            object.display().to_string(),
            runtime.display().to_string(),
        ])
        .map_err(error)?;
        Ok(NativeBuild {
            executable: output,
            llvm,
            generated,
        })
    }

    pub fn llvm_source(&self, entry: &str) -> Result<String> {
        self.verify()?;
        if self.region(entry).is_none() {
            return Err(error(format!("unknown entry region {entry}")));
        }
        if self.manifest().rust.is_some() {
            return Err(error("LLVM backend does not yet support Rust adapters"));
        }
        if self.manifest().launch.values().any(|c| c.signals.is_some()) {
            return Err(error(
                "LLVM backend does not yet support launch signal policies",
            ));
        }
        let entries = self.executable_entries(entry)?;
        let types = admitted_types(self.manifest());
        let mut emitter = Emitter {
            globals: String::new(),
            body: String::new(),
            next: 0,
            layout: FrameLayout::default(),
            controls: self
                .regions()
                .enumerate()
                .map(|(i, r)| (r.name.clone(), (i, self.runtime_name(&r.name).to_owned())))
                .collect(),
        };
        let mut metadata = vec![];
        let mut callbacks = vec![];
        for region in self.regions() {
            if region.asynchronous
                || region.boundary.is_some()
                || region.contingency
                || region.is_constructor()
                || region.slots.values().any(|s| !types.contains(&s.ty))
                || region.parameters.iter().any(|s| !types.contains(&s.ty))
                || region
                    .exits
                    .iter()
                    .flat_map(|e| &e.fields)
                    .any(|f| !types.contains(&f.ty))
            {
                return Err(error(format!(
                    "LLVM backend currently supports only synchronous scalar/owned-record controls: {}",
                    self.runtime_name(&region.name)
                )));
            }
            if let Some(instruction) = region.code.iter().find(|i| !supported(i, &types)) {
                return Err(Diagnostic::new(
                    "E_BACKEND",
                    instruction.provenance.span,
                    "LLVM backend does not yet support this operation; use --backend rust",
                ));
            }
            let index = emitter.controls[&region.name].0;
            emitter.control(region, index);
            if let Some(codes) = entries.get(&region.name) {
                callbacks.push(format!("ptr @control_{index}"));
                metadata.push(serde_json::json!({"name":region.name,"display":self.runtime_name(&region.name),"arguments":region.parameters.iter().map(|p|&p.ty).collect::<Vec<_>>(),"exits":codes.iter().collect::<Vec<_>>()}));
            }
        }
        let config=serde_json::json!({"abi":1,"manifest":serde_json::to_string(self.manifest()).map_err(|e|error(e.to_string()))?,"entry":entry,"sites":serde_json::to_string(&self.runtime_sites()).map_err(|e|error(e.to_string()))?,"entries":metadata}).to_string();
        let (config, len) = emitter.text(&config);
        let count = callbacks.len();
        writeln!(
            emitter.globals,
            "@entries = private constant [{count} x ptr] [{}]",
            callbacks.join(", ")
        )
        .unwrap();
        emitter.line(format!("define i32 @main() {{\nentry:\n%status = call i32 @tellur_rt_v1_launch(ptr {config}, i64 {len}, ptr @entries, i64 {count})\nret i32 %status\n}}"));
        Ok(format!(
            "; Tellur direct LLVM backend; private runtime ABI v1\n{DECLARATIONS}\n{}\n{}",
            emitter.globals, emitter.body
        ))
    }
}

fn abi_library(target: &Path) -> Result<PathBuf> {
    let cargo = RUNTIME_CARGO
        .replace("name = \"tellur_runtime\"", "name = \"tellur_runtime_abi\"")
        .replace(
            "path = \"lib.rs\"",
            "path = \"lib.rs\"\ncrate-type = [\"staticlib\"]",
        );
    let lib = format!(
        "#![allow(unused, unreachable_code, dead_code)]\n{}\npub mod abi {{\n{}\n}}",
        runtime_modules(),
        include_str!("abi.rs")
            .lines()
            .filter(|l| !l.starts_with("//!"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut hasher = std::hash::DefaultHasher::new();
    (&cargo, &lib).hash(&mut hasher);
    let hash = format!("{:016x}", hasher.finish());
    // Cargo serializes builds, but releases its target lock before LLD reads the
    // archive. Different runtime versions must therefore have distinct outputs.
    let library = format!("tellur_runtime_abi_{hash}");
    let cargo = cargo.replace("[lib]", &format!("[lib]\nname = \"{library}\""));
    let directory = target.join(format!("tellur-abi/{hash}"));
    fs::create_dir_all(&directory).map_err(|e| error(e.to_string()))?;
    for (name, contents) in [
        ("Cargo.toml", cargo),
        (
            "Cargo.lock",
            LOCK.replace("path_generated", "tellur_runtime_abi"),
        ),
        ("lib.rs", lib),
    ] {
        let file = directory.join(name);
        if fs::read_to_string(&file).ok().as_deref() != Some(contents.as_str()) {
            let temp = directory.join(format!("{name}.{}.partial", std::process::id()));
            fs::write(&temp, contents).map_err(|e| error(e.to_string()))?;
            fs::rename(temp, file).map_err(|e| error(e.to_string()))?;
        }
    }
    let build = target.join("abi-build");
    let result = cargo_command()
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("CARGO_PROFILE_DEV_OPT_LEVEL", "2")
        .arg("build")
        .arg("--locked")
        .arg("--manifest-path")
        .arg(directory.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&build)
        .output()
        .map_err(|e| error(format!("could not build ABI runtime: {e}")))?;
    if !result.status.success() {
        return Err(error(format!(
            "ABI runtime build failed: {}",
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(build.join(format!("debug/lib{library}.a")))
}
