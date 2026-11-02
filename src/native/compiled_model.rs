//! Test-only compiled continuation driver; no checked graph is interpreted here.
use super::{BoundaryState, Completion, Frames, Host, R, V, frames::Frame};

pub type Deliver = fn(&mut Activation, &mut Frames, Completion) -> R<()>;
pub struct Control {
    pub name: &'static str,
    pub slots: usize,
    pub atoms: usize,
    pub parameters: &'static [usize],
    pub constructor: bool,
    pub resume: fn(&mut Host, &mut Frames, &mut Activation) -> R<Step>,
}
pub struct Activation {
    frame: Frame,
    control: &'static Control,
    pending: Option<Deliver>,
    scopes: Vec<Scope>,
    pub facts: Vec<bool>,
    pub pc: usize,
    pub destination: Option<usize>,
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
        base: usize,
        result: &R<()>,
    ) -> R<()> {
        for slot in self.control.work.iter().chain(self.control.handling) {
            values[base + slot] = None;
        }
        host.finish_boundary(&mut self.state, result)
    }
    fn handle(
        &mut self,
        host: &mut Host,
        values: &mut Frames,
        base: usize,
        message: &str,
    ) -> R<()> {
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
    fn close_scopes(&mut self, host: &mut Host, values: &mut Frames, remaining: usize) -> R<()> {
        while self.scopes.len() > remaining {
            if let Some(Scope::Boundary(mut boundary)) = self.scopes.pop() {
                boundary.finish(host, values, self.base(), &Ok(()))?;
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
        control: &'static Control,
        args: Vec<V>,
    ) -> R<Self> {
        Ok(Self {
            stack: vec![Self::admit(host, values, control, args, None)?],
            finished: false,
        })
    }
    fn step(&mut self, host: &mut Host, values: &mut Frames) -> R<(bool, Option<Completion>)> {
        let frame = self
            .stack
            .last_mut()
            .ok_or("E_NATIVE: missing compiled activation")?;
        match (frame.control.resume)(host, values, frame)? {
            Step::Continue => {}
            Step::Boundary(control) => {
                let base = frame.base();
                let state = host.begin_boundary(
                    control.name,
                    control
                        .retained
                        .iter()
                        .chain(control.retire)
                        .filter_map(|id| values[base + id].as_ref()),
                    true,
                )?;
                frame
                    .scopes
                    .push(Scope::Boundary(BoundaryScope { control, state }));
                frame.pc = control.body;
            }
            Step::BoundaryEnd(control) => {
                let Some(Scope::Boundary(mut boundary)) = frame.scopes.pop() else {
                    return Err("E_NATIVE: missing compiled boundary continuation".into());
                };
                if !std::ptr::eq(boundary.control, control) {
                    return Err("E_NATIVE: mismatched compiled boundary continuation".into());
                }
                boundary.finish(host, values, frame.base(), &Ok(()))?;
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
            Step::Complete(Completion::Again(target, arguments)) => {
                let (index, control) = frame.loop_target(target)?;
                frame.close_scopes(host, values, index + 1)?;
                (control.reset)(frame, values, arguments)?;
                frame.pc = control.body;
            }
            Step::Complete(Completion::Recover(target, arguments)) => {
                let (index, control) = frame.loop_target(target)?;
                frame.close_scopes(host, values, index + 1)?;
                for slot in control.locals {
                    host.release_frame(std::slice::from_ref(&values[frame.base() + slot]));
                }
                (control.reset)(frame, values, arguments)?;
                frame.pc = control.body;
            }
            Step::Complete(Completion::Complete(target)) => {
                let (index, control) = frame.loop_target(target)?;
                frame.close_scopes(host, values, index)?;
                frame.pc = control.after;
            }
            Step::Call {
                control,
                arguments,
                destination,
                deliver,
            } => {
                if frame.pending.replace(deliver).is_some() {
                    return Err("E_NATIVE: duplicate compiled return continuation".into());
                }
                self.stack
                    .push(Self::admit(host, values, control, arguments, destination)?);
            }
            Step::Complete(result) => {
                frame.close_scopes(host, values, 0)?;
                let completed = self.stack.pop().unwrap();
                values.retire(&completed.frame)?;
                host.leave();
                if let Some(parent) = self.stack.last_mut() {
                    let deliver = parent
                        .pending
                        .take()
                        .ok_or("E_NATIVE: missing compiled return continuation")?;
                    deliver(parent, values, result)?;
                } else {
                    self.finished = true;
                    return Ok((true, Some(result)));
                }
            }
        }
        Ok((true, None))
    }
    fn unwind_error(&mut self, host: &mut Host, values: &mut Frames, mut message: String) -> R<()> {
        while let Some(frame) = self.stack.last_mut() {
            // A child fault retires its original frame before handover in its caller.
            while let Some(scope) = frame.scopes.pop() {
                if let Scope::Boundary(mut boundary) = scope {
                    if host.source_faulted() && boundary.state.fault_message.is_none() {
                        match boundary.handle(host, values, frame.base(), &message) {
                            Ok(()) => {
                                frame.pending = None;
                                frame.pc = boundary.control.handler;
                                frame.scopes.push(Scope::Boundary(boundary));
                                return Ok(());
                            }
                            Err(error) => message = error,
                        }
                    }
                    boundary.finish(host, values, frame.base(), &Err(message.clone()))?;
                }
            }
            let frame = self.stack.pop().unwrap();
            host.release_frame(values.frame_values(&frame.frame)?);
            values.retire(&frame.frame)?;
            // Failed activation diagnostics remain available until handover/reporting.
        }
        self.finished = true;
        Err(message)
    }
    pub fn advance(&mut self, host: &mut Host, values: &mut Frames, quota: usize) -> R<Poll> {
        if self.finished {
            return Err("E_NATIVE: compiled execution already retired".into());
        }
        let mut remaining = quota;
        while remaining > 0 {
            match self.step(host, values) {
                Ok((_, Some(result))) => return Ok(Poll::Complete(result)),
                Ok((instruction, None)) => remaining -= usize::from(instruction),
                Err(error) => {
                    self.unwind_error(host, values, error)?;
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
    let mut machine = Machine::start(host, &mut values, control, args)?;
    for quota in quotas.into_iter().cycle() {
        match machine.advance(host, &mut values, quota) {
            Ok(Poll::Paused) => {}
            Ok(Poll::Complete(result)) => {
                assert!(values.is_empty() && machine.stack.is_empty());
                assert!(machine.advance(host, &mut values, 1).is_err());
                return Ok(result);
            }
            Err(error) => {
                assert!(values.is_empty() && machine.stack.is_empty());
                assert!(machine.advance(host, &mut values, 1).is_err());
                return Err(error);
            }
        }
    }
    unreachable!()
}
