//! Hosts for the oracle.
//!
//! An extern control is a relation of the host: given the arguments it
//! yields an alternative and a payload ([E-Extern]). The oracle checks every
//! answer against the contract ([E-Host]), so a host here may be wrong; a
//! wrong answer ends the run in `violation`, not in a stuck state.

use crate::int::Int;
use crate::oracle::{Config, Value, signature_holds};
use crate::rng::{Rng, mix};
use crate::typed::{Control, IntType, Program, Type};
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

/// Draws a value of `ty` that fits under `config`, leaning toward small
/// numbers, numbers near `arguments`, and the bounds of the type.
pub fn draw_value(rng: &mut Rng, ty: Type, arguments: &[Value], config: &Config) -> Value {
    match ty {
        Type::Bool => Value::Bool(rng.chance(1, 2)),
        Type::Int(int_ty) => Value::Int(draw_int(rng, int_ty, arguments, config)),
    }
}

fn draw_int(rng: &mut Rng, ty: IntType, arguments: &[Value], config: &Config) -> Int {
    let (least, greatest) = ty.bounds(&config.usize_max);
    let low = least.to_i128().unwrap_or(i128::from(i64::MIN));
    let high = greatest.to_i128().unwrap_or(i128::from(u64::MAX));
    let chosen = match rng.below(6) {
        0 => rng.range(0.max(low), 7.min(high)),
        1 => rng.range((-7).max(low), 7.min(high)),
        2 => {
            let ints: Vec<i128> = arguments
                .iter()
                .filter_map(|value| match value {
                    Value::Int(number) => number.to_i128(),
                    Value::Bool(_) => None,
                })
                .collect();
            if ints.is_empty() {
                rng.range((-3).max(low), 3.min(high))
            } else {
                let base = *rng.pick(&ints);
                base.saturating_add(rng.range(-2, 2)).clamp(low, high)
            }
        }
        3 => rng.range((-50).max(low), 50.min(high)),
        4 => *rng.pick(&[
            low,
            (low + 1).min(high),
            0.clamp(low, high),
            1.clamp(low, high),
            (high - 1).max(low),
            high,
        ]),
        _ => rng.range(low, high),
    };
    Int::from_i128(chosen)
}

fn push_value(words: &mut Vec<u64>, value: &Value) {
    match value {
        Value::Bool(flag) => {
            words.push(0);
            words.push(u64::from(*flag));
        }
        Value::Int(number) => {
            words.push(if number.is_negative() { 2 } else { 1 });
            let raw = number.to_i128().unwrap_or(0) as u128;
            words.push(raw as u64);
            words.push((raw >> 64) as u64);
        }
    }
}

/// A host that draws answers from a seeded generator and honors every
/// contract ([E-Host]).
///
/// An extern control without an effect is a function of its arguments: its
/// answer comes from a generator seeded by the host's seed, the control's
/// name and the arguments (`rng::mix`), so two calls with the same arguments
/// get the same answer. A control with an effect draws from the host's state
/// and may answer two calls in two ways.
pub struct Seeded<'a> {
    program: &'a Program,
    seed: u64,
    rng: Rng,
}

impl<'a> Seeded<'a> {
    pub fn new(program: &'a Program, seed: u64) -> Seeded<'a> {
        Seeded {
            program,
            seed,
            rng: Rng::new(seed),
        }
    }
}

impl Host for Seeded<'_> {
    fn call(
        &mut self,
        control: &Control,
        arguments: &[Value],
        config: &Config,
    ) -> Result<Answer, String> {
        let mut rng = if control.is_pure() {
            let mut words = Vec::with_capacity(4 + control.name.len() + arguments.len() * 3);
            words.push(self.seed);
            words.push(control.name.len() as u64);
            for byte in control.name.as_bytes() {
                words.push(u64::from(*byte));
            }
            for argument in arguments {
                push_value(&mut words, argument);
            }
            Rng::new(mix(&words))
        } else {
            self.rng.fork()
        };
        let count = control.contract.len();
        let start = rng.index(count);
        for attempt in 0..96 {
            let alternative = &control.contract[(start + attempt) % count];
            let payload: Vec<Value> = alternative
                .fields
                .iter()
                .map(|field| draw_value(&mut rng, field.ty, arguments, config))
                .collect();
            let valid = match &alternative.condition {
                None => true,
                Some(condition) => {
                    signature_holds(self.program, config, condition, arguments, &payload)
                }
            };
            if valid {
                return Ok(Answer {
                    alternative: alternative.name.clone(),
                    payload,
                });
            }
        }
        Err(format!(
            "no answer found that satisfies the contract of `{}`",
            control.name
        ))
    }
}
