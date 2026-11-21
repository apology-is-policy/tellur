//! Differential fuzzing, construct coverage and mutation tests for slice 1.

use std::collections::BTreeSet;
use villanelle_v2::differential::{Verdict, compare};
use villanelle_v2::generate::{CONSTRUCTS, Coverage, free, sample_runs, valid};
use villanelle_v2::host::{Arithmetic, Seeded};
use villanelle_v2::mutate::{Class, Kind, classify, mutate};
use villanelle_v2::oracle::{Config, End};
use villanelle_v2::rng::Rng;
use villanelle_v2::{resolve, text};

const RULES: [&str; 12] = [
    "E-Skip", "E-Bind", "E-Assign", "E-Oblige", "E-Test", "E-Decide", "E-Call", "E-Exit",
    "E-Extern", "E-Loop", "E-Again", "E-Leave",
];

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

#[test]
fn valid_programs_cover_every_construct_and_rule_and_never_stick() {
    let mut rng = Rng::new(2026);
    let mut coverage = Coverage::new();
    let mut rules_seen = BTreeSet::new();
    let mut total_runs = 0usize;

    let config_64 = Config {
        fuel: 10_000,
        ..Config::default()
    };
    let config_32 = Config {
        fuel: 10_000,
        ..Config::with_usize_bits(32)
    };

    for seed in 0..80u64 {
        let tree = valid(&mut rng);
        coverage.observe(&tree);

        // Every generated program prints as core text and reads back to the
        // same text.
        let printed = tree.print();
        let reparsed = text::parse(&printed)
            .unwrap_or_else(|error| panic!("seed {seed}: parse failed: {error}\n{printed}"));
        assert_eq!(printed, reparsed.print(), "seed {seed}");

        let program = resolve::resolve(&reparsed)
            .unwrap_or_else(|error| panic!("seed {seed}: resolve failed: {error}\n{printed}"));

        for config in [&config_64, &config_32] {
            let runs = sample_runs(&mut rng, &program, config);
            assert!(!runs.is_empty(), "seed {seed}: no runs generated");
            for (index, (entry, arguments)) in runs.into_iter().enumerate() {
                let mut host = Arithmetic(Seeded::new(&program, seed * 100 + index as u64));
                let outcome = compare(&program, &entry, arguments.clone(), &mut host, config);
                assert!(
                    matches!(outcome.oracle.end, End::Done { .. }),
                    "seed {seed}: `{entry}({arguments:?})` ended in `{}`\n{printed}",
                    outcome.oracle.end
                );
                assert_eq!(
                    outcome.verdict,
                    Verdict::Same,
                    "seed {seed}: `{entry}({arguments:?})` differed: {:?}\n{printed}",
                    outcome.verdict
                );
                for clause in outcome.oracle.rules.keys() {
                    rules_seen.insert(*clause);
                }
                total_runs += 1;
            }
        }
    }

    assert!(
        coverage.missing().is_empty(),
        "valid generator missed constructs: {:?}",
        coverage.missing()
    );
    assert_eq!(coverage.counts.len(), CONSTRUCTS.len());
    for rule in RULES {
        assert!(
            rules_seen.contains(rule),
            "valid runs never applied [{rule}]"
        );
    }
    assert!(total_runs >= 800);
}

#[test]
fn free_programs_cover_every_construct_and_agree_with_erased_when_unstuck() {
    let mut rng = Rng::new(4096);
    let mut coverage = Coverage::new();
    let mut compared = 0usize;
    let mut stuck = 0usize;
    let config = Config {
        fuel: 500,
        ..Config::default()
    };

    for seed in 0..250u64 {
        let tree = free(&mut rng);
        coverage.observe(&tree);

        let printed = tree.print();
        let reparsed = text::parse(&printed)
            .unwrap_or_else(|error| panic!("free seed {seed}: parse failed: {error}\n{printed}"));
        assert_eq!(printed, reparsed.print(), "free seed {seed}");

        let program = resolve::resolve(&reparsed)
            .unwrap_or_else(|error| panic!("free seed {seed}: resolve failed: {error}\n{printed}"));

        for (index, (entry, arguments)) in sample_runs(&mut rng, &program, &config)
            .into_iter()
            .enumerate()
        {
            let mut host = Arithmetic(Seeded::new(&program, seed * 37 + index as u64));
            let outcome = compare(&program, &entry, arguments.clone(), &mut host, &config);
            match outcome.verdict {
                Verdict::Same => compared += 1,
                Verdict::NotCompared => {
                    if matches!(outcome.oracle.end, End::Stuck(_)) {
                        stuck += 1;
                    }
                }
                Verdict::Differ(difference) => {
                    panic!("free seed {seed}: `{entry}({arguments:?})`: {difference}\n{printed}");
                }
            }
        }
    }

    assert!(
        coverage.missing().is_empty(),
        "free generator missed constructs: {:?}",
        coverage.missing()
    );
    assert!(compared > 100, "compared = {compared}");
    assert!(stuck > 50, "stuck = {stuck}");
}

#[test]
fn mutations_of_valid_programs_reach_every_stuck_state_of_the_slice() {
    let mut rng = Rng::new(9001);
    let config = Config {
        fuel: 5_000,
        ..Config::default()
    };
    let mut kinds_seen = BTreeSet::new();
    let mut stuck_seen = BTreeSet::new();
    let mut refused_count = 0usize;
    let mut stuck_count = 0usize;
    let mut unstuck_count = 0usize;

    for seed in 0..16u64 {
        let tree = valid(&mut rng);
        let program = resolve::resolve(&tree).expect("a valid program");
        let runs = sample_runs(&mut rng, &program, &config);
        for mutant in mutate(&tree) {
            kinds_seen.insert(mutant.kind);
            match classify(&mutant, &runs, seed * 100, &config) {
                Class::Refused(_) => refused_count += 1,
                Class::Stuck(stuck) => {
                    stuck_count += 1;
                    stuck_seen.insert(format!("X{} {}", stuck.state, stuck.clause));
                }
                Class::Unstuck => unstuck_count += 1,
            }
        }
    }

    for kind in [
        Kind::RemoveArm,
        Kind::RemoveDisposition,
        Kind::DropGuard,
        Kind::DropInvariant,
        Kind::DropEffect,
    ] {
        assert!(kinds_seen.contains(&kind), "missing mutation kind {kind:?}");
    }
    for expected in STUCK {
        assert!(
            stuck_seen.contains(expected),
            "no mutant of a valid program was stuck at {expected}; seen: {stuck_seen:?}"
        );
    }
    assert!(stuck_count > 100);
    assert!(refused_count > 0);
    assert!(unstuck_count > 0);
}

#[test]
fn seeded_host_honors_contracts_and_purity() {
    let source = "
extern control pure_ bounded(x : i32) ■ { ok {v : i32} ⇐ v ≥ x ∧ v ≤ x + 4 }
extern control noisy(x : i32) ■ { a {v : i32} ⇐ v ≥ 0 ∧ v ≤ 3 │ b } uses io
extern control impossible_host(x : u8) ■ { bad {v : u8} ⇐ v > 300 }

control check(x : i32) ■ { same {v : i32} } uses io
⊤ ▷ d1 : Δ pure_bounded(x) ▸ ok {v1}
⊤ ▷ d2 : Δ pure_bounded(x) ▸ ok {v2}
⊤ ▷ ⊢ v1 = v2 ∧ v1 ≥ x ∧ v1 ≤ x + 4
⊤ ▷ n1 : Δ noisy(x) ▸ a {w1} │ b
⊤ ▷ n2 : Δ noisy(x) ▸ a {w2} │ b
⊤ ▷ ■ same {v1}

control unachievable(x : u8) ■ { done }
⊤ ▷ d : Δ impossible_host(x) ▸ bad {v}
⊤ ▷ ■ done
"
    .replace("pure_ bounded", "pure_bounded");
    let program = villanelle_v2::load(&source).expect("resolved");
    let config = Config::default();
    for seed in 0..30u64 {
        let mut host = Seeded::new(&program, seed);
        let outcome = compare(
            &program,
            "check",
            vec![villanelle_v2::oracle::Value::Int(
                villanelle_v2::int::Int::from_i128(10),
            )],
            &mut host,
            &config,
        );
        assert!(matches!(outcome.oracle.end, End::Done { .. }));
        assert_eq!(outcome.verdict, Verdict::Same);
    }
    let mut host = Seeded::new(&program, 1);
    let failed = villanelle_v2::oracle::run(
        &program,
        "unachievable",
        vec![villanelle_v2::oracle::Value::Int(
            villanelle_v2::int::Int::from_u64(1),
        )],
        &mut host,
        &config,
    );
    assert!(matches!(failed.end, End::HostFailed(_)));
}
