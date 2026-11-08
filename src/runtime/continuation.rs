//! Host stepping and explicit cooperative source scheduling over retained contexts.
use super::domains::{DomainId, Domains, Scope as DomainScope};
use super::tasks::{OwnerId, Readiness, Registry, TaskId, WaitId, retire_value};
use super::*;

#[cfg(test)]
#[path = "adapter_operation_model.rs"]
mod adapter_operation_model;
#[cfg(test)]
pub(crate) use adapter_operation_model::{
    HttpResponse, http_candidate_fault_model, http_source_operation_model,
    http_source_operation_sequence_model, pending_fault_model, source_operation_model,
};
#[cfg(test)]
#[path = "completion_model.rs"]
mod completion_model;
#[cfg(test)]
#[path = "context_tests.rs"]
mod context_tests;
#[cfg(test)]
#[path = "domain_tests.rs"]
mod domain_tests;
#[cfg(test)]
#[path = "readiness_model.rs"]
mod readiness_model;
#[cfg(test)]
#[path = "settlement_tests.rs"]
mod settlement_tests;
#[cfg(test)]
#[path = "task_tests.rs"]
mod task_tests;

#[cfg(test)]
#[path = "process_tests.rs"]
mod process_tests;

#[path = "completions.rs"]
mod completions;
// Internal started-task authority precedes source spawn/yield wiring.
#[allow(dead_code)]
#[path = "started.rs"]
mod started;
use started::StartedOrigin;
#[path = "source_scheduler.rs"]
mod source_scheduler;
use source_scheduler::Scheduler;
#[cfg(test)]
pub(crate) use source_scheduler::census_model;

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
                shared: SharedExecution {
                    next_base: 0,
                    identity: std::sync::Arc::new(()),
                    program: self,
                    calls: &scenario.calls,
                    cursor: 0,
                    events: vec![],
                    steps_left: steps,
                    values: BTreeMap::new(),
                    tasks: Registry::default(),
                    domains: Domains::default(),
                    suspended: SuspendedCalls::default(),
                    ready: BTreeMap::new(),
                    waiting: BTreeMap::new(),
                    completed: BTreeMap::new(),
                    scheduler: Scheduler::default(),
                },
                context: Context::new(crate::fault::Tracker::new(self.runtime_sites())),
                settlements: vec![],
                process_status: None,
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
                machine.context.flow = Some(Err(diagnostic));
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
        self.machine.as_ref().map_or(&[], |m| &m.shared.events)
    }
}

#[derive(Default)]
struct Frame {
    owner: Option<OwnerId>,
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

/// One scripted async host operation, prepared exactly once. Its arguments are
/// snapshotted for the completion event; a stopped operation publishes none.
struct HostOperation<'a> {
    operation: &'a str,
    arms: &'a [Arm],
    arguments: Vec<Value>,
    outcome: String,
    fields: Vec<Value>,
    effects: Vec<String>,
    waits: u32,
}

// Continuations represent sequential control flow, including consuming task awaits.
// Blocks borrow immutable checked code; only active invocation/loop state is kept.
enum Continuation<'a> {
    /// Test-only operation lifetime model; no public adapter suspension is admitted.
    #[cfg(test)]
    AdapterOperation(adapter_operation_model::Pending<'a>),
    /// A prepared async host operation that reported pending. Its completion was
    /// bound at preparation and is delivered once, after its remaining waits.
    HostOperation(HostOperation<'a>),
    Block {
        code: &'a [Instruction],
        next: usize,
    },
    Call {
        region: &'a Region,
        caller: Frame,
        root_domain: Option<DomainScope>,
    },
    Decision(&'a [Arm]),
    TaskDecision {
        task: TaskId,
        arms: &'a [Arm],
    },
    Dispatch,
    ResumeAwait {
        task: TaskId,
        wait: WaitId,
        caller: SuspensionId,
    },
    /// The owning source continuation was recovered after an unrelated child
    /// fault. Retire this await without returning to that caller a second time.
    StoppedAwait(TaskId),
    /// Drain retained borrowers before unwinding any inline helper frame.
    SourceStop,
    Loop(&'a Action),
    SupervisedWork {
        name: &'a str,
        handler: &'a str,
        parent: crate::fault::Tracker,
        domain: DomainScope,
    },
    SupervisedHandler {
        name: &'a str,
        parent: crate::fault::Tracker,
        domain: DomainScope,
    },
    BoundaryWork {
        action: &'a Action,
        name: String,
        parent: crate::fault::Tracker,
        domain: DomainScope,
    },
    BoundaryHandler {
        action: &'a Action,
        name: String,
        parent: crate::fault::Tracker,
        domain: DomainScope,
    },
}
/// One execution-wide arena and observation history. Selecting another context
/// must not clone values, reset address allocation, replay script inputs or refill
/// fuel. This is single-threaded state, not a source scheduler or shared-memory API.
struct SharedExecution<'a> {
    identity: std::sync::Arc<()>,
    next_base: Id,
    program: &'a CheckedProgram,
    calls: &'a [ScriptCall],
    cursor: usize,
    events: Vec<Event>,
    steps_left: usize,
    values: BTreeMap<Id, Value>,
    tasks: Registry,
    domains: Domains,
    suspended: SuspendedCalls<'a>,
    ready: BTreeMap<TaskId, ReadyCall<'a>>,
    waiting: BTreeMap<TaskId, Context<'a>>,
    completed: BTreeMap<TaskId, Context<'a>>,
    scheduler: Scheduler<'a>,
}

/// A resumable control chain. Storage and effect history belong to the execution;
/// facts, return/unwind continuations and fault context belong to this chain.
/// Moving a context preserves it without moving any referenced frame storage.
struct Context<'a> {
    domain: Option<DomainId>,
    task: Option<(TaskId, OwnerId)>,
    started: Option<StartedOrigin<'a>>,
    source_cause: Option<crate::fault::Report>,
    // A routed child fault skips intervening inline boundary contingencies.
    source_fault_target: Option<DomainId>,
    contingencies: Vec<Diagnostic>,
    activations: crate::fault::Tracker,
    phase: crate::fault::Phase,
    frame: Frame,
    stack: Vec<Continuation<'a>>,
    flow: Option<Result<Flow>>,
    handover: Vec<Value>,
}
impl Context<'_> {
    fn new(activations: crate::fault::Tracker) -> Self {
        Self {
            domain: None,
            task: None,
            started: None,
            source_cause: None,
            source_fault_target: None,
            contingencies: vec![],
            activations,
            phase: crate::fault::Phase::EntryValidation,
            frame: Frame::default(),
            stack: vec![],
            flow: None,
            handover: vec![],
        }
    }
}
/// One parking registration, distinct from a task and its completion wait. A
/// context may park repeatedly; every registration has a fresh, non-reused ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SuspensionId(u64);
struct SuspendedCall<'a> {
    task: TaskId,
    wait: WaitId,
    context: Context<'a>,
}
#[derive(Default)]
struct SuspendedCalls<'a> {
    next: u64,
    calls: BTreeMap<SuspensionId, SuspendedCall<'a>>,
}
impl<'a> SuspendedCalls<'a> {
    /// Preflight before task admission or moving the active context. No ID is
    /// reserved until commit, while the executor remains exclusively borrowed.
    fn prepare(&self) -> Result<SuspensionId> {
        self.next
            .checked_add(1)
            .map(SuspensionId)
            .ok_or_else(|| error("suspension identity space exhausted"))
    }
    fn commit(&mut self, id: SuspensionId, task: TaskId, wait: WaitId, context: Context<'a>) {
        assert_eq!(self.next.checked_add(1), Some(id.0));
        self.next = id.0;
        assert!(
            self.calls
                .insert(
                    id,
                    SuspendedCall {
                        task,
                        wait,
                        context
                    }
                )
                .is_none()
        );
    }
    fn get(&self, id: SuspensionId, task: TaskId, wait: WaitId) -> Result<&Context<'a>> {
        let call = self
            .calls
            .get(&id)
            .ok_or_else(|| error("suspended caller is unknown or already resumed"))?;
        if call.task != task || call.wait != wait {
            return Err(error(
                "suspended caller does not match task completion wait",
            ));
        }
        Ok(&call.context)
    }
    /// Validate before removing anything. Normal completion and emergency unwind
    /// both transfer the original context once; neither clones its retained state.
    fn take(&mut self, id: SuspensionId, task: TaskId, wait: WaitId) -> Result<Context<'a>> {
        self.get(id, task, wait)?;
        Ok(self.calls.remove(&id).unwrap().context)
    }
}
enum ReadyCall<'a> {
    Start {
        invocation: PreparedInvocation<'a>,
        context: Context<'a>,
    },
    Resume(Context<'a>),
}
struct Machine<'a> {
    shared: SharedExecution<'a>,
    context: Context<'a>,
    // Original emergency contexts, including the unconsumed boundary continuation.
    settlements: Vec<Context<'a>>,
    process_status: Option<u8>,
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
        let region = self.shared.program.region(&scenario.entry);
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
    fn require_live_process(&self) -> Result<()> {
        if self.process_status.is_some() {
            return Err(error("process execution has terminated"));
        }
        Ok(())
    }
    fn prepare_call(&self, name: &str, args: Vec<Value>) -> Result<PreparedInvocation<'a>> {
        let region = self.prepare_call_region(name, &args)?;
        Ok(PreparedInvocation {
            region,
            arguments: args,
            diagnostic_name: self.shared.program.runtime_name(name),
        })
    }
    fn prepare_call_region(&self, name: &str, args: &[Value]) -> Result<&'a Region> {
        self.require_live_process()?;
        let region = self
            .shared
            .program
            .region(name)
            .ok_or_else(|| error(format!("unknown entry region `{name}`")))?;
        if args.len() != region.parameters.len() {
            return Err(error("wrong entry argument count"));
        }
        // Validate without moving captures; queue admission may still fail.
        for (param, value) in region.parameters.iter().zip(args) {
            validate(value, &param.ty, self.shared.program.manifest())?;
        }
        Ok(region)
    }
    fn start_call(&mut self, invocation: PreparedInvocation<'a>) -> Result<()> {
        self.require_live_process()?;
        let PreparedInvocation {
            region,
            arguments,
            diagnostic_name,
        } = invocation;
        let base = self.shared.next_base;
        let next_base = base
            .checked_add(self.shared.program.next_id.max(1))
            .ok_or_else(|| error("activation storage address space exhausted"))?;
        let owner = self.shared.tasks.prepare_owner()?;
        let root_domain = match self.context.domain {
            None => Some(self.shared.domains.prepare(None)?),
            Some(domain) => {
                if self.context.contingencies.is_empty() {
                    self.shared.domains.require_open(domain)?;
                }
                None
            }
        };
        // All fallible admission checks precede frame installation. Preparing
        // an invocation neither reserves addresses nor consumes an activation ID.
        self.context
            .activations
            .enter(&diagnostic_name)
            .map_err(error)?;
        if let Some(scope) = root_domain {
            self.shared.domains.commit(scope);
            self.context.domain = Some(scope.id);
        }
        self.shared.tasks.commit_owner(owner);
        if let Some((task, _)) = self.context.task {
            self.shared.tasks.record_owner_task(owner, task);
        }
        self.shared.next_base = next_base;
        for (param, value) in region.parameters.iter().zip(arguments) {
            self.shared.values.insert(base + param.id, value);
        }
        self.context.phase = crate::fault::Phase::Execution;
        let caller = std::mem::replace(
            &mut self.context.frame,
            Frame {
                owner: Some(owner),
                base,
                facts: BTreeSet::new(),
            },
        );
        self.context.stack.push(Continuation::Call {
            region,
            caller,
            root_domain,
        });
        self.block(&region.code);
        Ok(())
    }
    fn block(&mut self, code: &'a [Instruction]) {
        self.context.flow = None;
        self.context
            .stack
            .push(Continuation::Block { code, next: 0 });
    }
    fn arguments(&mut self, args: &[Expr]) -> Result<Vec<Value>> {
        args.iter()
            .map(|e| evaluate(e, &mut self.shared.values, self.context.frame.base))
            .collect()
    }
    /// Returns false before the next source instruction or while settlement lacks
    /// an eligible child context. Neither case consumes the waiting continuation.
    /// No recursive source calls/loops/unwinding occur here.
    fn drive(&mut self, mut quota: usize) -> bool {
        loop {
            self.prune_source();
            match self.route_source_fault() {
                Ok(true) => continue,
                Ok(false) => {}
                Err(diagnostic) => {
                    self.context.flow = Some(Err(diagnostic));
                }
            }
            if let Some(Ok(Flow::Process(status))) = &self.context.flow {
                if let Some(original) = self.process_status {
                    assert_eq!(*status, original, "process retirement changed status");
                } else {
                    self.process_status = Some(*status);
                    self.shared.domains.stop_all();
                }
            }
            let Some(continuation) = self.context.stack.pop() else {
                if let Some(owner) = self.settlements.last() {
                    // This slice supports infallible reference retirement only.
                    // Never silently replace an unexpected cleanup failure with
                    // the original emergency or restore an incompletely unwound chain.
                    assert!(self.context.frame.owner.is_none());
                    match (self.context.flow.as_ref(), owner.flow.as_ref()) {
                        (Some(Err(actual)), Some(Err(expected))) => assert_eq!(actual, expected),
                        (Some(Ok(Flow::Process(actual))), Some(Ok(Flow::Process(expected)))) => {
                            assert_eq!(actual, expected)
                        }
                        _ => panic!("emergency retirement changed the retained disposition"),
                    }
                    // Independent work has no ResumeAwait continuation to
                    // retire its task. Release that obligation only after its
                    // complete emergency unwind, before resuming the owner.
                    if self.context.started.is_some() {
                        self.shared.tasks.abandon(self.context.task.unwrap().0);
                    }
                    let owner = self.settlements.pop().unwrap();
                    self.context = owner;
                    continue;
                }
                if self.process_status.is_some() && self.begin_retained_settlement(None) {
                    continue;
                }
                match self.source_complete() {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(diagnostic) => self.context.flow = Some(Err(diagnostic)),
                }
                return true;
            };
            if matches!(self.context.flow, Some(Err(_))) {
                if let Some(domain) = self.context.domain {
                    if let Err(diagnostic) = self.shared.domains.stop(domain) {
                        self.context.flow = Some(Err(diagnostic));
                    }
                }
            }
            #[cfg(test)]
            if matches!(continuation, Continuation::AdapterOperation(_))
                && self.context.flow.is_none()
            {
                self.context.stack.push(continuation);
                return false;
            }
            // Keep both the original emergency and the exact continuation before
            // inspecting children. A quota pause or unavailable child context must
            // never consume the boundary or release its owners prematurely.
            if matches!(self.context.flow, Some(Err(_) | Ok(Flow::Process(_))))
                && self.needs_settlement(&continuation)
            {
                self.context.stack.push(continuation);
                if self.begin_settlement() {
                    continue;
                }
                return false;
            }
            if let Continuation::Block { code, next } = continuation {
                if self.context.flow.is_some() {
                    continue;
                }
                let Some(instruction) = code.get(next) else {
                    self.context.flow = Some(Ok(Flow::Fallthrough));
                    continue;
                };
                if quota == 0 {
                    self.context.stack.push(Continuation::Block { code, next });
                    return false;
                }
                self.context.activations.at(instruction.id);
                if self.shared.steps_left == 0 {
                    self.context.flow = Some(Err(Diagnostic::new(
                        "E_FUEL",
                        instruction.provenance.span,
                        "reference interpreter step limit reached; this is a host limit, not a language outcome",
                    )));
                    continue;
                }
                quota -= 1;
                self.shared.steps_left -= 1;
                self.context.stack.push(Continuation::Block {
                    code,
                    next: next + 1,
                });
                if !instruction.guard.evaluate(&self.context.frame.facts) {
                    continue;
                }
                match self.instruction(instruction) {
                    Ok(Flow::Fallthrough) => {}
                    result => self.context.flow = Some(result),
                }
            } else if matches!(continuation, Continuation::HostOperation(_))
                && self.context.flow.is_none()
            {
                let Continuation::HostOperation(operation) = continuation else {
                    unreachable!()
                };
                if let Err(diagnostic) = self.poll_host_operation(operation) {
                    self.context.flow = Some(Err(diagnostic));
                }
            } else if let Continuation::Dispatch = continuation {
                // A source await requests dispatch. Host quota exhaustion alone
                // never selects another ready context.
                if let Err(diagnostic) = self.dispatch_ready() {
                    self.context.flow = Some(Err(diagnostic));
                }
            } else if let Err(diagnostic) = self.resume(continuation) {
                self.context.flow = Some(Err(diagnostic));
            }
        }
    }
    fn needs_settlement(&self, continuation: &Continuation<'a>) -> bool {
        let boundary = match continuation {
            Continuation::Call {
                region,
                root_domain,
                ..
            } => {
                if self
                    .context
                    .frame
                    .owner
                    .is_some_and(|owner| self.shared.tasks.has_started_children(owner))
                {
                    return true;
                }
                root_domain.is_some() || region.boundary.is_some()
            }
            Continuation::SupervisedWork { .. }
            | Continuation::SupervisedHandler { .. }
            | Continuation::BoundaryWork { .. }
            | Continuation::BoundaryHandler { .. } => true,
            _ => false,
        };
        boundary
            && self.context.domain.is_some_and(|domain| {
                self.shared
                    .domains
                    .has_children(domain)
                    .expect("a retained context has a live fault domain")
                    || self.has_retained_contexts(domain)
            })
    }
    /// Select affected descendants only for emergency retirement. Their nested
    /// contingencies cannot run; only the original owning boundary may handle the
    /// fault after every selected chain has retired. No candidate leaves the
    /// original boundary parked intact rather than discarding its continuation.
    fn begin_settlement(&mut self) -> bool {
        let Some(domain) = self.context.domain else {
            return false;
        };
        self.begin_retained_settlement(Some(domain))
    }
    /// None selects all registered domains only after the terminating chain has
    /// retired. Nested selections still drain their own borrowers before owners.
    fn begin_retained_settlement(&mut self, domain: Option<DomainId>) -> bool {
        if let Some(caller) = self.settle_completed_task(domain) {
            if let Some(context) = caller {
                self.select_settlement(context);
            }
            return true;
        }
        let ready = self.shared.ready.iter().filter_map(|(task, call)| {
            let context = match call {
                ReadyCall::Start { context, .. } | ReadyCall::Resume(context) => context,
            };
            context
                .domain
                .filter(|id| domain.is_none_or(|domain| self.shared.domains.contains(domain, *id)))
                .map(|_| *task)
        });
        let waiting = self.shared.waiting.iter().filter_map(|(task, context)| {
            context
                .domain
                .filter(|id| domain.is_none_or(|domain| self.shared.domains.contains(domain, *id)))
                .map(|_| *task)
        });
        let Some(task) = ready.chain(waiting).min() else {
            return self.settle_source(domain);
        };
        let context = if let Some(call) = self.shared.ready.remove(&task) {
            match call {
                ReadyCall::Start {
                    invocation,
                    context,
                } => {
                    // Never install a not-yet-started body during emergency cleanup.
                    // Reference retirement is infallible here; fallible/suspending
                    // external cleanup needs its own future settlement contract.
                    for argument in invocation.arguments {
                        retire_value(&mut self.shared.tasks, argument, true)
                            .expect("emergency captured-value retirement is infallible");
                    }
                    context
                }
                ReadyCall::Resume(context) => context,
            }
        } else {
            self.shared.waiting.remove(&task).unwrap()
        };
        self.select_settlement(context);
        true
    }
    fn select_settlement(&mut self, context: Context<'a>) {
        let disposition = match self.context.flow.as_ref().unwrap() {
            Err(diagnostic) => Err(diagnostic.clone()),
            Ok(Flow::Process(status)) => Ok(Flow::Process(*status)),
            _ => unreachable!("settlement requires an emergency disposition"),
        };
        let original = std::mem::replace(&mut self.context, context);
        self.settlements.push(original);
        self.context.flow = Some(disposition);
    }
    fn dispatch_ready(&mut self) -> Result<()> {
        self.require_live_process()?;
        if !self.context.stack.is_empty() || self.context.frame.owner.is_some() {
            return Err(error("dispatch cannot replace an active control chain"));
        }
        let task = self
            .shared
            .tasks
            .next_ready()
            .ok_or_else(|| error("dispatch has no ready task"))?;
        let context = match self.shared.ready.get(&task) {
            Some(ReadyCall::Start { context, .. } | ReadyCall::Resume(context)) => context,
            None => return Err(error("ready task has no retained execution context")),
        };
        self.shared.domains.require_open(
            context
                .domain
                .ok_or_else(|| error("ready context has no fault domain"))?,
        )?;
        let (_, owner) = context
            .task
            .filter(|(id, _)| *id == task)
            .ok_or_else(|| error("ready context has lost its original task"))?;
        self.shared.tasks.require_queued(task, owner)?;
        // Validate queue state before taking the prepared context. Installation
        // failure unwinds its already-retained caller through ResumeAwait.
        self.shared.tasks.dispatch(task)?;
        match self.shared.ready.remove(&task).unwrap() {
            ReadyCall::Start {
                invocation,
                context,
            } => {
                self.context = context;
                self.start_call(invocation)
            }
            ReadyCall::Resume(context) => {
                self.context = context;
                Ok(())
            }
        }
    }
    /// Internal safe-point admission only. Source/adapter wiring waits for owning
    /// fault-domain settlement; host quota exhaustion must never call this method.
    fn park_for_readiness(&mut self) -> Result<Readiness> {
        self.require_live_process()?;
        if self.context.frame.owner.is_none()
            || self.context.flow.is_some()
            || !self.context.contingencies.is_empty()
        {
            return Err(error("readiness requires a live normal continuation"));
        }
        self.shared.domains.require_open(
            self.context
                .domain
                .ok_or_else(|| error("running context has no fault domain"))?,
        )?;
        let (task, owner) = self
            .context
            .task
            .ok_or_else(|| error("readiness requires an admitted running task"))?;
        if self.shared.waiting.contains_key(&task) || self.shared.ready.contains_key(&task) {
            return Err(error("running task already has a retained context"));
        }
        let token = self.shared.tasks.prepare_readiness(task, owner)?;
        self.shared.tasks.park(owner, &token)?;
        let context = std::mem::replace(&mut self.context, Context::new(Default::default()));
        assert!(self.shared.waiting.insert(task, context).is_none());
        Ok(token)
    }
    /// Readiness changes eligibility only. Validate registry and context storage
    /// before consuming the one-use registration; dispatch remains explicit.
    fn signal_readiness(&mut self, token: &Readiness) -> Result<()> {
        self.require_live_process()?;
        let task = self.shared.tasks.waiting_task(token)?;
        if !self.shared.waiting.contains_key(&task) || self.shared.ready.contains_key(&task) {
            return Err(error(
                "waiting task has no unique retained execution context",
            ));
        }
        self.shared.domains.require_open(
            self.shared.waiting[&task]
                .domain
                .ok_or_else(|| error("waiting context has no fault domain"))?,
        )?;
        self.shared.tasks.wake(token)?;
        let context = self.shared.waiting.remove(&task).unwrap();
        assert!(
            self.shared
                .ready
                .insert(task, ReadyCall::Resume(context))
                .is_none()
        );
        Ok(())
    }
    fn resume(&mut self, continuation: Continuation<'a>) -> Result<()> {
        let flow = self
            .context
            .flow
            .take()
            .expect("a continuation receives completed nested work");
        match continuation {
            #[cfg(test)]
            Continuation::AdapterOperation(pending) => {
                assert!(matches!(flow, Err(_) | Ok(Flow::Process(_))));
                pending.retire(self)?;
                self.context.flow = Some(flow);
            }
            Continuation::HostOperation(operation) => {
                assert!(matches!(flow, Err(_) | Ok(Flow::Process(_))));
                for value in operation.arguments.into_iter().chain(operation.fields) {
                    retire_value(&mut self.shared.tasks, value, true)?;
                }
                self.context.flow = Some(flow);
            }
            Continuation::Block { .. } | Continuation::Dispatch => unreachable!(),
            Continuation::Call {
                region,
                caller,
                root_domain,
            } => {
                self.context.handover.clear();
                let result = (|| {
                    if root_domain.is_some() || region.boundary.is_some() {
                        self.require_domain_quiescent(self.context.domain.unwrap())?;
                    }
                    if flow.is_err()
                        && self.context.activations.is_source_fault()
                        && self.settlements.is_empty()
                        && self
                            .context
                            .source_fault_target
                            .is_none_or(|target| Some(target) == self.context.domain)
                    {
                        if let Some(boundary) = &region.boundary {
                            for id in &boundary.retained {
                                self.context.handover.push(
                                    self.shared
                                        .values
                                        .remove(&(self.context.frame.base + id))
                                        .ok_or_else(|| {
                                            error("retained boundary resource is unavailable")
                                        })?,
                                );
                            }
                        }
                    }
                    let abrupt = !matches!(&flow, Ok(Flow::Exit(..)));
                    self.retire_slots(
                        region
                            .slots
                            .keys()
                            .map(|id| self.context.frame.base + id)
                            .collect(),
                        abrupt,
                    )?;
                    self.shared
                        .tasks
                        .retire_owner(self.context.frame.owner.unwrap(), abrupt)?;
                    let flow = flow?;
                    match &flow {
                        Flow::Process(_) | Flow::Terminate | Flow::Exit(..) => {
                            self.context.activations.leave()
                        }
                        _ => {
                            return Err(error(
                                "interpreter invariant: checked region fell through",
                            ));
                        }
                    }
                    Ok(flow)
                })();
                self.context.frame = caller;
                self.context.flow = Some(result);
                if let Some(scope) = root_domain {
                    self.close_domain(scope)?;
                }
            }
            Continuation::Decision(arms) => match flow {
                Ok(flow @ Flow::Process(_)) => self.context.flow = Some(Ok(flow)),
                other => {
                    let (outcome, fields) = completion(other?)?.outcome()?;
                    self.bind(arms, outcome, fields)?;
                }
            },
            Continuation::TaskDecision { task, arms } => {
                if let Ok(flow @ Flow::Process(_)) = flow {
                    self.shared.tasks.abandon(task);
                    self.context.flow = Some(Ok(flow));
                    return Ok(());
                }
                match flow.and_then(completion).and_then(Completion::outcome) {
                    Ok((outcome, fields)) => {
                        let owner = self.context.frame.owner.unwrap();
                        self.shared.tasks.consume(task, owner)?;
                        self.bind(arms, outcome, fields)?;
                    }
                    Err(diagnostic) => {
                        if !self.shared.completed.contains_key(&task) {
                            self.shared.tasks.abandon(task);
                        }
                        return Err(diagnostic);
                    }
                }
            }
            Continuation::ResumeAwait { task, wait, caller } => {
                let owner = self
                    .shared
                    .suspended
                    .get(caller, task, wait)?
                    .frame
                    .owner
                    .unwrap();
                let result = match flow {
                    Ok(flow @ Flow::Exit(..)) => self.complete_await(task, owner, wait, flow),
                    other => {
                        // Fault delivery is an unwind, never a normal completion
                        // signal. Retire the wait before the caller's contingency.
                        self.shared.tasks.abandon(task);
                        other
                    }
                };
                let mut caller = self.shared.suspended.take(caller, task, wait)?;
                // The sequential diagnostic chain travels with the selected
                // child, preserving activation IDs, fault causes and call sites.
                caller.activations = std::mem::take(&mut self.context.activations);
                caller.phase = self.context.phase.clone();
                caller.handover = std::mem::take(&mut self.context.handover);
                caller.flow = Some(result);
                caller.source_fault_target = self
                    .context
                    .source_fault_target
                    .take()
                    .or(caller.source_fault_target);
                self.context = caller;
            }
            Continuation::SourceStop => {
                assert!(matches!(flow, Err(_)));
                self.context.flow = Some(flow);
                // Retire selectable borrowers before releasing inline frames.
                // Suspended await ancestors can share this domain, but must
                // resume through this chain's unwind before the owning boundary
                // requires full domain quiescence. They cannot be settled here.
                self.context.stack.push(Continuation::SourceStop);
                let domain = self.context.source_fault_target.or(self.context.domain);
                if !self.begin_retained_settlement(domain) {
                    self.context.stack.pop();
                }
            }
            Continuation::StoppedAwait(task) => {
                assert!(!self.settlements.is_empty());
                assert!(matches!(flow, Err(_) | Ok(Flow::Process(_))));
                self.shared.tasks.abandon(task);
                self.context.flow = Some(flow);
            }
            Continuation::Loop(action) => {
                let Action::Loop { id, .. } = action else {
                    unreachable!()
                };
                match flow? {
                    Flow::Again(target, args) if target == *id => {
                        self.iteration(action, args, false)?
                    }
                    Flow::Recover(target, args) if target == *id => {
                        self.iteration(action, args, true)?
                    }
                    Flow::Complete(target) if target == *id => {}
                    Flow::Fallthrough => {
                        return Err(error("interpreter invariant: loop fell through"));
                    }
                    other => self.context.flow = Some(Ok(other)),
                }
            }
            Continuation::SupervisedWork {
                name,
                handler,
                parent,
                domain,
            } => match flow {
                Err(diagnostic)
                    if self.context.activations.is_source_fault()
                        && self.settlements.is_empty()
                        && self
                            .context
                            .source_fault_target
                            .is_none_or(|target| target == domain.id) =>
                {
                    self.context.source_fault_target = None;
                    self.require_domain_quiescent(domain.id)?;
                    let report = self.report(&self.shared.program.runtime_name(name), &diagnostic);
                    self.shared.events.push(Event::Fault {
                        report: Box::new(report.clone()),
                    });
                    let mut tracker = self.context.activations.child();
                    tracker.with_cause(report);
                    self.context.activations = tracker;
                    let mut retained = std::mem::take(&mut self.context.handover);
                    retained.push(Value::String(diagnostic.message.clone()));
                    self.context.contingencies.push(diagnostic);
                    self.context.stack.push(Continuation::SupervisedHandler {
                        name,
                        parent,
                        domain,
                    });
                    self.call(handler, retained)?;
                }
                other => self.finish_supervision(name, parent, domain, other)?,
            },
            Continuation::SupervisedHandler {
                name,
                parent,
                domain,
            } => {
                self.context.contingencies.pop();
                self.finish_supervision(name, parent, domain, flow)?;
            }
            Continuation::BoundaryWork {
                action,
                name,
                parent,
                domain,
            } => {
                match flow {
                    Err(diagnostic)
                        if self.context.activations.is_source_fault()
                            && self.settlements.is_empty()
                            && self
                                .context
                                .source_fault_target
                                .is_none_or(|target| target == domain.id) =>
                    {
                        self.context.source_fault_target = None;
                        self.require_domain_quiescent(domain.id)?;
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
                        self.shared.events.push(Event::Fault {
                            report: Box::new(report.clone()),
                        });
                        self.retire_slots(
                            retire
                                .iter()
                                .map(|id| self.context.frame.base + id)
                                .collect(),
                            true,
                        )?;
                        let (message, stack, paths) = report.source_fields();
                        let strings = |xs: Vec<String>| Value::Construct {
                            ty: "vector<String>".into(),
                            name: "vector".into(),
                            fields: xs.into_iter().map(Value::String).collect(),
                        };
                        self.shared.values.insert(
                            self.context.frame.base + fault_slot,
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
                        let mut tracker = self.context.activations.child();
                        tracker.with_cause(report);
                        self.context.activations = tracker;
                        // Preserve restoration even if admission fails before the handler starts.
                        self.context.stack.push(Continuation::BoundaryHandler {
                            action,
                            name: name.clone(),
                            parent,
                            domain,
                        });
                        self.context.contingencies.push(diagnostic);
                        self.context
                            .activations
                            .enter(&format!("{name}$contingency"))
                            .map_err(error)?;
                        self.block(handler);
                    }
                    other => self.finish_boundary(action, &name, parent, domain, other)?,
                }
            }
            Continuation::BoundaryHandler {
                action,
                name,
                parent,
                domain,
            } => {
                self.context.contingencies.pop();
                self.finish_boundary(action, &name, parent, domain, flow)?;
            }
        }
        Ok(())
    }
    fn has_retained_contexts(&self, domain: DomainId) -> bool {
        let ready = self.shared.ready.values().any(|call| {
            let context = match call {
                ReadyCall::Start { context, .. } | ReadyCall::Resume(context) => context,
            };
            context.domain == Some(domain)
        });
        ready
            || self.source_retained(domain)
            || self
                .shared
                .completed
                .values()
                .any(|c| c.domain == Some(domain))
            || self
                .shared
                .waiting
                .values()
                .any(|c| c.domain == Some(domain))
            || self
                .shared
                .suspended
                .calls
                .values()
                .any(|call| call.context.domain == Some(domain))
    }
    fn require_domain_quiescent(&self, domain: DomainId) -> Result<()> {
        self.shared.domains.require_no_children(domain)?;
        if self.has_retained_contexts(domain) {
            return Err(error(
                "fault domain still has retained execution contexts requiring settlement",
            ));
        }
        Ok(())
    }
    fn close_domain(&mut self, scope: DomainScope) -> Result<()> {
        self.require_domain_quiescent(scope.id)?;
        self.shared.domains.close(scope)?;
        self.context.domain = scope.parent;
        Ok(())
    }
    fn report(&mut self, name: &str, diagnostic: &Diagnostic) -> crate::fault::Report {
        self.context.activations.failure(
            name,
            crate::fault::Phase::Execution,
            &format!("{}: {}", diagnostic.code, diagnostic.message),
        )
    }
    fn restore(&mut self, name: &str, mut parent: crate::fault::Tracker, flow: &Result<Flow>) {
        parent.absorb(&self.context.activations);
        if let Err(diagnostic) = flow {
            if self.context.activations.is_source_fault() {
                parent.source_fault();
            }
            parent.with_cause(self.report(name, diagnostic));
        }
        self.context.activations = parent;
    }
    fn finish_supervision(
        &mut self,
        name: &str,
        parent: crate::fault::Tracker,
        domain: DomainScope,
        flow: Result<Flow>,
    ) -> Result<()> {
        self.require_domain_quiescent(domain.id)?;
        self.close_domain(domain)?;
        let result = match flow {
            Ok(flow @ Flow::Process(_)) => Ok(flow),
            other => other
                .and_then(completion)
                .and_then(Completion::settled)
                .map(|()| Flow::Fallthrough),
        };
        self.restore(&self.shared.program.runtime_name(name), parent, &result);
        self.context.flow = match result {
            Ok(Flow::Fallthrough) => None,
            other => Some(other),
        };
        Ok(())
    }
    fn finish_boundary(
        &mut self,
        action: &Action,
        name: &str,
        parent: crate::fault::Tracker,
        domain: DomainScope,
        flow: Result<Flow>,
    ) -> Result<()> {
        let Action::Boundary {
            work_slots,
            handler_slots,
            ..
        } = action
        else {
            unreachable!()
        };
        self.require_domain_quiescent(domain.id)?;
        let abrupt = matches!(
            &flow,
            Err(_) | Ok(Flow::Recover(..) | Flow::Process(_) | Flow::Terminate)
        );
        self.retire_slots(
            work_slots
                .iter()
                .chain(handler_slots)
                .map(|id| self.context.frame.base + id)
                .collect(),
            abrupt,
        )?;
        self.close_domain(domain)?;
        self.restore(name, parent, &flow);
        self.context.flow = match flow {
            Ok(Flow::Fallthrough) => None,
            other => Some(other),
        };
        Ok(())
    }
    fn retire_slots(&mut self, slots: Vec<Id>, abrupt: bool) -> Result<()> {
        for slot in slots {
            if let Some(value) = self.shared.values.remove(&slot) {
                retire_value(&mut self.shared.tasks, value, abrupt)?;
            }
        }
        Ok(())
    }
    fn iteration(&mut self, action: &'a Action, arguments: Vec<Value>, abrupt: bool) -> Result<()> {
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
        self.retire_slots(
            local_slots
                .iter()
                .map(|id| self.context.frame.base + id)
                .collect(),
            abrupt,
        )?;
        for atom in local_atoms {
            self.context.frame.facts.remove(atom);
        }
        for ((slot, _), value) in parameters.iter().zip(arguments) {
            self.shared
                .values
                .insert(self.context.frame.base + slot, value);
        }
        self.context.stack.push(Continuation::Loop(action));
        self.block(body);
        Ok(())
    }
    fn bind(&mut self, arms: &[Arm], outcome: String, fields: Vec<Value>) -> Result<()> {
        let arm = arms
            .iter()
            .find(|a| a.outcome == outcome)
            .ok_or_else(|| error("interpreter invariant: missing decision arm"))?;
        if fields.len() != arm.slots.len() {
            return Err(error("interpreter invariant: payload arity"));
        }
        self.context.frame.facts.insert(arm.atom);
        for (slot, value) in arm.slots.iter().zip(fields) {
            self.shared
                .values
                .insert(self.context.frame.base + *slot, value);
        }
        Ok(())
    }
    fn instruction(&mut self, instruction: &'a Instruction) -> Result<Flow> {
        match &instruction.action {
            Action::Boundary { body, .. } => {
                let domain = self.shared.domains.prepare(self.context.domain)?;
                let mut child = self.context.activations.child();
                let name = format!(
                    "{}$boundary@{}",
                    instruction.provenance.region, instruction.provenance.span.line
                );
                child.enter(&name).map_err(error)?;
                let parent = std::mem::replace(&mut self.context.activations, child);
                self.shared.domains.commit(domain);
                self.context.domain = Some(domain.id);
                self.context.stack.push(Continuation::BoundaryWork {
                    action: &instruction.action,
                    name,
                    parent,
                    domain,
                });
                self.block(body);
            }
            Action::Loop { parameters, .. } => {
                let args = parameters
                    .iter()
                    .map(|(_, e)| evaluate(e, &mut self.shared.values, self.context.frame.base))
                    .collect::<Result<_>>()?;
                self.iteration(&instruction.action, args, false)?;
            }
            Action::Supervise { boundary, args } => {
                let args = self.arguments(args)?;
                let handler = &self
                    .shared
                    .program
                    .region(boundary)
                    .and_then(|r| r.boundary.as_ref())
                    .ok_or_else(|| error("invalid boundary"))?
                    .handler;
                let domain = self.shared.domains.prepare(self.context.domain)?;
                let child = self.context.activations.child();
                let parent = std::mem::replace(&mut self.context.activations, child);
                self.shared.domains.commit(domain);
                self.context.domain = Some(domain.id);
                self.context.stack.push(Continuation::SupervisedWork {
                    name: boundary,
                    handler,
                    parent,
                    domain,
                });
                self.call(boundary, args)?;
            }
            Action::Decide {
                operation: Operation::Spawn(name),
                args,
                arms,
            } => {
                self.source_spawn(name, args, arms)?;
            }
            Action::Decide {
                operation: Operation::Join(name),
                args,
                arms,
            } => {
                self.source_join(name, args, arms)?;
            }
            Action::Decide {
                operation: Operation::SpawnInto(name),
                args,
                arms,
            } => {
                self.source_spawn_into(name, args, arms)?;
            }
            Action::Decide {
                operation: Operation::Next(name),
                args,
                arms,
            } => {
                self.source_next(name, args, arms)?;
            }
            Action::Decide {
                operation: Operation::Cancel(name),
                args,
                arms,
            } => {
                self.source_cancel(name, args, arms)?;
            }
            Action::Decide {
                operation: Operation::Yield,
                arms,
                ..
            } => {
                self.bind(arms, "$yield".into(), vec![])?;
                self.yield_source()?;
            }
            Action::Decide {
                operation: op @ (Operation::AwaitTask(name) | Operation::Await(name)),
                args,
                arms,
            } => {
                self.shared
                    .domains
                    .require_open(self.context.domain.unwrap())?;
                let mut values = self.arguments(args)?;
                let (task, arguments) = if matches!(op, Operation::AwaitTask(_)) {
                    let Some(Value::Task {
                        id,
                        operation,
                        arguments,
                        ..
                    }) = values.pop()
                    else {
                        return Err(error(
                            "interpreter invariant: await requires a pending task",
                        ));
                    };
                    if !values.is_empty() || operation != *name {
                        return Err(error(
                            "interpreter invariant: pending task producer differs",
                        ));
                    }
                    (Some(TaskId(id)), arguments)
                } else {
                    (None, values)
                };
                let invocation = self.prepare_call(name, arguments)?;
                let suspension = self.shared.suspended.prepare()?;
                let owner = self.context.frame.owner.unwrap();
                let (task, wait) = match task {
                    Some(task) => (task, self.shared.tasks.schedule(task, owner, name)?),
                    None => self.shared.tasks.queue_await(owner, name)?,
                };
                self.context
                    .stack
                    .push(Continuation::TaskDecision { task, arms });
                let activations = std::mem::take(&mut self.context.activations);
                let mut child = Context::new(activations);
                child.task = Some((task, self.context.frame.owner.unwrap()));
                child.domain = self.context.domain;
                child.phase = self.context.phase.clone();
                let caller = std::mem::replace(&mut self.context, Context::new(Default::default()));
                self.shared.suspended.commit(suspension, task, wait, caller);
                child.stack.push(Continuation::ResumeAwait {
                    task,
                    wait,
                    caller: suspension,
                });
                assert!(
                    self.shared
                        .ready
                        .insert(
                            task,
                            ReadyCall::Start {
                                invocation,
                                context: child
                            }
                        )
                        .is_none()
                );
                if self.shared.scheduler.enabled {
                    self.queue_source_await(task)?;
                } else {
                    self.context.stack.push(Continuation::Dispatch);
                }
            }
            Action::Decide {
                operation: Operation::Primitive(name),
                args,
                arms,
            } if self.host_operation(name).is_some() => {
                if self.observes_cancellation(&self.context) {
                    // A cancelled operation does not start: no script entry is
                    // consumed and no completion event is published.
                    for value in self.arguments(args)? {
                        retire_value(&mut self.shared.tasks, value, true)?;
                    }
                    return Ok(Flow::Exit("cancelled".into(), vec![]));
                }
                self.start_host_operation(name, args, arms)?;
            }
            Action::Decide {
                operation: Operation::Region(name) | Operation::Run(name),
                args,
                arms,
            } => {
                let args = self.arguments(args)?;
                self.context.stack.push(Continuation::Decision(arms));
                self.call(name, args)?;
            }
            _ => return self.leaf(instruction),
        }
        Ok(Flow::Fallthrough)
    }
    fn host_operation(&self, name: &str) -> Option<&'a crate::manifest::Primitive> {
        let program: &'a CheckedProgram = self.shared.program;
        program
            .manifest
            .primitives
            .iter()
            .find(|p| p.name == name && p.asynchronous)
    }
    /// Validate and consume the scripted completion when the operation starts.
    /// Pending reports never consume or replay script entries.
    fn start_host_operation(
        &mut self,
        name: &'a str,
        args: &[Expr],
        arms: &'a [Arm],
    ) -> Result<()> {
        let primitive = self.host_operation(name).unwrap();
        let arguments = self.arguments(args)?;
        let call = self
            .shared
            .calls
            .get(self.shared.cursor)
            .ok_or_else(|| error(format!("script exhausted before `{name}`")))?;
        if call.operation != *name {
            return Err(error(format!(
                "expected script operation `{name}`, found `{}`",
                call.operation
            )));
        }
        let outcome = primitive
            .outcomes
            .iter()
            .find(|o| o.name == call.outcome)
            .ok_or_else(|| error(format!("undeclared driver outcome `{}`", call.outcome)))?;
        if outcome.fields.len() != call.fields.len() {
            return Err(error("driver outcome payload arity mismatch"));
        }
        for (field, value) in outcome.fields.iter().zip(&call.fields) {
            validate(value, &field.ty, &self.shared.program.manifest)?;
        }
        let operation = HostOperation {
            operation: name,
            arms,
            arguments,
            outcome: call.outcome.clone(),
            fields: call.fields.clone(),
            effects: primitive.effects.clone(),
            waits: call.waits,
        };
        self.shared.cursor += 1;
        if operation.waits == 0 {
            return self.complete_host_operation(operation);
        }
        self.context
            .stack
            .push(Continuation::HostOperation(operation));
        self.park_host_operation()
    }
    /// A woken operation polls once without instruction fuel or host quota.
    fn poll_host_operation(&mut self, mut operation: HostOperation<'a>) -> Result<()> {
        if self.observes_cancellation(&self.context) {
            // The pending operation stops without publishing its completion.
            for value in operation.arguments.into_iter().chain(operation.fields) {
                retire_value(&mut self.shared.tasks, value, true)?;
            }
            self.context.flow = Some(Ok(Flow::Exit("cancelled".into(), vec![])));
            return Ok(());
        }
        operation.waits -= 1;
        if operation.waits == 0 {
            return self.complete_host_operation(operation);
        }
        self.context
            .stack
            .push(Continuation::HostOperation(operation));
        self.park_host_operation()
    }
    fn complete_host_operation(&mut self, operation: HostOperation<'a>) -> Result<()> {
        self.bind(
            operation.arms,
            operation.outcome.clone(),
            operation.fields.clone(),
        )?;
        self.shared.events.push(Event::Primitive {
            operation: operation.operation.into(),
            arguments: operation.arguments,
            outcome: operation.outcome,
            fields: operation.fields,
            effects: operation.effects,
        });
        Ok(())
    }
    fn leaf(&mut self, instruction: &Instruction) -> Result<Flow> {
        let values = &mut self.shared.values;
        let facts = &mut self.context.frame.facts;
        let base = self.context.frame.base;
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
                self.context.activations.source_fault();
                return Err(self
                    .context
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
                let value = evaluate(expr, values, base)?;
                retire_value(&mut self.shared.tasks, value, false)?;
            }
            Action::Release(ids) => {
                for id in ids {
                    if let Some(value) = values.remove(&(base + id)) {
                        retire_value(&mut self.shared.tasks, value, false)?;
                    }
                }
            }
            Action::Emit(expr) => {
                let Value::String(message) = evaluate(expr, values, base)? else {
                    return Err(error("interpreter invariant: emit type"));
                };
                self.shared.events.push(Event::Emit { message });
            }
            Action::Fault(expr) => {
                let Value::String(message) = evaluate(expr, values, base)? else {
                    return Err(error("interpreter invariant: fault type"));
                };
                self.context.activations.source_fault();
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
                    if let Some(value) = values.remove(&(base + id)) {
                        retire_value(&mut self.shared.tasks, value, false)?;
                    }
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
                if matches!(operation, Operation::Defer(_)) {
                    self.shared
                        .domains
                        .require_open(self.context.domain.unwrap())?;
                }
                let args = args
                    .iter()
                    .map(|a| evaluate(a, values, base))
                    .collect::<Result<Vec<_>>>()?;
                let (outcome, fields) = match operation {
                    Operation::Defer(name) => (
                        "$pending".into(),
                        vec![Value::Task {
                            id: self
                                .shared
                                .tasks
                                .defer(self.context.frame.owner.unwrap(), name)?
                                .0,
                            ty: crate::manifest::task_type(name),
                            operation: name.clone(),
                            arguments: args,
                        }],
                    ),
                    Operation::AwaitTask(_)
                    | Operation::Join(_)
                    | Operation::Spawn(_)
                    | Operation::SpawnInto(_)
                    | Operation::Next(_)
                    | Operation::Cancel(_)
                    | Operation::Yield => unreachable!("task operation has a continuation"),
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
                            .shared
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
                            .shared
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
                            let call =
                                self.shared.calls.get(self.shared.cursor).ok_or_else(|| {
                                    error(format!("script exhausted before `{name}`"))
                                })?;
                            if call.operation != *name {
                                return Err(error(format!(
                                    "expected script operation `{name}`, found `{}`",
                                    call.operation
                                )));
                            }
                            if call.waits != 0 {
                                return Err(error(format!(
                                    "scripted waits require an async host operation; `{name}` completes immediately"
                                )));
                            }
                            self.shared.cursor += 1;
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
                                validate(value, &field.ty, &self.shared.program.manifest)?;
                            }
                            self.shared.events.push(Event::Primitive {
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
                self.context
                    .flow
                    .take()
                    .expect("completed entry has a disposition")?,
            )?;
            self.context.phase = crate::fault::Phase::ResultValidation;
            if self.shared.cursor != self.shared.calls.len() {
                return Err(error(format!(
                    "{} unconsumed script calls",
                    self.shared.calls.len() - self.shared.cursor
                )));
            }
            let events = std::mem::take(&mut self.shared.events);
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
                events: std::mem::take(&mut self.shared.events),
                report: self.context.activations.failure(
                    &self
                        .shared
                        .program
                        .region(entry)
                        .map(|r| self.shared.program.runtime_name(&r.name))
                        .unwrap_or_else(|| entry.into()),
                    self.context.phase,
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
        assert_eq!(machine.shared.next_base, next_base);
        assert!(machine.shared.values.is_empty());
        assert!(machine.shared.tasks.is_empty());
        assert!(machine.context.stack.is_empty());
        assert!(machine.shared.events.is_empty());
        assert_eq!(machine.context.frame.base, 0);
        assert!(machine.context.frame.facts.is_empty());
        assert_eq!(machine.shared.cursor, 0);
        assert_eq!(machine.shared.steps_left, 100);
        assert!(matches!(
            machine.context.phase,
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
        assert!(machine.shared.events.is_empty());
        assert_eq!(machine.shared.values.len(), 2);
        assert!(machine.drive(100));
        assert!(machine.shared.values.is_empty());
        assert!(machine.shared.tasks.is_empty());
        assert!(machine.context.stack.is_empty());
        machine.start_call(first).unwrap();
        assert!(machine.drive(100));
        assert_eq!(
            machine.shared.events,
            vec![
                Event::Emit {
                    message: "second".into()
                },
                Event::Emit {
                    message: "first".into()
                }
            ]
        );
        assert!(machine.shared.values.is_empty());
        assert!(machine.shared.tasks.is_empty());
        assert!(machine.context.stack.is_empty());
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
        let mut probe = machine.context.activations.child();
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
                machine.shared.next_base = usize::MAX;
            } else {
                machine.context.activations.failure(
                    "host",
                    crate::fault::Phase::EntryValidation,
                    "already failed",
                );
            }
            let next = machine.shared.next_base;
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
                machine.context.stack.len() <= 8,
                "completed continuations accumulated"
            );
            assert!(
                machine.shared.values.len() <= 1,
                "completed storage accumulated"
            );
            assert!(
                machine.context.frame.facts.len() <= 1,
                "loop decisions accumulated"
            );
            assert!(machine.context.contingencies.is_empty());
        }
        let machine = session.machine.as_ref().unwrap();
        assert_eq!(machine.shared.steps_left, 1000);
        assert!(machine.shared.next_base > p.next_id * 100);
        let ExecutionProgress::Complete(Err(failure)) = session.advance(2000) else {
            panic!()
        };
        assert_eq!(failure.diagnostic.code, "E_FUEL");
        assert!(failure.report.activations.len() <= 2);
        assert_eq!(failure.report.omitted_activations, 0);
        assert!(matches!(session.advance(1), ExecutionProgress::Finished));
    }
}
