//! Test-only compiled continuation driver; no checked graph is interpreted here.
use super::{BoundaryState, Completion, Frames, Host, R, V, frames::Frame};
use domain_model::{DomainKey, DomainScope, Domains};

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
    control: &'static Control,
    pending: Option<Pending>,
    scopes: Vec<Scope>,
    pub facts: Vec<bool>,
    pub pc: usize,
    pub destination: Option<usize>,
}
enum Pending {
    Call(Deliver),
    Supervise(Supervision),
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
    ) -> R<()> {
        domains.require_quiescent(&self.domain, current)?;
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
    ) -> R<()> {
        while self.scopes.len() > remaining {
            if let Some(Scope::Boundary(mut boundary)) = self.scopes.pop() {
                boundary.finish(host, values, domains, current, self.base(), &Ok(()))?;
            }
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
    Complete(Completion),
}
pub struct Machine {
    stack: Vec<Activation>,
    finished: bool,
    root: DomainScope,
    current: DomainKey,
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
        let frame = values.enter(control.slots)?;
        if let Err(error) = host.enter(control.name) {
            values.retire(&frame)?;
            return Err(error);
        }
        for (slot, value) in control.parameters.iter().zip(args) {
            values[frame.base() + slot] = Some(value);
        }
        Ok(Activation {
            frame,
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
        let activation = Self::admit(host, values, control, args, None)?;
        domains.commit(&root)?;
        Ok(Self {
            stack: vec![activation],
            finished: false,
            current: root.key.clone(),
            root,
        })
    }
    fn require_domain(&self, domains: &Domains) -> R<()> {
        if self.finished {
            return Err("E_NATIVE: compiled execution already retired".into());
        }
        domains.require_member(&self.root.key, &self.current)?;
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
    fn finish_root(&mut self, domains: &mut Domains) -> R<()> {
        domains.close(&self.root, &self.current)?;
        self.finished = true;
        Ok(())
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
        match (frame.control.resume)(host, values, frame)? {
            Step::Continue => {}
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
                let Some(Scope::Boundary(mut boundary)) = frame.scopes.pop() else {
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
                    frame.base(),
                    &Ok(()),
                )?;
                frame.pc = control.after;
                // This sentinel is administrative and consumes no instruction quota.
                return Ok((false, None));
            }
            Step::Loop { control, arguments } => {
                if frame.loop_target(control.target).is_ok() {
                    return Err("E_NATIVE: compiled loop already active".into());
                }
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
                frame.close_scopes(host, values, domains, &mut self.current, index + 1)?;
                (control.reset)(frame, values, arguments)?;
                frame.pc = control.body;
            }
            Step::Complete(Completion::Recover(target, arguments)) => {
                let (index, control) = frame.loop_target(target)?;
                frame.close_scopes(host, values, domains, &mut self.current, index + 1)?;
                for slot in control.locals {
                    host.release_frame(std::slice::from_ref(&values[frame.base() + slot]));
                }
                (control.reset)(frame, values, arguments)?;
                frame.pc = control.body;
            }
            Step::Complete(Completion::Complete(target)) => {
                let (index, control) = frame.loop_target(target)?;
                frame.close_scopes(host, values, domains, &mut self.current, index)?;
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
                frame.close_scopes(host, values, domains, &mut self.current, 0)?;
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
                        Pending::Supervise(mut supervision) => {
                            let result = result.settled();
                            supervision.finish(host, domains, &mut self.current, &result)?;
                            result?;
                        }
                    }
                } else {
                    self.finish_root(domains)?;
                    return Ok((true, Some(result)));
                }
            }
        }
        Ok((true, None))
    }
    fn unwind_error(
        &mut self,
        host: &mut Host,
        values: &mut Frames,
        domains: &mut Domains,
        mut message: String,
    ) -> R<()> {
        let mut retained = vec![];
        while let Some(frame) = self.stack.last_mut() {
            // A named work frame has already handed over retained parameters and
            // retired. Its caller still owns the original supervision record.
            if let Some(Pending::Supervise(mut supervision)) = frame.pending.take() {
                if host.source_faulted() && supervision.state.fault_message.is_none() {
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
                            return Ok(());
                        }
                        Err(error) => message = error,
                    }
                }
                supervision.finish(host, domains, &mut self.current, &Err(message.clone()))?;
            }
            retained.clear();
            // A child fault retires its original frame before handover in its caller.
            while let Some(scope) = frame.scopes.pop() {
                if let Scope::Boundary(mut boundary) = scope {
                    if host.source_faulted() && boundary.state.fault_message.is_none() {
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
                    )?;
                }
            }
            let frame = self.stack.pop().unwrap();
            if host.source_faulted() {
                for slot in frame.control.retained {
                    retained.push(
                        values[frame.base() + slot]
                            .take()
                            .ok_or("E_NATIVE: retained boundary parameter unavailable")?,
                    );
                }
            }
            host.release_frame(values.frame_values(&frame.frame)?);
            values.retire(&frame.frame)?;
            // Failed activation diagnostics remain available until handover/reporting.
        }
        self.finish_root(domains)?;
        Err(message)
    }
    pub fn advance(
        &mut self,
        host: &mut Host,
        values: &mut Frames,
        domains: &mut Domains,
        quota: usize,
    ) -> R<Poll> {
        self.require_domain(domains)?;
        let mut remaining = quota;
        while remaining > 0 {
            match self.step(host, values, domains) {
                Ok((_, Some(result))) => return Ok(Poll::Complete(result)),
                Ok((instruction, None)) => remaining -= usize::from(instruction),
                Err(error) => {
                    self.unwind_error(host, values, domains, error)?;
                    remaining -= 1;
                }
            }
        }
        Ok(Poll::Paused)
    }
}
pub fn drive(host: &mut Host, args: Vec<V>, control: &'static Control) -> R<Completion> {
    let quotas: Vec<usize> = std::env::var("TELLUR_NATIVE_MODEL_QUOTAS")
        .unwrap_or("1".into())
        .split(',')
        .map(|s| {
            s.parse()
                .map_err(|_| "E_NATIVE: invalid model quota".to_string())
        })
        .collect::<R<_>>()?;
    if !quotas.iter().any(|q| *q > 0) {
        return Err("E_NATIVE: model quotas cannot make progress".into());
    }
    let mut values = Frames::default();
    let mut domains = Domains::default();
    let mut machine = Machine::start(host, &mut values, &mut domains, control, args)?;
    for quota in quotas.into_iter().cycle() {
        match machine.advance(host, &mut values, &mut domains, quota) {
            Ok(Poll::Paused) => {}
            Ok(Poll::Complete(result)) => {
                assert!(values.is_empty() && machine.stack.is_empty());
                assert_eq!(domains.len(), 0);
                assert!(machine.advance(host, &mut values, &mut domains, 1).is_err());
                return Ok(result);
            }
            Err(error) => {
                assert!(values.is_empty() && machine.stack.is_empty());
                assert_eq!(domains.len(), 0);
                assert!(machine.advance(host, &mut values, &mut domains, 1).is_err());
                return Err(error);
            }
        }
    }
    unreachable!()
}
