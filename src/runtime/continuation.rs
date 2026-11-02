//! Host stepping of synchronous checked execution. No source scheduler or task policy.
use super::*;

type RunResult = std::result::Result<RunCompletion, Box<Failure>>;

/// Host control of execution, separate from Tellur outcomes and fault settlement.
#[derive(Debug)]
pub enum ExecutionProgress {
    /// The next checked instruction has not run. Stored frames and loans remain live.
    Paused,
    /// The result is delivered exactly once, including terminal host/source failures.
    Complete(RunResult),
    /// A previous call already delivered the result; no execution can resume.
    Finished,
}

/// A reference execution with explicit continuations and one fixed total fuel limit.
/// Dropping this host-side object abandons the scripted simulation; it is not a
/// Tellur cancellation request, resource settlement, or permission to detach work.
pub struct ExecutionSession<'a> {
    machine: Option<Machine<'a>>,
    scenario: &'a Scenario,
    initialized: bool,
}
impl CheckedProgram {
    /// Prepare a fresh reference execution without evaluating its entry or effects.
    pub fn start_execution<'a>(
        &'a self,
        scenario: &'a Scenario,
        steps: usize,
    ) -> ExecutionSession<'a> {
        ExecutionSession {
            machine: Some(Machine {
                next_base: 0,
                contingencies: vec![],
                program: self,
                calls: &scenario.calls,
                cursor: 0,
                events: vec![],
                steps_left: steps,
                activations: crate::fault::Tracker::new(self.runtime_sites()),
                phase: crate::fault::Phase::EntryValidation,
                values: BTreeMap::new(),
                frame: Frame::default(),
                stack: vec![],
                flow: None,
                handover: vec![],
            }),
            scenario,
            initialized: false,
        }
    }
}
impl ExecutionSession<'_> {
    /// Execute at most `instructions` attempted instructions, counting guarded skips
    /// just like fuel. Zero never makes progress. Administrative return/unwind work
    /// is drained before pausing; a quota never refills the fixed execution fuel.
    pub fn advance(&mut self, instructions: usize) -> ExecutionProgress {
        let Some(machine) = &mut self.machine else {
            return ExecutionProgress::Finished;
        };
        if instructions == 0 {
            return ExecutionProgress::Paused;
        }
        if !self.initialized {
            self.initialized = true;
            if let Err(diagnostic) = machine.enter(self.scenario) {
                machine.flow = Some(Err(diagnostic));
            }
        }
        if !machine.drive(instructions) {
            return ExecutionProgress::Paused;
        }
        let machine = self.machine.take().unwrap();
        ExecutionProgress::Complete(machine.finish(&self.scenario.entry))
    }
    /// Effects already observed in this simulation. Completion transfers them to
    /// the delivered result or failure, leaving no second copy in the session.
    pub fn events(&self) -> &[Event] {
        self.machine.as_ref().map_or(&[], |m| &m.events)
    }
}

#[derive(Default)]
struct Frame {
    base: Id,
    facts: BTreeSet<Id>,
}

/// Validated invocation arguments, without an activation or body execution.
/// This internal handoff is consumed when installed. It is not a source task:
/// task ownership, captured loans and fault settlement still require checking.
struct PreparedInvocation<'a> {
    region: &'a Region,
    arguments: Vec<Value>,
    diagnostic_name: String,
}

// Every continuation represents existing synchronous control flow, not a task.
// Blocks borrow immutable checked code; only active invocation/loop state is kept.
enum Continuation<'a> {
    Block {
        code: &'a [Instruction],
        next: usize,
    },
    Call {
        region: &'a Region,
        caller: Frame,
    },
    Decision(&'a [Arm]),
    Loop(&'a Action),
    SupervisedWork {
        name: &'a str,
        handler: &'a str,
        parent: crate::fault::Tracker,
    },
    SupervisedHandler {
        name: &'a str,
        parent: crate::fault::Tracker,
    },
    BoundaryWork {
        action: &'a Action,
        name: String,
        parent: crate::fault::Tracker,
    },
    BoundaryHandler {
        action: &'a Action,
        name: String,
        parent: crate::fault::Tracker,
    },
}
struct Machine<'a> {
    next_base: Id,
    contingencies: Vec<Diagnostic>,
    program: &'a CheckedProgram,
    calls: &'a [ScriptCall],
    cursor: usize,
    events: Vec<Event>,
    steps_left: usize,
    activations: crate::fault::Tracker,
    phase: crate::fault::Phase,
    values: BTreeMap<Id, Value>,
    frame: Frame,
    stack: Vec<Continuation<'a>>,
    flow: Option<Result<Flow>>,
    handover: Vec<Value>,
}
fn completion(flow: Flow) -> Result<Completion> {
    match flow {
        Flow::Process(status) => Ok(Completion::Process(status)),
        Flow::Terminate => Ok(Completion::Settled),
        Flow::Exit(outcome, fields) => Ok(Completion::Outcome(outcome, fields)),
        _ => Err(error("interpreter invariant: checked region fell through")),
    }
}
impl<'a> Machine<'a> {
    fn enter(&mut self, scenario: &Scenario) -> Result<()> {
        let region = self.program.region(&scenario.entry);
        if region.is_some_and(|r| r.asynchronous) {
            return Err(error(
                "async controls require a synchronous entry using run",
            ));
        }
        if region.is_some_and(|r| r.is_constructor()) {
            return Err(error(
                "constructors produce values and cannot be executable entries",
            ));
        }
        if region.is_some_and(|r| r.boundary.is_some() || r.contingency) {
            return Err(error(
                "boundaries are entered using supervise, not as executable controls",
            ));
        }
        if region.is_some_and(|r| r.parameters.iter().any(|p| p.ty.starts_with('&'))) {
            return Err(error("entry controls cannot require borrowed arguments"));
        }
        self.call(&scenario.entry, scenario.arguments.clone())
    }
    fn call(&mut self, name: &str, args: Vec<Value>) -> Result<()> {
        let invocation = self.prepare_call(name, args)?;
        self.start_call(invocation)
    }
    fn prepare_call(&self, name: &str, args: Vec<Value>) -> Result<PreparedInvocation<'a>> {
        let region = self
            .program
            .region(name)
            .ok_or_else(|| error(format!("unknown entry region `{name}`")))?;
        if args.len() != region.parameters.len() {
            return Err(error("wrong entry argument count"));
        }
        // Validate the entire captured value graph before mutating frame storage.
        // A later malformed argument must not leave an earlier one installed.
        for (param, value) in region.parameters.iter().zip(&args) {
            validate(value, &param.ty, self.program.manifest())?;
        }
        Ok(PreparedInvocation {
            region,
            arguments: args,
            diagnostic_name: self.program.runtime_name(name),
        })
    }
    fn start_call(&mut self, invocation: PreparedInvocation<'a>) -> Result<()> {
        let PreparedInvocation {
            region,
            arguments,
            diagnostic_name,
        } = invocation;
        let base = self.next_base;
        let next_base = base
            .checked_add(self.program.next_id.max(1))
            .ok_or_else(|| error("activation storage address space exhausted"))?;
        // Both fallible admission checks precede frame installation. Preparing
        // an invocation neither reserves addresses nor consumes an activation ID.
        self.activations.enter(&diagnostic_name).map_err(error)?;
        self.next_base = next_base;
        for (param, value) in region.parameters.iter().zip(arguments) {
            self.values.insert(base + param.id, value);
        }
        self.phase = crate::fault::Phase::Execution;
        let caller = std::mem::replace(
            &mut self.frame,
            Frame {
                base,
                facts: BTreeSet::new(),
            },
        );
        self.stack.push(Continuation::Call { region, caller });
        self.block(&region.code);
        Ok(())
    }
    fn block(&mut self, code: &'a [Instruction]) {
        self.flow = None;
        self.stack.push(Continuation::Block { code, next: 0 });
    }
    fn arguments(&mut self, args: &[Expr]) -> Result<Vec<Value>> {
        args.iter()
            .map(|e| evaluate(e, &mut self.values, self.frame.base))
            .collect()
    }
    /// Returns false only at an instruction boundary with no effects performed for
    /// that next instruction. No recursive source calls/loops/unwinding occur here.
    fn drive(&mut self, mut quota: usize) -> bool {
        while let Some(continuation) = self.stack.pop() {
            if let Continuation::Block { code, next } = continuation {
                if self.flow.is_some() {
                    continue;
                }
                let Some(instruction) = code.get(next) else {
                    self.flow = Some(Ok(Flow::Fallthrough));
                    continue;
                };
                if quota == 0 {
                    self.stack.push(Continuation::Block { code, next });
                    return false;
                }
                self.activations.at(instruction.id);
                if self.steps_left == 0 {
                    self.flow = Some(Err(Diagnostic::new(
                        "E_FUEL",
                        instruction.provenance.span,
                        "reference interpreter step limit reached; this is a host limit, not a language outcome",
                    )));
                    continue;
                }
                quota -= 1;
                self.steps_left -= 1;
                self.stack.push(Continuation::Block {
                    code,
                    next: next + 1,
                });
                if !instruction.guard.evaluate(&self.frame.facts) {
                    continue;
                }
                match self.instruction(instruction) {
                    Ok(Flow::Fallthrough) => {}
                    result => self.flow = Some(result),
                }
            } else if let Err(diagnostic) = self.resume(continuation) {
                self.flow = Some(Err(diagnostic));
            }
        }
        true
    }
    fn resume(&mut self, continuation: Continuation<'a>) -> Result<()> {
        let flow = self
            .flow
            .take()
            .expect("a continuation receives completed nested work");
        match continuation {
            Continuation::Block { .. } => unreachable!(),
            Continuation::Call { region, caller } => {
                self.handover.clear();
                let result = (|| {
                    if flow.is_err() && self.activations.is_source_fault() {
                        if let Some(boundary) = &region.boundary {
                            for id in &boundary.retained {
                                self.handover.push(
                                    self.values.remove(&(self.frame.base + id)).ok_or_else(
                                        || error("retained boundary resource is unavailable"),
                                    )?,
                                );
                            }
                        }
                    }
                    for id in region.slots.keys() {
                        self.values.remove(&(self.frame.base + id));
                    }
                    let flow = flow?;
                    match &flow {
                        Flow::Process(_) | Flow::Terminate | Flow::Exit(..) => {
                            self.activations.leave()
                        }
                        _ => {
                            return Err(error(
                                "interpreter invariant: checked region fell through",
                            ));
                        }
                    }
                    Ok(flow)
                })();
                self.frame = caller;
                self.flow = Some(result);
            }
            Continuation::Decision(arms) => {
                let (outcome, fields) = completion(flow?)?.outcome()?;
                self.bind(arms, outcome, fields)?;
            }
            Continuation::Loop(action) => {
                let Action::Loop { id, .. } = action else {
                    unreachable!()
                };
                match flow? {
                    Flow::Again(target, args) | Flow::Recover(target, args) if target == *id => {
                        self.iteration(action, args)
                    }
                    Flow::Complete(target) if target == *id => {}
                    Flow::Fallthrough => {
                        return Err(error("interpreter invariant: loop fell through"));
                    }
                    other => self.flow = Some(Ok(other)),
                }
            }
            Continuation::SupervisedWork {
                name,
                handler,
                parent,
            } => match flow {
                Err(diagnostic) if self.activations.is_source_fault() => {
                    let report = self.report(&self.program.runtime_name(name), &diagnostic);
                    self.events.push(Event::Fault {
                        report: Box::new(report.clone()),
                    });
                    let mut tracker = self.activations.child();
                    tracker.with_cause(report);
                    self.activations = tracker;
                    let mut retained = std::mem::take(&mut self.handover);
                    retained.push(Value::String(diagnostic.message.clone()));
                    self.contingencies.push(diagnostic);
                    self.stack
                        .push(Continuation::SupervisedHandler { name, parent });
                    self.call(handler, retained)?;
                }
                other => self.finish_supervision(name, parent, other),
            },
            Continuation::SupervisedHandler { name, parent } => {
                self.contingencies.pop();
                self.finish_supervision(name, parent, flow);
            }
            Continuation::BoundaryWork {
                action,
                name,
                parent,
            } => {
                match flow {
                    Err(diagnostic) if self.activations.is_source_fault() => {
                        let Action::Boundary {
                            handler,
                            retire,
                            fault_slot,
                            ..
                        } = action
                        else {
                            unreachable!()
                        };
                        let report = self.report(&name, &diagnostic);
                        self.events.push(Event::Fault {
                            report: Box::new(report.clone()),
                        });
                        for id in retire {
                            self.values.remove(&(self.frame.base + id));
                        }
                        let (message, stack, paths) = report.source_fields();
                        let strings = |xs: Vec<String>| Value::Construct {
                            ty: "vector<String>".into(),
                            name: "vector".into(),
                            fields: xs.into_iter().map(Value::String).collect(),
                        };
                        self.values.insert(
                            self.frame.base + fault_slot,
                            Value::Construct {
                                ty: "fault_report".into(),
                                name: "fault_report".into(),
                                fields: vec![
                                    Value::String(message),
                                    strings(stack),
                                    strings(paths),
                                ],
                            },
                        );
                        let mut tracker = self.activations.child();
                        tracker.with_cause(report);
                        self.activations = tracker;
                        // Preserve restoration even if admission fails before the handler starts.
                        self.stack.push(Continuation::BoundaryHandler {
                            action,
                            name: name.clone(),
                            parent,
                        });
                        self.contingencies.push(diagnostic);
                        self.activations
                            .enter(&format!("{name}$contingency"))
                            .map_err(error)?;
                        self.block(handler);
                    }
                    other => self.finish_boundary(action, &name, parent, other),
                }
            }
            Continuation::BoundaryHandler {
                action,
                name,
                parent,
            } => {
                self.contingencies.pop();
                self.finish_boundary(action, &name, parent, flow);
            }
        }
        Ok(())
    }
    fn report(&mut self, name: &str, diagnostic: &Diagnostic) -> crate::fault::Report {
        self.activations.failure(
            name,
            crate::fault::Phase::Execution,
            &format!("{}: {}", diagnostic.code, diagnostic.message),
        )
    }
    fn restore(&mut self, name: &str, mut parent: crate::fault::Tracker, flow: &Result<Flow>) {
        parent.absorb(&self.activations);
        if let Err(diagnostic) = flow {
            if self.activations.is_source_fault() {
                parent.source_fault();
            }
            parent.with_cause(self.report(name, diagnostic));
        }
        self.activations = parent;
    }
    fn finish_supervision(
        &mut self,
        name: &str,
        parent: crate::fault::Tracker,
        flow: Result<Flow>,
    ) {
        let result = flow
            .and_then(completion)
            .and_then(Completion::settled)
            .map(|()| Flow::Fallthrough);
        self.restore(&self.program.runtime_name(name), parent, &result);
        self.flow = match result {
            Ok(Flow::Fallthrough) => None,
            other => Some(other),
        };
    }
    fn finish_boundary(
        &mut self,
        action: &Action,
        name: &str,
        parent: crate::fault::Tracker,
        flow: Result<Flow>,
    ) {
        let Action::Boundary {
            work_slots,
            handler_slots,
            ..
        } = action
        else {
            unreachable!()
        };
        for id in work_slots.iter().chain(handler_slots) {
            self.values.remove(&(self.frame.base + id));
        }
        self.restore(name, parent, &flow);
        self.flow = match flow {
            Ok(Flow::Fallthrough) => None,
            other => Some(other),
        };
    }
    fn iteration(&mut self, action: &'a Action, arguments: Vec<Value>) {
        let Action::Loop {
            parameters,
            local_slots,
            local_atoms,
            body,
            ..
        } = action
        else {
            unreachable!()
        };
        for slot in local_slots {
            self.values.remove(&(self.frame.base + slot));
        }
        for atom in local_atoms {
            self.frame.facts.remove(atom);
        }
        for ((slot, _), value) in parameters.iter().zip(arguments) {
            self.values.insert(self.frame.base + slot, value);
        }
        self.stack.push(Continuation::Loop(action));
        self.block(body);
    }
    fn bind(&mut self, arms: &[Arm], outcome: String, fields: Vec<Value>) -> Result<()> {
        let arm = arms
            .iter()
            .find(|a| a.outcome == outcome)
            .ok_or_else(|| error("interpreter invariant: missing decision arm"))?;
        if fields.len() != arm.slots.len() {
            return Err(error("interpreter invariant: payload arity"));
        }
        self.frame.facts.insert(arm.atom);
        for (slot, value) in arm.slots.iter().zip(fields) {
            self.values.insert(self.frame.base + *slot, value);
        }
        Ok(())
    }
    fn instruction(&mut self, instruction: &'a Instruction) -> Result<Flow> {
        match &instruction.action {
            Action::Boundary { body, .. } => {
                let child = self.activations.child();
                let parent = std::mem::replace(&mut self.activations, child);
                let name = format!(
                    "{}$boundary@{}",
                    instruction.provenance.region, instruction.provenance.span.line
                );
                self.activations.enter(&name).map_err(error)?;
                self.stack.push(Continuation::BoundaryWork {
                    action: &instruction.action,
                    name,
                    parent,
                });
                self.block(body);
            }
            Action::Loop { parameters, .. } => {
                let args = parameters
                    .iter()
                    .map(|(_, e)| evaluate(e, &mut self.values, self.frame.base))
                    .collect::<Result<_>>()?;
                self.iteration(&instruction.action, args);
            }
            Action::Supervise { boundary, args } => {
                let args = self.arguments(args)?;
                let handler = &self
                    .program
                    .region(boundary)
                    .and_then(|r| r.boundary.as_ref())
                    .ok_or_else(|| error("invalid boundary"))?
                    .handler;
                let child = self.activations.child();
                let parent = std::mem::replace(&mut self.activations, child);
                self.stack.push(Continuation::SupervisedWork {
                    name: boundary,
                    handler,
                    parent,
                });
                self.call(boundary, args)?;
            }
            Action::Decide {
                operation: Operation::Region(name) | Operation::Run(name) | Operation::Await(name),
                args,
                arms,
            } => {
                let args = self.arguments(args)?;
                self.stack.push(Continuation::Decision(arms));
                self.call(name, args)?;
            }
            _ => return self.leaf(instruction),
        }
        Ok(Flow::Fallthrough)
    }
    fn leaf(&mut self, instruction: &Instruction) -> Result<Flow> {
        let values = &mut self.values;
        let facts = &mut self.frame.facts;
        let base = self.frame.base;
        match &instruction.action {
            Action::Recover { target, args } => {
                return Ok(Flow::Recover(
                    *target,
                    args.iter()
                        .map(|e| evaluate(e, values, base))
                        .collect::<Result<_>>()?,
                ));
            }
            Action::Again { target, args } => {
                return Ok(Flow::Again(
                    *target,
                    args.iter()
                        .map(|e| evaluate(e, values, base))
                        .collect::<Result<_>>()?,
                ));
            }
            Action::Terminate => return Ok(Flow::Terminate),
            Action::TerminateProcess(status) => return Ok(Flow::Process(*status)),
            Action::Escalate => {
                self.activations.source_fault();
                return Err(self
                    .contingencies
                    .last()
                    .cloned()
                    .ok_or_else(|| error("escalate outside contingency"))?);
            }
            Action::Bind(id, expr) => {
                let value = evaluate(expr, values, base)?;
                values.insert(base + *id, value);
            }
            Action::Assign(place, expr) => {
                let value = evaluate(expr, values, base)?;
                let place = address(&shift(place, base), values)?;
                if place.fields.is_empty() {
                    values.insert(place.root, value);
                } else {
                    *place_mut(&place, values)? = value;
                }
            }
            Action::Drop(expr) => {
                drop(evaluate(expr, values, base)?);
            }
            Action::Release(ids) => {
                for id in ids {
                    values.remove(&(base + id));
                }
            }
            Action::Emit(expr) => {
                let Value::String(message) = evaluate(expr, values, base)? else {
                    return Err(error("interpreter invariant: emit type"));
                };
                self.events.push(Event::Emit { message });
            }
            Action::Fault(expr) => {
                let Value::String(message) = evaluate(expr, values, base)? else {
                    return Err(error("interpreter invariant: fault type"));
                };
                self.activations.source_fault();
                return Err(Diagnostic::new(
                    "E_FAULT",
                    instruction.provenance.span,
                    message,
                ));
            }
            Action::Exit(outcome, args) => {
                return Ok(Flow::Exit(
                    outcome.clone(),
                    args.iter()
                        .map(|a| evaluate(a, values, base))
                        .collect::<Result<_>>()?,
                ));
            }
            Action::Complete {
                atom,
                bindings,
                unwind,
                release,
            } => {
                let payloads = bindings
                    .iter()
                    .map(|(id, e)| Ok((*id, evaluate(e, values, base)?)))
                    .collect::<Result<Vec<_>>>()?;
                for (id, value) in payloads {
                    values.insert(base + id, value);
                }
                for id in release {
                    values.remove(&(base + id));
                }
                facts.insert(*atom);
                if let Some(target) = unwind {
                    return Ok(Flow::Complete(*target));
                }
            }
            Action::Decide {
                operation,
                args,
                arms,
            } => {
                let args = args
                    .iter()
                    .map(|a| evaluate(a, values, base))
                    .collect::<Result<Vec<_>>>()?;
                let (outcome, fields) = match operation {
                    Operation::Collection(name) => collection(name, args, values)?,
                    Operation::Region(name) | Operation::Run(name) | Operation::Await(name) => {
                        unreachable!("control calls have an explicit continuation: {name}")
                    }
                    Operation::Variant(expr) => match evaluate(expr, values, base)? {
                        Value::Bool(b) => (b.to_string(), vec![]),
                        Value::Construct { name, fields, .. } => (name, fields),
                        _ => return Err(error("interpreter invariant: decision operand")),
                    },
                    Operation::BorrowedVariant(expr) => {
                        let Value::Reference { ty, place } = evaluate(expr, values, base)? else {
                            return Err(error("invalid borrowed decision address"));
                        };
                        let outcomes = self
                            .program
                            .manifest
                            .borrowed_outcomes(&ty)
                            .ok_or_else(|| error("invalid borrowed decision type"))?;
                        match place_mut(&place, values)?.clone() {
                            Value::Bool(b) => (b.to_string(), vec![]),
                            Value::Construct {
                                name,
                                fields: payloads,
                                ..
                            } => {
                                let outcome = outcomes
                                    .iter()
                                    .find(|o| o.name == name)
                                    .ok_or_else(|| error("invalid borrowed decision tag"))?;
                                let fields = outcome
                                    .fields
                                    .iter()
                                    .enumerate()
                                    .map(|(i, f)| {
                                        let place =
                                            if let Value::Reference { place, .. } = &payloads[i] {
                                                place.clone()
                                            } else {
                                                let mut place = place.clone();
                                                place.fields.push(i);
                                                place
                                            };
                                        Value::Reference {
                                            ty: f.ty.clone(),
                                            place,
                                        }
                                    })
                                    .collect();
                                (name, fields)
                            }
                            _ => return Err(error("invalid borrowed decision storage")),
                        }
                    }
                    Operation::Primitive(name) => {
                        let primitive = self
                            .program
                            .manifest
                            .primitives
                            .iter()
                            .find(|p| p.name == *name)
                            .unwrap();
                        if primitive.pure {
                            let expression = Expr::Pure(
                                name.clone(),
                                args.into_iter().map(Expr::Value).collect(),
                            );
                            (
                                primitive.outcomes[0].name.clone(),
                                vec![evaluate(&expression, values, base)?],
                            )
                        } else {
                            let call = self.calls.get(self.cursor).ok_or_else(|| {
                                error(format!("script exhausted before `{name}`"))
                            })?;
                            if call.operation != *name {
                                return Err(error(format!(
                                    "expected script operation `{name}`, found `{}`",
                                    call.operation
                                )));
                            }
                            self.cursor += 1;
                            let outcome = primitive
                                .outcomes
                                .iter()
                                .find(|o| o.name == call.outcome)
                                .ok_or_else(|| {
                                    error(format!("undeclared driver outcome `{}`", call.outcome))
                                })?;
                            if outcome.fields.len() != call.fields.len() {
                                return Err(error("driver outcome payload arity mismatch"));
                            }
                            for (field, value) in outcome.fields.iter().zip(&call.fields) {
                                validate(value, &field.ty, &self.program.manifest)?;
                            }
                            self.events.push(Event::Primitive {
                                operation: name.clone(),
                                arguments: args,
                                outcome: call.outcome.clone(),
                                fields: call.fields.clone(),
                                effects: primitive.effects.clone(),
                            });
                            (call.outcome.clone(), call.fields.clone())
                        }
                    }
                };
                let arm = arms
                    .iter()
                    .find(|arm| arm.outcome == outcome)
                    .ok_or_else(|| error("interpreter invariant: missing decision arm"))?;
                if fields.len() != arm.slots.len() {
                    return Err(error("interpreter invariant: payload arity"));
                }
                facts.insert(arm.atom);
                for (slot, value) in arm.slots.iter().zip(fields) {
                    values.insert(base + *slot, value);
                }
            }
            Action::Boundary { .. } | Action::Loop { .. } | Action::Supervise { .. } => {
                unreachable!("structured instruction requires a continuation")
            }
        }
        Ok(Flow::Fallthrough)
    }
    fn finish(mut self, entry: &str) -> RunResult {
        let result = (|| {
            let completion = completion(
                self.flow
                    .take()
                    .expect("completed entry has a disposition")?,
            )?;
            self.phase = crate::fault::Phase::ResultValidation;
            if self.cursor != self.calls.len() {
                return Err(error(format!(
                    "{} unconsumed script calls",
                    self.calls.len() - self.cursor
                )));
            }
            let events = std::mem::take(&mut self.events);
            match completion {
                Completion::Outcome(outcome, fields) => Ok(RunCompletion::Outcome(Execution {
                    outcome,
                    fields,
                    events,
                })),
                Completion::Process(status) => Ok(RunCompletion::Process(ProcessTermination {
                    termination: "process",
                    status,
                    events,
                })),
                Completion::Settled => {
                    Err(error("entry settled without an executable disposition"))
                }
            }
        })();
        result.map_err(|diagnostic| {
            Box::new(Failure {
                events: std::mem::take(&mut self.events),
                report: self.activations.failure(
                    &self
                        .program
                        .region(entry)
                        .map(|r| self.program.runtime_name(&r.name))
                        .unwrap_or_else(|| entry.into()),
                    self.phase,
                    &format!("{}: {}", diagnostic.code, diagnostic.message),
                ),
                diagnostic,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admission_program() -> CheckedProgram {
        crate::compile(
            "struct box {u32 n;} control leaf(string text, box item) exits done {emit(text);drop(item);exit done;}",
            Manifest::from_json(include_str!("../../playground/manifest.json")).unwrap(),
        ).unwrap()
    }
    fn admission_arguments(text: &str) -> Vec<Value> {
        vec![
            Value::String(text.into()),
            Value::Construct {
                ty: "box".into(),
                name: "box".into(),
                fields: vec![Value::U32(1)],
            },
        ]
    }
    fn admission_scenario() -> Scenario {
        Scenario {
            entry: "leaf".into(),
            arguments: vec![],
            calls: vec![],
        }
    }
    fn assert_uninstalled(machine: &Machine<'_>, next_base: Id) {
        assert_eq!(machine.next_base, next_base);
        assert!(machine.values.is_empty());
        assert!(machine.stack.is_empty());
        assert!(machine.events.is_empty());
        assert_eq!(machine.frame.base, 0);
        assert!(machine.frame.facts.is_empty());
        assert_eq!(machine.cursor, 0);
        assert_eq!(machine.steps_left, 100);
        assert!(matches!(
            machine.phase,
            crate::fault::Phase::EntryValidation
        ));
    }

    #[test]
    fn preparing_invocations_retains_arguments_without_running_or_allocating_frames() {
        let p = admission_program();
        let s = admission_scenario();
        let mut session = p.start_execution(&s, 100);
        let machine = session.machine.as_mut().unwrap();
        let first = machine
            .prepare_call("leaf", admission_arguments("first"))
            .unwrap();
        let second = machine
            .prepare_call("leaf", admission_arguments("second"))
            .unwrap();
        assert_uninstalled(machine, 0);
        // Prepared values are independent; only consuming the handoff installs
        // a frame. Selecting their execution order is internal test setup, not
        // source scheduling or permission to leave a task unsettled.
        machine.start_call(second).unwrap();
        assert!(machine.events.is_empty());
        assert_eq!(machine.values.len(), 2);
        assert!(machine.drive(100));
        assert!(machine.values.is_empty());
        assert!(machine.stack.is_empty());
        machine.start_call(first).unwrap();
        assert!(machine.drive(100));
        assert_eq!(
            machine.events,
            vec![
                Event::Emit {
                    message: "second".into()
                },
                Event::Emit {
                    message: "first".into()
                }
            ]
        );
        assert!(machine.values.is_empty());
        assert!(machine.stack.is_empty());
    }

    #[test]
    fn malformed_later_arguments_leave_no_partial_invocation_state() {
        let p = admission_program();
        let s = admission_scenario();
        let mut session = p.start_execution(&s, 100);
        let machine = session.machine.as_mut().unwrap();
        assert!(
            machine
                .prepare_call("missing", admission_arguments("unused"))
                .is_err()
        );
        assert!(
            machine
                .prepare_call("leaf", vec![Value::String("unused".into())])
                .is_err()
        );
        let mut malformed = admission_arguments("must not be installed");
        let Value::Construct { fields, .. } = &mut malformed[1] else {
            panic!()
        };
        fields[0] = Value::Bool(true);
        let error = machine.prepare_call("leaf", malformed).err().unwrap();
        assert!(error.message.contains("expected U32"));
        assert_uninstalled(machine, 0);
        // Preparation and rejection have not consumed any activation identity.
        let mut probe = machine.activations.child();
        probe.enter("probe").unwrap();
        let report = probe.failure("probe", crate::fault::Phase::Execution, "test");
        assert_eq!(report.activations[0].id, 1);
    }

    #[test]
    fn invocation_admission_failure_never_installs_or_advances_storage() {
        let p = admission_program();
        let s = admission_scenario();
        for storage_exhausted in [true, false] {
            let mut session = p.start_execution(&s, 100);
            let machine = session.machine.as_mut().unwrap();
            if storage_exhausted {
                machine.next_base = usize::MAX;
            } else {
                machine.activations.failure(
                    "host",
                    crate::fault::Phase::EntryValidation,
                    "already failed",
                );
            }
            let next = machine.next_base;
            let prepared = machine
                .prepare_call("leaf", admission_arguments("unused"))
                .unwrap();
            let error = machine.start_call(prepared).unwrap_err();
            assert!(
                error.message.contains(if storage_exhausted {
                    "storage address space exhausted"
                } else {
                    "already failed"
                }),
                "{error}"
            );
            assert_uninstalled(machine, next);
        }
    }

    #[test]
    fn repeated_calls_and_loop_restarts_keep_only_live_continuations_and_slots() {
        let p = crate::compile(
            r#"control leaf(string value) exits done {drop(value);exit done;}
            control main() faults {loop repeat {leaf("temporary");again repeat;}}"#,
            Manifest::from_json(include_str!("../../playground/manifest.json")).unwrap(),
        )
        .unwrap();
        let s = Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![],
        };
        let mut session = p.start_execution(&s, 2000);
        for _ in 0..1000 {
            assert!(matches!(session.advance(1), ExecutionProgress::Paused));
            let machine = session.machine.as_ref().unwrap();
            assert!(
                machine.stack.len() <= 8,
                "completed continuations accumulated"
            );
            assert!(machine.values.len() <= 1, "completed storage accumulated");
            assert!(machine.frame.facts.len() <= 1, "loop decisions accumulated");
            assert!(machine.contingencies.is_empty());
        }
        let machine = session.machine.as_ref().unwrap();
        assert_eq!(machine.steps_left, 1000);
        assert!(machine.next_base > p.next_id * 100);
        let ExecutionProgress::Complete(Err(failure)) = session.advance(2000) else {
            panic!()
        };
        assert_eq!(failure.diagnostic.code, "E_FUEL");
        assert!(failure.report.activations.len() <= 2);
        assert_eq!(failure.report.omitted_activations, 0);
        assert!(matches!(session.advance(1), ExecutionProgress::Finished));
    }
}
