//! Test directives: runs written beside a program, and what each must yield.
//!
//! A line of a core text file that begins with `//!` is a directive.
//!
//! ```text
//! //! run add(250, 5) => done sum {255}
//! //! run add(250, 6) => stuck X7 E-Arith
//! //! run pump(2) with [read -> value {7}; read -> closed] => done total {7}
//! //!     trace [read(2) -> value {7}; read(2) -> closed {}]
//! //! reject W1
//! ```
//!
//! `run` names an entry control and its arguments, then options, then `=>`
//! and the expected end: `done` with an alternative and its payload, `stuck`
//! with the row of the table of stuck states and the clause, `violation`,
//! `host-failed`, `fuel`, or `not-an-execution`. The options are
//! `with arithmetic` (the host computes the checked arithmetic of the
//! prelude), `with [...]` (the host answers from the list), `usize 32` (the
//! greatest `usize` is `2^32 - 1`; the default is `2^64 - 1`) and `fuel N`.
//! `trace [...]` after the end states the events of the run. A directive
//! continues on the following `//!` lines until the next `run` or `reject`.
//!
//! `reject` names the clause by which `resolve` must refuse the program.
//!
//! Every `run` that ends in `done` or in `fuel` is also carried out under the
//! erased semantics, with the answers of the host replayed, and the two
//! observations must be the same (Theorem 3). `erased` in the place of `run`
//! states a run of the erased semantics alone; it may also end in
//! `undefined`.

use crate::differential::{Verdict, compare};
use crate::host::{Answer, Arithmetic, Host, Script};
use crate::int::Int;
use crate::oracle::{Config, End, Event, Value};
use crate::{check, erased, resolve, surface, text};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Word(String),
    Mark(&'static str),
}

fn pieces(text: &str) -> Result<Vec<Piece>, String> {
    const MARKS: [&str; 10] = ["->", "=>", "(", ")", "{", "}", "[", "]", ",", ";"];
    let characters: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut index = 0;
    'next: while index < characters.len() {
        if characters[index].is_whitespace() {
            index += 1;
            continue;
        }
        let rest: String = characters[index..].iter().take(2).collect();
        for mark in MARKS {
            if rest.starts_with(mark) {
                out.push(Piece::Mark(mark));
                index += mark.len();
                continue 'next;
            }
        }
        let start = index;
        while index < characters.len() {
            let character = characters[index];
            let inner_dash = character == '-'
                && characters
                    .get(index + 1)
                    .is_some_and(|next| next.is_ascii_alphanumeric());
            if character.is_ascii_alphanumeric() || character == '_' || inner_dash {
                index += 1;
            } else {
                break;
            }
        }
        if index == start {
            return Err(format!(
                "`{}` is not part of a directive",
                characters[index]
            ));
        }
        out.push(Piece::Word(characters[start..index].iter().collect()));
    }
    Ok(out)
}

struct Reader {
    pieces: Vec<Piece>,
    at: usize,
}

impl Reader {
    fn mark(&mut self, mark: &str) -> bool {
        let found = matches!(self.pieces.get(self.at), Some(Piece::Mark(found)) if *found == mark);
        if found {
            self.at += 1;
        }
        found
    }

    fn need(&mut self, mark: &str) -> Result<(), String> {
        if self.mark(mark) {
            Ok(())
        } else {
            Err(format!("expected `{mark}`"))
        }
    }

    fn peek_word(&self) -> Option<&str> {
        match self.pieces.get(self.at) {
            Some(Piece::Word(word)) => Some(word),
            _ => None,
        }
    }

    fn word(&mut self, what: &str) -> Result<String, String> {
        match self.pieces.get(self.at) {
            Some(Piece::Word(word)) => {
                self.at += 1;
                Ok(word.clone())
            }
            _ => Err(format!("expected {what}")),
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        let word = self.word("a value")?;
        match word.as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            digits => Int::parse(digits)
                .map(Value::Int)
                .ok_or(format!("`{digits}` is not a value")),
        }
    }

    fn values(&mut self, close: &str) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        while !self.mark(close) {
            if !out.is_empty() {
                self.need(",")?;
            }
            out.push(self.value()?);
        }
        Ok(out)
    }

    /// `alternative [{values}]`
    fn outcome(&mut self) -> Result<(String, Vec<Value>), String> {
        let alternative = self.word("an alternative")?;
        let payload = if self.mark("{") {
            self.values("}")?
        } else {
            Vec::new()
        };
        Ok((alternative, payload))
    }
}

#[derive(Clone, Debug)]
enum Expect {
    Done {
        alternative: String,
        payload: Vec<Value>,
    },
    Stuck {
        state: String,
        clause: String,
    },
    Other(&'static str),
}

impl std::fmt::Display for Expect {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Expect::Done {
                alternative,
                payload,
            } => {
                write!(
                    formatter,
                    "done {alternative} {{{}}}",
                    crate::oracle::list(payload)
                )
            }
            Expect::Stuck { state, clause } => write!(formatter, "stuck {state} {clause}"),
            Expect::Other(kind) => formatter.write_str(kind),
        }
    }
}

#[derive(Clone, Debug)]
struct RunSpec {
    line: usize,
    /// A run of the erased semantics alone.
    erased: bool,
    entry: String,
    arguments: Vec<Value>,
    arithmetic: bool,
    answers: Vec<(String, Answer)>,
    config: Config,
    expect: Expect,
    trace: Option<Vec<Event>>,
}

#[derive(Clone, Debug)]
enum Directive {
    Run(Box<RunSpec>),
    Reject { line: usize, clause: String },
    Check { line: usize },
}

fn directive(line: usize, text: &str) -> Result<Directive, String> {
    let mut reader = Reader {
        pieces: pieces(text)?,
        at: 0,
    };
    let kind = reader.word("`run`, `erased`, `check` or `reject`")?;
    if kind == "check" {
        if reader.at != reader.pieces.len() {
            return Err("text after `check`".to_string());
        }
        return Ok(Directive::Check { line });
    }
    if kind == "reject" {
        return Ok(Directive::Reject {
            line,
            clause: reader.word("a clause")?,
        });
    }
    if kind != "run" && kind != "erased" {
        return Err(format!("`{kind}` is not a directive"));
    }
    let entry = reader.word("an entry control")?;
    reader.need("(")?;
    let arguments = reader.values(")")?;
    let mut spec = RunSpec {
        line,
        erased: kind == "erased",
        entry,
        arguments,
        arithmetic: false,
        answers: Vec::new(),
        config: Config::default(),
        expect: Expect::Other("fuel"),
        trace: None,
    };
    while !reader.mark("=>") {
        match reader.word("an option or `=>`")?.as_str() {
            "with" if reader.peek_word() == Some("arithmetic") => {
                reader.at += 1;
                spec.arithmetic = true;
            }
            "with" => {
                reader.need("[")?;
                while !reader.mark("]") {
                    if !spec.answers.is_empty() {
                        reader.need(";")?;
                    }
                    let control = reader.word("a control")?;
                    reader.need("->")?;
                    let (alternative, payload) = reader.outcome()?;
                    spec.answers.push((
                        control,
                        Answer {
                            alternative,
                            payload,
                        },
                    ));
                }
            }
            "usize" => {
                let bits = reader.word("32 or 64")?;
                let bits: u32 = bits.parse().map_err(|_| "expected 32 or 64".to_string())?;
                spec.config.usize_max = Int::power_of_two(bits).minus(&Int::from_u64(1));
            }
            "fuel" => {
                let fuel = reader.word("a number of steps")?;
                spec.config.fuel = fuel.parse().map_err(|_| "expected a number".to_string())?;
            }
            other => return Err(format!("`{other}` is not an option")),
        }
    }
    spec.expect = match reader.word("the expected end")?.as_str() {
        "done" => {
            let (alternative, payload) = reader.outcome()?;
            Expect::Done {
                alternative,
                payload,
            }
        }
        "stuck" if !spec.erased => Expect::Stuck {
            state: reader.word("a stuck state, X1 to X10")?,
            clause: reader.word("a clause")?,
        },
        "undefined" if spec.erased => Expect::Other("undefined"),
        "violation" => Expect::Other("violation"),
        "host-failed" => Expect::Other("host-failed"),
        "fuel" => Expect::Other("fuel"),
        "not-an-execution" => Expect::Other("not-an-execution"),
        other => return Err(format!("`{other}` is not an end of a run")),
    };
    if reader.peek_word() == Some("trace") {
        reader.at += 1;
        reader.need("[")?;
        let mut events = Vec::new();
        while !reader.mark("]") {
            if !events.is_empty() {
                reader.need(";")?;
            }
            let control = reader.word("a control")?;
            reader.need("(")?;
            let arguments = reader.values(")")?;
            reader.need("->")?;
            let (alternative, payload) = reader.outcome()?;
            events.push(Event {
                control,
                arguments,
                alternative,
                payload,
            });
        }
        spec.trace = Some(events);
    }
    if reader.at != reader.pieces.len() {
        return Err("text after the end of the directive".to_string());
    }
    Ok(Directive::Run(Box::new(spec)))
}

fn directives(source: &str) -> Result<Vec<Directive>, String> {
    // Join each directive with its continuation lines.
    let mut joined: Vec<(usize, String)> = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("//!").or(trimmed.strip_prefix("┄!")) else {
            continue;
        };
        let rest = rest.trim();
        let starts = ["run ", "erased ", "reject ", "check"]
            .iter()
            .any(|word| rest.starts_with(word));
        match joined.last_mut() {
            Some((_, text)) if !starts => {
                text.push(' ');
                text.push_str(rest);
            }
            _ => joined.push((index + 1, rest.to_string())),
        }
    }
    joined
        .into_iter()
        .map(|(line, text)| directive(line, &text).map_err(|error| format!("line {line}: {error}")))
        .collect()
}

/// What the directives of one file came to.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub runs: usize,
    /// The runs that were also carried out under the erased semantics.
    pub compared: usize,
    pub failures: Vec<String>,
    /// How often each rule of the semantics was applied, over all runs.
    pub rules: BTreeMap<&'static str, u64>,
    /// The stuck states that runs ended in, as `X7 E-Arith`.
    pub stuck: BTreeSet<String>,
    /// The clause by which the program was refused, when a directive asked.
    pub rejected: Option<String>,
}

fn kind(end: &End) -> &'static str {
    match end {
        End::Done { .. } => "done",
        End::Stuck(_) => "stuck",
        End::HostViolation(_) => "violation",
        End::HostFailed(_) => "host-failed",
        End::OutOfFuel => "fuel",
        End::NotAnExecution(_) => "not-an-execution",
    }
}

fn events(trace: &[Event]) -> String {
    let shown: Vec<String> = trace.iter().map(Event::to_string).collect();
    shown.join("; ")
}

/// Reads a core text file and carries out its directives.
pub fn check(source: &str) -> Report {
    check_with(source, false)
}

/// Reads a Rails surface file, elaborates it into the core form, and carries
/// out its directives.
pub fn check_surface(source: &str) -> Report {
    check_with(source, true)
}

fn check_with(source: &str, is_surface: bool) -> Report {
    let mut report = Report::default();
    let directives = match directives(source) {
        Ok(directives) => directives,
        Err(error) => {
            report.failures.push(error);
            return report;
        }
    };
    let rejection = directives.iter().find_map(|directive| match directive {
        Directive::Reject { line, clause } => Some((*line, clause.clone())),
        Directive::Run(_) | Directive::Check { .. } => None,
    });
    let tree = if is_surface {
        match (surface::elaborate(source), &rejection) {
            (Ok(tree), _) => tree,
            (Err(error), Some((_, clause))) if error.clause == *clause => {
                report.rejected = Some(clause.clone());
                if directives.len() > 1 {
                    report
                        .failures
                        .push("a refused program has no runs".to_string());
                }
                return report;
            }
            (Err(error), Some((line, clause))) => {
                report.failures.push(format!(
                    "line {line}: expected a refusal by [{clause}], got: {error}"
                ));
                return report;
            }
            (Err(error), None) => {
                report
                    .failures
                    .push(format!("the program is refused: {error}"));
                return report;
            }
        }
    } else {
        match text::parse(source) {
            Ok(tree) => tree,
            Err(error) => {
                report
                    .failures
                    .push(format!("the text is not read: {error}"));
                return report;
            }
        }
    };
    let program = match (resolve::resolve(&tree), rejection) {
        (Err(error), Some((_, clause))) if error.clause == clause => {
            report.rejected = Some(clause);
            if directives.len() > 1 {
                report
                    .failures
                    .push("a refused program has no runs".to_string());
            }
            return report;
        }
        (Err(error), Some((line, clause))) => {
            report.failures.push(format!(
                "line {line}: expected a refusal by [{clause}], got: {error}"
            ));
            return report;
        }
        (Err(error), None) => {
            report
                .failures
                .push(format!("the program is refused: {error}"));
            return report;
        }
        (Ok(program), Some((line, clause))) => match check::check(&program) {
            Err(error) if error.clause == clause => {
                report.rejected = Some(clause);
                if directives.len() > 1 {
                    report
                        .failures
                        .push("a refused program has no runs".to_string());
                }
                return report;
            }
            Err(error) => {
                report.failures.push(format!(
                    "line {line}: expected a refusal by [{clause}], got: {error}"
                ));
                return report;
            }
            Ok(_) => {
                report.failures.push(format!(
                    "line {line}: expected a refusal by [{clause}], and the program is accepted"
                ));
                return report;
            }
        },
        (Ok(program), None) => program,
    };
    for directive in &directives {
        if let Directive::Check { line } = directive
            && let Err(error) = check::check(&program)
        {
            report
                .failures
                .push(format!("line {line}: check failed: {error}"));
        }
    }
    for directive in directives {
        let Directive::Run(spec) = directive else {
            continue;
        };
        report.runs += 1;
        let mut host = Arithmetic(Script {
            answers: spec.answers.iter().cloned().collect(),
        });
        let host: &mut dyn Host = if spec.arithmetic {
            &mut host
        } else {
            &mut host.0
        };
        if spec.erased {
            let outcome = erased::run(
                &program,
                &spec.entry,
                spec.arguments.clone(),
                host,
                &spec.config,
            );
            let as_expected = match (&spec.expect, &outcome.end) {
                (
                    Expect::Done {
                        alternative,
                        payload,
                    },
                    erased::End::Done {
                        alternative: a,
                        payload: p,
                    },
                ) => alternative == a && payload == p,
                (Expect::Other(expected), end) => {
                    *expected
                        == match end {
                            erased::End::Done { .. } => "done",
                            erased::End::Undefined(_) => "undefined",
                            erased::End::HostFailed(_) => "host-failed",
                            erased::End::OutOfFuel => "fuel",
                            erased::End::NotAnExecution(_) => "not-an-execution",
                        }
                }
                _ => false,
            };
            if !as_expected {
                report.failures.push(format!(
                    "line {}: erased {}: expected {}, got: {}",
                    spec.line, spec.entry, spec.expect, outcome.end
                ));
            } else if let Some(expected) = &spec.trace
                && *expected != outcome.trace
            {
                report.failures.push(format!(
                    "line {}: erased {}: the trace is [{}]",
                    spec.line,
                    spec.entry,
                    events(&outcome.trace)
                ));
            }
            continue;
        }
        let compared = compare(
            &program,
            &spec.entry,
            spec.arguments.clone(),
            host,
            &spec.config,
        );
        match &compared.verdict {
            Verdict::Same => report.compared += 1,
            Verdict::NotCompared => {}
            Verdict::Differ(difference) => report.failures.push(format!(
                "line {}: run {}: {difference}",
                spec.line, spec.entry
            )),
        }
        let outcome = compared.oracle;
        for (clause, count) in &outcome.rules {
            *report.rules.entry(clause).or_insert(0) += count;
        }
        let as_expected = match (&spec.expect, &outcome.end) {
            (
                Expect::Done {
                    alternative,
                    payload,
                },
                End::Done {
                    alternative: a,
                    payload: p,
                },
            ) => alternative == a && payload == p,
            (Expect::Stuck { state, clause }, End::Stuck(stuck)) => {
                report
                    .stuck
                    .insert(format!("X{} {}", stuck.state, stuck.clause));
                *state == format!("X{}", stuck.state) && clause == stuck.clause
            }
            (Expect::Other(expected), end) => *expected == kind(end),
            _ => false,
        };
        if !as_expected {
            report.failures.push(format!(
                "line {}: run {}: expected {}, got: {}",
                spec.line, spec.entry, spec.expect, outcome.end
            ));
            continue;
        }
        if let Some(expected) = &spec.trace
            && *expected != outcome.trace
        {
            report.failures.push(format!(
                "line {}: run {}: the trace is [{}]",
                spec.line,
                spec.entry,
                events(&outcome.trace)
            ));
        }
    }
    report
}
