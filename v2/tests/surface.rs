//! Conformance and clause-coverage tests for the Rails surface reader and
//! elaborator (`docs/v2/surface.md`).
//!
//! Every layout (`[Y1]` to `[Y10]`), grammar (`[G1]` to `[G9]`) and
//! elaboration (`[U1]` to `[U4]`, `[U6]` to `[U9]`, `[U13]` to `[U15]`) clause
//! of slice 1 has both an accepting test (elaborated, resolved and executed in
//! the oracle and under the erased semantics) and a rejecting test (refused
//! with the clause's identifier).

use std::collections::BTreeSet;
use std::path::PathBuf;
use villanelle_v2::{script, surface};

const SURFACE_CLAUSES: [&str; 29] = [
    "Y1", "Y2", "Y3", "Y4", "Y5", "Y6", "Y7", "Y8", "Y9", "Y10", "G1", "G2", "G3", "G4", "G5",
    "G6", "G8", "G9", "U1", "U2", "U3", "U4", "U6", "U7", "U8", "U9", "U13", "U14", "U15",
];

fn surface_files() -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/slice1_surface");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).expect("tests/slice1_surface") {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|ext| ext == "rails") {
            let name = path
                .file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned();
            out.push((name, std::fs::read_to_string(&path).expect("readable")));
        }
    }
    out.sort();
    assert!(!out.is_empty());
    out
}

#[test]
fn surface_files_run_and_agree_with_erased_semantics() {
    let mut runs = 0;
    let mut compared = 0;
    let mut failures = Vec::new();
    for (name, source) in surface_files() {
        let report = script::check_surface(&source);
        if report.runs == 0 && report.rejected.is_none() {
            failures.push(format!("{name}: no directives"));
        }
        for failure in report.failures {
            failures.push(format!("{name}: {failure}"));
        }
        runs += report.runs;
        compared += report.compared;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(runs >= 14 && compared == runs);
}

#[test]
fn every_surface_clause_of_slice_one_accepts_and_rejects() {
    let accepting: &[(&[&str], &str)] = &[
        // Y1, Y2, Y3, Y8, Y9, G1
        (
            &["Y1", "Y2", "Y3", "Y8", "Y9", "G1"],
            "\
control add(a: u32, b: u32)
    ■ sum {value: u32}
    ■ zero
│ ┄ comment on rail
│
│ x: u32 := a +
│     b
│ ? x = 0 → ■ zero
│ ■ sum {x}
╵
//! run add(2, 3) => done sum {5}
//! run add(0, 0) => done zero
",
        ),
        // Y4, Y5, Y6, G5, G6, U4, U9
        (
            &["Y4", "Y5", "Y6", "G5", "G6", "U4", "U9"],
            "\
extern control probe(x: u32) ■ hit │ miss

control count_down(n: u32) ■ ended {steps: u32}
│ Δ probe(n): hit │ miss
│ ↻ step(rem: u32 := n, ~acc: u32 := 0)
│ ║ ⊢ rem ≤ n
│ ║ ? rem = 0 →
│ ║ ┆ □ done {acc}
│ ╠═ ¬miss ∧ hit: acc ← acc + 1
│ ╠═ miss
│ ║ ┆ acc ← acc + 2
│ ║ ↑ step(rem - 1, acc)
╞═ done: ■ ended {acc}
╵
//! run count_down(2) with [probe -> hit] => done ended {2}
//! run count_down(2) with [probe -> miss] => done ended {4}
",
        ),
        // Y7, U1, U2, U3, U6, U7, U8, U15
        (
            &["Y7", "U1", "U2", "U3", "U6", "U7", "U8", "U15"],
            "\
extern control ping(x: u32) ■ ok {v: u32} │ err
extern control log(x: u32) ■ logged

control check(x: u32, flag: bool) ■ out {v: u32} │ bad
│ first : Δ ping(x)
│ ├ ok {v: u32} → got
│ └ err →
│   ┆ log(x)
│   ┆ ■ bad
│ second : Δ ping(x): ok {w} → got │ err → ■ bad
│ Δ flag: true → high │ false → low
╞═ first.ok ∧ second.ok ∧ (high ∨ low) ∧ got
│ ┆ ■ out {x}
╵
//! run check(5, true) with [ping -> ok {5}; ping -> ok {5}] => done out {5}
//! run check(5, false) with [ping -> err; log -> logged] => done bad
",
        ),
        // G2, G3, G4, U13, U14
        (
            &["G2", "G3", "G4", "U13", "U14"],
            "\
outcome err_group: e1 ↦ 1 │ e2 {code: u32} ↦ 2

extern control step(x: u32) ■ good {v: u32} │ …err_group

control adapt(x: u32) ■ auto
│ ■ Δ step(x): good {v} → wrapped {v} │ …err_group
╵

control caller(x: u32) ■ wrapped {v: u32} │ …err_group
│ forward adapt(x)
╵
//! run caller(3) with [step -> good {9}] => done wrapped {9}
//! run caller(3) with [step -> e2 {4}] => done e2 {4}
",
        ),
        // G8, G9, Y10
        (
            &["G8", "G9", "Y10"],
            "\
function inc(x: u32) where x < 100 : u32 := x + 1

outcome res { ok {v: u32} |-> 0, fail |-> 1 }

extern control ask(x: u32) exits ..res

control run_plain(x: u32) exits ..res {
    mutable y: u32 := inc(x)
    delta ask(y) { ok {v} => got, fail => exit fail }
    [got]: {
        if v != 0 && v <= 50 => exit ok {v}
        exit fail
    }
}
//! run run_plain(4) with [ask -> ok {5}] => done ok {5}
",
        ),
    ];

    let mut accepted_clauses = BTreeSet::new();
    for (clauses, source) in accepting {
        let report = script::check_surface(source);
        assert!(
            report.failures.is_empty(),
            "accepting test for {clauses:?} failed: {:?}\n{source}",
            report.failures
        );
        assert!(report.runs > 0);
        for clause in *clauses {
            accepted_clauses.insert(*clause);
        }
    }

    let rejecting: &[(&str, &str)] = &[
        // Y1: declaration indented or continuation indented < 4 spaces
        ("Y1", "  control bad() ■ ok\n│ ■ ok\n╵\n//! reject Y1\n"),
        // Y2: rail character without trailing space
        (
            "Y2",
            "\
control bad() ■ ok
│■ ok
╵
//! reject Y2
",
        ),
        // Y3: control body not closed with `╵`
        (
            "Y3",
            "\
control bad() ■ ok
│ ■ ok
//! reject Y3
",
        ),
        // Y4: loop header without `║` body lines
        (
            "Y4",
            "\
control bad() ■ ok
│ ↻ spin
│ ■ ok
╵
//! reject Y4
",
        ),
        // Y5: label inside a `┆` test block
        (
            "Y5",
            "\
extern control ask() ■ a │ b

control bad(x: u32) ■ ok
│ Δ ask(): a │ b
│ ? x = 0 →
│ ┆ ╞═ a: ■ ok
│ ■ ok
╵
//! reject Y5
",
        ),
        // Y6: test `?` without `→`
        (
            "Y6",
            "\
control bad(x: u32) ■ ok
│ ? x = 0
│ ■ ok
╵
//! reject Y6
",
        ),
        // Y7: vertical decision arms not ending with `└`
        (
            "Y7",
            "\
extern control ask() ■ a │ b

control bad() ■ ok
│ Δ ask()
│ ├ a → ■ ok
│ ├ b → ■ ok
╵
//! reject Y7
",
        ),
        // Y8: continuation line at the start of a body with no statement above it
        (
            "Y8",
            "\
control bad() ■ ok
│   ■ ok
╵
//! reject Y8
",
        ),
        // Y9: blank line inside a body without its rail prefix
        (
            "Y9",
            "\
control bad() ■ ok
│ x := 1

│ ■ ok
╵
//! reject Y9
",
        ),
        // Y10: rail prefix `│ ┆ ` when no label, test or arm block is open
        (
            "Y10",
            "\
control bad() ■ ok
│ ┆ ■ ok
╵
//! reject Y10
",
        ),
        // G1: empty contract
        (
            "G1",
            "\
control bad() ■
│ ■ ok
╵
//! reject G1
",
        ),
        // G2: `…unknown` outcome group in contract
        (
            "G2",
            "\
control bad() ■ …unknown_group
│ ■ ok
╵
//! reject G2
",
        ),
        // G3: `■ auto` on a `pub` control
        (
            "G3",
            "\
pub control bad() ■ auto
│ ■ ok
╵
//! reject G3
",
        ),
        // G4: `↦` on a control contract instead of an `outcome` declaration
        (
            "G4",
            "\
control bad() ■ ok ↦ 0
│ ■ ok
╵
//! reject G4
",
        ),
        // G5: `¬` applied to a parenthesized mask `¬(a ∧ b)`
        (
            "G5",
            "\
extern control ask() ■ a │ b

control bad() ■ ok
│ Δ ask(): a │ b
╞═ ¬(a ∧ b): ■ ok
│ ■ ok
╵
//! reject G5
",
        ),
        // G6: first `⊢` line in loop body is the loop's invariant (stuck at X7 E-Again when violated)
        // and for syntax rejection, a malformed token in G8 / unclosed brace in G9:
        (
            "G8",
            "\
control bad() ■ ok
│ x := @
│ ■ ok
╵
//! reject G8
",
        ),
        (
            "G9",
            "\
control bad() exits ok {
    exit ok
//! reject G9
",
        ),
        // U1: two decisions share default site name `ask` and a label qualifies `ask.a`
        (
            "U1",
            "\
extern control ask() ■ a │ b

control bad() ■ ok
│ Δ ask(): a │ b
│ Δ ask(): a │ b
╞═ ask.a: ■ ok
│ ■ ok
╵
//! reject U1
",
        ),
        // U2: referencing original alternative name `a` after it was renamed by a route `a → routed`
        (
            "U2",
            "\
extern control ask() ■ a │ b

control bad() ■ ok
│ Δ ask(): a → routed │ b → ■ ok
╞═ a: ■ ok
╵
//! reject U2
",
        ),
        // U3: unqualified `failed` when two decisions have `failed` by default
        (
            "U3",
            "\
extern control first() ■ ok │ failed
extern control second() ■ ok │ failed

control bad() ■ done
│ Δ first(): ok │ failed
│ Δ second(): ok │ failed
╞═ failed: ■ done
│ ■ done
╵
//! reject U3
",
        ),
        // U4: mask references a path of a decision that appears after the label
        (
            "U4",
            "\
extern control ask() ■ a │ b

control bad() ■ ok
╞═ a: ■ ok
│ Δ ask(): a │ b
│ ■ ok
╵
//! reject U4
",
        ),
        // U6: calling a multi-outcome operation without `Δ`
        (
            "U6",
            "\
extern control ask() ■ a │ b

control bad() ■ ok
│ ask()
│ ■ ok
╵
//! reject U6
",
        ),
        // U7: decision omits an alternative of the operation's contract
        (
            "U7",
            "\
extern control ask() ■ a │ b

control bad() ■ ok
│ Δ ask(): a
│ ■ ok
╵
//! reject U7
",
        ),
        // U8: arm pattern has wrong number of payload fields
        (
            "U8",
            "\
extern control ask() ■ a {x: u32}

control bad() ■ ok
│ Δ ask(): a {x, y}
│ ■ ok
╵
//! reject U8
",
        ),
        // U9: loop mixes bare `□` with named `□ done`
        (
            "U9",
            "\
control bad(n: u32) ■ ok
│ ↻ step(i: u32 := n)
│ ║ ? i = 0 → □
│ ║ ? i = 1 → □ done
│ ║ ↑ step(i - 1)
│ ■ ok
╵
//! reject U9
",
        ),
        // U13: adapter `■ Δ` arm opens a block instead of exiting
        (
            "U13",
            "\
extern control ask() ■ a │ b

control bad() ■ a │ b
│ ■ Δ ask()
│ ├ a →
│ │ ┆ x := 1
│ │ ┆ ■ a
│ └ b
╵
//! reject U13
",
        ),
        // U14: `forward` where callee includes group `…err` and caller lists alternatives individually
        (
            "U14",
            "\
outcome err: e1 │ e2

extern control step() ■ ok │ …err

control bad() ■ ok │ e1 │ e2
│ forward step()
╵
//! reject U14
",
        ),
        // U15: exit with an alternative not in the control's contract
        (
            "U15",
            "\
control bad() ■ ok
│ ■ missing
╵
//! reject U15
",
        ),
    ];

    let mut rejected_clauses = BTreeSet::new();
    for (clause, source) in rejecting {
        let report = script::check_surface(source);
        assert!(
            report.failures.is_empty(),
            "rejecting test for [{clause}] failed: {:?}\n{source}",
            report.failures
        );
        assert_eq!(
            report.rejected.as_deref(),
            Some(*clause),
            "expected rejection by [{clause}]\n{source}"
        );
        rejected_clauses.insert(*clause);
    }

    // [G6] negative test: the first `⊢` line of a loop body is the loop's
    // invariant, so when it is false on `↑`, the oracle is stuck at `X7 E-Again`
    // (whereas a second `⊢` line is an obligation, stuck at `X7 E-Oblige`).
    let g6_check = "\
control inv_fails(n: u32) ■ ok
│ ↻ step(i: u32 := 0)
│ ║ ⊢ i < 1
│ ║ ⊢ i < 10
│ ║ ↑ step(i + 1)
╵
//! run inv_fails(2) => stuck X7 E-Again
";
    let g6_report = script::check_surface(g6_check);
    assert!(g6_report.failures.is_empty(), "{:?}", g6_report.failures);
    rejected_clauses.insert("G6");

    for clause in SURFACE_CLAUSES {
        assert!(
            accepted_clauses.contains(clause),
            "no accepting surface test covers [{clause}]"
        );
        assert!(
            rejected_clauses.contains(clause),
            "no rejecting surface test covers [{clause}]"
        );
    }

    // Also verify `surface::elaborate` produces a tree whose printed core text
    // re-parses identically.
    for (name, source) in surface_files() {
        let tree = surface::elaborate(&source).unwrap_or_else(|e| panic!("{name}: {e}"));
        let printed = tree.print();
        let reparsed = villanelle_v2::text::parse(&printed)
            .unwrap_or_else(|e| panic!("{name}: core parse failed: {e}\n{printed}"));
        assert_eq!(printed, reparsed.print(), "{name}");
    }
}
