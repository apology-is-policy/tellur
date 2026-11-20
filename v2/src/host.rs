//! Hosts for the oracle.
//!
//! An extern control is a relation of the host: given the arguments it
//! yields an alternative and a payload ([E-Extern]). The oracle checks every
//! answer against the contract ([E-Host]), so a host here may be wrong; a
//! wrong answer ends the run in `violation`, not in a stuck state.

use crate::int::Int;
use crate::oracle::{Config, Value};
use crate::typed::{Control, Type};
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub alternative: String,
    pub payload: Vec<Value>,
}

pub trait Host {
    /// The answer to one call, or the reason this host has none.
    fn call(
        &mut self,
        control: &Control,
        arguments: &[Value],
        config: &Config,
    ) -> Result<Answer, String>;
}

/// A host that answers from a list, in order. Each entry names the control
/// it answers.
#[derive(Clone, Debug, Default)]
pub struct Script {
    pub answers: VecDeque<(String, Answer)>,
}

impl Host for Script {
    fn call(
        &mut self,
        control: &Control,
        _arguments: &[Value],
        _config: &Config,
    ) -> Result<Answer, String> {
        match self.answers.pop_front() {
            None => Err(format!(
                "the script has no answer left for `{}`",
                control.name
            )),
            Some((name, _)) if name != control.name => Err(format!(
                "the script answers `{name}` next, and `{}` was called",
                control.name
            )),
            Some((_, answer)) => Ok(answer),
        }
    }
}

/// A host that computes the checked arithmetic of the prelude and passes
/// every other call on.
///
/// A control whose name begins with `checked_add`, `checked_sub`,
/// `checked_mul`, `checked_div` or `checked_rem` takes two integers. Its
/// first alternative carries the result and its second has no payload; the
/// second is the answer when the result is not a value of the payload type.
pub struct Arithmetic<H: Host>(pub H);

fn checked(
    control: &Control,
    arguments: &[Value],
    config: &Config,
) -> Option<Result<Answer, String>> {
    let operations = [
        "checked_add",
        "checked_sub",
        "checked_mul",
        "checked_div",
        "checked_rem",
    ];
    let operation = operations
        .iter()
        .find(|name| control.name.starts_with(**name))?;
    let shape = || {
        Err(format!(
            "`{}` does not have the shape of checked arithmetic",
            control.name
        ))
    };
    let [Value::Int(left), Value::Int(right)] = arguments else {
        return Some(shape());
    };
    let [fits, fails] = control.contract.as_slice() else {
        return Some(shape());
    };
    let ([field], true) = (fits.fields.as_slice(), fails.fields.is_empty()) else {
        return Some(shape());
    };
    let result: Int = match *operation {
        "checked_add" => left.plus(right),
        "checked_sub" => left.minus(right),
        "checked_mul" => left.times(right),
        _ => {
            let Some((quotient, remainder)) = left.divide(right) else {
                return Some(Err(format!(
                    "`{}` was asked to divide by zero; its contract must require a divisor that is not zero",
                    control.name
                )));
            };
            if *operation == "checked_div" {
                quotient
            } else {
                remainder
            }
        }
    };
    let result = Value::Int(result);
    Some(Ok(
        if matches!(field.ty, Type::Int(_)) && config.fits(&result, field.ty) {
            Answer {
                alternative: fits.name.clone(),
                payload: vec![result],
            }
        } else {
            Answer {
                alternative: fails.name.clone(),
                payload: Vec::new(),
            }
        },
    ))
}

impl<H: Host> Host for Arithmetic<H> {
    fn call(
        &mut self,
        control: &Control,
        arguments: &[Value],
        config: &Config,
    ) -> Result<Answer, String> {
        match checked(control, arguments, config) {
            Some(answer) => answer,
            None => self.0.call(control, arguments, config),
        }
    }
}
