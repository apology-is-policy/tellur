//! Slice 2 Phase 2 conformance suite: core semantics, erased differential parity
//! (`[R4]`, `[R5]`), Rails surface syntax (`[Y12]`, `[G4a]`, `[G4b]`, `[G7a]`,
//! `[G12]`, `[U6]`, `[U10]`, `[U11]`, `[U12]`, `[U14]`), and canonical formatter
//! idempotence (`[G10]`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use villanelle_v2::{script, surface, text};

fn core_files() -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/slice2");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).expect("tests/slice2") {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|ext| ext == "core") {
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

fn surface_files() -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/slice2_surface");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).expect("tests/slice2_surface") {
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

const SLICE2_RULES: [&str; 9] = [
    "E-Unpack",
    "E-Capture",
    "E-Outcome",
    "E-Fault",
    "E-Unwind",
    "E-Boundary",
    "E-Contingency",
    "E-Supervise",
    "E-Assign",
];

const SLICE2_STUCK: [&str; 9] = [
    "X2 E-Var",
    "X2 E-Field",
    "X2 E-Index",
    "X2 E-Supervise",
    "X5 E-Scope",
    "X5 E-Assign",
    "X5 E-Temp",
    "X7 E-Index",
    "X8 E-Fault",
];

const SLICE2_REFUSALS: [&str; 6] = ["T1", "W5a", "W6", "W7", "W8", "F6"];

#[test]
fn slice_two_core_conforms_and_matches_erased() {
    let mut failures = Vec::new();
    let mut rules: BTreeMap<&str, u64> = BTreeMap::new();
    let mut stuck = BTreeSet::new();
    let mut refusals = BTreeSet::new();
    let mut runs = 0usize;
    let mut compared = 0usize;

    for (name, source) in core_files() {
        let report = script::check(&source);
        if report.runs == 0 && report.rejected.is_none() && report.failures.is_empty() {
            failures.push(format!("{name}: no directives"));
        }
        for failure in report.failures {
            failures.push(format!("{name}: {failure}"));
        }
        runs += report.runs;
        compared += report.compared;
        for (rule, count) in report.rules {
            *rules.entry(rule).or_insert(0) += count;
        }
        stuck.extend(report.stuck);
        refusals.extend(report.rejected);
    }

    for rule in SLICE2_RULES {
        if !rules.contains_key(rule) {
            failures.push(format!("no slice 2 run applied [{rule}]"));
        }
    }
    for state in SLICE2_STUCK {
        if !stuck.contains(state) {
            failures.push(format!("no slice 2 run reached stuck state {state}"));
        }
    }
    if !stuck.contains("X8 E-Unwind") {
        failures.push("no slice 2 run reached stuck state X8 E-Unwind".to_string());
    }
    for clause in SLICE2_REFUSALS {
        if !refusals.contains(clause) {
            failures.push(format!("no slice 2 core file was refused by [{clause}]"));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(runs >= 25);
    // Every non-stuck run (including faulted runs with `on fault release`) is
    // differentially checked against the erased semantics ([R4], [R5]).
    assert!(compared >= 15 && compared < runs);
}

#[test]
fn slice_two_core_printer_and_reader_agree() {
    for (name, source) in core_files() {
        let first = text::parse(&source)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .print();
        let second = text::parse(&first)
            .unwrap_or_else(|e| panic!("{name}: {e}\n{first}"))
            .print();
        assert_eq!(first, second, "{name}");
    }
}

#[test]
fn slice_two_surface_files_run_format_and_match_erased() {
    let mut runs = 0usize;
    let mut compared = 0usize;
    let mut failures = Vec::new();

    for (name, source) in surface_files() {
        let report = script::check_surface(&source);
        if report.runs == 0 {
            failures.push(format!("{name}: no runs"));
        }
        for failure in report.failures {
            failures.push(format!("{name}: {failure}"));
        }
        runs += report.runs;
        compared += report.compared;

        // Canonical Rails formatting idempotence ([G10]).
        let formatted =
            surface::format(&source).unwrap_or_else(|e| panic!("{name}: format failed: {e}"));
        assert_eq!(
            formatted, source,
            "{name} is not in canonical Rails form ([G10])"
        );
        let again = surface::format(&formatted)
            .unwrap_or_else(|e| panic!("{name}: second format failed: {e}"));
        assert_eq!(formatted, again, "{name}: format not idempotent");
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(runs >= 12 && compared == runs);
}

#[test]
fn slice_two_surface_clauses_accept_and_reject() {
    // Plain-form + ASCII word spellings ([G8], [G9], [G4a], [G4b], [G7a], [G12], [U10], [U11], [U12], [U14]).
    let plain_source = "\
extern resource handle on fault release
extern control acquire() exits opened {h: handle}
extern control release_it(h: handle) exits closed

copy struct pair {a: u32, b: u32}

struct box_h {inner: handle}

control use_plain(xs: vector<u32>) exits ok {sum: u32} {
    mutable total: u32 := 0
    for item in xs {
        total <- total + item
    }
    mutable m: map<string, u32> := map {}
    delta m.insert(\"k\", total) { inserted => total <- total + 1, replaced {p} => impossible }
    delta acquire() { opened {h} }
    b: box_h := box_h {move h}
    box_h {h2} := move b
    release_it(move h2)
    exit ok {total}
}

boundary guarded(n: u32) retains n {
    if n == 0 => fault \"zero\"
    terminate
}
contingency (msg: string) {
    terminate
}

control call_guard(n: u32) exits done {
    supervise guarded(n)
    exit done
}

//! run use_plain([1, 2, 3]) with [acquire -> opened {handle#1}; release_it -> closed] => done ok {7}
//! run call_guard(0) => done done
//! run call_guard(5) => done done
";
    let report = script::check_surface(plain_source);
    assert!(
        report.failures.is_empty(),
        "plain slice 2 program failed: {:?}",
        report.failures
    );
    assert_eq!(report.runs, 3);
    assert_eq!(report.compared, 3);

    // Format plain form to canonical Rails and re-run to verify semantic equivalence.
    let formatted = surface::format(plain_source).expect("format plain slice 2");
    assert!(formatted.contains("∀ item ∈ xs"));
    assert!(formatted.contains("string ⇀ u32"));
    assert!(formatted.contains("∅"));
    assert!(formatted.contains("▾h"));
    assert_eq!(
        surface::format(&formatted).expect("format idempotent"),
        formatted
    );
    let formatted_with_directives = format!(
        "{formatted}\n//! run use_plain([1, 2, 3]) with [acquire -> opened {{handle#1}}; release_it -> closed] => done ok {{7}}\n//! run call_guard(0) => done done\n"
    );
    let formatted_report = script::check_surface(&formatted_with_directives);
    assert!(
        formatted_report.failures.is_empty(),
        "formatted slice 2 program failed: {:?}",
        formatted_report.failures
    );

    // Rejecting cases for every new Slice 2 surface clause.
    let rejecting: &[(&str, &str)] = &[
        // [Y12]: `boundary` not followed at once by `contingency`.
        (
            "Y12",
            "\
boundary lonely(x: u32)
│ terminate
╵

control ok() ■ done
│ ■ done
╵
//! reject Y12
",
        ),
        // [G12]: `contingency` must take one parameter `(m: string)`.
        (
            "G12",
            "\
boundary bad_cont(x: u32)
│ terminate
╵
contingency ()
│ terminate
╵
//! reject G12
",
        ),
        // [U11]: Moving a resource without `▾` is rejected.
        (
            "U11",
            "\
extern resource file_handle
extern control open() ■ opened {f: file_handle}
extern control close(f: file_handle) ■ closed

control missing_move() ■ done
│ Δ open(): opened {f}
│ close(f)
│ ■ done
╵
//! reject U11
",
        ),
        // [U11]: Writing `▾` on a type that does NOT hold an obligation (`string`) is rejected.
        (
            "U11",
            "\
control extra_move(s: string) ■ done
│ t: string := ▾s
│ ■ done
╵
//! reject U11
",
        ),
        // [U11]: Calling an `&~` parameter without `~` is rejected.
        (
            "U11",
            "\
extern control bump(x: &~u32) ■ done

control missing_tilde() ■ done
│ ~n: u32 := 1
│ bump(n)
│ ■ done
╵
//! reject U11
",
        ),
        // [U11]: Deciding on a captured outcome without `▾` is rejected.
        (
            "U11",
            "\
extern control fetch() ■ ok {v: u32} │ err

control bad_outcome() ■ done
│ res := capture fetch()
│ Δ res: ok {v} → ■ done │ err → ■ done
╵
//! reject U11
",
        ),
        // [U12]: A control of the module cannot be called in method form (`x.helper()`).
        (
            "U12",
            "\
control helper(x: u32) ■ done
│ ■ done
╵

control caller(x: u32) ■ done
│ x.helper()
│ ■ done
╵
//! reject U12
",
        ),
        // [U10]: `∀` on a non-vector is rejected.
        (
            "U10",
            "\
control bad_forall(n: u32) ■ done
│ ∀ x ∈ n
│ ║ ■ done
│ ■ done
╵
//! reject U10
",
        ),
        // [U14]: `■ ▾x` on a captured outcome whose alternative is missing from caller's contract.
        (
            "U14",
            "\
extern control fetch() ■ ok {v: u32} │ err

control bad_forward_outcome() ■ ok {v: u32}
│ res := capture fetch()
│ ■ ▾res
╵
//! reject U14
",
        ),
    ];

    for (clause, source) in rejecting {
        let report = script::check_surface(source);
        assert!(
            report.failures.is_empty(),
            "rejecting test for [{clause}] failed: {:?}\n{source}",
            report.failures
        );
        assert_eq!(report.rejected.as_deref(), Some(*clause));
    }
}
