//! Single-step mutations of valid core programs, classified by the oracle.
//!
//! Each mutant seeds one defect into a valid [`syntax::Program`]: removing an
//! arm of a decision, removing a disposition (`■`, `□` or `↑`), dropping a
//! statement's guard selector to `⊤`, dropping a loop's invariant, or
//! dropping an effect from a control's `uses` clause.
//!
//! [`classify`] runs a mutant through `resolve` and the oracle across a set of
//! inputs. The mutants that are stuck in the oracle are those that the checker
//! of phase 3 must reject.

use crate::host::{Arithmetic, Seeded};
use crate::oracle::{self, Config, End, Stuck, Value};
use crate::resolve;
use crate::syntax::{Command, Declaration, Program, Selector, Statement};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// An arm of a decision is removed ([C2], X10).
    RemoveArm,
    /// An exit, `□` or `↑` is removed ([K7], X1).
    RemoveDisposition,
    /// A selector is replaced by `⊤` ([O2] to [O4], [P1] to [P8], X2, X3, X6, X7).
    DropGuard,
    /// The invariant of a loop is dropped ([K4], [P4]).
    DropInvariant,
    /// An effect is removed from a control's `uses` clause ([F3], X9).
    DropEffect,
}

#[derive(Clone, Debug)]
pub struct Mutant {
    pub kind: Kind,
    pub control: String,
    pub detail: String,
    pub program: Program,
}

#[derive(Clone, Debug)]
pub enum Class {
    /// `resolve` refuses the mutant by this clause.
    Refused(&'static str),
    /// A run of the mutant in the oracle is stuck here.
    Stuck(Stuck),
    /// No tested run of the mutant was stuck.
    Unstuck,
}

fn body_mut<'a>(
    program: &'a mut Program,
    declaration: usize,
    loop_path: &[usize],
) -> &'a mut Vec<Statement> {
    let Declaration::Control(control) = &mut program.declarations[declaration] else {
        panic!("expected a control declaration");
    };
    let mut statements = &mut control.body;
    for index in loop_path {
        let Command::Loop { body, .. } = &mut statements[*index].command else {
            panic!("expected a loop statement");
        };
        statements = body;
    }
    statements
}

fn collect_from_body(
    program: &Program,
    declaration: usize,
    control_name: &str,
    loop_path: &mut Vec<usize>,
    out: &mut Vec<Mutant>,
) {
    let statements = {
        let Declaration::Control(control) = &program.declarations[declaration] else {
            return;
        };
        let mut current = &control.body;
        for index in loop_path.iter() {
            let Command::Loop { body, .. } = &current[*index].command else {
                return;
            };
            current = body;
        }
        current.clone()
    };

    for (index, statement) in statements.iter().enumerate() {
        if !matches!(statement.selector, Selector::Top) {
            let mut mutated = program.clone();
            body_mut(&mut mutated, declaration, loop_path)[index].selector = Selector::Top;
            out.push(Mutant {
                kind: Kind::DropGuard,
                control: control_name.to_string(),
                detail: format!("drop guard at statement {index}"),
                program: mutated,
            });
        }

        match &statement.command {
            Command::Decide { site, arms, .. } => {
                for (arm_index, arm) in arms.iter().enumerate() {
                    let mut mutated = program.clone();
                    let Command::Decide { arms: target, .. } =
                        &mut body_mut(&mut mutated, declaration, loop_path)[index].command
                    else {
                        unreachable!();
                    };
                    target.remove(arm_index);
                    out.push(Mutant {
                        kind: Kind::RemoveArm,
                        control: control_name.to_string(),
                        detail: format!("remove arm `{}` of `{site}`", arm.alternative),
                        program: mutated,
                    });
                }
            }
            Command::Exit { alternative, .. }
            | Command::Leave { alternative, .. }
            | Command::Again {
                name: alternative, ..
            } => {
                let mut mutated = program.clone();
                body_mut(&mut mutated, declaration, loop_path).remove(index);
                out.push(Mutant {
                    kind: Kind::RemoveDisposition,
                    control: control_name.to_string(),
                    detail: format!("remove disposition `{alternative}` at statement {index}"),
                    program: mutated,
                });
            }
            Command::Loop {
                name, invariant, ..
            } => {
                if invariant.is_some() {
                    let mut mutated = program.clone();
                    let Command::Loop {
                        invariant: target, ..
                    } = &mut body_mut(&mut mutated, declaration, loop_path)[index].command
                    else {
                        unreachable!();
                    };
                    *target = None;
                    out.push(Mutant {
                        kind: Kind::DropInvariant,
                        control: control_name.to_string(),
                        detail: format!("drop invariant of `{name}`"),
                        program: mutated,
                    });
                }
                loop_path.push(index);
                collect_from_body(program, declaration, control_name, loop_path, out);
                loop_path.pop();
            }
            _ => {}
        }
    }
}

/// Produces all single-step mutants of `program`.
pub fn mutate(program: &Program) -> Vec<Mutant> {
    let mut out = Vec::new();
    for (declaration, item) in program.declarations.iter().enumerate() {
        let Declaration::Control(control) = item else {
            continue;
        };
        if control.external {
            continue;
        }
        for (effect_index, effect) in control.uses.iter().enumerate() {
            let mut mutated = program.clone();
            let Declaration::Control(target) = &mut mutated.declarations[declaration] else {
                unreachable!();
            };
            target.uses.remove(effect_index);
            out.push(Mutant {
                kind: Kind::DropEffect,
                control: control.name.clone(),
                detail: format!("drop effect `{effect}`"),
                program: mutated,
            });
        }
        let mut loop_path = Vec::new();
        collect_from_body(
            program,
            declaration,
            &control.name,
            &mut loop_path,
            &mut out,
        );
    }
    out
}

/// Resolves `mutant` and runs it in the oracle across `runs` (each a control
/// name and its arguments), under a few host seeds, returning the first
/// refusal or stuck state it meets.
pub fn classify(
    mutant: &Mutant,
    runs: &[(String, Vec<Value>)],
    seed: u64,
    config: &Config,
) -> Class {
    let typed = match resolve::resolve(&mutant.program) {
        Ok(typed) => typed,
        Err(error) => return Class::Refused(error.clause),
    };
    for (run_index, (entry, arguments)) in runs.iter().enumerate() {
        for host_offset in 0..3u64 {
            let run_seed = seed
                .wrapping_add((run_index as u64).wrapping_mul(31))
                .wrapping_add(host_offset);
            let mut host = Arithmetic(Seeded::new(&typed, run_seed));
            let outcome = oracle::run(&typed, entry, arguments.clone(), &mut host, config);
            if let End::Stuck(stuck) = outcome.end {
                return Class::Stuck(stuck);
            }
        }
    }
    Class::Unstuck
}
