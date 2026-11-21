//! Phase 3 checker conformance tests for slice 1.
//!
//! Tests every checking clause of `docs/v2/core.md` (`[K1]`–`[K8]`,
//! `[N1]`–`[N5]`, `[C2]`, `[O1]`, `[O2]`, `[O4]`, `[O6]`, `[P1]`–`[P8]`,
//! `[F3]`) and every fact-logic, certificate and counterexample clause of
//! `docs/v2/facts.md` (`[S1]`–`[S6]`, `[I1]`–`[I6]`, `[I8]`–`[I10]`, `[I13]`,
//! `[A1]`, `[A2]`, `[A7]`–`[A9]`, `[J1]`–`[J6]`, `[V1]`–`[V5]`,
//! `[M1]`–`[M6]`).

use villanelle_v2::check::{self, CheckedProgram};
use villanelle_v2::facts::ProofStep;
use villanelle_v2::generate;
use villanelle_v2::int::Int;
use villanelle_v2::kernel;
use villanelle_v2::mutate::{self, Class};
use villanelle_v2::oracle::Config;
use villanelle_v2::rng::Rng;
use villanelle_v2::{load, load_surface, resolve, text};

fn check_core(source: &str) -> Result<CheckedProgram, check::Error> {
    let program = load(source).unwrap_or_else(|error| panic!("resolve failed: {error}\n{source}"));
    check::check(&program)
}

fn check_rails(source: &str) -> Result<CheckedProgram, check::Error> {
    let program =
        load_surface(source).unwrap_or_else(|error| panic!("resolve failed: {error}\n{source}"));
    check::check(&program)
}

fn accept_core(clause: &str, source: &str) -> CheckedProgram {
    match check_core(source) {
        Ok(report) => report,
        Err(error) => panic!("[{clause}] expected acceptance, got: {error}\n{source}"),
    }
}

fn reject_core(clause: &str, source: &str) -> check::Error {
    match check_core(source) {
        Ok(_) => panic!(
            "[{clause}] expected rejection by [{clause}], but program was accepted:\n{source}"
        ),
        Err(error) => {
            assert_eq!(
                error.clause, clause,
                "[{clause}] expected rejection by [{clause}], got [{}]: {error}\n{source}",
                error.clause
            );
            error
        }
    }
}

fn accept_rails(clause: &str, source: &str) -> CheckedProgram {
    match check_rails(source) {
        Ok(report) => report,
        Err(error) => panic!("[{clause}] expected Rails acceptance, got: {error}\n{source}"),
    }
}

fn reject_rails(clause: &str, source: &str) -> check::Error {
    match check_rails(source) {
        Ok(_) => panic!(
            "[{clause}] expected Rails rejection by [{clause}], but program was accepted:\n{source}"
        ),
        Err(error) => {
            assert_eq!(
                error.clause, clause,
                "[{clause}] expected Rails rejection by [{clause}], got [{}]: {error}\n{source}",
                error.clause
            );
            error
        }
    }
}

#[test]
fn coverage_and_history_clauses_k1_to_k8() {
    // [K1], [K2], [K3], [K4], [K5], [K7]: Boolean selectors, site structure,
    // test totality, and full coverage across split dispositions.
    accept_core(
        "K1-K7",
        "
        extern control probe(x : u32) ■ { a │ b │ c }

        pub control cover(x : u32) ■ { ok {v : u32} │ nope } :=
            ⊤ ▷ d : Δ probe(x) ▸ a │ b │ c
            d.a ▷ t : ? x > 10
            d.a ∧ t.true ▷ ■ ok {x}
            d.a ∧ ¬t.true ▷ ■ nope
            d.b ∨ d.c ▷ ■ nope
        ",
    );

    // [K7] rejection: `d.c` is not disposed and can reach the end of the body.
    let err = reject_core(
        "K7",
        "
        extern control probe(x : u32) ■ { a │ b │ c }

        pub control uncover(x : u32) ■ { ok │ nope } :=
            ⊤ ▷ d : Δ probe(x) ▸ a │ b │ c
            d.a ▷ ■ ok
            d.b ▷ ■ nope
        ",
    );
    let cex = err.counterexample.expect("counterexample for K7");
    assert!(
        cex.history.contains(&("d".to_string(), "c".to_string())),
        "counterexample history should witness d.c: {cex}"
    );

    // [K6], [K7], [K8]: Loop body coverage and exact loop arm reachability.
    // Here `□ unused` is guarded by `t.true ∧ t.false`, so `[K8]` makes
    // `lp.unused ↔ ⊥`; the body after the loop only needs to dispose `lp.done`.
    accept_core(
        "K6-K8",
        "
        pub control loop_exact(n : u32) ⊢ n ≤ 100 ■ { finished {v : u32} } :=
            ⊤ ▷ lp : ↻ step(i : u32 := 0) {
                ⊢ i ≤ n
                ⊤ ▷ t : ? i < n
                t.true ∧ t.false ▷ □ unused
                t.true ▷ ↑ step(i + 1)
                t.false ▷ □ done {i}
            } ▸ done {res} │ unused
            lp.done ▷ ■ finished {res}
        ",
    );

    // [K6], [K7] rejection inside a loop body: `t.false` neither repeats with
    // `↑` nor leaves with `□`/`■`/`⊥`.
    reject_core(
        "K7",
        "
        pub control loop_uncovered(n : u32) ⊢ n ≤ 100 ■ { finished } :=
            ⊤ ▷ lp : ↻ step(i : u32 := 0) {
                ⊢ i ≤ n
                ⊤ ▷ t : ? i < n
                t.true ▷ □ done
            } ▸ done
            lp.done ▷ ■ finished
        ",
    );
}

#[test]
fn facts_and_renaming_clauses_n1_to_n5() {
    // [N1], [N2], [N3], [N4], [N5]: Parameter bounds + precondition at entry,
    // binding and assignment equality facts, `⊢` assumption, test and decision
    // outcome conditions, and SSA renaming across assignments and loops.
    accept_core(
        "N1-N5",
        "
        extern control step_down(x : u32) ⊢ x > 0
            ■ { next {y : u32} ⇐ y = x - 1 }

        pub control facts_flow(a : u32) ⊢ a ≥ 10 ∧ a ≤ 200
            ■ { done {out : u32} ⇐ out = a } :=
            ⊤ ▷ ~cur : u32 := a
            ⊤ ▷ d : Δ step_down(cur) ▸ next {y}
            d.next ▷ ⊢ y = a - 1
            d.next ▷ cur ← y + 1
            d.next ▷ ⊢ cur = a
            d.next ▷ ■ done {cur}
        ",
    );

    // [N5] rejection: after `cur ← cur + 1`, the old fact `cur = 5` no longer
    // holds for the new version of `cur`.
    reject_core(
        "P1",
        "
        pub control stale_version() ■ { done } :=
            ⊤ ▷ ~cur : u32 := 5
            ⊤ ▷ cur ← cur + 1
            ⊤ ▷ ⊢ cur = 5
            ⊤ ▷ ■ done
        ",
    );

    // [N5] rejection across a loop boundary: `acc` is modified inside the loop,
    // so `acc = 0` before the loop does not survive into or after the loop
    // without an invariant.
    reject_core(
        "P1",
        "
        pub control loop_clobbers_outer(n : u32) ■ { done } :=
            ⊤ ▷ ~acc : u32 := 0
            ⊤ ▷ lp : ↻ step(i : u32 := 0) {
                ⊤ ▷ t : ? i < n
                t.true ▷ acc ← 1
                t.true ▷ □ left
                t.false ▷ □ left
            } ▸ left
            lp.left ▷ ⊢ acc = 0
            lp.left ▷ ■ done
        ",
    );
}

#[test]
fn contract_acknowledgement_c2() {
    // [C2] acceptance: every alternative of the operation is listed.
    accept_core(
        "C2",
        "
        extern control two_way(x : u32) ■ { yes │ no }

        pub control handle_both(x : u32) ■ { done } :=
            ⊤ ▷ d : Δ two_way(x) ▸ yes │ no
            d.yes ∨ d.no ▷ ■ done
        ",
    );

    // [C2] rejection on a typed program whose decision lists fewer arms than
    // the callee's contract.
    let tree = text::parse(
        "
        extern control two_way(x : u32) ■ { yes │ no }

        pub control handle_one(x : u32) ■ { done } :=
            ⊤ ▷ d : Δ two_way(x) ▸ yes │ no
            d.yes ∨ d.no ▷ ■ done
        ",
    )
    .unwrap();
    let mut program = resolve::resolve(&tree).unwrap();
    let body = program.controls[1].body.as_mut().unwrap();
    if let villanelle_v2::typed::Command::Decide { arms, .. } =
        &mut body.bodies[0].statements[0].command
    {
        arms.pop();
    }
    let err = check::check(&program).expect_err("missing arm must fail C2");
    assert_eq!(err.clause, "C2");
}

#[test]
fn availability_clauses_o1_o2_o4_o6() {
    // [O1], [O2], [O4], [O6] acceptance: `v` is bound on disjoint histories
    // (`t.true` and `t.false`), read after the join, and a mutable outer
    // variable `m` bound on `t.true` is updated inside a loop and read under
    // `t.true`.
    accept_core(
        "O1-O6",
        "
        pub control avail_ok(flag : bool) ■ { done {res : u32} } :=
            ⊤ ▷ t : ? flag
            t.true ▷ v : u32 := 10
            t.false ▷ v : u32 := 20
            t.true ▷ ~m : u32 := 1
            ⊤ ▷ lp : ↻ step(i : u32 := 0) {
                t.true ▷ m ← 2
                ⊤ ▷ □ out
            } ▸ out
            t.true ▷ ⊢ m ≥ 1
            ⊤ ▷ ■ done {v}
        ",
    );

    // [O1] rejection: two bindings of `v` can both execute on the same feasible
    // history (`t1.true ∧ t2.true`).
    reject_core(
        "O1",
        "
        pub control double_bind(a : bool, b : bool) ■ { done {res : u32} } :=
            ⊤ ▷ t1 : ? a
            ⊤ ▷ t2 : ? b
            t1.true ▷ v : u32 := 1
            t2.true ▷ v : u32 := 2
            t1.true ∧ t2.true ▷ ■ done {v}
            ¬(t1.true ∧ t2.true) ▷ ■ done {0}
        ",
    );

    // [O2] rejection: `v` is bound only on `t.true`, but read on `⊤`.
    reject_core(
        "O2",
        "
        pub control read_unbound(flag : bool) ■ { done {res : u32} } :=
            ⊤ ▷ t : ? flag
            t.true ▷ v : u32 := 10
            ⊤ ▷ ■ done {v}
        ",
    );

    // [O4] rejection: mutable `~m` is bound only on `t.true`, but assigned on `⊤`.
    reject_core(
        "O4",
        "
        pub control assign_unbound(flag : bool) ■ { done } :=
            ⊤ ▷ t : ? flag
            t.true ▷ ~m : u32 := 10
            ⊤ ▷ m ← 20
            ⊤ ▷ ■ done
        ",
    );
}

#[test]
fn obligation_clauses_p1_to_p8() {
    // [P1] acceptance and rejection.
    accept_core(
        "P1",
        "
        pub control p1_ok(x : u32) ⊢ x < 100 ■ { done } :=
            ⊤ ▷ ⊢ x ≤ 99
            ⊤ ▷ ■ done
        ",
    );
    reject_core(
        "P1",
        "
        pub control p1_bad(x : u32) ⊢ x < 100 ■ { done } :=
            ⊤ ▷ ⊢ x ≤ 50
            ⊤ ▷ ■ done
        ",
    );

    // [P2] acceptance, entry rejection, and repetition (`↑`) rejection.
    accept_core(
        "P2",
        "
        pub control p2_ok(n : u32) ⊢ n ≤ 10 ■ { done } :=
            ⊤ ▷ lp : ↻ step(i : u32 := 0) {
                ⊢ i ≤ n
                ⊤ ▷ t : ? i < n
                t.true ▷ ↑ step(i + 1)
                t.false ▷ □ out
            } ▸ out
            lp.out ▷ ■ done
        ",
    );
    reject_core(
        "P2",
        "
        pub control p2_bad_entry(n : u32) ■ { done } :=
            ⊤ ▷ lp : ↻ step(i : u32 := 5) {
                ⊢ i ≤ 3
                ⊤ ▷ □ out
            } ▸ out
            lp.out ▷ ■ done
        ",
    );
    reject_core(
        "P2",
        "
        pub control p2_bad_again() ■ { done } :=
            ⊤ ▷ lp : ↻ step(i : u32 := 0) {
                ⊢ i ≤ 3
                ⊤ ▷ t : ? i ≤ 3
                t.true ▷ ↑ step(i + 1)
                t.false ▷ □ out
            } ▸ out
            lp.out ▷ ■ done
        ",
    );

    // [P3] acceptance and rejection for function and control preconditions.
    accept_core(
        "P3",
        "
        function dec(x : u32) ⊢ x > 0 : u32 := x - 1
        extern control req_pos(x : u32) ⊢ x > 0 ■ { ok }

        pub control p3_ok(x : u32) ■ { done } :=
            ⊤ ▷ t : ? x > 0
            t.true ▷ y : u32 := dec(x)
            t.true ▷ d : Δ req_pos(x) ▸ ok
            t.true ∧ d.ok ▷ ■ done
            t.false ▷ ■ done
        ",
    );
    reject_core(
        "P3",
        "
        function dec(x : u32) ⊢ x > 0 : u32 := x - 1

        pub control p3_bad_fn(x : u32) ■ { done {v : u32} } :=
            ⊤ ▷ ■ done {dec(x)}
        ",
    );
    reject_core(
        "P3",
        "
        extern control req_pos(x : u32) ⊢ x > 0 ■ { ok }

        pub control p3_bad_call(x : u32) ■ { done } :=
            ⊤ ▷ d : Δ req_pos(x) ▸ ok
            d.ok ▷ ■ done
        ",
    );

    // [P4] acceptance and rejection for exit contract condition `⇐ φₐ`.
    accept_core(
        "P4",
        "
        pub control p4_ok(x : u32) ⊢ x < 10
            ■ { small {v : u32} ⇐ v ≤ 10 } :=
            ⊤ ▷ ■ small {x + 1}
        ",
    );
    reject_core(
        "P4",
        "
        pub control p4_bad(x : u32) ⊢ x < 10
            ■ { small {v : u32} ⇐ v < 10 } :=
            ⊤ ▷ ■ small {x + 1}
        ",
    );

    // [P5]: Checked arithmetic, unary negation, fixed-width bounds (`[A1]`),
    // target-independent `usize` bounds (`[A2]`), and non-zero divisors.
    accept_core(
        "P5",
        "
        pub control p5_ok(a : u8, b : i8, n : usize) ⊢ a ≤ 200 ∧ b > -128 ∧ n ≤ 1000
            ■ { done {x : u8, y : i8, z : usize} } :=
            ⊤ ▷ t : ? a > 0
            t.true ▷ q : u8 := 200 ÷ a
            t.true ▷ r : u8 := 200 mod a
            ⊤ ▷ ■ done {a + 55, -b, n + 1}
        ",
    );
    // Overflow on `u8`: `a + 56` can reach 256 when `a = 200`.
    reject_core(
        "P5",
        "
        pub control p5_overflow_u8(a : u8) ⊢ a ≤ 200 ■ { done {x : u8} } :=
            ⊤ ▷ ■ done {a + 56}
        ",
    );
    // Overflow on unary negation of `i8`: `-(-128) = 128 > 127`.
    reject_core(
        "P5",
        "
        pub control p5_negate_i8_min(b : i8) ■ { done {y : i8} } :=
            ⊤ ▷ ■ done {-b}
        ",
    );
    // `[A2]` on `usize`: `n + 1` without an upper bound on `n` is rejected,
    // while `n ≤ 4294967294` (`2^32 - 2`) is accepted because `high(usize) ≥ 2^32 - 1`.
    accept_core(
        "A2-P5",
        "
        pub control usize_within_32bit(n : usize) ⊢ n ≤ 4294967294 ■ { done {z : usize} } :=
            ⊤ ▷ ■ done {n + 1}
        ",
    );
    reject_core(
        "P5",
        "
        pub control usize_unbounded(n : usize) ■ { done {z : usize} } :=
            ⊤ ▷ ■ done {n + 1}
        ",
    );
    // Division and remainder by a possibly-zero divisor fail [P5].
    reject_core(
        "P5",
        "
        pub control p5_div_zero(a : u32, b : u32) ■ { done {q : u32} } :=
            ⊤ ▷ ■ done {a ÷ b}
        ",
    );
    reject_core(
        "P5",
        "
        pub control p5_rem_zero(a : u32, b : u32) ■ { done {r : u32} } :=
            ⊤ ▷ ■ done {a mod b}
        ",
    );

    // [P6]: `⊥` is accepted when unreachable and rejected when reachable.
    accept_core(
        "P6",
        "
        extern control checked_sub(a : u32, b : u32)
            ■ { difference {value : u32} ⇐ value = a - b │ underflow ⇐ a < b }

        pub control spend(total : u32, used : u32) ■ { left {value : u32} │ over } :=
            ⊤ ▷ t : ? used > total
            t.true ▷ ■ over
            t.false ▷ d : Δ checked_sub(total, used) ▸ difference {value} │ underflow
            t.false ∧ d.underflow ▷ ⊥
            t.false ∧ d.difference ▷ ■ left {value}
        ",
    );
    reject_core(
        "P6",
        "
        extern control checked_sub(a : u32, b : u32)
            ■ { difference {value : u32} ⇐ value = a - b │ underflow ⇐ a < b }

        pub control spend_bug(total : u32, used : u32) ■ { left {value : u32} │ over } :=
            ⊤ ▷ t : ? used ≥ total
            t.false ▷ ■ over
            t.true ▷ d : Δ checked_sub(total, used) ▸ difference {value} │ underflow
            t.true ∧ d.underflow ▷ ⊥
            t.true ∧ d.difference ▷ ■ left {value}
        ",
    );

    // [P7]: Short-circuit evaluation order of `∧` and `∨` (`e₂` is checked
    // under `e₁` for `∧`, and under `¬e₁` for `∨`).
    accept_core(
        "P7",
        "
        pub control short_circuit_ok(x : u32) ■ { done {a : bool, b : bool} } :=
            ⊤ ▷ a : bool := x > 0 ∧ 100 ÷ x > 2
            ⊤ ▷ b : bool := x = 0 ∨ 100 ÷ x > 2
            ⊤ ▷ ■ done {a, b}
        ",
    );
    reject_core(
        "P5",
        "
        pub control short_circuit_bad(x : u32) ■ { done {a : bool} } :=
            ⊤ ▷ a : bool := x = 0 ∧ 100 ÷ x > 2
            ⊤ ▷ ■ done {a}
        ",
    );

    // [P8]: Function declarations have their body obligations checked under
    // parameter range facts and the function's precondition.
    accept_core(
        "P8",
        "
        function safe_step(x : u8) ⊢ x < 255 : u8 := x + 1

        pub control use_fn(x : u8) ⊢ x < 100 ■ { done {v : u8} } :=
            ⊤ ▷ ■ done {safe_step(x)}
        ",
    );
    reject_core(
        "P5",
        "
        function unsafe_step(x : u8) : u8 := x + 1

        pub control use_fn() ■ { done } :=
            ⊤ ▷ ■ done
        ",
    );
}

#[test]
fn effects_clause_f3() {
    // [F3] acceptance: `pub` control lists `uses io`; dead call on an
    // infeasible history does not require the effect; private surface control
    // infers `uses io`.
    accept_core(
        "F3",
        "
        extern control write_byte(b : u8) ■ { ok } uses io

        pub control allowed(b : u8) ■ { done } uses io :=
            ⊤ ▷ d : Δ write_byte(b) ▸ ok
            d.ok ▷ ■ done

        pub control dead_effect_call(b : u8) ■ { done } :=
            ⊤ ▷ t : ? b < 0
            t.true ▷ d : Δ write_byte(b) ▸ ok
            t.true ∧ d.ok ▷ ■ done
            t.false ▷ ■ done
        ",
    );

    // [F3] rejection in core: calling `write_byte` on a feasible history without `uses io`.
    reject_core(
        "F3",
        "
        extern control write_byte(b : u8) ■ { ok } uses io

        pub control forbidden(b : u8) ■ { done } :=
            ⊤ ▷ d : Δ write_byte(b) ▸ ok
            d.ok ▷ ■ done
        ",
    );

    // [F3] in Rails surface syntax: private control infers `uses io`, while a
    // `pub` caller without `uses io` is rejected by `[F3]`.
    accept_rails(
        "F3-surface-ok",
        "\
extern control write_byte(b: u8) ■ ok uses io

control helper(b: u8) ■ done
│ write_byte(b)
│ ■ done
╵

pub control entry(b: u8) ■ done uses io
│ helper(b)
│ ■ done
╵
",
    );
    reject_rails(
        "F3",
        "\
extern control write_byte(b: u8) ■ ok uses io

control helper(b: u8) ■ done
│ write_byte(b)
│ ■ done
╵

pub control entry(b: u8) ■ done
│ helper(b)
│ ■ done
╵
",
    );
}

#[test]
fn fact_logic_axioms_a7_a8_a9_i10_and_chvatal_gomory_j3() {
    // [A7]: Division and remainder by a constant (`e₁ = k·q + r`, `0 ≤ r < |k|`).
    accept_core(
        "A7",
        "
        pub control div_mod_const(x : u32) ⊢ x ≤ 100 ■ { done } :=
            ⊤ ▷ q : u32 := x ÷ 4
            ⊤ ▷ r : u32 := x mod 4
            ⊤ ▷ ⊢ r ≤ 3 ∧ q ≤ 25 ∧ x = 4 * q + r
            ⊤ ▷ ■ done
        ",
    );

    // [A8]: Division and remainder by a non-constant term (`t ≥ 1, s ≥ 0` and
    // `t ≤ -1, s ≥ 0`).
    accept_core(
        "A8",
        "
        pub control div_mod_var(a : u32, d : u32, s : i32, t : i32)
            ⊢ d > 0 ∧ s ≥ 0 ∧ s ≤ 100 ∧ t < 0 ∧ t ≥ -50
            ■ { done } :=
            ⊤ ▷ r : u32 := a mod d
            ⊤ ▷ q : u32 := a ÷ d
            ⊤ ▷ ⊢ r < d ∧ r ≤ a ∧ q ≤ a
            ⊤ ▷ qs : i32 := s ÷ t
            ⊤ ▷ rs : i32 := s mod t
            ⊤ ▷ ⊢ qs ≥ -s ∧ qs ≤ 0 ∧ rs ≥ 0 ∧ rs ≤ -t - 1
            ⊤ ▷ ■ done
        ",
    );

    // [A9]: Product bounds from declared intervals (`[0, 7] * [0, 7] ⊆ [0, 49]`,
    // `[-7, 7] * [-7, 7] ⊆ [-49, 49]`, and `u8 * u8` fitting in `u16`).
    accept_core(
        "A9",
        "
        pub control product_bounds(a : u8, s : i8) ■ { done {p : u8, q : i8} } :=
            ⊤ ▷ p : u8 := (a mod 8) * ((a ÷ 2) mod 8)
            ⊤ ▷ ⊢ p ≤ 49
            ⊤ ▷ q : i8 := (s mod 8) * (-(s mod 8))
            ⊤ ▷ ⊢ q ≥ -49 ∧ q ≤ 49
            ⊤ ▷ ■ done {p, q}
        ",
    );

    // [I10]: Pure operation congruence (`alt_f` and `pay_{f, a, i}`): calling a
    // pure extern control twice with equal arguments cannot take two different
    // alternatives, and on the same alternative yields equal payloads.
    accept_core(
        "I10",
        "
        extern control lookup(key : u32)
            ■ { found {val : u32} │ missing }

        pub control pure_congruence(k1 : u32, k2 : u32) ⊢ k1 = k2 ■ { done } :=
            ⊤ ▷ d1 : Δ lookup(k1) ▸ found {v1} │ missing
            ⊤ ▷ d2 : Δ lookup(k2) ▸ found {v2} │ missing
            d1.found ∧ d2.missing ▷ ⊥
            d1.missing ∧ d2.found ▷ ⊥
            d1.found ∧ d2.found ▷ ⊢ v1 = v2
            d1.found ∧ d2.found ▷ ■ done
            d1.missing ∧ d2.missing ▷ ■ done
        ",
    );

    // [J3]: Chvátal–Gomory integer rounding cut: `x + y ≥ 1` and `x >= y`
    // combine (eliminating `y`) into `2 * x ≥ 1`, which rounds via `[J3] Round`
    // to `x ≥ 1`, contradicting `x ≤ 0`.
    let report = accept_core(
        "J3",
        "
        pub control parity_impossible(x : i32, y : i32)
            ⊢ x ≥ -100 ∧ x ≤ 100 ∧ y ≥ -100 ∧ y ≤ 100
            ■ { done } :=
            ⊤ ▷ t1 : ? x + y ≥ 1
            ⊤ ▷ t2 : ? x ≥ y
            ⊤ ▷ t3 : ? x ≤ 0
            t1.true ∧ t2.true ∧ t3.true ▷ ⊥
            ¬(t1.true ∧ t2.true ∧ t3.true) ▷ ■ done
        ",
    );
    let saw_round = report.certificates.iter().any(|(_, _, _, cert)| {
        cert.lemmas.iter().any(|lemma| {
            lemma
                .steps
                .iter()
                .any(|step| matches!(step, ProofStep::Round { .. }))
        })
    });
    assert!(saw_round, "expected a [J3] Round step in the certificate");
}

#[test]
fn certificate_kernel_rejects_corrupted_proofs_v1_to_v5() {
    let report = accept_core(
        "V1-V5",
        "
        extern control checked_sub(a : u32, b : u32)
            ■ { difference {value : u32} ⇐ value = a - b │ underflow ⇐ a < b }

        pub control spend(total : u32, used : u32) ■ { left {value : u32} │ over } :=
            ⊤ ▷ t : ? used > total
            t.true ▷ ■ over
            t.false ▷ d : Δ checked_sub(total, used) ▸ difference {value} │ underflow
            t.false ∧ d.underflow ▷ ⊥
            t.false ∧ d.difference ▷ ⊢ value ≤ total
            t.false ∧ d.difference ▷ ■ left {value}
        ",
    );
    assert!(
        !report.certificates.is_empty(),
        "expected validated certificates"
    );
    for (_, _, _, cert) in &report.certificates {
        kernel::validate(cert).expect("every emitted certificate must pass kernel::validate");
        let printed = cert.to_string();
        assert!(
            printed.contains("certificate (atoms:"),
            "certificate text must match [V5]:\n{printed}"
        );
    }

    // Tamper 1 ([V4]): delete the theory lemmas from a certificate that relies
    // on linear arithmetic so the propositional implication is no longer a tautology.
    let (_, _, _, lemma_cert) = report
        .certificates
        .iter()
        .find(|(_, _, _, cert)| !cert.lemmas.is_empty())
        .expect("certificate with a theory lemma");
    let mut missing_lemma = lemma_cert.clone();
    missing_lemma.lemmas.clear();
    assert!(
        kernel::validate(&missing_lemma).is_err(),
        "kernel must reject a certificate whose theory lemma was stripped ([V4])"
    );

    // Tamper 2 ([V2]/[V3]): replace a `Combine` multiplier with -1.
    let mut bad_mult = lemma_cert.clone();
    bad_mult.lemmas[0]
        .steps
        .push(ProofStep::Combine(vec![(0, Int::from_i128(-1))]));
    let last_idx = bad_mult.lemmas[0].steps.len() - 1;
    bad_mult.lemmas[0].steps.push(ProofStep::Close(last_idx));
    assert!(
        kernel::validate(&bad_mult).is_err(),
        "kernel must reject a negative Combine multiplier ([V3])"
    );

    // Tamper 3 ([V3]): reference an out-of-range premise row index in `Combine`.
    let mut bad_index = lemma_cert.clone();
    bad_index.lemmas[0].steps = vec![
        ProofStep::Row(0),
        ProofStep::Combine(vec![(99, Int::from_i128(1))]),
        ProofStep::Close(1),
    ];
    assert!(
        kernel::validate(&bad_index).is_err(),
        "kernel must reject an out-of-range premise index ([V3])"
    );

    // Tamper 4 ([V3]): close on a non-contradictory row (`ProofStep::Row(0)`).
    let mut non_contradictory = lemma_cert.clone();
    non_contradictory.lemmas[0].steps = vec![ProofStep::Row(0), ProofStep::Close(0)];
    assert!(
        kernel::validate(&non_contradictory).is_err(),
        "kernel must reject a lemma that closes on a non-contradictory row ([V3])"
    );
}

#[test]
fn counterexample_clauses_m1_to_m6() {
    // [M1], [M2], [M6]: Counterexample includes history witness, integer
    // valuation satisfying all active facts and negating the obligation, and
    // readable display.
    let err = reject_core(
        "P1",
        "
        pub control cex_basic(x : u8) ⊢ x ≥ 10 ■ { done } :=
            ⊤ ▷ t : ? x < 20
            t.true ▷ ⊢ x < 15
            t.true ▷ ■ done
            t.false ▷ ■ done
        ",
    );
    let cex = err.counterexample.expect("counterexample for P1");
    assert!(cex.history.contains(&("t".to_string(), "true".to_string())));
    let x_str = cex.values.get("x").expect("valuation for x");
    let x_val = Int::parse(x_str).expect("integer value for x");
    assert!(
        x_val >= Int::from_i128(15) && x_val < Int::from_i128(20),
        "x = {x_val} must satisfy 15 <= x < 20"
    );

    // [M3]: Nonlinear incompleteness note when a nonlinear product is treated
    // as an abstract term with `[A9]` bounds.
    let err_nl = reject_core(
        "P1",
        "
        pub control cex_nonlinear(a : u8, b : u8) ■ { done } :=
            ⊤ ▷ p : u8 := (a mod 8) * (b mod 8)
            ⊤ ▷ ⊢ p ≠ 11
            ⊤ ▷ ■ done
        ",
    );
    let cex_nl = err_nl
        .counterexample
        .expect("counterexample for nonlinear P1");
    assert!(
        cex_nl.assumed_remark.is_some(),
        "expected [M3] nonlinear remark on unproved product obligation"
    );
    assert!(
        cex_nl.to_string().contains("depends on `*`"),
        "expected [M3] text in counterexample output:\n{cex_nl}"
    );

    // [M4]: Loop boundary note when an obligation fails inside a loop body.
    let err_loop = reject_core(
        "P1",
        "
        pub control cex_loop(n : u32) ■ { done } :=
            ⊤ ▷ lp : ↻ step(i : u32 := 0) {
                ⊢ i ≤ 10
                ⊤ ▷ ⊢ i < 10
                ⊤ ▷ □ out
            } ▸ out
            lp.out ▷ ■ done
        ",
    );
    let cex_loop = err_loop.counterexample.expect("counterexample inside loop");
    assert!(
        cex_loop.in_loop,
        "expected [M4] loop boundary note inside loop body"
    );
    assert!(
        cex_loop.to_string().contains("start of an iteration"),
        "expected [M4] text in counterexample output:\n{cex_loop}"
    );
}

#[test]
fn surface_files_and_generated_valid_programs_pass_the_checker() {
    for path in [
        "tests/slice1_surface/spend.rails",
        "tests/slice1_surface/show_file.rails",
        "tests/slice1_surface/groups_and_auto.rails",
        "tests/slice1_surface/loops_and_paths.rails",
    ] {
        let source = std::fs::read_to_string(path).unwrap();
        accept_rails(path, &source);
    }

    // Every generated `valid` program across 80 seeds must pass `check::check`
    // and have every emitted certificate validated by `kernel::validate`.
    let mut total_certificates = 0usize;
    for seed in 1..=80 {
        let mut rng = Rng::new(seed);
        let syntax_program = generate::valid(&mut rng);
        let program = resolve::resolve(&syntax_program).unwrap();
        let report = check::check(&program).unwrap_or_else(|error| {
            panic!(
                "seed {seed}: generated valid program failed checker: {error}\n{}",
                syntax_program.print()
            )
        });
        total_certificates += report.certificates.len();
    }
    assert!(
        total_certificates >= 80 * 10,
        "expected at least 800 validated certificates across 80 seeds, got {total_certificates}"
    );
}

#[test]
fn every_stuck_mutation_and_dropped_invariant_is_rejected_by_the_checker() {
    let config = Config {
        fuel: 5_000,
        ..Config::default()
    };
    let mut stuck_mutants = 0usize;
    let mut dropped_invariants = 0usize;

    for seed in 1..=8 {
        let mut rng = Rng::new(seed);
        let syntax_program = generate::valid(&mut rng);
        let typed_program = resolve::resolve(&syntax_program).unwrap();
        let runs = generate::sample_runs(&mut rng, &typed_program, &config);

        for mutant in mutate::mutate(&syntax_program) {
            let class = mutate::classify(&mutant, &runs, seed * 100, &config);
            let is_stuck = matches!(class, Class::Stuck(_));
            let is_drop_inv = mutant.kind == mutate::Kind::DropInvariant;
            if !is_stuck && !is_drop_inv {
                continue;
            }
            if is_stuck {
                stuck_mutants += 1;
            }
            if is_drop_inv {
                dropped_invariants += 1;
            }
            let rejected = match resolve::resolve(&mutant.program) {
                Err(_) => true,
                Ok(resolved) => check::check(&resolved).is_err(),
            };
            assert!(
                rejected,
                "seed {seed} ({:?}, {}, class {:?}) was NOT rejected by resolve or check:\n{}",
                mutant.kind,
                mutant.detail,
                class,
                mutant.program.print()
            );
        }
    }
    assert!(
        stuck_mutants >= 180,
        "expected >= 180 stuck mutants, got {stuck_mutants}"
    );
    assert_eq!(dropped_invariants, 8, "expected 8 DropInvariant mutants");
}

#[test]
fn u16_warns_when_alternative_is_proved_impossible_without_bottom() {
    // [U16]: When the fact logic proves that an alternative of a decision
    // cannot happen and the arm is not `⊥`, the compiler warns and suggests `⊥`.
    let with_exit_instead_of_bottom = "\
extern control checked_sub(a: u32, b: u32)
    ■ difference {value: u32} ⇐ value = a - b
    ■ underflow ⇐ a < b

pub control spend(total: u32, used: u32) ■ left {value: u32} │ over
│ ? used > total → ■ over
│ Δ checked_sub(total, used): difference {value} │ underflow → ■ over
│ ■ left {value}
╵
//! warn U16
";
    let report = villanelle_v2::script::check_surface(with_exit_instead_of_bottom);
    assert!(
        report.failures.is_empty(),
        "expected U16 warning, got failures: {:?}",
        report.failures
    );

    // When the arm IS marked `→ ⊥`, no [U16] warning is emitted.
    let with_bottom = "\
extern control checked_sub(a: u32, b: u32)
    ■ difference {value: u32} ⇐ value = a - b
    ■ underflow ⇐ a < b

pub control spend(total: u32, used: u32) ■ left {value: u32} │ over
│ ? used > total → ■ over
│ Δ checked_sub(total, used): difference {value} │ underflow → ⊥
│ ■ left {value}
╵
";
    let prog = villanelle_v2::load_surface(with_bottom).unwrap();
    let checked = check::check(&prog).unwrap();
    assert!(
        checked.warnings.is_empty(),
        "expected no U16 warning when `→ ⊥` is used, got {:?}",
        checked.warnings
    );

    // Pure operation Determinism ([I10]) also triggers [U16] when a second call
    // with the same arguments leaves a contradicting arm without `→ ⊥`.
    let pure_second_call = "\
extern control classify(x: u32) ■ low │ high

pub control twice(x: u32) ■ same │ diff
│ Δ classify(x)
│ ├ low →
│ │ ┆ Δ classify(x): low → ■ same │ high → ■ diff
│ └ high → ■ same
╵
//! warn U16
";
    let report2 = villanelle_v2::script::check_surface(pure_second_call);
    assert!(
        report2.failures.is_empty(),
        "expected U16 warning on second pure call, got failures: {:?}",
        report2.failures
    );
}
