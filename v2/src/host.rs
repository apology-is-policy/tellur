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
use std::collections::{BTreeMap, VecDeque};

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
        if matches!(field.ty, Type::Int(_)) && config.fits(&result, &field.ty) {
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

/// Draws a scalar value of `ty` that fits under `config`.
pub fn draw_value(rng: &mut Rng, ty: &Type, arguments: &[Value], config: &Config) -> Value {
    match ty.deref_type() {
        Type::Bool => Value::Bool(rng.chance(1, 2)),
        Type::Int(int_ty) => Value::Int(draw_int(rng, *int_ty, arguments, config)),
        Type::String => {
            let samples = ["", "a", "b", "ok", "hello", "key"];
            Value::String((*rng.pick(&samples)).to_string())
        }
        Type::Vector(_) => Value::Vector(Vec::new()),
        Type::Map(_, _) => Value::Map(BTreeMap::new()),
        Type::Resource(rid, name) => Value::Resource {
            ty: *rid,
            name: name.clone(),
            id: rng.below(1_000_000) + 1,
        },
        _ => Value::Int(Int::zero()),
    }
}

/// Draws a value of any slice 1 or slice 2 type `ty` using `program` definitions.
pub fn draw_value_in_program(
    program: &Program,
    rng: &mut Rng,
    ty: &Type,
    arguments: &[Value],
    config: &Config,
    next_res: &mut u64,
) -> Value {
    match ty.deref_type() {
        Type::Bool => Value::Bool(rng.chance(1, 2)),
        Type::Int(int_ty) => Value::Int(draw_int(rng, *int_ty, arguments, config)),
        Type::String => {
            let samples = ["", "a", "b", "ok", "hello", "key"];
            Value::String((*rng.pick(&samples)).to_string())
        }
        Type::Struct(sid, name) => {
            let fields = program.structs[*sid]
                .fields
                .iter()
                .map(|f| draw_value_in_program(program, rng, &f.ty, arguments, config, next_res))
                .collect();
            Value::Struct {
                ty: *sid,
                name: name.clone(),
                fields,
            }
        }
        Type::Variant(vid, name) => {
            let vdef = &program.variants[*vid];
            let cidx = rng.index(vdef.constructors.len());
            let ctor = &vdef.constructors[cidx];
            let fields = ctor
                .fields
                .iter()
                .map(|f| draw_value_in_program(program, rng, &f.ty, arguments, config, next_res))
                .collect();
            Value::Variant {
                ty: *vid,
                name: name.clone(),
                constructor: cidx,
                constructor_name: ctor.name.clone(),
                fields,
            }
        }
        Type::Vector(elem_ty) => {
            let len = rng.below(4) as usize;
            let items = (0..len)
                .map(|_| draw_value_in_program(program, rng, elem_ty, arguments, config, next_res))
                .collect();
            Value::Vector(items)
        }
        Type::Map(k_ty, v_ty) => {
            let len = rng.below(3) as usize;
            let mut entries = BTreeMap::new();
            for _ in 0..len {
                let k = draw_value_in_program(program, rng, k_ty, arguments, config, next_res);
                let v = draw_value_in_program(program, rng, v_ty, arguments, config, next_res);
                entries.insert(k, v);
            }
            Value::Map(entries)
        }
        Type::Resource(rid, name) => {
            *next_res += 1;
            Value::Resource {
                ty: *rid,
                name: name.clone(),
                id: *next_res,
            }
        }
        Type::Outcome(oid, _) => {
            let odef = &program.outcomes[*oid];
            let aidx = rng.index(odef.alternatives.len());
            let alt = &odef.alternatives[aidx];
            let payload = alt
                .fields
                .iter()
                .map(|f| draw_value_in_program(program, rng, &f.ty, arguments, config, next_res))
                .collect();
            Value::Outcome {
                ty: *oid,
                alternative: aidx,
                alternative_name: alt.name.clone(),
                payload,
            }
        }
        _ => Value::Int(Int::zero()),
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
                    _ => None,
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
        Value::String(s) => {
            words.push(3);
            words.push(s.len() as u64);
            for b in s.as_bytes() {
                words.push(u64::from(*b));
            }
        }
        Value::Struct { ty, fields, .. } => {
            words.push(4);
            words.push(*ty as u64);
            words.push(fields.len() as u64);
            for f in fields {
                push_value(words, f);
            }
        }
        Value::Variant {
            ty,
            constructor,
            fields,
            ..
        } => {
            words.push(5);
            words.push(*ty as u64);
            words.push(*constructor as u64);
            words.push(fields.len() as u64);
            for f in fields {
                push_value(words, f);
            }
        }
        Value::Vector(items) => {
            words.push(6);
            words.push(items.len() as u64);
            for item in items {
                push_value(words, item);
            }
        }
        Value::Map(entries) => {
            words.push(7);
            words.push(entries.len() as u64);
            for (k, v) in entries {
                push_value(words, k);
                push_value(words, v);
            }
        }
        Value::Resource { ty, id, .. } => {
            words.push(8);
            words.push(*ty as u64);
            words.push(*id);
        }
        Value::Outcome {
            ty,
            alternative,
            payload,
            ..
        } => {
            words.push(9);
            words.push(*ty as u64);
            words.push(*alternative as u64);
            for p in payload {
                push_value(words, p);
            }
        }
    }
}

/// A host that draws answers from a seeded generator and honors every
/// contract ([E-Host]).
pub struct Seeded<'a> {
    program: &'a Program,
    seed: u64,
    rng: Rng,
    next_res: u64,
}

impl<'a> Seeded<'a> {
    pub fn new(program: &'a Program, seed: u64) -> Seeded<'a> {
        Seeded {
            program,
            seed,
            rng: Rng::new(seed),
            next_res: 0,
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
            let mut res_counter = self.next_res;
            let payload: Vec<Value> = alternative
                .fields
                .iter()
                .map(|field| {
                    draw_value_in_program(
                        self.program,
                        &mut rng,
                        &field.ty,
                        arguments,
                        config,
                        &mut res_counter,
                    )
                })
                .collect();
            let valid = match &alternative.condition {
                None => true,
                Some(condition) => {
                    signature_holds(self.program, config, condition, arguments, &payload)
                }
            };
            if valid {
                self.next_res = res_counter;
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
