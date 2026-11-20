//! The conformance suite of the current slice.
//!
//! Every file under `tests/slice1` is a program in the core text form with
//! its directives. The suite carries them out, and then checks that the
//! files together reach what the slice must reach: every rule of the
//! semantics applied, every way of being stuck met, and every refusal of
//! `resolve` given, each by the clause it is named after.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use villanelle_v2::{script, text};

fn files() -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/slice1");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).expect("the directory of slice 1") {
        let path = entry.expect("an entry").path();
        if path
            .extension()
            .is_some_and(|extension| extension == "core")
        {
            let name = path
                .file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned();
            out.push((
                name,
                std::fs::read_to_string(&path).expect("a readable file"),
            ));
        }
    }
    out.sort();
    assert!(!out.is_empty());
    out
}

/// The rules of section 6 that a statement of slice 1 can apply.
const RULES: [&str; 12] = [
    "E-Skip", "E-Bind", "E-Assign", "E-Oblige", "E-Test", "E-Decide", "E-Call", "E-Exit",
    "E-Extern", "E-Loop", "E-Again", "E-Leave",
];

/// The ways of being stuck that slice 1 has, as the row of the table of
/// section 6.10 and the clause without a successor.
const STUCK: [&str; 15] = [
    "X1 E-End",
    "X2 E-Var",
    "X2 E-Assign",
    "X3 E-Bind",
    "X6 E-Impossible",
    "X7 E-Arith",
    "X7 E-Fn",
    "X7 E-Oblige",
    "X7 E-Call",
    "X7 E-Extern",
    "X7 E-Exit",
    "X7 E-Loop",
    "X7 E-Again",
    "X9 E-Effect",
    "X10 E-Decide",
];

/// The clauses by which `resolve` refuses a program.
const REFUSALS: [&str; 9] = ["W1", "W2", "W3", "W4", "W5", "W7", "C1", "C2", "C3"];

#[test]
fn slice_one_conforms() {
    let mut failures = Vec::new();
    let mut rules: BTreeMap<&str, u64> = BTreeMap::new();
    let mut stuck = BTreeSet::new();
    let mut refusals = BTreeSet::new();
    let mut runs = 0;
    for (name, source) in files() {
        let report = script::check(&source);
        if report.runs == 0 && report.rejected.is_none() && report.failures.is_empty() {
            failures.push(format!("{name}: the file has no directive"));
        }
        failures.extend(
            report
                .failures
                .iter()
                .map(|failure| format!("{name}: {failure}")),
        );
        runs += report.runs;
        for (clause, count) in report.rules {
            *rules.entry(clause).or_insert(0) += count;
        }
        stuck.extend(report.stuck);
        refusals.extend(report.rejected);
    }
    for rule in RULES {
        if !rules.contains_key(rule) {
            failures.push(format!("no run applies [{rule}]"));
        }
    }
    for state in STUCK {
        if !stuck.contains(state) {
            failures.push(format!("no run is stuck by {state}"));
        }
    }
    for clause in REFUSALS {
        if !refusals.contains(clause) {
            failures.push(format!("no program is refused by [{clause}]"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(runs > 0);
}

/// Printing a program and reading the print gives the same program.
#[test]
fn the_printer_and_the_reader_agree() {
    for (name, source) in files() {
        let first = text::parse(&source)
            .unwrap_or_else(|error| panic!("{name}: {error}"))
            .print();
        let second = text::parse(&first)
            .unwrap_or_else(|error| panic!("{name}: {error}"))
            .print();
        assert_eq!(first, second, "{name}");
    }
}
