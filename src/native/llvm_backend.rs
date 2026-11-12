//! Direct LLVM emission for the synchronous profile and bounded retained controls.
//! Admission is explicit and occurs before writing/building anything. Every value
//! operation goes through the runtime's private C ABI; guards are native branches.
use super::*;

const DECLARATIONS: &str = r#"
declare i32 @tellur_rt_v1_launch(ptr, i64, ptr, i64)
declare void @tellur_rt_v4_run_entry(ptr, ptr, i64, i64, ptr, i64, i64, ptr, i64, i64)
declare void @tellur_rt_v1_await(ptr, i64, ptr, i64)
declare void @tellur_rt_v1_run_child(ptr, i64, ptr, i64)
declare void @tellur_rt_v1_construct_child(ptr, i64, i64, ptr, i64)
declare i64 @tellur_rt_v1_defer(ptr, i64, ptr, i64, ptr, i64)
declare void @tellur_rt_v1_await_task(ptr, i64, i64)
declare void @tellur_rt_v1_spawn(ptr, i64, i64, ptr, i64)
declare void @tellur_rt_v1_join(ptr, i64, i64)
declare void @tellur_rt_v1_cancel(ptr, i64, i64, i64)
declare i64 @tellur_rt_v1_task_set(ptr, i64)
declare void @tellur_rt_v1_spawn_into(ptr, i64, i64, i64, ptr, i64)
declare i8 @tellur_rt_v1_next(ptr, i64, i64)
declare void @tellur_rt_v1_cancel_set(ptr, i64, i64, i64)
declare void @tellur_rt_v1_host(ptr, i64, ptr, i64)
declare void @tellur_rt_v1_invalid_resume(ptr)
declare i32 @tellur_rt_v1_resume_state(ptr, i32)
declare void @tellur_rt_v1_scope(ptr, i64, i32)
declare void @tellur_rt_v1_loop(ptr, i64, i32, ptr, i64)
%compiled_control = type { ptr, i64, ptr, i64, i64, ptr, i64, i64, i64, i64, ptr, ptr, i64, ptr, i64 }
%compiled_child = type { i64, i64 }
%compiled_control_v3 = type { %compiled_control, ptr }
%compiled_control_v4 = type { %compiled_control_v3, i32 }
%compiled_slots = type { ptr, i64 }
%compiled_scope = type { i32, ptr, i64, ptr, i64 }
%compiled_boundary = type { ptr, i64, i32, i32, i32, %compiled_slots, %compiled_slots, %compiled_slots, %compiled_slots, i64 }
%compiled_loop = type { i64, i32, i32, %compiled_slots, %compiled_slots, ptr }
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
declare void @tellur_rt_v1_construct_call(ptr, ptr, ptr, i64, ptr, i64, i64)
declare void @tellur_rt_v1_constructed(ptr, i64)
declare i64 @tellur_rt_v1_lexical_begin(ptr, ptr, i64, ptr, i64, i64, i64, i64, i64)
declare i32 @tellur_rt_v1_lexical_catch(ptr, i64)
declare void @tellur_rt_v1_lexical_end(ptr, i64)
declare void @tellur_rt_v1_process(ptr, i32)
declare void @tellur_rt_v1_supervise_children(ptr, i64, i64, ptr, i64)
declare void @tellur_rt_v1_settled(ptr)
declare void @tellur_rt_v1_escalate(ptr)
declare void @tellur_rt_v1_supervise(ptr, ptr, ptr, ptr, i64, ptr, i64, ptr, i64, ptr, i64)
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
        Action::Release(_) | Action::Terminate | Action::Escalate | Action::TerminateProcess(_) => {
            true
        }
        Action::Boundary { body, handler, .. } => body
            .iter()
            .chain(handler)
            .all(|i| supported(i, types, primitive_types, manifest)),
        Action::Supervise { args, .. } => args.iter().all(|e| expression_supported(e, types)),
        Action::Exit(_, args) => args.iter().all(|e| expression_supported(e, types)),
        Action::Complete { bindings, .. } => {
            bindings.iter().all(|(_, e)| expression_supported(e, types))
        }
        Action::Again { args, .. } | Action::Recover { args, .. } => {
            args.iter().all(|e| expression_supported(e, types))
        }
        Action::Loop {
            parameters, body, ..
        } => {
            parameters
                .iter()
                .all(|(_, e)| expression_supported(e, types))
                && body
                    .iter()
                    .all(|i| supported(i, types, primitive_types, manifest))
        }
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
                && primitive_supported(manifest, name, primitive_types, false)
        }
        Action::Decide {
            operation: Operation::Variant(e) | Operation::BorrowedVariant(e),
            args,
            ..
        } => args.is_empty() && expression_supported(e, types),
        _ => false,
    }
}
fn frame_size(index: usize) -> String {
    format!("ptrtoint (ptr getelementptr (%frame_{index}, ptr null, i32 1) to i64)")
}
fn frame_align(index: usize) -> String {
    format!(
        "ptrtoint (ptr getelementptr ({{ i8, %frame_{index} }}, ptr null, i32 0, i32 1) to i64)"
    )
}
fn frame_offset(index: usize, field: usize) -> String {
    format!("ptrtoint (ptr getelementptr (%frame_{index}, ptr null, i32 0, i32 {field}) to i64)")
}
fn primitive_supported(
    manifest: &crate::manifest::Manifest,
    name: &str,
    types: &BTreeSet<String>,
    asynchronous: bool,
) -> bool {
    manifest
        .primitives
        .iter()
        .find(|p| p.name == name)
        .is_some_and(|p| {
            p.asynchronous == asynchronous
                && p.rust.is_none()
                && p.returns
                && p.parameters
                    .iter()
                    .all(|p| owned_type_supported(&p.ty, types))
                && p.outcomes
                    .iter()
                    .flat_map(|o| &o.fields)
                    .all(|f| owned_type_supported(&f.ty, types))
        })
}
fn retained_target<'a>(
    i: &'a Instruction,
    resumables: &BTreeMap<String, usize>,
) -> Option<&'a String> {
    match &i.action {
        Action::Decide {
            operation:
                Operation::Region(name)
                | Operation::Run(name)
                | Operation::Await(name)
                | Operation::AwaitTask(name),
            ..
        } if resumables.contains_key(name) => Some(name),
        _ => None,
    }
}
// Supervision owns two distinct inline children and has no outcome-delivery state.
// Use the same inventory for frame fields and recursive-layout cycle checks.
fn retained_child_targets<'a>(
    instruction: &'a Instruction,
    resumables: &BTreeMap<String, usize>,
    boundaries: &'a BTreeMap<String, (String, Vec<usize>)>,
) -> Vec<&'a String> {
    if let Some(target) = retained_target(instruction, resumables) {
        vec![target]
    } else if let Action::Supervise { boundary, .. } = &instruction.action {
        let handler = &boundaries[boundary].0;
        debug_assert!(resumables.contains_key(boundary) && resumables.contains_key(handler));
        vec![boundary, handler]
    } else {
        vec![]
    }
}
fn reaches_retained(i: &Instruction, names: &BTreeSet<String>) -> bool {
    match &i.action {
        Action::Decide {
            operation:
                Operation::Region(name)
                | Operation::Run(name)
                | Operation::Await(name)
                | Operation::AwaitTask(name),
            ..
        } => names.contains(name),
        Action::Decide {
            operation:
                Operation::Defer(_)
                | Operation::Spawn(_)
                | Operation::Join(_)
                | Operation::Cancel(_)
                | Operation::SpawnInto(_)
                | Operation::Next(_),
            ..
        } => true,
        Action::Supervise { boundary, .. } => names.contains(boundary),
        Action::Loop { body, .. } => body.iter().any(|i| reaches_retained(i, names)),
        Action::Boundary { body, handler, .. } => body
            .iter()
            .chain(handler)
            .any(|i| reaches_retained(i, names)),
        _ => false,
    }
}
// The same recursive inventory drives state planning, inline child metadata and
// cycle checks. A nested call must not disappear from any of those three views.
fn retained_instructions(code: &[Instruction]) -> Vec<&Instruction> {
    fn append<'a>(code: &'a [Instruction], out: &mut Vec<&'a Instruction>) {
        for instruction in code {
            out.push(instruction);
            match &instruction.action {
                Action::Loop { body, .. } => append(body, out),
                Action::Boundary { body, handler, .. } => {
                    append(body, out);
                    append(handler, out);
                }
                _ => {}
            }
        }
    }
    let mut out = vec![];
    append(code, &mut out);
    out
}

fn resumable_controls(
    program: &CheckedProgram,
    boundaries: &BTreeMap<String, (String, Vec<usize>)>,
) -> Result<BTreeMap<String, usize>> {
    let mut names = program
        .regions()
        .filter(|r| r.asynchronous)
        .map(|r| r.name.clone())
        .collect::<BTreeSet<_>>();
    loop {
        let before = names.len();
        for region in program.regions() {
            if region.code.iter().any(|i| reaches_retained(i, &names)) {
                names.insert(region.name.clone());
            }
            if names.contains(&region.name) {
                for instruction in retained_instructions(&region.code) {
                    if let Action::Supervise { boundary, .. } = &instruction.action {
                        names.insert(boundary.clone());
                        names.insert(boundaries[boundary].0.clone());
                    }
                }
            }
        }
        if names.len() == before {
            break;
        }
    }
    let result: BTreeMap<_, _> = program
        .regions()
        .filter(|r| names.contains(&r.name))
        .enumerate()
        .map(|(i, r)| (r.name.clone(), i))
        .collect();
    if result.len() > 65536 {
        return Err(error(
            "LLVM retained control table exceeds its current bound",
        ));
    }
    let mut edges = vec![vec![]; result.len()];
    let mut incoming = vec![0usize; result.len()];
    for region in program.regions().filter(|r| result.contains_key(&r.name)) {
        for instruction in retained_instructions(&region.code) {
            for target in retained_child_targets(instruction, &result, boundaries) {
                edges[result[&region.name]].push(result[target]);
                incoming[result[target]] += 1;
            }
        }
    }
    let mut ready = incoming
        .iter()
        .enumerate()
        .filter_map(|(i, n)| (*n == 0).then_some(i))
        .collect::<Vec<_>>();
    let mut count = 0;
    while let Some(i) = ready.pop() {
        count += 1;
        for child in &edges[i] {
            incoming[*child] -= 1;
            if incoming[*child] == 0 {
                ready.push(*child);
            }
        }
    }
    if count != result.len() {
        return Err(error(
            "LLVM backend does not yet support recursive by-value async frames; use --backend rust",
        ));
    }
    Ok(result)
}
fn retained_supported(
    i: &Instruction,
    host_operation: Option<&str>,
    types: &BTreeSet<String>,
    primitive_types: &BTreeSet<String>,
    manifest: &crate::manifest::Manifest,
) -> bool {
    match &i.action {
        Action::Bind(..)
        | Action::Assign(..)
        | Action::Release(..)
        | Action::Drop(..)
        | Action::Emit(..)
        | Action::Fault(..)
        | Action::Terminate
        | Action::Escalate
        | Action::Exit(..)
        | Action::TerminateProcess(..) => supported(i, types, primitive_types, manifest),
        Action::Supervise { args, .. } => args.iter().all(|e| expression_supported(e, types)),
        Action::Boundary { body, handler, .. } => body
            .iter()
            .chain(handler)
            .all(|i| retained_supported(i, host_operation, types, primitive_types, manifest)),
        Action::Complete { .. } | Action::Again { .. } | Action::Recover { .. } => {
            supported(i, types, primitive_types, manifest)
        }
        Action::Loop {
            parameters, body, ..
        } => {
            parameters
                .iter()
                .all(|(_, e)| expression_supported(e, types))
                && body.iter().all(|i| {
                    retained_supported(i, host_operation, types, primitive_types, manifest)
                })
        }
        Action::Decide {
            operation:
                Operation::Run(_)
                | Operation::Await(_)
                | Operation::Defer(_)
                | Operation::AwaitTask(_)
                | Operation::Spawn(_)
                | Operation::Join(_)
                | Operation::Cancel(_)
                | Operation::SpawnInto(_)
                | Operation::Next(_),
            args,
            ..
        } => args.iter().all(|e| expression_supported(e, types)),
        Action::Decide {
            operation: Operation::Yield,
            ..
        } => true,
        Action::Decide {
            operation: Operation::Primitive(name),
            args,
            ..
        } if host_operation == Some(name.as_str()) => {
            primitive_supported(manifest, name, primitive_types, true)
                && args.iter().all(|e| expression_supported(e, types))
        }
        Action::Decide { .. } => supported(i, types, primitive_types, manifest),
    }
}

#[derive(Clone)]
struct RetainedLoopTarget {
    index: usize,
    body: u32,
    complete: u32,
    parameters: Vec<Id>,
    local_slots: Vec<Id>,
    local_atoms: Vec<Id>,
}
struct RetainedBoundary<'a> {
    instruction: &'a Instruction,
    body: u32,
    handler: u32,
    after: u32,
    end: u32,
}
struct RetainedPlan<'a> {
    code: Vec<&'a Instruction>,
    next: BTreeMap<Id, u32>,
    loops: BTreeMap<Id, RetainedLoopTarget>,
    boundaries: Vec<RetainedBoundary<'a>>,
    invalid: u32,
}
impl<'a> RetainedPlan<'a> {
    fn new(code: &'a [Instruction]) -> Result<Self> {
        let flat = retained_instructions(code);
        let boundary_count = flat
            .iter()
            .filter(|i| matches!(i.action, Action::Boundary { .. }))
            .count();
        // 0 is initial, then instruction and delivery ranges, administrative
        // boundary ends, and one real invalid state. Metadata never uses MAX.
        let count = flat
            .len()
            .checked_mul(2)
            .and_then(|n| n.checked_add(boundary_count))
            .and_then(|n| n.checked_add(2))
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| error("LLVM retained resume-state range exceeds u32"))?;
        let states = flat
            .iter()
            .enumerate()
            .map(|(n, i)| (i.id, (n + 1) as u32))
            .collect::<BTreeMap<_, _>>();
        let mut plan = Self {
            code: flat,
            next: BTreeMap::new(),
            loops: BTreeMap::new(),
            boundaries: vec![],
            invalid: count - 1,
        };
        plan.block(code, &states, plan.invalid);
        Ok(plan)
    }
    fn block(&mut self, code: &'a [Instruction], states: &BTreeMap<Id, u32>, after: u32) {
        for (index, instruction) in code.iter().enumerate() {
            let next = code.get(index + 1).map_or(after, |i| states[&i.id]);
            self.next.insert(instruction.id, next);
            match &instruction.action {
                Action::Loop {
                    id,
                    parameters,
                    local_slots,
                    local_atoms,
                    body,
                    ..
                } => {
                    self.loops.insert(
                        *id,
                        RetainedLoopTarget {
                            index: self.loops.len(),
                            body: body.first().map_or(self.invalid, |i| states[&i.id]),
                            complete: next,
                            parameters: parameters.iter().map(|(id, _)| *id).collect(),
                            local_slots: local_slots.clone(),
                            local_atoms: local_atoms.clone(),
                        },
                    );
                    self.block(body, states, self.invalid);
                }
                Action::Boundary { body, handler, .. } => {
                    let end = (2 * self.code.len() + self.boundaries.len() + 1) as u32;
                    self.boundaries.push(RetainedBoundary {
                        instruction,
                        body: body.first().map_or(end, |i| states[&i.id]),
                        handler: handler.first().map_or(self.invalid, |i| states[&i.id]),
                        after: next,
                        end,
                    });
                    self.block(body, states, end);
                    self.block(handler, states, self.invalid);
                }
                _ => {}
            }
        }
    }
}

#[derive(Clone)]
struct LoopTarget {
    restart: String,
    complete: String,
    arguments: String,
    arity: usize,
    scope_depth: usize,
}
#[derive(Clone)]
struct ScopeTarget {
    handle: String,
    outer_error: String,
}
struct Emitter {
    frame_types: String,
    retained: bool,
    current_descriptor: usize,
    async_primitives: BTreeSet<String>,
    resumables: BTreeMap<String, usize>,
    descriptors: Vec<String>,
    inline_children: BTreeMap<Id, (usize, usize, usize)>,
    supervision_children: BTreeMap<Id, (usize, usize)>,
    retained_loops: BTreeMap<Id, RetainedLoopTarget>,
    retained_boundaries: BTreeMap<Id, usize>,
    task_set_slots: BTreeSet<Id>,
    resume_next: u32,
    delivery_pc: u32,
    globals: String,
    body: String,
    allocations: String,
    next: usize,
    layout: FrameLayout,
    controls: BTreeMap<String, (usize, String)>,
    constructors: BTreeSet<String>,
    boundaries: BTreeMap<String, (String, Vec<usize>)>,
    constructing: bool,
    loops: BTreeMap<Id, LoopTarget>,
    scopes: Vec<ScopeTarget>,
    error_target: String,
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
        self.check_to(self.error_target.clone());
    }
    fn check_to(&mut self, error: String) {
        let ok = self.value("call i32 @tellur_rt_v1_ok(ptr %ctx)");
        let ok = self.value(format!("icmp ne i32 {ok}, 0"));
        let next = self.id();
        self.line(format!(
            "br i1 {ok}, label %{next}, label %{error}\n{next}:"
        ));
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
        self.write_handles(&array, values);
        array
    }
    fn write_handles(&mut self, array: &str, values: &[String]) {
        for (i, v) in values.iter().enumerate() {
            let p = self.value(format!("getelementptr i64, ptr {array}, i64 {i}"));
            self.line(format!("store i64 {v}, ptr {p}"));
        }
    }
    fn region_call(&mut self, name: &str, args: &[Expr], arms: &[Arm]) {
        let (index, display) = self.controls[name].clone();
        let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
        let args = self.handles(&values);
        let constructor = self.constructors.contains(name);
        let (name, len) = self.text(&display);
        if constructor {
            assert_eq!(arms.len(), 1);
            let arm = &arms[0];
            assert_eq!(arm.outcome, "$constructed");
            assert_eq!(arm.slots.len(), 1);
            self.effect(format!(
                "call void @tellur_rt_v1_construct_call(ptr %ctx, ptr @control_{index}, ptr {name}, i64 {len}, ptr {args}, i64 {}, i64 {})",
                values.len(), self.layout.slots[&arm.slots[0]]
            ));
            self.line(format!(
                "store i1 true, ptr %a{}",
                self.layout.atoms[&arm.atom]
            ));
            return;
        }
        let result = self.call(format!(
            "call i64 @tellur_rt_v1_call(ptr %ctx, ptr @control_{index}, ptr {name}, i64 {len}, ptr {args}, i64 {})",
            values.len()
        ));
        self.route(&result, arms);
    }
    fn supervise(&mut self, name: &str, args: &[Expr]) {
        let (handler, retained) = self.boundaries[name].clone();
        let (work, display) = self.controls[name].clone();
        let (handler, handler_display) = self.controls[&handler].clone();
        let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
        let args = self.handles(&values);
        let slots = self.handles(&retained.iter().map(usize::to_string).collect::<Vec<_>>());
        let (name, len) = self.text(&display);
        let (handler_name, handler_len) = self.text(&handler_display);
        self.effect(format!(
            "call void @tellur_rt_v1_supervise(ptr %ctx, ptr @control_{work}, ptr @control_{handler}, ptr {name}, i64 {len}, ptr {handler_name}, i64 {handler_len}, ptr {args}, i64 {}, ptr {slots}, i64 {})",
            values.len(), retained.len()
        ));
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
        if let Some(producer) = crate::manifest::task_set_inner(ty) {
            debug_assert!(self.retained && name == "tasks" && values.is_empty());
            let producer = self.resumables[producer];
            return self.call(format!(
                "call i64 @tellur_rt_v1_task_set(ptr %ctx, i64 {producer})"
            ));
        }
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
        self.constructing = region.is_constructor();
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
        self.instructions(&region.code);
        debug_assert!(self.loops.is_empty() && self.scopes.is_empty());
        debug_assert_eq!(self.error_target, "error");
        self.line("ret void\nerror:\nret void\n}");
        self.body
            .insert_str(entry, &std::mem::take(&mut self.allocations));
    }
    fn inline_call(&mut self, name: &str, args: &[Expr], instruction: &Instruction, handoff: bool) {
        let (_, target, child) = self.inline_children[&instruction.id];
        let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
        let args = self.handles(&values);
        // This field's preceding invocation has delivered and retired before the
        // checked call site can run again. No dynamic child frame is allocated.
        self.line(format!(
            "store %frame_{target} zeroinitializer, ptr %child_{child}"
        ));
        let constructor = self.constructors.contains(name);
        if constructor {
            assert!(!handoff);
            let Action::Decide { arms, .. } = &instruction.action else {
                unreachable!()
            };
            assert_eq!(arms.len(), 1);
            assert_eq!(arms[0].outcome, "$constructed");
            assert_eq!(arms[0].slots.len(), 1);
            self.effect(format!(
                "call void @tellur_rt_v1_construct_child(ptr %ctx, i64 {child}, i64 {}, ptr {args}, i64 {})",
                self.layout.slots[&arms[0].slots[0]], values.len()
            ));
        } else if matches!(
            &instruction.action,
            Action::Decide {
                operation: Operation::AwaitTask(_),
                ..
            }
        ) {
            self.effect(format!(
                "call void @tellur_rt_v1_await_task(ptr %ctx, i64 {child}, i64 {})",
                values[0]
            ));
        } else {
            let op = if handoff { "await" } else { "run_child" };
            self.effect(format!(
                "call void @tellur_rt_v1_{op}(ptr %ctx, i64 {child}, ptr {args}, i64 {})",
                values.len()
            ));
        }
        self.line(format!(
            "store i32 {}, ptr %pc\nret i32 {}",
            self.delivery_pc,
            if constructor {
                19
            } else if handoff {
                3
            } else {
                5
            }
        ));
    }
    fn async_host_leaf(&self, instruction: &Instruction) -> bool {
        matches!(&instruction.action, Action::Decide { operation: Operation::Primitive(name), .. }
            if self.async_primitives.contains(name))
    }
    fn resume_control(&mut self, region: &Region, index: usize) -> Result<()> {
        self.layout = FrameLayout::new(region);
        self.constructing = region.is_constructor();
        self.retained = true;
        self.current_descriptor = self.resumables[&region.name];
        self.inline_children.clear();
        self.supervision_children.clear();
        let plan = RetainedPlan::new(&region.code)?;
        self.retained_boundaries = plan
            .boundaries
            .iter()
            .enumerate()
            .map(|(n, b)| (b.instruction.id, n))
            .collect();
        self.retained_loops = plan.loops.clone();
        self.task_set_slots = region
            .slots
            .iter()
            .filter_map(|(id, slot)| crate::manifest::task_set_inner(&slot.ty).map(|_| *id))
            .collect();
        let mut children = vec![];
        for &instruction in &plan.code {
            let start = children.len();
            let targets = retained_child_targets(instruction, &self.resumables, &self.boundaries);
            for name in targets {
                let target = self.controls[name].0;
                let descriptor = self.resumables[name];
                if !matches!(&instruction.action, Action::Supervise { .. }) {
                    self.inline_children
                        .insert(instruction.id, (descriptor, target, children.len()));
                }
                children.push((instruction.id, descriptor, target));
            }
            if matches!(&instruction.action, Action::Supervise { .. }) {
                debug_assert_eq!(children.len(), start + 2);
                self.supervision_children
                    .insert(instruction.id, (start, start + 1));
            }
        }
        let fields = children
            .iter()
            .map(|(_, _, target)| format!(", %frame_{target}"))
            .collect::<String>();
        writeln!(
            self.frame_types,
            "%frame_{index} = type {{ i32, [{} x i1]{fields} }}",
            self.layout.atoms.len()
        )
        .unwrap();
        self.line(format!(
            "define i32 @resume_{index}(ptr %ctx, ptr %frame, ptr %args, i64 %count) {{\nentry:"
        ));
        let entry = self.body.len();
        self.line(format!(
            "%pc = getelementptr %frame_{index}, ptr %frame, i32 0, i32 0"
        ));
        for atom in 0..self.layout.atoms.len() {
            self.line(format!(
                "%a{atom} = getelementptr %frame_{index}, ptr %frame, i32 0, i32 1, i64 {atom}"
            ));
        }
        for child in 0..children.len() {
            self.line(format!(
                "%child_{child} = getelementptr %frame_{index}, ptr %frame, i32 0, i32 {}",
                child + 2
            ));
        }
        let saved = self.value("load i32, ptr %pc");
        let state = self.call(format!(
            "call i32 @tellur_rt_v1_resume_state(ptr %ctx, i32 {saved})"
        ));
        self.line(format!("store i32 {state}, ptr %pc"));
        let mut cases = "i32 0, label %initial".to_owned();
        for (pc, &instruction) in plan.code.iter().enumerate() {
            write!(cases, "\ni32 {}, label %state_{}", pc + 1, pc + 1).unwrap();
            if self.async_host_leaf(instruction)
                || self.inline_children.contains_key(&instruction.id)
                || matches!(
                    &instruction.action,
                    Action::Decide {
                        operation: Operation::Join(_) | Operation::Next(_),
                        ..
                    }
                )
            {
                write!(
                    cases,
                    "\ni32 {}, label %delivery_{}",
                    plan.code.len() + pc + 1,
                    pc + 1
                )
                .unwrap();
            }
        }
        for boundary in &plan.boundaries {
            write!(
                cases,
                "\ni32 {}, label %state_{}",
                boundary.end, boundary.end
            )
            .unwrap();
        }
        write!(
            cases,
            "\ni32 {}, label %state_{}",
            plan.invalid, plan.invalid
        )
        .unwrap();
        self.line(format!(
            "switch i32 {state}, label %invalid [ {cases} ]\ninitial:"
        ));
        for (i, parameter) in region.parameters.iter().enumerate() {
            let ptr = self.value(format!("getelementptr i64, ptr %args, i64 {i}"));
            let value = self.value(format!("load i64, ptr {ptr}"));
            self.store(parameter.id, &value);
        }
        self.line(if plan.code.is_empty() {
            "br label %invalid"
        } else {
            "br label %state_1"
        });
        for (pc, &instruction) in plan.code.iter().enumerate() {
            self.resume_next = plan.next[&instruction.id];
            self.delivery_pc = (plan.code.len() + pc + 1) as u32;
            self.line(format!("state_{}:", pc + 1));
            self.instructions(std::slice::from_ref(instruction));
            self.line(format!(
                "store i32 {}, ptr %pc\nret i32 4",
                self.resume_next
            ));
            if self.async_host_leaf(instruction)
                || self.inline_children.contains_key(&instruction.id)
                || matches!(
                    &instruction.action,
                    Action::Decide {
                        operation: Operation::Join(_) | Operation::Next(_),
                        ..
                    }
                )
            {
                let Action::Decide { arms, .. } = &instruction.action else {
                    unreachable!()
                };
                self.line(format!("delivery_{}:", pc + 1));
                if matches!(&instruction.action, Action::Decide { operation: Operation::Region(name) | Operation::Run(name), .. } if self.constructors.contains(name))
                {
                    assert_eq!(arms.len(), 1);
                    assert_eq!(arms[0].outcome, "$constructed");
                    assert_eq!(arms[0].slots.len(), 1);
                    // Constructed has already published to the checked caller slot.
                    // Its administrative delivery carries no outcome/payload handles.
                    self.line(format!(
                        "store i1 true, ptr %a{}",
                        self.layout.atoms[&arms[0].atom]
                    ));
                } else {
                    let outcome = self.value("load i64, ptr %args");
                    self.route(&outcome, arms);
                }
                // Delivery is administrative: route payloads into source slots
                // before the next source tick, without an extra resume transition.
                if self.resume_next == u32::MAX {
                    self.line("br label %invalid");
                } else {
                    self.line(format!("br label %state_{}", self.resume_next));
                }
            }
        }
        for (scope, boundary) in plan.boundaries.iter().enumerate() {
            self.line(format!("state_{}:", boundary.end));
            self.effect(format!(
                "call void @tellur_rt_v1_scope(ptr %ctx, i64 {scope}, i32 1)"
            ));
            self.line("ret i32 14");
        }
        self.line(format!("state_{}:\nbr label %invalid", plan.invalid));
        self.line("invalid:\ncall void @tellur_rt_v1_invalid_resume(ptr %ctx)\nbr label %error\nerror:\nret i32 0\n}");
        self.body
            .insert_str(entry, &std::mem::take(&mut self.allocations));
        self.retained = false;
        self.retained_loops.clear();
        let size = frame_size(index);
        let align = frame_align(index);
        let prefix = if children.is_empty() {
            size.clone()
        } else {
            frame_offset(index, 2)
        };
        let (name, len) = self.text(&self.controls[&region.name].1.clone());
        let parameters = region
            .parameters
            .iter()
            .map(|p| format!("i64 {}", self.layout.slots[&p.id]))
            .collect::<Vec<_>>();
        writeln!(
            self.globals,
            "@parameters_{index} = private constant [{} x i64] [{}]",
            parameters.len(),
            parameters.join(", ")
        )
        .unwrap();
        let child_fields = children
            .iter()
            .enumerate()
            .map(|(field, (_, target, _))| {
                format!(
                    "%compiled_child {{ i64 {target}, i64 {} }}",
                    frame_offset(index, field + 2)
                )
            })
            .collect::<Vec<_>>();
        writeln!(
            self.globals,
            "@children_{index} = private constant [{} x %compiled_child] [{}]",
            children.len(),
            child_fields.join(", ")
        )
        .unwrap();
        let retained = region
            .boundary
            .as_ref()
            .map(|b| self.layout.slots(&b.retained))
            .unwrap_or_default();
        let retained_pointer = if retained.is_empty() {
            "null".to_owned()
        } else {
            let slots = retained
                .iter()
                .map(|n| format!("i64 {n}"))
                .collect::<Vec<_>>();
            writeln!(
                self.globals,
                "@retained_{index} = private constant [{} x i64] [{}]",
                slots.len(),
                slots.join(", ")
            )
            .unwrap();
            format!("@retained_{index}")
        };
        let scopes = self.scope_metadata(index, &plan);
        let (producer, producer_len) = self.text(&region.name);
        self.descriptors.push(format!("%compiled_control_v4 {{ %compiled_control_v3 {{ %compiled_control {{ ptr {producer}, i64 {producer_len}, ptr {name}, i64 {len}, i64 {}, ptr @parameters_{index}, i64 {}, i64 {size}, i64 {align}, i64 {prefix}, ptr @resume_{index}, ptr @children_{index}, i64 {}, ptr {retained_pointer}, i64 {} }}, ptr {scopes} }}, i32 {} }}", self.layout.slots.len(), parameters.len(), children.len(), retained.len(), u32::from(region.is_constructor())));
        let descriptor = self.resumables[&region.name];
        let count = self.resumables.len();
        self.line(format!("define void @control_{index}(ptr %ctx, ptr %args, i64 %count) {{\nentry:\n%frame = alloca %frame_{index}\nstore %frame_{index} zeroinitializer, ptr %frame\ncall void @tellur_rt_v4_run_entry(ptr %ctx, ptr @async_controls, i64 {count}, i64 {descriptor}, ptr %frame, i64 {size}, i64 {align}, ptr %args, i64 %count, i64 -1)\nret void\n}}"));
        Ok(())
    }

    fn scope_slots(&mut self, slots: &[Id]) -> String {
        if slots.is_empty() {
            return "%compiled_slots { ptr null, i64 0 }".into();
        }
        let name = format!("@scope_slots_{}", self.id());
        let fields = slots
            .iter()
            .map(|id| format!("i64 {}", self.layout.slots[id]))
            .collect::<Vec<_>>();
        writeln!(
            self.globals,
            "{name} = private constant [{} x i64] [{}]",
            fields.len(),
            fields.join(", ")
        )
        .unwrap();
        format!("%compiled_slots {{ ptr {name}, i64 {} }}", fields.len())
    }
    fn scope_metadata(&mut self, index: usize, plan: &RetainedPlan<'_>) -> String {
        if plan.boundaries.is_empty() && plan.loops.is_empty() {
            return "null".into();
        }
        let mut boundaries = vec![];
        for boundary in &plan.boundaries {
            let i = boundary.instruction;
            let Action::Boundary {
                retained,
                retire,
                work_slots,
                handler_slots,
                fault_slot,
                ..
            } = &i.action
            else {
                unreachable!()
            };
            let (name, len) = self.text(&format!(
                "{}$boundary@{}",
                i.provenance.region, i.provenance.span.line
            ));
            let retained = self.scope_slots(retained);
            let retire = self.scope_slots(retire);
            let work = self.scope_slots(work_slots);
            let handling = self.scope_slots(handler_slots);
            boundaries.push(format!("%compiled_boundary {{ ptr {name}, i64 {len}, i32 {}, i32 {}, i32 {}, {retained}, {retire}, {work}, {handling}, i64 {} }}", boundary.body, boundary.handler, boundary.after, self.layout.slots[fault_slot]));
        }
        let mut entries = plan.loops.iter().collect::<Vec<_>>();
        entries.sort_by_key(|(_, target)| target.index);
        let mut loops = vec![];
        for (target, entry) in entries {
            let locals = self.scope_slots(&entry.local_slots);
            let parameters = self.scope_slots(&entry.parameters);
            let reset = format!("@reset_{index}_{}", entry.index);
            self.line(format!("define void {reset}(ptr %frame) {{\nentry:"));
            for (n, atom) in entry.local_atoms.iter().enumerate() {
                self.line(format!("%fact_{n} = getelementptr %frame_{index}, ptr %frame, i32 0, i32 1, i64 {}\nstore i1 false, ptr %fact_{n}", self.layout.atoms[atom]));
            }
            self.line("ret void\n}");
            loops.push(format!("%compiled_loop {{ i64 {target}, i32 {}, i32 {}, {locals}, {parameters}, ptr {reset} }}", entry.body, entry.complete));
        }
        writeln!(
            self.globals,
            "@boundaries_{index} = private constant [{} x %compiled_boundary] [{}]",
            boundaries.len(),
            boundaries.join(", ")
        )
        .unwrap();
        writeln!(
            self.globals,
            "@loops_{index} = private constant [{} x %compiled_loop] [{}]",
            loops.len(),
            loops.join(", ")
        )
        .unwrap();
        writeln!(self.globals, "@scopes_{index} = private constant %compiled_scope {{ i32 {}, ptr @boundaries_{index}, i64 {}, ptr @loops_{index}, i64 {} }}", plan.invalid + 1, boundaries.len(), loops.len()).unwrap();
        format!("@scopes_{index}")
    }

    fn unwind(&mut self, depth: usize) {
        for scope in self.scopes[depth..]
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<_>>()
        {
            self.line(format!(
                "call void @tellur_rt_v1_lexical_end(ptr %ctx, i64 {})",
                scope.handle
            ));
            self.check_to(scope.outer_error);
        }
    }
    fn return_control(&mut self) {
        self.unwind(0);
        if self.retained {
            self.line("store i32 -1, ptr %pc\nret i32 2");
        } else {
            self.line("ret void");
        }
    }
    fn select_resume(&mut self, state: u32) {
        self.line(format!("store i32 {state}, ptr %pc\nret i32 4"));
    }
    fn request_retained_loop(&mut self, target: Id, kind: u32, values: Vec<String>) {
        let index = self.retained_loops[&target].index;
        let args = self.handles(&values);
        self.effect(format!(
            "call void @tellur_rt_v1_loop(ptr %ctx, i64 {index}, i32 {kind}, ptr {args}, i64 {})",
            values.len()
        ));
        self.line(format!("ret i32 {}", 15 + kind));
    }
    fn instructions(&mut self, code: &[Instruction]) {
        for i in code {
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
                        self.return_control();
                        terminal = true;
                    }
                }
                Action::Boundary { .. } if self.retained => {
                    let scope = self.retained_boundaries[&i.id];
                    self.effect(format!(
                        "call void @tellur_rt_v1_scope(ptr %ctx, i64 {scope}, i32 0)"
                    ));
                    self.line("ret i32 13");
                    terminal = true;
                }
                Action::Boundary {
                    body,
                    handler,
                    retained,
                    retire,
                    work_slots,
                    handler_slots,
                    fault_slot,
                } => {
                    let layout = retained
                        .iter()
                        .chain(retire)
                        .chain(work_slots)
                        .chain(handler_slots)
                        .map(|id| self.layout.slots[id].to_string())
                        .collect::<Vec<_>>();
                    let layout = self.handles(&layout);
                    let (name, len) = self.text(&format!(
                        "{}$boundary@{}",
                        i.provenance.region, i.provenance.span.line
                    ));
                    let handle = self.call(format!("call i64 @tellur_rt_v1_lexical_begin(ptr %ctx, ptr {name}, i64 {len}, ptr {layout}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})", retained.len(),retire.len(),work_slots.len(),handler_slots.len(),self.layout.slots[fault_slot]));
                    let outer_error = self.error_target.clone();
                    let failed = self.id();
                    let cleanup = self.id();
                    let cleanup_failed = self.id();
                    self.scopes.push(ScopeTarget {
                        handle: handle.clone(),
                        outer_error: outer_error.clone(),
                    });
                    self.error_target = failed.clone();
                    self.instructions(body);
                    self.unwind(self.scopes.len() - 1);
                    self.line(format!("br label %{next}\n{failed}:"));
                    let caught = self.value(format!(
                        "call i32 @tellur_rt_v1_lexical_catch(ptr %ctx, i64 {handle})"
                    ));
                    let caught = self.value(format!("icmp ne i32 {caught}, 0"));
                    self.line(format!(
                        "br i1 {caught}, label %{cleanup}, label %{cleanup_failed}\n{cleanup}:"
                    ));
                    self.error_target = cleanup_failed.clone();
                    self.instructions(handler);
                    // A checked handler has no normal fallthrough.
                    self.line(format!("br label %{cleanup_failed}\n{cleanup_failed}:\ncall void @tellur_rt_v1_lexical_end(ptr %ctx, i64 {handle})\nbr label %{outer_error}"));
                    self.scopes.pop();
                    self.error_target = outer_error;
                    terminal = true;
                }
                Action::TerminateProcess(status) => {
                    self.effect(format!(
                        "call void @tellur_rt_v1_process(ptr %ctx, i32 {status})"
                    ));
                    self.return_control();
                    terminal = true;
                }
                Action::Supervise { args, .. } if self.retained => {
                    let (work, handler) = self.supervision_children[&i.id];
                    let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
                    let args = self.handles(&values);
                    self.effect(format!("call void @tellur_rt_v1_supervise_children(ptr %ctx, i64 {work}, i64 {handler}, ptr {args}, i64 {})", values.len()));
                    self.line(format!(
                        "store i32 {}, ptr %pc\nret i32 12",
                        self.resume_next
                    ));
                    terminal = true;
                }
                Action::Supervise { boundary, args } => self.supervise(boundary, args),
                Action::Terminate | Action::Escalate => {
                    let op = if matches!(&i.action, Action::Terminate) {
                        "settled"
                    } else {
                        "escalate"
                    };
                    self.effect(format!("call void @tellur_rt_v1_{op}(ptr %ctx)"));
                    self.return_control();
                    terminal = true;
                }
                Action::Exit(name, args) => {
                    let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
                    if self.constructing {
                        assert_eq!(name, "$constructed");
                        assert_eq!(values.len(), 1);
                        self.effect(format!(
                            "call void @tellur_rt_v1_constructed(ptr %ctx, i64 {})",
                            values[0]
                        ));
                    } else {
                        let args = self.handles(&values);
                        let (name, len) = self.text(name);
                        self.effect(format!("call void @tellur_rt_v1_complete(ptr %ctx, ptr {name}, i64 {len}, ptr {args}, i64 {})",values.len()));
                    }
                    self.return_control();
                    terminal = true;
                }
                Action::Decide {
                    operation:
                        Operation::Region(name)
                        | Operation::Run(name)
                        | Operation::Await(name)
                        | Operation::AwaitTask(name),
                    args,
                    ..
                } if self.retained && self.resumables.contains_key(name) => {
                    self.inline_call(
                        name,
                        args,
                        i,
                        matches!(
                            &i.action,
                            Action::Decide {
                                operation: Operation::Await(_) | Operation::AwaitTask(_),
                                ..
                            }
                        ),
                    );
                    terminal = true;
                }
                Action::Decide {
                    operation: Operation::Defer(name),
                    args,
                    arms,
                } if self.retained => {
                    let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
                    let args = self.handles(&values);
                    let (ty, len) = self.text(&crate::manifest::task_type(name));
                    let producer = self.resumables[name];
                    let value = self.call(format!("call i64 @tellur_rt_v1_defer(ptr %ctx, i64 {producer}, ptr {ty}, i64 {len}, ptr {args}, i64 {})", values.len()));
                    self.store(arms[0].slots[0], &value);
                    self.line(format!(
                        "store i1 true, ptr %a{}",
                        self.layout.atoms[&arms[0].atom]
                    ));
                }
                Action::Decide {
                    operation: Operation::Spawn(name),
                    args,
                    arms,
                } if self.retained => {
                    let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
                    let args = self.handles(&values);
                    let producer = self.resumables[name];
                    let slot = self.layout.slots[&arms[0].slots[0]];
                    self.effect(format!("call void @tellur_rt_v1_spawn(ptr %ctx, i64 {producer}, i64 {slot}, ptr {args}, i64 {})", values.len()));
                    self.line(format!(
                        "store i1 true, ptr %a{}",
                        self.layout.atoms[&arms[0].atom]
                    ));
                    self.line(format!(
                        "store i32 {}, ptr %pc\nret i32 6",
                        self.resume_next
                    ));
                    terminal = true;
                }
                Action::Decide {
                    operation: Operation::SpawnInto(name),
                    args,
                    arms,
                } if self.retained => {
                    let mut values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
                    let set = values.pop().expect("checked spawn-into has a set operand");
                    let args = self.handles(&values);
                    let producer = self.resumables[name];
                    let slot = self.layout.slots[&arms[0].slots[0]];
                    self.effect(format!("call void @tellur_rt_v1_spawn_into(ptr %ctx, i64 {producer}, i64 {slot}, i64 {set}, ptr {args}, i64 {})", values.len()));
                    self.line(format!(
                        "store i1 true, ptr %a{}",
                        self.layout.atoms[&arms[0].atom]
                    ));
                    self.line(format!(
                        "store i32 {}, ptr %pc\nret i32 10",
                        self.resume_next
                    ));
                    terminal = true;
                }
                Action::Decide {
                    operation: Operation::Next(name),
                    args,
                    arms,
                } if self.retained => {
                    let set = self.expr(&args[0]);
                    let producer = self.resumables[name];
                    // call() checks the sticky error before zero can mean empty.
                    let pending = self.call(format!(
                        "call i8 @tellur_rt_v1_next(ptr %ctx, i64 {producer}, i64 {set})"
                    ));
                    let pending = self.value(format!("icmp ne i8 {pending}, 0"));
                    let wait = self.id();
                    let empty = self.id();
                    self.line(format!("br i1 {pending}, label %{wait}, label %{empty}\n{wait}:\nstore i32 {}, ptr %pc\nret i32 11\n{empty}:", self.delivery_pc));
                    let arm = arms
                        .iter()
                        .find(|arm| arm.outcome == "empty")
                        .expect("checked Next has an empty arm");
                    debug_assert!(arm.slots.is_empty());
                    self.line(format!(
                        "store i1 true, ptr %a{}",
                        self.layout.atoms[&arm.atom]
                    ));
                    self.select_resume(self.resume_next);
                    terminal = true;
                }
                Action::Decide {
                    operation: Operation::Join(name),
                    args,
                    ..
                } if self.retained => {
                    let value = self.expr(&args[0]);
                    let producer = self.resumables[name];
                    self.effect(format!(
                        "call void @tellur_rt_v1_join(ptr %ctx, i64 {producer}, i64 {value})"
                    ));
                    self.line(format!(
                        "store i32 {}, ptr %pc\nret i32 7",
                        self.delivery_pc
                    ));
                    terminal = true;
                }
                Action::Decide {
                    operation: Operation::Cancel(name),
                    args,
                    arms,
                } if self.retained => {
                    let value = self.expr(&args[0]);
                    let producer = self.resumables[name];
                    let slot = self.layout.slots[&arms[0].slots[0]];
                    let abi = if self.task_set_slots.contains(&arms[0].slots[0]) {
                        "cancel_set"
                    } else {
                        "cancel"
                    };
                    self.effect(format!(
                        "call void @tellur_rt_v1_{abi}(ptr %ctx, i64 {producer}, i64 {slot}, i64 {value})"
                    ));
                    self.line(format!(
                        "store i1 true, ptr %a{}",
                        self.layout.atoms[&arms[0].atom]
                    ));
                    self.line(format!(
                        "store i32 {}, ptr %pc\nret i32 8",
                        self.resume_next
                    ));
                    terminal = true;
                }
                Action::Decide {
                    operation: Operation::Yield,
                    arms,
                    ..
                } if self.retained => {
                    self.line(format!(
                        "store i1 true, ptr %a{}",
                        self.layout.atoms[&arms[0].atom]
                    ));
                    self.line(format!(
                        "store i32 {}, ptr %pc\nret i32 1",
                        self.resume_next
                    ));
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
                    ..
                } if self.retained && self.async_primitives.contains(name) => {
                    let values = args.iter().map(|a| self.expr(a)).collect::<Vec<_>>();
                    let args = self.handles(&values);
                    self.effect(format!(
                        "call void @tellur_rt_v1_host(ptr %ctx, i64 {}, ptr {args}, i64 {})",
                        self.current_descriptor,
                        values.len()
                    ));
                    self.line(format!(
                        "store i32 {}, ptr %pc\nret i32 9",
                        self.delivery_pc
                    ));
                    terminal = true;
                }
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
                    unwind,
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
                    if let Some(target) = unwind {
                        if self.retained {
                            self.request_retained_loop(*target, 3, vec![]);
                        } else {
                            let target = self.loops[target].clone();
                            self.unwind(target.scope_depth);
                            self.line(format!("br label %{}", target.complete));
                        }
                        terminal = true;
                    }
                }
                Action::Loop { id, parameters, .. } if self.retained => {
                    let values = parameters.iter().map(|(_, e)| self.expr(e)).collect();
                    self.request_retained_loop(*id, 0, values);
                    terminal = true;
                }
                Action::Again { target, args } | Action::Recover { target, args }
                    if self.retained =>
                {
                    let kind = if matches!(&i.action, Action::Recover { .. }) {
                        2
                    } else {
                        1
                    };
                    let values = args.iter().map(|e| self.expr(e)).collect();
                    self.request_retained_loop(*target, kind, values);
                    terminal = true;
                }
                Action::Loop {
                    id,
                    parameters,
                    local_slots,
                    local_atoms,
                    body,
                    ..
                } => {
                    // Initial and restart expressions all finish before any local
                    // owner is reset. The transfer buffer is allocated at entry.
                    let values = parameters
                        .iter()
                        .map(|(_, e)| self.expr(e))
                        .collect::<Vec<_>>();
                    let arguments = self.handles(&values);
                    let restart = self.id();
                    self.loops.insert(
                        *id,
                        LoopTarget {
                            restart: restart.clone(),
                            complete: next.clone(),
                            arguments: arguments.clone(),
                            arity: parameters.len(),
                            scope_depth: self.scopes.len(),
                        },
                    );
                    self.line(format!("br label %{restart}\n{restart}:"));
                    for id in local_slots {
                        self.release(*id);
                    }
                    for atom in local_atoms {
                        self.line(format!("store i1 false, ptr %a{}", self.layout.atoms[atom]));
                    }
                    for (index, (id, _)) in parameters.iter().enumerate() {
                        let ptr =
                            self.value(format!("getelementptr i64, ptr {arguments}, i64 {index}"));
                        let value = self.value(format!("load i64, ptr {ptr}"));
                        self.store(*id, &value);
                    }
                    self.instructions(body);
                    // Checked loop bodies always restart, complete or exit. Keep
                    // impossible fallthrough out of the loop's continuation.
                    self.line("br label %error");
                    self.loops.remove(id);
                    terminal = true;
                }
                Action::Again { target, args } | Action::Recover { target, args } => {
                    let target = self.loops[target].clone();
                    let values = args.iter().map(|e| self.expr(e)).collect::<Vec<_>>();
                    debug_assert_eq!(values.len(), target.arity);
                    self.write_handles(&target.arguments, &values);
                    self.unwind(target.scope_depth);
                    self.line(format!("br label %{}", target.restart));
                    terminal = true;
                }
                _ => unreachable!(),
            }
            if !terminal {
                self.line(format!("br label %{next}"));
            }
            self.line(format!("{next}:"));
        }
    }
}

impl CheckedProgram {
    /// Builds only the admitted synchronous/retained subset. The cached Rust staticlib is
    /// transitional tooling; no per-program Rust is generated or compiled.
    pub fn build_llvm(&self, entry: &str, output: &Path, opt: u8) -> Result<NativeBuild> {
        let source = self.llvm_source(entry)?;
        self.build_llvm_source(source, output, opt)
    }
    fn build_llvm_source(&self, source: String, output: &Path, opt: u8) -> Result<NativeBuild> {
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
        let entries = self.executable_entries(entry)?;
        let mut types = admitted_types(self.manifest(), true);
        // Exact producer-indexed cold, started and set identities. Frontend local
        // ownership and reference-free set capture/result rules still apply.
        types.extend(self.regions().filter(|r| r.asynchronous).flat_map(|r| {
            [
                crate::manifest::task_type(&r.name),
                crate::manifest::started_type(&r.name),
                crate::manifest::task_set_type(&r.name),
            ]
        }));
        // Host operations remain reference-free even when controls admit carriers.
        let primitive_types = admitted_types(self.manifest(), false);
        let boundaries = self
            .regions()
            .filter_map(|r| {
                r.boundary.as_ref().map(|b| {
                    (
                        r.name.clone(),
                        (b.handler.clone(), FrameLayout::new(r).slots(&b.retained)),
                    )
                })
            })
            .collect();
        let resumables = resumable_controls(self, &boundaries)?;
        let mut emitter = Emitter {
            frame_types: String::new(),
            retained: false,
            current_descriptor: 0,
            async_primitives: self
                .manifest()
                .primitives
                .iter()
                .filter(|p| p.asynchronous)
                .map(|p| p.name.clone())
                .collect(),
            resumables,
            descriptors: vec![],
            inline_children: BTreeMap::new(),
            supervision_children: BTreeMap::new(),
            retained_loops: BTreeMap::new(),
            retained_boundaries: BTreeMap::new(),
            task_set_slots: BTreeSet::new(),
            resume_next: 0,
            delivery_pc: 0,
            globals: String::new(),
            body: String::new(),
            allocations: String::new(),
            next: 0,
            layout: FrameLayout::default(),
            loops: BTreeMap::new(),
            scopes: vec![],
            error_target: "error".into(),
            constructors: self
                .regions()
                .filter(|r| r.is_constructor())
                .map(|r| r.name.clone())
                .collect(),
            constructing: false,
            boundaries,
            controls: self
                .regions()
                .enumerate()
                .map(|(i, r)| (r.name.clone(), (i, self.runtime_name(&r.name).to_owned())))
                .collect(),
        };
        let mut metadata = vec![];
        let mut callbacks = vec![];
        for region in self.regions() {
            let retained = emitter.resumables.contains_key(&region.name);
            if region
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
                    "LLVM backend does not yet support this control layout: {}",
                    self.runtime_name(&region.name)
                )));
            }
            if let Some(instruction) = region.code.iter().find(|i| {
                if retained {
                    !retained_supported(
                        i,
                        crate::host_operations::operation(&region.name),
                        &types,
                        &primitive_types,
                        self.manifest(),
                    )
                } else {
                    !supported(i, &types, &primitive_types, self.manifest())
                }
            }) {
                return Err(Diagnostic::new(
                    "E_BACKEND",
                    instruction.provenance.span,
                    "LLVM backend does not yet support this operation; use --backend rust",
                ));
            }
            let index = emitter.controls[&region.name].0;
            if retained {
                emitter.resume_control(region, index)?;
            } else {
                emitter.control(region, index);
            }
            if let Some(codes) = entries.get(&region.name) {
                callbacks.push(format!("ptr @control_{index}"));
                metadata.push(serde_json::json!({"name":region.name,"display":self.runtime_name(&region.name),"retained":retained,"arguments":region.parameters.iter().map(|p|&p.ty).collect::<Vec<_>>(),"exits":codes.iter().collect::<Vec<_>>()}));
            }
        }
        if !emitter.descriptors.is_empty() {
            writeln!(
                emitter.globals,
                "@async_controls = private constant [{} x %compiled_control_v4] [{}]",
                emitter.descriptors.len(),
                emitter.descriptors.join(", ")
            )
            .unwrap();
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
            "; Tellur direct LLVM backend; private runtime ABI v1\n{DECLARATIONS}\n{}\n{}\n{}",
            emitter.frame_types, emitter.globals, emitter.body
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
        .arg("--features=runtime-abi")
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
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn generated_async_resume_functions_preserve_reports_across_host_quotas() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static ID: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "tellur-llvm-async-quota-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../playground/manifest.json")).unwrap();
        let host: serde_json::Value =
            serde_json::from_str(include_str!("../../playground/llvm_host.manifest.json")).unwrap();
        manifest["primitives"]
            .as_array_mut()
            .unwrap()
            .extend(host["primitives"].as_array().unwrap().iter().cloned());
        let manifest = crate::manifest::Manifest::from_json(&manifest.to_string()).unwrap();
        for mode in 0..16 {
            let source = r#"
async control child(bool fail, ref mutable string text) exits done faults {
    yield;delta fail {true=>bad,false=>good}
    [bad]:fault "quota child failure";
    [good]:deref text="updated";emit(text);exit done;
}
async control startup(bool fail) exits done faults {
    mutable string text="caller";auto pending=defer child(fail,borrow mutable text);emit("captured");auto moved=move pending;await moved;
    yield;emit(text);exit done;
}
control main(bool fail) exits result faults {forward run startup(fail);}
outcome result {@exit 0 done}
"#;
            let source = match mode {
                0 => source.to_owned(),
                1 => source.replace("defer child", "spawn child"),
                _ => source.replace("defer child", "spawn child").replace(
                    "auto moved=move pending;",
                    "cancel pending;cancel pending;auto moved=move pending;",
                ),
            };
            let source = if (3..=4).contains(&mode) {
                source.replace("[good]:deref text=", "[good]:delta await tick(\"quota\") {ticked {string response}=>ready,cancelled=>stopped}[ready|stopped]:deref text=")
                    .replace("cancel pending;cancel pending;", if mode==3 {""} else {"yield;yield;yield;cancel pending;cancel pending;"})
            } else {
                source
            };
            let source = if mode >= 5 {
                let body = r#"
async control child(bool fail, ref mutable string text) exits done faults {
    yield;delta fail {true=>bad,false=>good}
    [bad]:fault "loop quota child failure";
    [good]:deref text=concat(clone(text),"!");exit done;
}
async control startup(bool fail) exits done faults {
    mutable string text="caller";
    loop turn(bool first=true) {
        auto pending=defer child(fail && !first,borrow mutable text);emit("captured");await pending;
        yield;delta first {true=>repeat,false=>finished}
        [repeat]:again turn(false);
        [finished]:emit(borrow text);exit done;
    }
}
control main(bool fail) exits result faults {forward run startup(fail);}
outcome result {@exit 0 done}
"#;
                if mode == 6 {
                    body.replace("defer child", "spawn child")
                } else {
                    body.into()
                }
            } else {
                source
            };
            let source = if mode >= 7 {
                let body = r#"
async control member(bool fail, string text) exits done {string text} faults {
    yield;delta fail {true=>bad,false=>good}
    [bad]:fault "set quota child failure";
    [good]:exit done {text};
}
async control startup(bool fail) exits done faults {
    tasks<member> active=tasks();cancel active;
    spawn member(false,"first") into active;spawn member(fail,"second") into active;
    cancel active;cancel active;
    loop drain(tasks<member> pending=move active) {
        delta await next pending {done {string text}=>member,empty=>drained}
        [member]:emit(text);again drain(pending);
        [drained]:exit done;
    }
}
control main(bool fail) exits result faults {forward run startup(fail);}
outcome result {@exit 0 done}
"#;
                if mode == 8 {
                    body.replace("[good]:exit done {text};", "[good]:delta await tick(text) {ticked {string response}=>ready,cancelled=>stopped}[ready]:exit done {response};[stopped]:exit done {\"cancelled\"};")
                } else {
                    body.into()
                }
            } else {
                source
            };
            let source = if mode == 9 {
                r#"
async control child(bool fail,ref string text) exits done faults {
    yield;emit(text);delta fail {true=>bad,false=>good}
    [bad]:fault "quota supervised work";
    [good]:exit done;
}
boundary request(bool fail,string context) retains context {
    run child(fail,borrow context);terminate;
}
contingency(string reason){emit(context);emit(reason);terminate;}
async control startup(bool fail) exits done {
    string parent="parent";
    loop turn(bool first=true) {
        supervise request(fail && !first,"retained");emit(borrow parent);
        delta first {true=>more,false=>finished}
        [more]:again turn(false);
        [finished]:exit done;
    }
}
control main(bool fail) exits result {forward run startup(fail);}

outcome result {@exit 0 done}
"#
                .to_owned()
            } else if mode == 10 {
                r#"async control operation(bool fail) exits done faults {
    delta await tick("request") {
        ticked { string text } => ready,
        cancelled => stopped
    }
    [stopped]: emit("cancelled");
        exit done;
    [ready]: emit(text);
        delta fail {
            true => bad,
            false => ok
        }
    [bad]: fault "after host";
    [ok]: exit done;
}
boundary request(bool fail, string context) retains context {
    run operation(fail);
    emit("request complete");
    terminate;
}
contingency(string reason) {
    emit("owning handler");
    emit(context);
    emit(reason);
    terminate;
}
async control supervisor(bool fail) exits done {
    supervise request(fail, "retained host");
    emit("supervisor continued");
    exit done;
}
control main(bool fail) exits result {
    forward run supervisor(fail);
}

outcome result {@exit 0 done}
"#
                .to_owned()
            } else {
                source
            };
            let source = if mode == 11 {
                include_str!("../../playground/llvm_async_recovery.tellur").to_owned()
            } else if mode == 12 {
                include_str!("../../playground/llvm_async_recovery.tellur").replace("            yield;", r#"            delta await tick("request") {ticked {string reply}=>ready,cancelled=>stopped}
            [ready]:emit(reply);
            [ready|stopped]:yield;"#)
            } else {
                source
            };
            let source = if mode == 13 {
                include_str!("../../playground/llvm_async_constructors.tellur").to_owned()
            } else if mode == 14 || mode == 15 {
                r#"
struct message {string text;constructor(string input,bool fail) {
 delta run tick(input) {ticked {string reply}=>ready,cancelled=>stopped}
 [ready]:self.text=reply;[stopped]:self.text="cancelled";
 [entry]:delta fail {true=>bad,false=>good}[bad]:fault "quota constructor after host";
}}
async control child(bool fail) exits done {string value} faults {
 auto value=message("request",fail);exit done {value.text};
}
async control startup(bool fail) exits done faults {
 auto pending=spawn child(fail);yield;cancel pending;
 delta await pending {done {string value}=>ready}
 [ready]:emit(value);exit done;
}
control main(bool fail) exits result faults {forward run startup(fail);}
outcome result {@exit 0 done}
"#
                .to_owned()
            } else {
                source
            };
            let source = if mode == 15 {
                source.replace("yield;cancel pending;", "yield;")
            } else {
                source
            };
            let mut selected_manifest = manifest.clone();
            if mode == 15 {
                selected_manifest.launch.insert(
                    "main".into(),
                    crate::launch::EntryConfiguration {
                        exit_codes: Default::default(),
                        signals: Some(crate::launch::SignalPolicy::Cancel),
                    },
                );
            }
            let checked = crate::compile(&source, selected_manifest).unwrap();
            for expanded in [false, true] {
                let program = if expanded {
                    checked.expanded("main").unwrap()
                } else {
                    checked.clone()
                };
                for opt in [0, 2] {
                    let rust = program
                        .build_native("main", &directory.join("rust"), opt)
                        .unwrap()
                        .executable;
                    let source = program.llvm_source("main").unwrap();
                    assert!(source.contains("i64 %count, i64 -1)"));
                    let mut expected = vec![];
                    for fail in [false, true] {
                        let scenario = directory.join(format!("{fail}.json"));
                        fs::write(&scenario,serde_json::json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":if mode==15 {let mut calls=vec![serde_json::json!({"operation":"tick","outcome":"ticked","fields":[{"kind":"string","value":"quota signal"}],"waits":7}),serde_json::json!({"operation":"$signal","outcome":"SIGTERM","fields":[],"waits":0})];if fail {calls.push(serde_json::json!({"operation":"$signal","outcome":"SIGINT","fields":[],"waits":0}));}calls} else if mode == 12 {vec![serde_json::json!({"operation":"tick","outcome":"ticked","fields":[{"kind":"string","value":"quota lexical"}],"waits":7});if fail {2} else {1}]} else if (3..=4).contains(&mode) || mode == 10 || mode == 14 {vec![serde_json::json!({"operation":"tick","outcome":"ticked","fields":[{"kind":"string","value":"quota"}],"waits":7})]} else {vec![]}}).to_string()).unwrap();
                        for fuel in 0..=if mode >= 5 { 64 } else { 36 } {
                            let out = Command::new(&rust)
                                .arg("--scenario")
                                .arg(&scenario)
                                .arg("--fuel")
                                .arg(fuel.to_string())
                                .output()
                                .unwrap();
                            expected.push((scenario.clone(), fuel, out));
                        }
                    }
                    for quota in [1, 3, usize::MAX] {
                        let source = source
                            .replace("i64 %count, i64 -1)", &format!("i64 %count, i64 {quota})"));
                        let llvm = program
                            .build_llvm_source(source, &directory.join("llvm"), opt)
                            .unwrap()
                            .executable;
                        for (scenario, fuel, want) in &expected {
                            let got = Command::new(&llvm)
                                .arg("--scenario")
                                .arg(scenario)
                                .arg("--fuel")
                                .arg(fuel.to_string())
                                .output()
                                .unwrap();
                            assert_eq!(
                                got.status.code(),
                                want.status.code(),
                                "{expanded}/{opt}/{quota}/{fuel}: {got:?} vs {want:?}"
                            );
                            assert_eq!(got.stdout, want.stdout, "{expanded}/{opt}/{quota}/{fuel}");
                            assert_eq!(got.stderr, want.stderr, "{expanded}/{opt}/{quota}/{fuel}");
                        }
                    }
                }
            }
        }
        fs::remove_dir_all(directory).unwrap();
    }
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
