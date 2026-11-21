//! The differential harness between the two semantics.
//!
//! Theorem 3 of the core says that, for a checked program and a faithful
//! host, the instrumented and the erased semantics yield the same observation
//! from the same initial state and the same host choices. The harness tests
//! the statement run by run, and for every program, checked or not: where
//! the oracle is not stuck, the erased run of the same control on the same
//! arguments, with the host's answers replayed, must end the same way with
//! the same trace.
//!
//! This is the first of the three comparisons of the plan. The compiled code
//! of phase 4 is compared with the oracle in the same way.

use crate::erased;
use crate::host::{Answer, Host, Script};
use crate::oracle::{self, Config, Value};
use crate::typed::{Control, Program};

/// A host that passes every call on and keeps the answers.
pub struct Recording<'a> {
    pub host: &'a mut dyn Host,
    pub answers: Vec<(String, Answer)>,
}

impl Host for Recording<'_> {
    fn call(
        &mut self,
        control: &Control,
        arguments: &[Value],
        config: &Config,
    ) -> Result<Answer, String> {
        let answer = self.host.call(control, arguments, config)?;
        self.answers.push((control.name.clone(), answer.clone()));
        Ok(answer)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Both runs give the same observation.
    Same,
    /// The run in the oracle has no observation to compare: it is stuck, the
    /// host broke a contract or had no answer, or it is not an execution.
    NotCompared,
    Differ(String),
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub oracle: oracle::Run,
    pub erased: Option<erased::Run>,
    /// What the host answered in the oracle's run, in order.
    pub answers: Vec<(String, Answer)>,
    pub verdict: Verdict,
}

fn differ(oracle: &oracle::Run, erased: &erased::Run, unused: usize) -> Option<String> {
    let same_end = match (&oracle.end, &erased.end) {
        (
            oracle::End::Done {
                alternative,
                payload,
            },
            erased::End::Done {
                alternative: a,
                payload: p,
            },
        ) => alternative == a && payload == p,
        (oracle::End::OutOfFuel, erased::End::OutOfFuel) => true,
        _ => false,
    };
    if !same_end {
        return Some(format!(
            "the oracle ends in `{}` and the erased run in `{}`",
            oracle.end, erased.end
        ));
    }
    if oracle.trace != erased.trace {
        let at = oracle
            .trace
            .iter()
            .zip(&erased.trace)
            .position(|(a, b)| a != b)
            .unwrap_or(oracle.trace.len().min(erased.trace.len()));
        let show = |trace: &[oracle::Event]| match trace.get(at) {
            Some(event) => event.to_string(),
            None => "nothing".to_string(),
        };
        return Some(format!(
            "the traces differ at event {at}: the oracle has `{}` and the erased run `{}`",
            show(&oracle.trace),
            show(&erased.trace)
        ));
    }
    if oracle.steps != erased.steps {
        return Some(format!(
            "the oracle takes {} steps and the erased run {}",
            oracle.steps, erased.steps
        ));
    }
    if unused > 0 {
        return Some(format!(
            "the erased run leaves {unused} answers of the host unused"
        ));
    }
    None
}

/// Runs `entry` in the oracle and, where that run has an observation, under
/// the erased semantics with the same answers of the host.
pub fn compare(
    program: &Program,
    entry: &str,
    arguments: Vec<Value>,
    host: &mut dyn Host,
    config: &Config,
) -> Outcome {
    let mut recording = Recording {
        host,
        answers: Vec::new(),
    };
    let oracle = oracle::run(program, entry, arguments.clone(), &mut recording, config);
    let answers = recording.answers;
    if !matches!(
        oracle.end,
        oracle::End::Done { .. } | oracle::End::OutOfFuel
    ) {
        return Outcome {
            oracle,
            erased: None,
            answers,
            verdict: Verdict::NotCompared,
        };
    }
    let mut replay = Script {
        answers: answers.iter().cloned().collect(),
    };
    let erased = erased::run(program, entry, arguments, &mut replay, config);
    let verdict = match differ(&oracle, &erased, replay.answers.len()) {
        None => Verdict::Same,
        Some(difference) => Verdict::Differ(difference),
    };
    Outcome {
        oracle,
        erased: Some(erased),
        answers,
        verdict,
    }
}
