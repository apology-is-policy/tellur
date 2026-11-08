//! Shared retained compiled continuations; no checked graph is interpreted here.
use super::{
    BoundaryState, Completion, Descriptor, Frames, Host, HostPoll, HostProgress, HostStart,
    Outcome, R, V, frames::Frame,
};
use domains::{DomainKey, DomainScope, Domains, Membership};

#[cfg(test)]
#[path = "access_resolution.rs"]
pub mod access_resolution;
#[cfg(test)]
#[path = "dependencies.rs"]
pub mod dependencies;
#[cfg(test)]
#[path = "reference_evidence.rs"]
pub mod reference_evidence;
use dependencies::Dependencies;

pub type Deliver = fn(&mut Activation, &mut Frames, Completion) -> R<()>;
/// Binds an async host operation's completion into its generated control.
pub type HostDeliver = fn(&mut Activation, &mut Frames, String, Vec<V>) -> R<()>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessKind {
    Read,
    Move,
    Shared,
    Exclusive,
    Write,
    Retire,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access {
    /// None denotes the constructor's caller-supplied destination.
    pub slot: Option<usize>,
    pub fields: &'static [usize],
    pub indirect: bool,
    pub kind: AccessKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    Unknown,
    Skip,
    Local,
    Effect,
    Call,
    Run,
    Await,
    AwaitTask,
    Defer,
    Boundary,
    BoundaryEnd,
    Loop,
    Restart,
    Recover,
    Return,
    Complete,
    Fault,
    Process,
    Delivery,
}
#[derive(Clone, Copy)]
pub struct ColdAwait {
    pub slot: usize,
    pub producer: &'static Control,
}
impl std::fmt::Debug for ColdAwait {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColdAwait")
            .field("slot", &self.slot)
            .field("producer", &self.producer.name)
            .finish()
    }
}
impl PartialEq for ColdAwait {
    fn eq(&self, other: &Self) -> bool {
        self.slot == other.slot && std::ptr::eq(self.producer, other.producer)
    }
}
impl Eq for ColdAwait {}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessPlan {
    /// Exact local handle and compiled producer; not a child-body access plan.
    pub cold_await: Option<ColdAwait>,
    pub transition: Transition,
    pub accesses: &'static [Access],
    /// Union of possible normal result writes; separate from argument evaluation.
    pub delivery: &'static [Access],
    /// Calls/operations with transitive accesses not described by this local plan.
    pub opaque: bool,
}
impl AccessPlan {
    pub const UNKNOWN: Self = Self {
        cold_await: None,
        transition: Transition::Unknown,
        accesses: &[],
        delivery: &[],
        opaque: true,
    };
    pub const SKIP: Self = Self {
        cold_await: None,
        transition: Transition::Skip,
        accesses: &[],
        delivery: &[],
        opaque: false,
    };
    pub fn with_cold_await(mut self, slot: usize, producer: &'static Control) -> Self {
        self.cold_await = Some(ColdAwait { slot, producer });
        self.opaque = false;
        self
    }
    /// Only a consuming move of one whole local cold handle is supported.
    fn checked_cold_await(self) -> R<Option<ColdAwait>> {
        if self.transition != Transition::AwaitTask {
            return if self.cold_await.is_none() {
                Ok(None)
            } else {
                Err("E_NATIVE: cold await descriptor on another transition".into())
            };
        }
        let descriptor = self
            .cold_await
            .ok_or("E_NATIVE: cold await has no exact handle descriptor")?;
        if self.opaque
            || self.accesses.len() != 1
            || self.accesses[0]
                != (Access {
                    slot: Some(descriptor.slot),
                    fields: &[],
                    indirect: false,
                    kind: AccessKind::Move,
                })
            || self.delivery.len() > 4096
            || self.delivery.iter().any(|a| {
                a.slot.is_none()
                    || a.indirect
                    || !a.fields.is_empty()
                    || a.kind != AccessKind::Write
            })
        {
            return Err("E_NATIVE: unsupported cold await access plan".into());
        }
        Ok(Some(descriptor))
    }
    fn delivered(self) -> Self {
        if self.transition == Transition::Unknown {
            return self;
        }
        Self {
            cold_await: None,
            transition: Transition::Delivery,
            accesses: self.delivery,
            delivery: &[],
            opaque: false,
        }
    }
}
pub struct Control {
    pub name: &'static str,
    pub slots: usize,
    pub atoms: usize,
    pub parameters: &'static [usize],
    pub retained: &'static [usize],
    pub constructor: bool,
    pub access: Option<fn(&Activation) -> AccessPlan>,
    pub resume: fn(&mut Host, &mut Frames, &mut Activation) -> R<Step>,
}
pub struct Activation {
    frame: Frame,
    owner: super::tasks::OwnerId,
    registry: std::sync::Arc<()>,
    control: &'static Control,
    pending: Option<Pending>,
    scopes: Vec<Scope>,
    pub facts: Vec<bool>,
    pub pc: usize,
    pub destination: Option<usize>,
}
enum Pending {
    #[cfg(test)]
    Adapter(Box<contexts::adapter_operation_model::Operation>),
    Call(Deliver),
    Await(AwaitContinuation),
    Supervise(Supervision),
    Host(HostOperation),
}
/// A started async host operation, delivered once when it completes. A scripted
/// completion was bound and validated at start; a live adapter is polled.
struct HostOperation {
    operation: &'static str,
    arguments: Vec<V>,
    /// A cancelled task stops the operation and selects `cancelled`.
    cancellable: bool,
    /// Stopped by cancellation: completes without publishing an event.
    cancelled: bool,
    source: HostSource,
    /// Readiness was signalled: the next turn polls once without fuel.
    poll: bool,
    deliver: HostDeliver,
}
enum HostSource {
    Scripted {
        outcome: String,
        fields: Vec<V>,
        waits: u32,
    },
    Live {
        operation: Box<dyn HostPoll>,
        due: Option<std::time::Instant>,
        /// Readiness token and descriptor registered with the host backend.
        registered: Option<(u64, Descriptor)>,
        readable: bool,
    },
    Ready(Outcome),
}
/// When a pending host operation may next be polled.
pub enum HostWait {
    Due,
    At(std::time::Instant),
    Readable {
        token: u64,
        at: Option<std::time::Instant>,
    },
}
fn deregister(host: &mut Host, registered: &mut Option<(u64, Descriptor)>) -> R<()> {
    if let Some((_, descriptor)) = registered.take() {
        host.readiness
            .as_mut()
            .ok_or("E_NATIVE: readiness registration without a backend")?
            .deregister(descriptor.fd)?;
    }
    Ok(())
}
impl HostOperation {
    fn start(
        host: &mut Host,
        operation: &'static str,
        arguments: Vec<V>,
        deliver: HostDeliver,
        cancellable: bool,
    ) -> R<Self> {
        if cancellable && host.task_cancelled() {
            // A cancelled operation does not start: no script entry or adapter.
            host.release_consumed(operation, &arguments)?;
            return Ok(Self {
                operation,
                arguments: vec![],
                cancellable,
                cancelled: true,
                poll: false,
                source: HostSource::Ready(("cancelled".into(), vec![])),
                deliver,
            });
        }
        let source = match host.start_host_operation(operation, &arguments)? {
            HostStart::Scripted {
                outcome,
                fields,
                waits: 0,
            } => HostSource::Ready((outcome, fields)),
            HostStart::Scripted {
                outcome,
                fields,
                waits,
            } => HostSource::Scripted {
                outcome,
                fields,
                waits,
            },
            HostStart::Live(operation) => HostSource::Live {
                operation,
                due: None,
                registered: None,
                readable: false,
            },
        };
        let mut started = Self {
            operation,
            arguments,
            cancellable,
            cancelled: false,
            // A live operation is polled once while its start is selected; a
            // scripted one reports its first pending wait now.
            poll: matches!(source, HostSource::Live { .. }),
            source,
            deliver,
        };
        started.progress(host)?;
        Ok(started)
    }
    /// Consume one readiness report: a scripted wait or one live adapter poll.
    fn progress(&mut self, host: &mut Host) -> R<()> {
        if !std::mem::take(&mut self.poll) {
            return Ok(());
        }
        if self.cancellable && host.task_cancelled() {
            // The pending operation stops before its state and supplier resources.
            if let HostSource::Live { registered, .. } = &mut self.source {
                deregister(host, registered)?;
            }
            host.release_consumed(self.operation, &self.arguments)?;
            self.arguments.clear();
            self.cancelled = true;
            self.source = HostSource::Ready(("cancelled".into(), vec![]));
            return Ok(());
        }
        match &mut self.source {
            HostSource::Scripted {
                outcome,
                fields,
                waits,
            } => {
                *waits -= 1;
                if *waits == 0 {
                    self.source =
                        HostSource::Ready((std::mem::take(outcome), std::mem::take(fields)));
                }
            }
            HostSource::Live {
                operation,
                due,
                registered,
                readable,
            } => {
                *readable = false;
                match host.poll_host_operation(self.operation, operation)? {
                    HostProgress::Ready(outcome) => {
                        deregister(host, registered)?;
                        // Adapter state is destroyed before the result is delivered.
                        self.source = HostSource::Ready(outcome);
                    }
                    HostProgress::Pending {
                        delay,
                        descriptor: fd,
                    } => {
                        if delay.is_none() && fd.is_none() {
                            return Err(format!(
                                "E_HOST: pending `{}` requested neither a delay nor readiness",
                                self.operation
                            ));
                        }
                        *due = delay.map(|delay| std::time::Instant::now() + delay);
                        if registered.is_some_and(|(_, old)| Some(old) != fd) {
                            deregister(host, registered)?;
                        }
                        if let (None, Some(fd)) = (*registered, fd) {
                            host.next_readiness_token += 1;
                            let token = host.next_readiness_token;
                            host.readiness
                                .as_mut()
                                .ok_or("E_HOST: readiness requires a polled Rust adapter backend")?
                                .register(token, fd)?;
                            *registered = Some((token, fd));
                        }
                    }
                }
            }
            HostSource::Ready(_) => {}
        }
        Ok(())
    }
    fn waiting(&self) -> Option<HostWait> {
        match &self.source {
            _ if self.poll => None,
            HostSource::Scripted { .. } => Some(HostWait::Due),
            HostSource::Live { readable: true, .. } => Some(HostWait::Due),
            HostSource::Live {
                due,
                registered: Some((token, _)),
                ..
            } => Some(HostWait::Readable {
                token: *token,
                at: *due,
            }),
            HostSource::Live { due, .. } => Some(due.map_or(HostWait::Due, HostWait::At)),
            HostSource::Ready(_) => None,
        }
    }
    /// A stopped operation releases its readiness registration before its state
    /// and any supplier resource; it publishes nothing.
    fn stop(mut self, host: &mut Host) -> R<()> {
        if let HostSource::Live { registered, .. } = &mut self.source {
            deregister(host, registered)?;
        }
        Ok(())
    }
    /// Publish the completion event with the arguments captured at start, then
    /// bind the outcome. Neither consumes instruction fuel.
    fn complete(self, host: &mut Host, values: &mut Frames, frame: &mut Activation) -> R<()> {
        let HostSource::Ready((outcome, fields)) = self.source else {
            return Err("E_NATIVE: host operation has no completion".into());
        };
        if !self.cancelled {
            host.finish_host_operation(self.operation, self.arguments, &outcome, &fields)?;
        }
        (self.deliver)(frame, values, outcome, fields)
    }
}
struct AwaitContinuation {
    // Query provenance stays bound to the invocation/domain where captures left
    // the frame, including after the completed child's membership is retired.
    owner: super::tasks::OwnerId,
    domain: DomainKey,
    queued: Option<QueuedTask>,
    child: Option<ContextKey>,
    result: Option<R<Completion>>,
    deliver: Deliver,
    access: AccessPlan,
    dependencies: Dependencies,
}
impl AwaitContinuation {
    fn retire(&mut self, host: &mut Host) -> R<()> {
        if let Some(task) = self.queued.take() {
            task.retire(host)?;
        }
        if let Some(Ok(Completion::Outcome((_, fields)))) = self.result.take() {
            task_payload::retire_tasks(host, &fields, true)?;
            host.release_frame(&fields.into_iter().map(Some).collect::<Vec<_>>());
        }
        Ok(())
    }
}
struct Supervision {
    state: BoundaryState,
    handler: &'static Control,
    domain: DomainScope,
}
impl Supervision {
    fn finish(
        &mut self,
        host: &mut Host,
        domains: &mut Domains,
        current: &mut DomainKey,
        result: &R<()>,
    ) -> R<()> {
        domains.require_quiescent(&self.domain, current)?;
        host.finish_boundary(&mut self.state, result)?;
        domains.close(&self.domain, current)?;
        *current = self.domain.parent.clone().unwrap();
        Ok(())
    }
}
pub struct LoopControl {
    pub target: usize,
    pub body: usize,
    pub after: usize,
    pub locals: &'static [usize],
    pub reset: fn(&mut Activation, &mut Frames, Vec<V>) -> R<()>,
}
pub struct BoundaryControl {
    pub name: &'static str,
    pub body: usize,
    pub handler: usize,
    pub after: usize,
    pub retained: &'static [usize],
    pub retire: &'static [usize],
    pub work: &'static [usize],
    pub handling: &'static [usize],
    pub fault_slot: usize,
}
struct BoundaryScope {
    control: &'static BoundaryControl,
    state: BoundaryState,
    domain: DomainScope,
}
enum Scope {
    Loop(&'static LoopControl),
    Boundary(BoundaryScope),
}
impl BoundaryScope {
    fn finish(
        &mut self,
        host: &mut Host,
        values: &mut Frames,
        domains: &mut Domains,
        current: &mut DomainKey,
        base: usize,
        result: &R<()>,
        abrupt: bool,
    ) -> R<()> {
        domains.require_quiescent(&self.domain, current)?;
        task_payload::retire_tasks(
            host,
            self.control
                .work
                .iter()
                .chain(self.control.handling)
                .filter_map(|slot| values[base + slot].as_ref()),
            abrupt,
        )?;
        for slot in self.control.work.iter().chain(self.control.handling) {
            values[base + slot] = None;
        }
        host.finish_boundary(&mut self.state, result)?;
        domains.close(&self.domain, current)?;
        *current = self.domain.parent.clone().unwrap();
        Ok(())
    }
    fn handle(
        &mut self,
        host: &mut Host,
        values: &mut Frames,
        domains: &Domains,
        current: &DomainKey,
        base: usize,
        message: &str,
    ) -> R<()> {
        domains.require_quiescent(&self.domain, current)?;
        let keep = host.resources.owned_tokens(
            self.control
                .retained
                .iter()
                .filter_map(|id| values[base + id].as_ref()),
        )?;
        task_payload::retire_tasks(
            host,
            self.control
                .retire
                .iter()
                .filter_map(|slot| values[base + slot].as_ref()),
            true,
        )?;
        for slot in self.control.retire {
            values[base + slot] = None;
        }
        let report = host.boundary_fault(&mut self.state, message, &keep)?;
        let (message, stack, paths) = report.source_fields();
        let strings = |xs: Vec<String>| V::Construct {
            ty: "vector<String>".into(),
            name: "vector".into(),
            fields: xs.into_iter().map(V::String).collect(),
        };
        values[base + self.control.fault_slot] = Some(V::Construct {
            ty: "fault_report".into(),
            name: "fault_report".into(),
            fields: vec![V::String(message), strings(stack), strings(paths)],
        });
        host.enter_contingency(&mut self.state)
    }
}
impl Activation {
    pub fn base(&self) -> usize {
        self.frame.base()
    }
    fn close_scopes(
        &mut self,
        host: &mut Host,
        values: &mut Frames,
        domains: &mut Domains,
        current: &mut DomainKey,
        remaining: usize,
        abrupt: bool,
    ) -> R<()> {
        let base = self.base();
        while self.scopes.len() > remaining {
            if let Some(Scope::Boundary(boundary)) = self.scopes.last_mut() {
                boundary.finish(host, values, domains, current, base, &Ok(()), abrupt)?;
            }
            self.scopes.pop();
        }
        Ok(())
    }
    fn loop_target(&self, target: usize) -> R<(usize, &'static LoopControl)> {
        self.scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, scope)| match scope {
                Scope::Loop(control) if control.target == target => Some((index, *control)),
                _ => None,
            })
            .ok_or("E_NATIVE: compiled transfer has no active loop".into())
    }
}
pub enum Step {
    #[cfg(test)]
    Adapter {
        arguments: Vec<V>,
        deliver: Deliver,
    },
    Continue,
    Host {
        operation: &'static str,
        arguments: Vec<V>,
        deliver: HostDeliver,
        /// Entered through a cancellable generated control.
        cancellable: bool,
    },
    /// A cancellation request was recorded; the driver wakes observing operations.
    Cancel,
    Spawn {
        task: QueuedTask,
        slot: usize,
        ty: &'static str,
    },
    Join {
        value: V,
        producer: &'static Control,
        deliver: Deliver,
    },
    /// Queue a child and add its handle to the moved task set.
    SpawnInto {
        task: QueuedTask,
        set: V,
        slot: usize,
        ty: &'static str,
    },
    /// Join the earliest completed member of a nonempty moved task set.
    Next {
        set: V,
        producer: &'static Control,
        deliver: Deliver,
    },
    Yield,
    Await {
        task: QueuedTask,
        deliver: Deliver,
    },
    Complete(Completion),
    Boundary(&'static BoundaryControl),
    BoundaryEnd(&'static BoundaryControl),
    Supervise {
        work: &'static Control,
        handler: &'static Control,
        arguments: Vec<V>,
    },
    Loop {
        control: &'static LoopControl,
        arguments: Vec<V>,
    },
    Call {
        control: &'static Control,
        arguments: Vec<V>,
        destination: Option<usize>,
        deliver: Deliver,
    },
}
// Preflight rejection is kept separate from an admitted source failure, so the
// context layer can restore selection without fault settlement on rejection.
enum OneCallbackPoll {
    Rejected(String),
    Advanced {
        result: R<Poll>,
        instruction: bool,
        faulted: bool,
    },
}
pub enum Poll {
    Paused,
    Await,
    Complete(Completion),
    Settling(DomainKey),
}
enum SourceHandoff {
    Spawn {
        task: QueuedTask,
        slot: usize,
        ty: &'static str,
    },
    Join {
        value: V,
        producer: &'static Control,
        deliver: Deliver,
    },
    SpawnInto {
        task: QueuedTask,
        set: V,
        slot: usize,
        ty: &'static str,
    },
    Next {
        set: V,
        producer: &'static Control,
        deliver: Deliver,
    },
    Yield,
    Cancel,
}
pub struct Machine {
    source_checked: bool,
    source_handoff: Option<SourceHandoff>,
    stack: Vec<Activation>,
    finished: bool,
    root: DomainScope,
    current: DomainKey,
    member: Membership,
    owns_root: bool,
    retain_completion: bool,
    fault: Option<String>,
    handover: Vec<V>,
    fault_target: Option<DomainKey>,
}
impl Machine {
    /// Symbolic local accesses only. This never grants selection permission or
    /// resolves captured loans, and does not evaluate source values or consume fuel.
    pub fn next_access(&self) -> AccessPlan {
        let Some(frame) = self.stack.last() else {
            return AccessPlan::UNKNOWN;
        };
        match &frame.pending {
            Some(Pending::Await(p)) if matches!(&p.result, Some(Ok(Completion::Outcome(_)))) => {
                p.access.delivered()
            }
            Some(_) => AccessPlan::UNKNOWN,
            None => frame
                .control
                .access
                .map_or(AccessPlan::UNKNOWN, |access| access(frame)),
        }
    }
    fn admit(
        host: &mut Host,
        values: &mut Frames,
        control: &'static Control,
        args: Vec<V>,
        destination: Option<usize>,
    ) -> R<Activation> {
        if args.len() != control.parameters.len() || control.constructor != destination.is_some() {
            return Err("E_NATIVE: invalid compiled activation admission".into());
        }
        let owner = host.tasks.prepare_owner()?;
        let frame = values.enter(control.slots)?;
        if let Err(error) = host.enter(control.name) {
            values.retire(&frame)?;
            return Err(error);
        }
        host.tasks.commit_owner(owner);
        host.record_owner(owner);
        for (slot, value) in control.parameters.iter().zip(args) {
            values[frame.base() + slot] = Some(value);
        }
        Ok(Activation {
            frame,
            owner,
            registry: host.resources.identity.clone(),
            control,
            pending: None,
            scopes: vec![],
            facts: vec![false; control.atoms],
            pc: 0,
            destination,
        })
    }
    pub fn start(
        host: &mut Host,
        values: &mut Frames,
        domains: &mut Domains,
        control: &'static Control,
        args: Vec<V>,
    ) -> R<Self> {
        let root = domains.prepare(None)?;
        domains.require_member_capacity()?;
        let activation = Self::admit(host, values, control, args, None)?;
        domains.commit(&root)?;
        let member = domains.attach(&root.key, None)?;
        Ok(Self {
            stack: vec![activation],
            finished: false,
            current: root.key.clone(),
            root,
            member,
            owns_root: true,
            retain_completion: false,
            fault: None,
            handover: vec![],
            fault_target: None,
            source_checked: false,
            source_handoff: None,
        })
    }
    fn start_related(
        host: &mut Host,
        values: &mut Frames,
        domains: &mut Domains,
        control: &'static Control,
        args: Vec<V>,
        owner: &DomainKey,
        parent: &Membership,
    ) -> R<Self> {
        domains.require_open(owner, &[])?;
        domains.require_member_capacity()?;
        let root = domains.scope(owner)?;
        let activation = Self::admit(host, values, control, args, None)?;
        let member = domains.attach(owner, Some(parent))?;
        Ok(Self {
            stack: vec![activation],
            finished: false,
            current: owner.clone(),
            root,
            member,
            owns_root: false,
            retain_completion: false,
            fault: None,
            handover: vec![],
            fault_target: None,
            source_checked: false,
            source_handoff: None,
        })
    }
    fn require_domain(&self, domains: &Domains) -> R<()> {
        if self.finished {
            return Err("E_NATIVE: compiled execution already retired".into());
        }
        domains.require_member(&self.root.key, &self.current)?;
        domains.require_membership(&self.member, &self.root.key)?;
        let expected = self
            .stack
            .iter()
            .rev()
            .find_map(|frame| {
                if let Some(Pending::Supervise(supervision)) = &frame.pending {
                    return Some(&supervision.domain);
                }
                frame.scopes.iter().rev().find_map(|scope| match scope {
                    Scope::Boundary(boundary) => Some(&boundary.domain),
                    Scope::Loop(_) => None,
                })
            })
            .unwrap_or(&self.root);
        domains.require_selected(expected, &self.current)
    }
    fn require_finish_root(&self, domains: &Domains) -> R<()> {
        if self.owns_root {
            domains.require_finish_root(&self.root, &self.current, &self.member)
        } else {
            domains.require_selected(&self.root, &self.current)?;
            domains.require_membership(&self.member, &self.root.key)?;
            if domains.has_children(&self.member)? {
                return Err("E_NATIVE: completed context has retained children".into());
            }
            Ok(())
        }
    }
    fn finish_root(&mut self, domains: &mut Domains) -> R<()> {
        if self.owns_root {
            domains.finish_root(&self.root, &self.current, &self.member)?;
        } else {
            domains.require_selected(&self.root, &self.current)?;
            domains.detach(&self.member)?;
        }
        self.finished = true;
        Ok(())
    }
    fn require_runnable(&self, domains: &Domains) -> R<()> {
        self.require_domain(domains)?;
        let mut handling = vec![];
        for frame in &self.stack {
            if let Some(Pending::Supervise(supervision)) = &frame.pending {
                if supervision.state.contingency_active {
                    handling.push(&supervision.domain.key);
                }
            }
            for scope in &frame.scopes {
                if let Scope::Boundary(boundary) = scope {
                    if boundary.state.contingency_active {
                        handling.push(&boundary.domain.key);
                    }
                }
            }
        }
        domains.require_open(&self.current, &handling)
    }
    fn owns_domain(&self, domains: &Domains, key: &DomainKey) -> bool {
        (self.owns_root && domains.same(&self.root.key, key)) || self.stack.iter().any(|frame| {
            matches!(&frame.pending, Some(Pending::Supervise(supervision)) if domains.same(&supervision.domain.key, key))
                || frame.scopes.iter().any(|scope| matches!(scope, Scope::Boundary(boundary) if domains.same(&boundary.domain.key, key)))
        })
    }
    fn step(
        &mut self,
        host: &mut Host,
        values: &mut Frames,
        domains: &mut Domains,
    ) -> R<(bool, Option<Completion>)> {
        let access = self.next_access();
        let frame = self
            .stack
            .last_mut()
            .ok_or("E_NATIVE: missing compiled activation")?;
        #[cfg(test)]
        if let Some(Pending::Adapter(operation)) = &frame.pending {
            operation.require(host, values, frame, domains, &self.current)?;
            if !operation.ready() {
                return Ok((false, None));
            }
            let Some(Pending::Adapter(operation)) = frame.pending.take() else {
                unreachable!()
            };
            operation.deliver(values, frame)?;
            return Ok((false, None));
        }
        // A pending host operation polls and completes administratively.
        if let Some(Pending::Host(operation)) = &mut frame.pending {
            operation.progress(host)?;
            if !matches!(operation.source, HostSource::Ready(_)) {
                return Ok((false, None));
            }
            let Some(Pending::Host(operation)) = frame.pending.take() else {
                unreachable!()
            };
            operation.complete(host, values, frame)?;
            return Ok((false, None));
        }
        // Returning a child is administrative: it neither evaluates the source
        // await again nor consumes another instruction/fuel allowance.
        let (instruction, step) = if matches!(&frame.pending, Some(Pending::Await(p)) if p.result.is_some())
        {
            let Some(Pending::Await(mut pending)) = frame.pending.take() else {
                unreachable!()
            };
            let result = pending.result.take().unwrap();
            pending.retire(host)?;
            match result? {
                outcome @ Completion::Outcome(_) => {
                    (pending.deliver)(frame, values, outcome)?;
                    return Ok((false, None));
                }
                completion => (false, Step::Complete(completion)),
            }
        } else {
            (true, (frame.control.resume)(host, values, frame)?)
        };
        // Context membership includes unbound host children and completed results,
        // which invocation task obligations alone cannot see. Check before any
        // runtime transition clears scopes, resets loop storage or retires frames.
        // This is deliberately conservative across synchronous frames. It does not
        // authorize running the callback above with live children: evaluating its
        // operands can already move values. Ordinary advancement keeps its child
        // gate; explicit one-transition advancement requires fresh sealed admission.
        if matches!(
            &step,
            Step::Loop { .. }
                | Step::BoundaryEnd(_)
                | Step::Complete(
                    Completion::Outcome(_)
                        | Completion::Constructed
                        | Completion::Again(..)
                        | Completion::Recover(..)
                        | Completion::Complete(_)
                )
        ) {
            if !self.source_checked {
                domains.require_no_children(&self.member)?;
            }
        }
        match step {
            #[cfg(test)]
            Step::Adapter { arguments, deliver } => {
                if frame.pending.is_some() {
                    return Err("E_NATIVE: duplicate adapter continuation".into());
                }
                frame.pending = Some(Pending::Adapter(Box::new(
                    contexts::adapter_operation_model::Operation::new(
                        host,
                        values,
                        frame,
                        self.current.clone(),
                        arguments,
                        deliver,
                    )?,
                )));
            }
            Step::Continue => {}
            Step::Host {
                operation,
                arguments,
                deliver,
                cancellable,
            } => {
                if frame.pending.is_some() {
                    return Err("E_NATIVE: duplicate host operation continuation".into());
                }
                let operation =
                    HostOperation::start(host, operation, arguments, deliver, cancellable)?;
                if matches!(operation.source, HostSource::Ready(_)) {
                    operation.complete(host, values, frame)?;
                } else {
                    frame.pending = Some(Pending::Host(operation));
                }
            }
            Step::Spawn { task, slot, ty } => {
                self.source_handoff = Some(SourceHandoff::Spawn { task, slot, ty });
            }
            Step::Join {
                value,
                producer,
                deliver,
            } => {
                self.source_handoff = Some(SourceHandoff::Join {
                    value,
                    producer,
                    deliver,
                });
            }
            Step::SpawnInto {
                task,
                set,
                slot,
                ty,
            } => {
                self.source_handoff = Some(SourceHandoff::SpawnInto {
                    task,
                    set,
                    slot,
                    ty,
                });
            }
            Step::Next {
                set,
                producer,
                deliver,
            } => {
                self.source_handoff = Some(SourceHandoff::Next {
                    set,
                    producer,
                    deliver,
                });
            }
            Step::Yield => self.source_handoff = Some(SourceHandoff::Yield),
            Step::Cancel => self.source_handoff = Some(SourceHandoff::Cancel),
            Step::Await { task, deliver } => {
                if frame.pending.is_some() {
                    return Err("E_NATIVE: duplicate compiled await continuation".into());
                }
                let dependencies = task.dependencies(values);
                frame.pending = Some(Pending::Await(AwaitContinuation {
                    owner: frame.owner,
                    domain: self.current.clone(),
                    queued: Some(task),
                    child: None,
                    result: None,
                    deliver,
                    access,
                    dependencies,
                }));
            }
            Step::Boundary(control) => {
                let base = frame.base();
                let domain = domains.prepare(Some(&self.current))?;
                let state = host.begin_boundary(
                    control.name,
                    control
                        .retained
                        .iter()
                        .chain(control.retire)
                        .filter_map(|id| values[base + id].as_ref()),
                    true,
                )?;
                domains.commit(&domain)?;
                self.current = domain.key.clone();
                frame.scopes.push(Scope::Boundary(BoundaryScope {
                    control,
                    state,
                    domain,
                }));
                frame.pc = control.body;
            }
            Step::BoundaryEnd(control) => {
                let base = frame.base();
                let Some(Scope::Boundary(boundary)) = frame.scopes.last_mut() else {
                    return Err("E_NATIVE: missing compiled boundary continuation".into());
                };
                if !std::ptr::eq(boundary.control, control) {
                    return Err("E_NATIVE: mismatched compiled boundary continuation".into());
                }
                boundary.finish(
                    host,
                    values,
                    domains,
                    &mut self.current,
                    base,
                    &Ok(()),
                    false,
                )?;
                frame.scopes.pop();
                frame.pc = control.after;
                // This sentinel is administrative and consumes no instruction quota.
                return Ok((false, None));
            }
            Step::Loop { control, arguments } => {
                if frame.loop_target(control.target).is_ok() {
                    return Err("E_NATIVE: compiled loop already active".into());
                }
                task_payload::retire_tasks(
                    host,
                    control
                        .locals
                        .iter()
                        .filter_map(|slot| values[frame.base() + slot].as_ref()),
                    false,
                )?;
                (control.reset)(frame, values, arguments)?;
                frame.scopes.push(Scope::Loop(control));
                frame.pc = control.body;
            }
            Step::Supervise {
                work,
                handler,
                arguments,
            } => {
                if frame.pending.is_some() {
                    return Err("E_NATIVE: duplicate compiled supervision continuation".into());
                }
                let domain = domains.prepare(Some(&self.current))?;
                let state = host.begin_boundary(work.name, &arguments, false)?;
                domains.commit(&domain)?;
                self.current = domain.key.clone();
                frame.pending = Some(Pending::Supervise(Supervision {
                    state,
                    handler,
                    domain,
                }));
                self.stack
                    .push(Self::admit(host, values, work, arguments, None)?);
            }
            Step::Complete(Completion::Again(target, arguments)) => {
                let (index, control) = frame.loop_target(target)?;
                frame.close_scopes(host, values, domains, &mut self.current, index + 1, false)?;
                task_payload::retire_tasks(
                    host,
                    control
                        .locals
                        .iter()
                        .filter_map(|slot| values[frame.base() + slot].as_ref()),
                    false,
                )?;
                (control.reset)(frame, values, arguments)?;
                frame.pc = control.body;
            }
            Step::Complete(Completion::Recover(target, arguments)) => {
                let (index, control) = frame.loop_target(target)?;
                frame.close_scopes(host, values, domains, &mut self.current, index + 1, true)?;
                task_payload::retire_tasks(
                    host,
                    control
                        .locals
                        .iter()
                        .filter_map(|slot| values[frame.base() + slot].as_ref()),
                    true,
                )?;
                for slot in control.locals {
                    host.release_frame(std::slice::from_ref(&values[frame.base() + slot]));
                }
                (control.reset)(frame, values, arguments)?;
                frame.pc = control.body;
            }
            Step::Complete(Completion::Complete(target)) => {
                let (index, control) = frame.loop_target(target)?;
                frame.close_scopes(host, values, domains, &mut self.current, index, false)?;
                frame.pc = control.after;
            }
            Step::Call {
                control,
                arguments,
                destination,
                deliver,
            } => {
                if frame.pending.replace(Pending::Call(deliver)).is_some() {
                    return Err("E_NATIVE: duplicate compiled return continuation".into());
                }
                self.stack
                    .push(Self::admit(host, values, control, arguments, destination)?);
            }
            Step::Complete(result) => {
                let abrupt = !matches!(&result, Completion::Outcome(_) | Completion::Constructed);
                // Reject outstanding obligations while the original frame and
                // boundary state remain available for emergency retirement.
                host.tasks.require_retire_owner(frame.owner, abrupt)?;
                frame.close_scopes(host, values, domains, &mut self.current, 0, abrupt)?;
                let owner = frame.owner;
                // Root/domain authority must also be checked before releasing the
                // final activation, including when its result will retain membership.
                if self.stack.len() == 1 {
                    self.require_finish_root(domains)?;
                }
                host.tasks.retire_owner(owner, abrupt)?;
                let completed = self.stack.pop().unwrap();
                values.retire(&completed.frame)?;
                host.leave();
                if let Some(parent) = self.stack.last_mut() {
                    let pending = parent
                        .pending
                        .take()
                        .ok_or("E_NATIVE: missing compiled return continuation")?;
                    match pending {
                        #[cfg(test)]
                        Pending::Adapter(_) => {
                            return Err(
                                "E_NATIVE: call returned into an adapter continuation".into()
                            );
                        }
                        Pending::Call(deliver) => deliver(parent, values, result)?,
                        Pending::Host(_) => {
                            return Err("E_NATIVE: call returned into a host operation".into());
                        }
                        Pending::Await(_) => {
                            return Err("E_NATIVE: call returned into an await continuation".into());
                        }
                        Pending::Supervise(mut supervision) => {
                            let result = result.settled();
                            supervision.finish(host, domains, &mut self.current, &result)?;
                            result?;
                        }
                    }
                } else {
                    if self.retain_completion && matches!(result, Completion::Outcome(_)) {
                        // Frames can retire while result ownership keeps membership live.
                        self.finished = true;
                    } else {
                        self.finish_root(domains)?;
                    }
                    return Ok((instruction, Some(result)));
                }
            }
        }
        Ok((instruction, None))
    }
    fn unwind_error(
        &mut self,
        host: &mut Host,
        values: &mut Frames,
        domains: &mut Domains,
        mut message: String,
        handle: bool,
    ) -> R<()> {
        let mut retained = std::mem::take(&mut self.handover);
        while !self.stack.is_empty() {
            if handle && domains.has_others(&self.current, &self.member)? {
                self.fault = Some(message);
                self.handover = retained;
                return Ok(());
            }
            let frame = self.stack.last_mut().unwrap();
            #[cfg(test)]
            if let Some(Pending::Adapter(operation)) = &frame.pending {
                operation.require(host, values, frame, domains, &self.current)?;
                // Drop the partial host operation before any supplier scope or frame.
                drop(frame.pending.take());
            }
            if matches!(&frame.pending, Some(Pending::Host(_))) {
                // A stopped host operation publishes nothing. Its borrowed
                // resource arguments are supplier handles, never released here.
                let Some(Pending::Host(operation)) = frame.pending.take() else {
                    unreachable!()
                };
                operation.stop(host)?;
            }
            if let Some(Pending::Await(pending)) = &mut frame.pending {
                // Captures may still be queued after failed admission; returned
                // fields may already be adopted but not yet delivered.
                pending.retire(host)?;
            }
            // A named work frame has already handed over retained parameters and
            // retired. Its caller still owns the original supervision record.
            if let Some(Pending::Supervise(mut supervision)) = frame.pending.take() {
                if handle
                    && self
                        .fault_target
                        .as_ref()
                        .is_none_or(|target| domains.same(target, &supervision.domain.key))
                    && host.source_faulted()
                    && supervision.state.fault_message.is_none()
                {
                    let handler = (|| {
                        domains.require_quiescent(&supervision.domain, &self.current)?;
                        let keep = host.resources.owned_tokens(&retained)?;
                        host.boundary_fault(&mut supervision.state, &message, &keep)?;
                        retained.push(V::String(
                            message.strip_prefix("E_FAULT: ").unwrap_or(&message).into(),
                        ));
                        host.enter_contingency(&mut supervision.state)?;
                        Self::admit(
                            host,
                            values,
                            supervision.handler,
                            std::mem::take(&mut retained),
                            None,
                        )
                    })();
                    match handler {
                        Ok(handler) => {
                            frame.pending = Some(Pending::Supervise(supervision));
                            self.stack.push(handler);
                            self.fault_target = None;
                            return Ok(());
                        }
                        Err(error) => message = error,
                    }
                }
                supervision.finish(host, domains, &mut self.current, &Err(message.clone()))?;
            }
            retained.clear();
            // A child fault retires its original frame before handover in its caller.
            while !frame.scopes.is_empty() {
                if handle && domains.has_others(&self.current, &self.member)? {
                    self.fault = Some(message);
                    self.handover = retained;
                    return Ok(());
                }
                let scope = frame.scopes.pop().unwrap();
                if let Scope::Boundary(mut boundary) = scope {
                    if !handle && boundary.state.fault_message.is_none() {
                        // Emergency sibling retirement applies the nested fault
                        // disposition without entering that boundary's handler.
                        boundary.state.fault_message = Some(message.clone());
                    }
                    if handle
                        && self
                            .fault_target
                            .as_ref()
                            .is_none_or(|target| domains.same(target, &boundary.domain.key))
                        && host.source_faulted()
                        && boundary.state.fault_message.is_none()
                    {
                        match boundary.handle(
                            host,
                            values,
                            domains,
                            &self.current,
                            frame.base(),
                            &message,
                        ) {
                            Ok(()) => {
                                frame.pending = None;
                                frame.pc = boundary.control.handler;
                                frame.scopes.push(Scope::Boundary(boundary));
                                self.fault_target = None;
                                return Ok(());
                            }
                            Err(error) => message = error,
                        }
                    }
                    boundary.finish(
                        host,
                        values,
                        domains,
                        &mut self.current,
                        frame.base(),
                        &Err(message.clone()),
                        true,
                    )?;
                }
            }
            let frame = self.stack.pop().unwrap();
            if handle && host.source_faulted() {
                for slot in frame.control.retained {
                    retained.push(
                        values[frame.base() + slot]
                            .take()
                            .ok_or("E_NATIVE: retained boundary parameter unavailable")?,
                    );
                }
            }
            host.release_frame(values.frame_values(&frame.frame)?);
            host.tasks.retire_owner(frame.owner, true)?;
            values.retire(&frame.frame)?;
            // Failed activation diagnostics remain available until handover/reporting.
        }
        self.finish_root(domains)?;
        Err(message)
    }
    fn abort(
        &mut self,
        host: &mut Host,
        values: &mut Frames,
        domains: &mut Domains,
        message: &str,
    ) -> R<()> {
        self.require_domain(domains)?;
        self.fault = None;
        host.activations.source_fault();
        let result = self.unwind_error(host, values, domains, message.into(), false);
        if self.finished { Ok(()) } else { result }
    }
    fn advance_admitted_one(
        &mut self,
        host: &mut Host,
        values: &mut Frames,
        domains: &mut Domains,
        admission: contexts::OneCallback,
    ) -> OneCallbackPoll {
        if let Err(error) = admission.check(self, host, values, domains) {
            return OneCallbackPoll::Rejected(error);
        }
        // Exactly one transition, including instruction-free pending delivery.
        // Never feed this witness into the quota loop or a subsequent callback.
        let delivery = matches!(&self.stack.last().unwrap().pending,
            Some(Pending::Await(p)) if p.result.is_some());
        let (result, instruction, faulted) = match self.step(host, values, domains) {
            Ok((instruction, Some(result))) => (Ok(Poll::Complete(result)), instruction, false),
            Ok((instruction, None)) => {
                let awaiting = matches!(&self.stack.last().unwrap().pending,
                    Some(Pending::Await(p)) if p.queued.is_some() && p.result.is_none());
                (
                    Ok(if awaiting { Poll::Await } else { Poll::Paused }),
                    instruction,
                    false,
                )
            }
            Err(error) => (
                self.unwind_error(host, values, domains, error, true)
                    .map(|()| {
                        if self.fault.is_some() {
                            Poll::Settling(self.current.clone())
                        } else {
                            Poll::Paused
                        }
                    }),
                !delivery,
                true,
            ),
        };
        OneCallbackPoll::Advanced {
            result,
            instruction,
            faulted,
        }
    }
    /// The pending host operation was entered through a cancellable control.
    pub fn host_cancellable(&self) -> bool {
        matches!(
            self.stack.last().and_then(|a| a.pending.as_ref()),
            Some(Pending::Host(operation)) if operation.cancellable
        )
    }
    /// The selected activation awaits readiness of a started host operation.
    pub fn host_waiting(&self) -> Option<HostWait> {
        match self.stack.last().and_then(|a| a.pending.as_ref()) {
            Some(Pending::Host(operation)) => operation.waiting(),
            _ => None,
        }
    }
    /// Descriptor readiness reported for these tokens makes a live operation due.
    pub fn host_readable(&mut self, tokens: &[u64]) {
        if let Some(Pending::Host(HostOperation {
            source:
                HostSource::Live {
                    registered: Some((token, _)),
                    readable,
                    ..
                },
            ..
        })) = self.stack.last_mut().and_then(|a| a.pending.as_mut())
        {
            *readable |= tokens.contains(token);
        }
    }
    /// Readiness for the pending host operation: its next turn polls once.
    /// `elapse` completes scripted waits when no other source work can observe them.
    pub fn host_ready(&mut self, elapse: bool) {
        if let Some(Pending::Host(operation)) =
            self.stack.last_mut().and_then(|a| a.pending.as_mut())
        {
            if let (true, HostSource::Scripted { waits, .. }) = (elapse, &mut operation.source) {
                *waits = 1;
            }
            operation.poll = true;
        }
    }
    pub fn advance(
        &mut self,
        host: &mut Host,
        values: &mut Frames,
        domains: &mut Domains,
        quota: usize,
    ) -> R<Poll> {
        self.require_runnable(domains)?;
        if self.fault.is_some() {
            return Ok(Poll::Settling(self.current.clone()));
        }
        // Selection rejection is administrative, not an execution failure. Keep it
        // outside the step/error-unwind loop and before pending delivery, callback
        // operand moves, path facts, effects or fuel. Fine-grained parent progress
        // requires checked access metadata; context membership stays conservative.
        if !self.source_checked {
            domains.require_no_children(&self.member)?;
        }
        let mut remaining = quota;
        while remaining > 0 {
            #[cfg(test)]
            if matches!(&self.stack.last().unwrap().pending,
                Some(Pending::Adapter(operation)) if !operation.ready())
            {
                return Ok(Poll::Paused);
            }
            if self.host_waiting().is_some() {
                return Ok(Poll::Paused);
            }
            let delivery =
                if let Some(Pending::Await(pending)) = &self.stack.last().unwrap().pending {
                    if pending.result.is_none() {
                        return if pending.queued.is_some() {
                            Ok(Poll::Await)
                        } else {
                            Err("E_NATIVE: await completion is not available".into())
                        };
                    }
                    true
                } else {
                    false
                };
            match self.step(host, values, domains) {
                Ok((_, Some(result))) => return Ok(Poll::Complete(result)),
                Ok((instruction, None)) => remaining -= usize::from(instruction),
                Err(error) => {
                    self.unwind_error(host, values, domains, error, true)?;
                    if self.fault.is_some() {
                        return Ok(Poll::Settling(self.current.clone()));
                    }
                    remaining -= usize::from(!delivery);
                }
            }
            if self.source_handoff.is_some()
                || matches!(&self.stack.last().unwrap().pending, Some(Pending::Await(p)) if p.queued.is_some() && p.result.is_none())
            {
                return Ok(Poll::Await);
            }
        }
        Ok(Poll::Paused)
    }
}

#[cfg(test)]
#[path = "contexts.rs"]
mod contexts;
#[cfg(test)]
#[path = "domains.rs"]
mod domains;
pub use contexts::{AdmissionProgress, AdmissionStop};
#[allow(unused_imports)]
pub use contexts::{
    ContextKey, ContextPoll, Contexts, Readiness, StartedTask, TaskJoin, run_entry,
    run_source_entry,
};

#[cfg(test)]
#[path = "invocation_tests.rs"]
mod invocation_tests;

#[cfg(test)]
#[path = "task_payload.rs"]
mod task_payload;
pub use task_payload::{QueuedTask, TaskBinding};

#[cfg(test)]
#[path = "task_retirement_tests.rs"]
mod task_retirement_tests;
