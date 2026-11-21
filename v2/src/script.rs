//! Test directives: runs written beside a program, and what each must yield.

use crate::differential::{Verdict, compare};
use crate::host::{Answer, Arithmetic, Host, Script};
use crate::int::Int;
use crate::oracle::{Config, End, Event, Value};
use crate::typed::Program;
use crate::{check, erased, resolve, surface, text};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Word(String),
    Str(String),
    Mark(&'static str),
}

fn pieces(text: &str) -> Result<Vec<Piece>, String> {
    const MARKS: [&str; 14] = [
        "->", "=>", "|->", "↦", "(", ")", "{", "}", "[", "]", ",", ";", ".", "#",
    ];
    let characters: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut index = 0;
    'next: while index < characters.len() {
        if characters[index].is_whitespace() {
            index += 1;
            continue;
        }
        if characters[index] == '∅' {
            out.push(Piece::Mark("∅"));
            index += 1;
            continue;
        }
        if characters[index] == '"' {
            index += 1;
            let mut s = String::new();
            let mut closed = false;
            while index < characters.len() {
                let ch = characters[index];
                if ch == '"' {
                    index += 1;
                    closed = true;
                    break;
                }
                if ch == '\\' {
                    index += 1;
                    let Some(&esc) = characters.get(index) else {
                        return Err("unterminated escape in directive string".to_string());
                    };
                    match esc {
                        '"' => s.push('"'),
                        '\\' => s.push('\\'),
                        'n' => s.push('\n'),
                        'r' => s.push('\r'),
                        't' => s.push('\t'),
                        other => s.push(other),
                    }
                    index += 1;
                } else {
                    s.push(ch);
                    index += 1;
                }
            }
            if !closed {
                return Err("unterminated string in directive".to_string());
            }
            out.push(Piece::Str(s));
            continue;
        }
        let rest: String = characters[index..].iter().take(3).collect();
        for mark in MARKS {
            if rest.starts_with(mark) {
                out.push(Piece::Mark(mark));
                index += mark.chars().count();
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

#[derive(Clone, Debug)]
enum RawValue {
    Bool(bool),
    Int(Int),
    String(String),
    Vector(Vec<RawValue>),
    Map(Vec<(RawValue, RawValue)>),
    Struct {
        name: String,
        fields: Vec<RawValue>,
    },
    Variant {
        name: String,
        ctor: String,
        fields: Vec<RawValue>,
    },
    Resource {
        name: String,
        id: u64,
    },
}

fn resolve_raw_value(program: &Program, raw: &RawValue) -> Result<Value, String> {
    match raw {
        RawValue::Bool(b) => Ok(Value::Bool(*b)),
        RawValue::Int(i) => Ok(Value::Int(i.clone())),
        RawValue::String(s) => Ok(Value::String(s.clone())),
        RawValue::Vector(items) => Ok(Value::Vector(
            items
                .iter()
                .map(|i| resolve_raw_value(program, i))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        RawValue::Map(entries) => {
            let mut map = BTreeMap::new();
            for (k, v) in entries {
                map.insert(
                    resolve_raw_value(program, k)?,
                    resolve_raw_value(program, v)?,
                );
            }
            Ok(Value::Map(map))
        }
        RawValue::Struct { name, fields } => {
            let Some(sid) = program.structs.iter().position(|s| s.name == *name) else {
                return Err(format!("`{name}` is not a struct in this program"));
            };
            let vals = fields
                .iter()
                .map(|f| resolve_raw_value(program, f))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Struct {
                ty: sid,
                name: name.clone(),
                fields: vals,
            })
        }
        RawValue::Variant { name, ctor, fields } => {
            let Some(vid) = program.variants.iter().position(|v| v.name == *name) else {
                return Err(format!("`{name}` is not a variant in this program"));
            };
            let Some(cidx) = program.variants[vid]
                .constructors
                .iter()
                .position(|c| c.name == *ctor)
            else {
                return Err(format!("`{name}` has no constructor `{ctor}`"));
            };
            let vals = fields
                .iter()
                .map(|f| resolve_raw_value(program, f))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Variant {
                ty: vid,
                name: name.clone(),
                constructor: cidx,
                constructor_name: ctor.clone(),
                fields: vals,
            })
        }
        RawValue::Resource { name, id } => {
            let Some(rid) = program.resources.iter().position(|r| r.name == *name) else {
                return Err(format!("`{name}` is not a resource in this program"));
            };
            Ok(Value::Resource {
                ty: rid,
                name: name.clone(),
                id: *id,
            })
        }
    }
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

    fn peek_mark(&self, mark: &str) -> bool {
        matches!(self.pieces.get(self.at), Some(Piece::Mark(found)) if *found == mark)
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

    fn dotted_word(&mut self, what: &str) -> Result<String, String> {
        let mut out = self.word(what)?;
        while self.mark(".") {
            out.push('.');
            out.push_str(&self.word(what)?);
        }
        Ok(out)
    }

    fn string_lit(&mut self, what: &str) -> Result<String, String> {
        match self.pieces.get(self.at) {
            Some(Piece::Str(s)) => {
                self.at += 1;
                Ok(s.clone())
            }
            _ => Err(format!("expected {what}")),
        }
    }

    fn value(&mut self) -> Result<RawValue, String> {
        if let Some(Piece::Str(s)) = self.pieces.get(self.at).cloned() {
            self.at += 1;
            return Ok(RawValue::String(s));
        }
        if self.mark("∅") {
            return Ok(RawValue::Map(Vec::new()));
        }
        if self.mark("[") {
            let items = self.values("]")?;
            return Ok(RawValue::Vector(items));
        }
        if self.mark("{") {
            let mut entries = Vec::new();
            while !self.mark("}") {
                if !entries.is_empty() {
                    self.need(",")?;
                }
                let k = self.value()?;
                if !self.mark("↦") && !self.mark("|->") && !self.mark("->") {
                    return Err("expected `↦` or `->` in map literal".to_string());
                }
                let v = self.value()?;
                entries.push((k, v));
            }
            return Ok(RawValue::Map(entries));
        }
        let word = self.word("a value")?;
        if word == "map" && self.peek_mark("{") {
            self.need("{")?;
            let mut entries = Vec::new();
            while !self.mark("}") {
                if !entries.is_empty() {
                    self.need(",")?;
                }
                let k = self.value()?;
                if !self.mark("↦") && !self.mark("|->") && !self.mark("->") {
                    return Err("expected `↦` or `->` in map literal".to_string());
                }
                let v = self.value()?;
                entries.push((k, v));
            }
            return Ok(RawValue::Map(entries));
        }
        if self.mark("#") {
            let id_str = self.word("a resource handle id")?;
            let id: u64 = id_str
                .parse()
                .map_err(|_| format!("`{id_str}` is not a resource id"))?;
            return Ok(RawValue::Resource { name: word, id });
        }
        if self.mark(".") {
            let ctor = self.word("a variant constructor")?;
            let fields = if self.mark("{") {
                self.values("}")?
            } else {
                Vec::new()
            };
            return Ok(RawValue::Variant {
                name: word,
                ctor,
                fields,
            });
        }
        if self.mark("{") {
            let fields = self.values("}")?;
            return Ok(RawValue::Struct { name: word, fields });
        }
        match word.as_str() {
            "true" => Ok(RawValue::Bool(true)),
            "false" => Ok(RawValue::Bool(false)),
            digits => Int::parse(digits)
                .map(RawValue::Int)
                .ok_or(format!("`{digits}` is not a value")),
        }
    }

    fn values(&mut self, close: &str) -> Result<Vec<RawValue>, String> {
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
    fn outcome(&mut self) -> Result<(String, Vec<RawValue>), String> {
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
enum RawExpect {
    Done {
        alternative: String,
        payload: Vec<RawValue>,
    },
    Faulted(String),
    Stuck {
        state: String,
        clause: String,
    },
    Other(&'static str),
}

#[derive(Clone, Debug)]
enum Expect {
    Done {
        alternative: String,
        payload: Vec<Value>,
    },
    Faulted(String),
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
            Expect::Faulted(msg) => write!(formatter, "faulted \"{msg}\""),
            Expect::Stuck { state, clause } => write!(formatter, "stuck {state} {clause}"),
            Expect::Other(kind) => formatter.write_str(kind),
        }
    }
}

#[derive(Clone, Debug)]
struct RawEvent {
    control: String,
    arguments: Vec<RawValue>,
    alternative: String,
    payload: Vec<RawValue>,
}

#[derive(Clone, Debug)]
struct RawRunSpec {
    line: usize,
    erased: bool,
    entry: String,
    arguments: Vec<RawValue>,
    arithmetic: bool,
    answers: Vec<(String, String, Vec<RawValue>)>,
    config: Config,
    expect: RawExpect,
    trace: Option<Vec<RawEvent>>,
}

#[derive(Clone, Debug)]
enum Directive {
    Run(Box<RawRunSpec>),
    Reject { line: usize, clause: String },
    Check { line: usize },
    Warn { line: usize, clause: String },
}

fn directive(line: usize, text: &str) -> Result<Directive, String> {
    let mut reader = Reader {
        pieces: pieces(text)?,
        at: 0,
    };
    let kind = reader.word("`run`, `erased`, `check`, `warn` or `reject`")?;
    if kind == "check" {
        if reader.at != reader.pieces.len() {
            return Err("text after `check`".to_string());
        }
        return Ok(Directive::Check { line });
    }
    if kind == "warn" {
        let clause = reader.word("a warning clause")?;
        if reader.at != reader.pieces.len() {
            return Err("text after `warn` clause".to_string());
        }
        return Ok(Directive::Warn { line, clause });
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
    let mut spec = RawRunSpec {
        line,
        erased: kind == "erased",
        entry,
        arguments,
        arithmetic: false,
        answers: Vec::new(),
        config: Config::default(),
        expect: RawExpect::Other("fuel"),
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
                    spec.answers.push((control, alternative, payload));
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
            RawExpect::Done {
                alternative,
                payload,
            }
        }
        "faulted" => RawExpect::Faulted(reader.string_lit("a fault message string")?),
        "stuck" if !spec.erased => RawExpect::Stuck {
            state: reader.word("a stuck state, X1 to X10")?,
            clause: reader.word("a clause")?,
        },
        "undefined" if spec.erased => RawExpect::Other("undefined"),
        "violation" => RawExpect::Other("violation"),
        "host-failed" => RawExpect::Other("host-failed"),
        "fuel" => RawExpect::Other("fuel"),
        "not-an-execution" => RawExpect::Other("not-an-execution"),
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
            let control = reader.dotted_word("a control")?;
            reader.need("(")?;
            let arguments = reader.values(")")?;
            reader.need("->")?;
            let (alternative, payload) = reader.outcome()?;
            events.push(RawEvent {
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
    let mut joined: Vec<(usize, String)> = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("//!").or(trimmed.strip_prefix("┄!")) else {
            continue;
        };
        let rest = rest.trim();
        let starts = ["run ", "erased ", "reject ", "warn ", "check"]
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
    pub compared: usize,
    pub failures: Vec<String>,
    pub rules: BTreeMap<&'static str, u64>,
    pub stuck: BTreeSet<String>,
    pub rejected: Option<String>,
}

fn kind(end: &End) -> &'static str {
    match end {
        End::Done { .. } => "done",
        End::Faulted(_) => "faulted",
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
        Directive::Run(_) | Directive::Check { .. } | Directive::Warn { .. } => None,
    });
    let mut all_warnings: Vec<surface::Warning> = Vec::new();
    let tree = if is_surface {
        match (surface::elaborate_with_warnings(source), &rejection) {
            (Ok(elaborated), _) => {
                all_warnings.extend(elaborated.warnings);
                elaborated.program
            }
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
    let needs_checker = directives
        .iter()
        .any(|d| matches!(d, Directive::Check { .. } | Directive::Warn { .. }));
    if needs_checker {
        match check::check(&program) {
            Ok(checked) => {
                all_warnings.extend(checked.warnings);
            }
            Err(error) => {
                for directive in &directives {
                    if let Directive::Check { line } | Directive::Warn { line, .. } = directive {
                        report
                            .failures
                            .push(format!("line {line}: check failed: {error}"));
                    }
                }
            }
        }
    }
    for directive in &directives {
        if let Directive::Warn { line, clause } = directive {
            let matched = all_warnings.iter().any(|w| {
                w.clause == clause.as_str()
                    || (clause == "U18" && w.clause == "U5")
                    || (clause == "U5" && w.clause == "U18")
            });
            if !matched {
                report.failures.push(format!(
                    "line {line}: expected a warning by [{clause}], got {:?}",
                    all_warnings
                ));
            }
        }
    }
    for directive in directives {
        let Directive::Run(raw_spec) = directive else {
            continue;
        };
        report.runs += 1;
        let arguments = match raw_spec
            .arguments
            .iter()
            .map(|v| resolve_raw_value(&program, v))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(args) => args,
            Err(err) => {
                report
                    .failures
                    .push(format!("line {}: {err}", raw_spec.line));
                continue;
            }
        };
        let mut answers = Vec::with_capacity(raw_spec.answers.len());
        let mut bad_answer = None;
        for (ctrl, alt, raw_payload) in &raw_spec.answers {
            match raw_payload
                .iter()
                .map(|v| resolve_raw_value(&program, v))
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(payload) => answers.push((
                    ctrl.clone(),
                    Answer {
                        alternative: alt.clone(),
                        payload,
                    },
                )),
                Err(err) => {
                    bad_answer = Some(err);
                    break;
                }
            }
        }
        if let Some(err) = bad_answer {
            report
                .failures
                .push(format!("line {}: {err}", raw_spec.line));
            continue;
        }
        let expect = match &raw_spec.expect {
            RawExpect::Done {
                alternative,
                payload,
            } => match payload
                .iter()
                .map(|v| resolve_raw_value(&program, v))
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(p) => Expect::Done {
                    alternative: alternative.clone(),
                    payload: p,
                },
                Err(err) => {
                    report
                        .failures
                        .push(format!("line {}: {err}", raw_spec.line));
                    continue;
                }
            },
            RawExpect::Faulted(m) => Expect::Faulted(m.clone()),
            RawExpect::Stuck { state, clause } => Expect::Stuck {
                state: state.clone(),
                clause: clause.clone(),
            },
            RawExpect::Other(s) => Expect::Other(s),
        };
        let trace = match &raw_spec.trace {
            None => None,
            Some(raw_events) => {
                let mut evs = Vec::with_capacity(raw_events.len());
                let mut err_msg = None;
                for re in raw_events {
                    let args = re
                        .arguments
                        .iter()
                        .map(|v| resolve_raw_value(&program, v))
                        .collect::<Result<Vec<_>, _>>();
                    let pay = re
                        .payload
                        .iter()
                        .map(|v| resolve_raw_value(&program, v))
                        .collect::<Result<Vec<_>, _>>();
                    match (args, pay) {
                        (Ok(a), Ok(p)) => evs.push(Event {
                            control: re.control.clone(),
                            arguments: a,
                            alternative: re.alternative.clone(),
                            payload: p,
                        }),
                        (Err(e), _) | (_, Err(e)) => {
                            err_msg = Some(e);
                            break;
                        }
                    }
                }
                if let Some(e) = err_msg {
                    report.failures.push(format!("line {}: {e}", raw_spec.line));
                    continue;
                }
                Some(evs)
            }
        };

        let mut host = Arithmetic(Script {
            answers: answers.into_iter().collect(),
        });
        let host: &mut dyn Host = if raw_spec.arithmetic {
            &mut host
        } else {
            &mut host.0
        };
        if raw_spec.erased {
            let outcome = erased::run(&program, &raw_spec.entry, arguments, host, &raw_spec.config);
            let as_expected = match (&expect, &outcome.end) {
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
                (Expect::Faulted(m1), erased::End::Faulted(m2)) => m1 == m2,
                (Expect::Other(expected), end) => {
                    *expected
                        == match end {
                            erased::End::Done { .. } => "done",
                            erased::End::Faulted(_) => "faulted",
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
                    raw_spec.line, raw_spec.entry, expect, outcome.end
                ));
            } else if let Some(expected) = &trace
                && *expected != outcome.trace
            {
                report.failures.push(format!(
                    "line {}: erased {}: the trace is [{}]",
                    raw_spec.line,
                    raw_spec.entry,
                    events(&outcome.trace)
                ));
            }
            continue;
        }
        let compared = compare(&program, &raw_spec.entry, arguments, host, &raw_spec.config);
        match &compared.verdict {
            Verdict::Same => report.compared += 1,
            Verdict::NotCompared => {}
            Verdict::Differ(difference) => report.failures.push(format!(
                "line {}: run {}: {difference}",
                raw_spec.line, raw_spec.entry
            )),
        }
        let outcome = compared.oracle;
        for (clause, count) in &outcome.rules {
            *report.rules.entry(clause).or_insert(0) += count;
        }
        let as_expected = match (&expect, &outcome.end) {
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
            (Expect::Faulted(m1), End::Faulted(m2)) => m1 == m2,
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
                raw_spec.line, raw_spec.entry, expect, outcome.end
            ));
            continue;
        }
        if let Some(expected) = &trace
            && *expected != outcome.trace
        {
            report.failures.push(format!(
                "line {}: run {}: the trace is [{}]",
                raw_spec.line,
                raw_spec.entry,
                events(&outcome.trace)
            ));
        }
    }
    report
}
