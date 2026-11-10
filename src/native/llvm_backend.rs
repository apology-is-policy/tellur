//! First direct LLVM emitter: owned synchronous values, controls and Boolean path facts.
//! Admission is explicit and occurs before writing/building anything. Every value
//! operation goes through the runtime's private C ABI; guards are native branches.
use super::*;

const DECLARATIONS: &str = r#"
declare i32 @tellur_rt_v1_launch(ptr, i64, ptr, i64)
declare i32 @tellur_rt_v1_ok(ptr)
declare void @tellur_rt_v1_frame(ptr, i64)
declare i64 @tellur_rt_v1_local(ptr, i64, ptr, i64, i32)
declare void @tellur_rt_v1_store(ptr, i64, ptr, i64, i64)
declare void @tellur_rt_v1_release_local(ptr, i64)
declare i64 @tellur_rt_v1_reference(ptr, i64, ptr, i64, i32, ptr, i64)
declare i64 @tellur_rt_v1_deref(ptr, i64, ptr, i64, i32)
declare void @tellur_rt_v1_assign_through(ptr, i64, ptr, i64, i64)
declare i64 @tellur_rt_v1_evidence(ptr, i64, ptr, i64)
declare i64 @tellur_rt_v1_borrowed_variant(ptr, i64)
declare i64 @tellur_rt_v1_value(ptr, i32, i64, ptr, i64)
declare i64 @tellur_rt_v1_copy(ptr, i64)
declare i64 @tellur_rt_v1_record(ptr, ptr, i64, ptr, i64)
declare i64 @tellur_rt_v1_construct(ptr, ptr, i64, ptr, i64, ptr, i64)
declare i64 @tellur_rt_v1_variant(ptr, i64)
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
declare i64 @tellur_rt_v1_primitive(ptr, ptr, i64, ptr, i64)
declare i64 @tellur_rt_v1_collection(ptr, ptr, i64, ptr, i64)
declare i64 @tellur_rt_v1_length(ptr, i64)
declare i32 @tellur_rt_v1_route(ptr, i64, ptr, i64, ptr, i64)
declare void @tellur_rt_v1_unmatched(ptr, i64)
"#;
fn admitted_types(manifest: &crate::manifest::Manifest, references: bool) -> BTreeSet<String> {
    let mut types: BTreeSet<String> = ["Bool", "U32", "String", "Bytes", "Unit"]
        .into_iter()
        .map(String::from)
        .collect();
    loop {
        let before = types.len();
        for ty in &manifest.types {
            let (name, fields) = match ty {
                crate::manifest::TypeDef::Record { name, fields } => {
                    (name, fields.iter().collect::<Vec<_>>())
                }
                crate::manifest::TypeDef::Variant { name, constructors }
                | crate::manifest::TypeDef::Outcome { name, constructors } => {
                    (name, constructors.iter().flat_map(|c| &c.fields).collect())
                }
                crate::manifest::TypeDef::Opaque { name, .. } => (name, vec![]),
            };
            if fields.iter().all(|f| {
                if references {
                    type_supported(&f.ty, &types)
                } else {
                    owned_type_supported(&f.ty, &types)
                }
            }) {
                types.insert(name.clone());
            }
        }
        if types.len() == before {
            return types;
        }
    }
}
fn type_supported(ty: &str, types: &BTreeSet<String>) -> bool {
    types.contains(ty)
        || crate::collection_support::Collection::parse(ty)
            .is_some_and(|c| c.types().iter().all(|t| type_supported(t, types)))
        || ty
            .strip_prefix("&mutable ")
            .or_else(|| ty.strip_prefix('&'))
            .or_else(|| ty.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')))
            .is_some_and(|inner| type_supported(inner, types))
}
fn owned_type_supported(ty: &str, types: &BTreeSet<String>) -> bool {
    types.contains(ty)
        || crate::collection_support::Collection::parse(ty)
            .is_some_and(|c| c.types().iter().all(|t| owned_type_supported(t, types)))
        || ty
            .strip_prefix("Option<")
            .and_then(|t| t.strip_suffix('>'))
            .is_some_and(|inner| owned_type_supported(inner, types))
}
fn value_supported(v: &Value, types: &BTreeSet<String>) -> bool {
    match v {
        Value::Bool(_) | Value::U32(_) | Value::String(_) | Value::Bytes(_) | Value::Unit => true,
        Value::Construct { ty, fields, .. } => {
            type_supported(ty, types) && fields.iter().all(|v| value_supported(v, types))
        }
        _ => false,
    }
}
fn expression_supported(e: &Expr, types: &BTreeSet<String>) -> bool {
    match e {
        Expr::Value(v) => value_supported(v, types),
        Expr::Move(_) | Expr::Place(_) => true,
        Expr::Reference(_, ty) => type_supported(ty, types),
        Expr::WithReferenceEvidence(value, _) => expression_supported(value, types),
        Expr::Pure(_, args) => args.iter().all(|e| expression_supported(e, types)),
        Expr::Construct(ty, _, args) => {
            type_supported(ty, types) && args.iter().all(|e| expression_supported(e, types))
        }
        Expr::Record(ty, fields) => {
            type_supported(ty, types) && fields.iter().all(|(_, e)| expression_supported(e, types))
        }
        Expr::Project(e, _) | Expr::Length(e) => expression_supported(e, types),
        Expr::Not(e) => expression_supported(e, types),
        Expr::Binary(_, a, b) => expression_supported(a, types) && expression_supported(b, types),
    }
}
fn supported(
    i: &Instruction,
    types: &BTreeSet<String>,
    primitive_types: &BTreeSet<String>,
    manifest: &crate::manifest::Manifest,
) -> bool {
    match &i.action {
        Action::Bind(_, e) | Action::Drop(e) | Action::Emit(e) | Action::Fault(e) => {
            expression_supported(e, types)
        }
        Action::Assign(_, e) => expression_supported(e, types),
        Action::Release(_) => true,
        Action::Exit(_, args) => args.iter().all(|e| expression_supported(e, types)),
        Action::Complete {
            bindings,
            unwind: None,
            ..
        } => bindings.iter().all(|(_, e)| expression_supported(e, types)),
        Action::Decide {
            operation: Operation::Region(_) | Operation::Collection(_),
            args,
            ..
        } => args.iter().all(|e| expression_supported(e, types)),
        Action::Decide {
            operation: Operation::Primitive(name),
            args,
            ..
        } => {
            args.iter().all(|e| expression_supported(e, types))
                && manifest
                    .primitives
                    .iter()
                    .find(|p| &p.name == name)
                    .is_some_and(|p| {
                        !p.asynchronous
                            && p.rust.is_none()
                            && p.returns
                            && p.parameters
                                .iter()
                                .all(|p| owned_type_supported(&p.ty, primitive_types))
                            && p.outcomes
                                .iter()
                                .flat_map(|o| &o.fields)
                                .all(|f| owned_type_supported(&f.ty, primitive_types))
                    })
        }
        Action::Decide {
            operation: Operation::Variant(e) | Operation::BorrowedVariant(e),
            args,
            ..
        } => args.is_empty() && expression_supported(e, types),
        _ => false,
    }
}
struct Emitter {
    globals: String,
    body: String,
    allocations: String,
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
    // Sizes are static. Keep scratch in the entry block so later backedges do
    // not allocate again and grow the native stack at O0.
    fn scratch(&mut self, ty: &str) -> String {
        let name = format!("%{}", self.id());
        writeln!(self.allocations, "{name} = alloca {ty}").unwrap();
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
        let array = self.scratch(&format!("[{} x i64]", values.len()));
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
        self.route(&result, arms);
    }
    fn runtime_operation(&mut self, abi: &str, name: &str, args: &[Expr], arms: &[Arm]) {
        let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
        let args = self.handles(&values);
        let (name, len) = self.text(name);
        let result = self.call(format!(
            "call i64 @tellur_rt_v1_{abi}(ptr %ctx, ptr {name}, i64 {len}, ptr {args}, i64 {})",
            values.len()
        ));
        self.route(&result, arms);
    }
    fn route(&mut self, result: &str, arms: &[Arm]) {
        let delivered = self.id();
        for arm in arms {
            let payload = if arm.slots.is_empty() {
                "null".into()
            } else {
                self.scratch(&format!("[{} x i64]", arm.slots.len()))
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
    fn construct(&mut self, ty: &str, name: &str, values: &[String]) -> String {
        let args = self.handles(values);
        let (ty, ty_len) = self.text(ty);
        let (name, name_len) = self.text(name);
        self.call(format!("call i64 @tellur_rt_v1_construct(ptr %ctx, ptr {ty}, i64 {ty_len}, ptr {name}, i64 {name_len}, ptr {args}, i64 {})", values.len()))
    }
    fn embedded(&mut self, value: &Value) -> String {
        match value {
            Value::Bool(b) => self.literal(1, u8::from(*b), b""),
            Value::U32(n) => self.literal(2, n, b""),
            Value::String(s) => self.literal(3, 0, s.as_bytes()),
            Value::Bytes(b) => self.literal(4, 0, b),
            Value::Unit => self.literal(0, 0, b""),
            Value::Construct { ty, name, fields } => {
                let fields = fields.iter().map(|v| self.embedded(v)).collect::<Vec<_>>();
                self.construct(ty, name, &fields)
            }
            _ => unreachable!(),
        }
    }
    fn boolean(&mut self, value: &str) -> String {
        let v = self.call(format!(
            "call i32 @tellur_rt_v1_bool(ptr %ctx, i64 {value})"
        ));
        self.value(format!("icmp ne i32 {v}, 0"))
    }
    fn expr(&mut self, e: &Expr) -> String {
        match e {
            Expr::Value(v) => self.embedded(v),
            Expr::Place(p) | Expr::Move(p) => {
                let slot = self.layout.slots[&p.root];
                let path = self.handles(&p.fields.iter().map(usize::to_string).collect::<Vec<_>>());
                let operation = if p.indirect { "deref" } else { "local" };
                self.call(format!("call i64 @tellur_rt_v1_{operation}(ptr %ctx, i64 {slot}, ptr {path}, i64 {}, i32 {})", p.fields.len(), u8::from(matches!(e, Expr::Move(_)))))
            }
            Expr::Reference(p, ty) => {
                let slot = self.layout.slots[&p.root];
                let path = self.handles(&p.fields.iter().map(usize::to_string).collect::<Vec<_>>());
                let (ty, len) = self.text(ty);
                self.call(format!("call i64 @tellur_rt_v1_reference(ptr %ctx, i64 {slot}, ptr {path}, i64 {}, i32 {}, ptr {ty}, i64 {len})", p.fields.len(), u8::from(p.indirect)))
            }
            Expr::WithReferenceEvidence(value, evidence) => {
                let fresh = matches!(value.as_ref(), Expr::Reference(p, _) if !p.indirect);
                let handle = self.expr(value);
                let kind = match evidence {
                    ReferenceEvidence::Partial(_) => 0,
                    ReferenceEvidence::Complete(_) => 1,
                    ReferenceEvidence::Incoming => 2,
                    ReferenceEvidence::GuardedInput { .. } => 3,
                    ReferenceEvidence::CarrierInput { .. } => 4,
                };
                let mut words = vec![kind, usize::from(fresh && kind < 2)];
                if kind >= 3 {
                    words.push(0);
                }
                let origin_start = words.len();
                for origin in evidence.origins() {
                    words.extend([
                        self.layout.slots[&origin.place.root],
                        usize::from(origin.place.indirect)
                            | (usize::from(origin.guard) << 1)
                            | (usize::from(origin.opaque) << 2),
                        origin.place.fields.len(),
                    ]);
                    words.extend(&origin.place.fields);
                }
                if kind >= 3 {
                    words[2] = words.len() - origin_start;
                }
                match evidence {
                    ReferenceEvidence::GuardedInput { leaf, holder, .. } => {
                        // Match the Rust emitter: holder.fields is the actual
                        // path inside the original borrowed aggregate, separate
                        // from symbolic indirect/guard/opaque origin paths.
                        words.extend([
                            self.layout.slots[leaf],
                            self.layout.slots[&holder.root],
                            holder.fields.len(),
                        ]);
                        words.extend(&holder.fields);
                    }
                    ReferenceEvidence::CarrierInput { holder, leaves, .. } => {
                        words.extend([self.layout.slots[holder], leaves.len()]);
                        for leaf in leaves {
                            words.extend([self.layout.slots[&leaf.leaf], leaf.fields.len()]);
                            words.extend(&leaf.fields);
                        }
                    }
                    _ => {}
                }
                let name = self.id();
                let values = words
                    .iter()
                    .map(|n| format!("i64 {n}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(
                    self.globals,
                    "@{name} = private constant [{} x i64] [{values}]",
                    words.len()
                )
                .unwrap();
                self.call(format!(
                    "call i64 @tellur_rt_v1_evidence(ptr %ctx, i64 {handle}, ptr @{name}, i64 {})",
                    words.len()
                ))
            }
            Expr::Construct(ty, name, args) => {
                let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
                self.construct(ty, name, &values)
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
            Expr::Length(e) => {
                let value = self.expr(e);
                self.call(format!(
                    "call i64 @tellur_rt_v1_length(ptr %ctx, i64 {value})"
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
                let result = self.scratch("i1");
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
        self.effect(format!(
            "call void @tellur_rt_v1_release_local(ptr %ctx, i64 {slot})"
        ));
    }
    fn store(&mut self, id: Id, value: &str) {
        self.effect(format!(
            "call void @tellur_rt_v1_store(ptr %ctx, i64 {}, ptr null, i64 0, i64 {value})",
            self.layout.slots[&id]
        ));
    }
    fn control(&mut self, region: &Region, index: usize) {
        self.layout = FrameLayout::new(region);
        self.line(format!(
            "define void @control_{index}(ptr %ctx, ptr %args, i64 %count) {{\nentry:"
        ));
        let entry = self.body.len();
        self.effect(format!(
            "call void @tellur_rt_v1_frame(ptr %ctx, i64 {})",
            self.layout.slots.len()
        ));
        for atom in 0..self.layout.atoms.len() {
            writeln!(self.allocations, "%a{atom} = alloca i1").unwrap();
            self.line(format!("store i1 false, ptr %a{atom}"));
        }
        for (i, p) in region.parameters.iter().enumerate() {
            let ptr = self.value(format!("getelementptr i64, ptr %args, i64 {i}"));
            let value = self.value(format!("load i64, ptr {ptr}"));
            self.store(p.id, &value);
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
                    if p.fields.is_empty() && !p.indirect {
                        self.store(p.root, &v)
                    } else {
                        let slot = self.layout.slots[&p.root];
                        let path = self
                            .handles(&p.fields.iter().map(usize::to_string).collect::<Vec<_>>());
                        let operation = if p.indirect {
                            "assign_through"
                        } else {
                            "store"
                        };
                        self.effect(format!("call void @tellur_rt_v1_{operation}(ptr %ctx, i64 {slot}, ptr {path}, i64 {}, i64 {v})", p.fields.len()));
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
                    operation: Operation::Primitive(name),
                    args,
                    arms,
                } => self.runtime_operation("primitive", name, args, arms),
                Action::Decide {
                    operation: Operation::Collection(name),
                    args,
                    arms,
                } => self.runtime_operation("collection", name, args, arms),
                Action::Decide {
                    operation: Operation::Variant(e),
                    arms,
                    ..
                } => {
                    let v = self.expr(e);
                    let outcome =
                        self.call(format!("call i64 @tellur_rt_v1_variant(ptr %ctx, i64 {v})"));
                    self.route(&outcome, arms);
                }
                Action::Decide {
                    operation: Operation::BorrowedVariant(e),
                    arms,
                    ..
                } => {
                    let value = self.expr(e);
                    let outcome = self.call(format!(
                        "call i64 @tellur_rt_v1_borrowed_variant(ptr %ctx, i64 {value})"
                    ));
                    self.route(&outcome, arms);
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
        self.body
            .insert_str(entry, &std::mem::take(&mut self.allocations));
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
        let types = admitted_types(self.manifest(), true);
        // Host operations remain reference-free even when controls admit carriers.
        let primitive_types = admitted_types(self.manifest(), false);
        let mut emitter = Emitter {
            globals: String::new(),
            body: String::new(),
            allocations: String::new(),
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
                || region
                    .slots
                    .values()
                    .any(|s| !type_supported(&s.ty, &types))
                || region
                    .parameters
                    .iter()
                    .any(|s| !type_supported(&s.ty, &types))
                || region
                    .exits
                    .iter()
                    .flat_map(|e| &e.fields)
                    .any(|f| !type_supported(&f.ty, &types))
            {
                return Err(error(format!(
                    "LLVM backend currently supports only synchronous scalar/aggregate controls: {}",
                    self.runtime_name(&region.name)
                )));
            }
            if let Some(instruction) = region
                .code
                .iter()
                .find(|i| !supported(i, &types, &primitive_types, self.manifest()))
            {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn carrier_admission_does_not_enable_reference_bearing_primitive_signatures() {
        use crate::manifest::{Manifest, TypeDef};
        let mut manifest = Manifest::from_json(r#"{
            "version":"0.1",
            "types":[{"kind":"record","name":"packet","fields":[{"name":"value","type":"U32"}]}],
            "primitives":[{"name":"custom","parameters":[],"outcomes":[{"name":"ready","fields":[{"name":"value","type":"Option<packet>"}]}],"effects":[],"returns":true}]
        }"#).unwrap();
        let checked=crate::compile("control main() exits done {delta custom() {ready {auto value}=>ready} [ready]:drop(value);exit done;}",manifest.clone()).unwrap();
        let instruction=checked.region("main").unwrap().code.iter().find(|i|matches!(&i.action,Action::Decide {operation:Operation::Primitive(name),..} if name=="custom")).unwrap();
        assert!(supported(
            instruction,
            &admitted_types(&manifest, true),
            &admitted_types(&manifest, false),
            &manifest
        ));
        // Exercise the backend gate independently of the frontend's own current
        // host restrictions, including nested reference-bearing result types.
        for ty in [
            "&U32",
            "&mutable U32",
            "Option<&U32>",
            "vector<&U32>",
            "map<String,array<&U32,2>>",
        ] {
            let TypeDef::Record { fields, .. } = &mut manifest.types[0] else {
                panic!()
            };
            fields[0].ty = ty.into();
            assert!(type_supported(
                "Option<packet>",
                &admitted_types(&manifest, true)
            ));
            assert!(!supported(
                instruction,
                &admitted_types(&manifest, true),
                &admitted_types(&manifest, false),
                &manifest
            ));
        }
    }
}
