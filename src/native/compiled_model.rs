//! Test-only compiled continuation driver; no checked graph is interpreted here.
use super::{Completion, Frames, Host, R, V, frames::Frame};

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
    pub facts: Vec<bool>,
    pub pc: usize,
    pub destination: Option<usize>,
}
impl Activation {
    pub fn base(&self) -> usize {
        self.frame.base()
    }
}
pub enum Step {
    Continue,
    Complete(Completion),
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
    fn advance_inner(&mut self, host: &mut Host, values: &mut Frames, quota: usize) -> R<Poll> {
        for _ in 0..quota {
            let frame = self
                .stack
                .last_mut()
                .ok_or("E_NATIVE: missing compiled activation")?;
            match (frame.control.resume)(host, values, frame)? {
                Step::Continue => {}
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
                        return Ok(Poll::Complete(result));
                    }
                }
            }
        }
        Ok(Poll::Paused)
    }
    pub fn advance(&mut self, host: &mut Host, values: &mut Frames, quota: usize) -> R<Poll> {
        if self.finished {
            return Err("E_NATIVE: compiled execution already retired".into());
        }
        let result = self.advance_inner(host, values, quota);
        if result.is_err() {
            self.finished = true;
            while let Some(frame) = self.stack.pop() {
                host.release_frame(values.frame_values(&frame.frame)?);
                values.retire(&frame.frame)?;
            }
        }
        result
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
