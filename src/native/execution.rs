//! Shared retained compiled continuations; no checked graph is interpreted here.
use super::{BoundaryState, Completion, Frames, Host, R, V, frames::Frame};
use domains::{DomainKey, DomainScope, Domains, Membership};

pub type Deliver = fn(&mut Activation, &mut Frames, Completion) -> R<()>;
pub struct Control {
    pub name: &'static str,
    pub slots: usize,
    pub atoms: usize,
    pub parameters: &'static [usize],
    pub retained: &'static [usize],
    pub constructor: bool,
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
    Call(Deliver),
    Await(AwaitContinuation),
    Supervise(Supervision),
}
struct AwaitContinuation {
    queued: Option<QueuedTask>,
    child: Option<ContextKey>,
    result: Option<R<Completion>>,
    deliver: Deliver,
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
    Continue,
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
pub enum Poll {
    Paused,
    Await,
    Complete(Completion),
    Settling(DomainKey),
}
pub struct Machine {
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
        let frame = self
            .stack
            .last_mut()
            .ok_or("E_NATIVE: missing compiled activation")?;
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
        // operands can already move values, so the parent-selection guard remains.
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
            domains.require_no_children(&self.member)?;
        }
        match step {
            Step::Continue => {}
            Step::Await { task, deliver } => {
                if frame.pending.is_some() {
                    return Err("E_NATIVE: duplicate compiled await continuation".into());
                }
                frame.pending = Some(Pending::Await(AwaitContinuation {
                    queued: Some(task),
                    child: None,
                    result: None,
                    deliver,
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
                        Pending::Call(deliver) => deliver(parent, values, result)?,
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
        domains.require_no_children(&self.member)?;
        let mut remaining = quota;
        while remaining > 0 {
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
            if matches!(&self.stack.last().unwrap().pending, Some(Pending::Await(p)) if p.queued.is_some() && p.result.is_none())
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
pub use contexts::{ContextKey, ContextPoll, Contexts, Readiness, run_entry};

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
