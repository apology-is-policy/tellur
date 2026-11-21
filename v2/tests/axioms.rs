//! The axioms of the fact logic for quotients, remainders and products
//! (`docs/v2/facts.md`, [A7] to [A9]), tested against the arithmetic the
//! oracle evaluates formulas with.
//!
//! The checker of phase 3 will give these axioms to its decision procedure.
//! An axiom that the oracle's integers did not satisfy would let the checker
//! prove an obligation that is false in a run. Each test draws operands that
//! lean toward the ends of the integer types, and some far beyond them, since
//! a formula is evaluated over all integers.

use villanelle_v2::host::Script;
use villanelle_v2::int::Int;
use villanelle_v2::load;
use villanelle_v2::oracle::{Config, End, Value, run};
use villanelle_v2::rng::Rng;

fn int(value: i128) -> Int {
    Int::from_i128(value)
}

/// An integer that is often near a bound of a type, sometimes small, and
/// sometimes wider than any machine word.
fn sample(rng: &mut Rng) -> Int {
    let near = |rng: &mut Rng, bits: &[u32], sign: i128| {
        let center = sign * (1i128 << *rng.pick(bits));
        int(center + rng.range(-3, 3))
    };
    match rng.below(8) {
        0 => int(rng.range(-4, 4)),
        1 => near(rng, &[7, 8, 15, 16, 31, 32], 1),
        2 => near(rng, &[7, 15, 31, 63], -1),
        3 => near(rng, &[63, 64], 1),
        4 => int(rng.range(-1000, 1000)),
        5 => int(rng.range(i128::from(i64::MIN), i128::from(u64::MAX))),
        6 => {
            let wide = int(rng.range(-(1 << 100), 1 << 100));
            wide.times(&Int::power_of_two(rng.below(120) as u32))
                .plus(&int(rng.range(-5, 5)))
        }
        _ => int(rng.range(-70000, 70000)),
    }
}

fn quotient(s: &Int, t: &Int) -> (Int, Int) {
    s.divide(t).expect("a divisor that is not zero")
}

#[test]
fn a7_division_by_a_constant() {
    let mut rng = Rng::new(7);
    let zero = Int::zero();
    let one = int(1);
    for _ in 0..20_000 {
        let t = sample(&mut rng);
        let mut k = sample(&mut rng);
        if k < one {
            k = one.minus(&k);
        }
        let (q, r) = quotient(&t, &k);
        let kq = k.times(&q);
        let slack = k.minus(&one);
        if t >= zero {
            assert!(kq <= t && t <= kq.plus(&slack), "{t} ÷ {k}");
        }
        if t <= zero {
            assert!(kq.minus(&slack) <= t && t <= kq, "{t} ÷ {k}");
        }
        assert_eq!(r, t.minus(&kq), "{t} mod {k}");
        // Division by `-k` is `-(t ÷ k)`, and `t mod (-k) = t mod k`.
        let (negative_q, negative_r) = quotient(&t, &k.negate());
        assert_eq!(negative_q, q.negate(), "{t} ÷ -{k}");
        assert_eq!(negative_r, r, "{t} mod -{k}");
    }
}

#[test]
fn a8_division_by_a_term() {
    let mut rng = Rng::new(8);
    let zero = Int::zero();
    let one = int(1);
    let mut rows = [0u32; 4];
    for _ in 0..40_000 {
        let s = sample(&mut rng);
        let t = sample(&mut rng);
        if t.is_zero() {
            continue;
        }
        let (q, r) = quotient(&s, &t);
        let case = format!("{s} by {t}: q = {q}, r = {r}");
        if t >= one && s >= zero {
            rows[0] += 1;
            assert!(zero <= q && q <= s, "{case}");
            assert!(zero <= r && r <= t.minus(&one) && r <= s, "{case}");
        }
        if t >= one && s <= zero {
            rows[1] += 1;
            assert!(s <= q && q <= zero, "{case}");
            assert!(one.minus(&t) <= r && r <= zero && r >= s, "{case}");
        }
        if t <= one.negate() && s >= zero {
            rows[2] += 1;
            assert!(s.negate() <= q && q <= zero, "{case}");
            assert!(zero <= r && r <= t.negate().minus(&one), "{case}");
        }
        if t <= one.negate() && s <= zero {
            rows[3] += 1;
            assert!(zero <= q && q <= s.negate(), "{case}");
            assert!(t.plus(&one) <= r && r <= zero, "{case}");
        }
    }
    assert!(rows.iter().all(|count| *count > 1000), "{rows:?}");
}

#[test]
fn a9_products() {
    let mut rng = Rng::new(9);
    for _ in 0..40_000 {
        // Two intervals, and a point of each.
        let mut ends = [
            sample(&mut rng),
            sample(&mut rng),
            sample(&mut rng),
            sample(&mut rng),
        ];
        if ends[0] > ends[1] {
            ends.swap(0, 1);
        }
        if ends[2] > ends[3] {
            ends.swap(2, 3);
        }
        let [a, big_a, b, big_b] = ends;
        let within = |rng: &mut Rng, low: &Int, high: &Int| {
            let width = high.minus(low);
            let (_, offset) = sample(rng)
                .times(&sample(rng))
                .divide(&width.plus(&int(1)))
                .expect("a width that is not negative");
            let offset = if offset.is_negative() {
                offset.negate()
            } else {
                offset
            };
            low.plus(&offset)
        };
        let s = within(&mut rng, &a, &big_a);
        let t = within(&mut rng, &b, &big_b);
        assert!(a <= s && s <= big_a && b <= t && t <= big_b);
        let w = s.times(&t);
        let case = format!("{s} in [{a}, {big_a}] times {t} in [{b}, {big_b}]");
        let line = |x: &Int, y: &Int| x.times(&t).plus(&y.times(&s)).minus(&x.times(y));
        assert!(w >= line(&a, &b), "{case}");
        assert!(w >= line(&big_a, &big_b), "{case}");
        assert!(w <= line(&big_a, &b), "{case}");
        assert!(w <= line(&a, &big_b), "{case}");
    }
}

/// The same axioms as obligations of a core program, so that they pass
/// through the reader, `resolve` and the oracle's evaluation of formulas.
/// An implication `p ⇒ q` is written `¬p ∨ q`.
const AXIOMS: &str = "
control a7(t : i64) ■ { holds }
⊤ ▷ ⊢ ¬(t ≥ 0) ∨ (7 * (t ÷ 7) ≤ t ∧ t ≤ 7 * (t ÷ 7) + 6)
⊤ ▷ ⊢ ¬(t ≤ 0) ∨ (7 * (t ÷ 7) - 6 ≤ t ∧ t ≤ 7 * (t ÷ 7))
⊤ ▷ ⊢ t mod 7 = t - 7 * (t ÷ 7)
⊤ ▷ ⊢ t ÷ -7 = -(t ÷ 7) ∧ t mod -7 = t mod 7
⊤ ▷ ■ holds

control a8(s : i64, t : i64) ■ { holds }
⊤ ▷ ⊢ ¬(t ≥ 1 ∧ s ≥ 0) ∨ (0 ≤ s ÷ t ∧ s ÷ t ≤ s ∧ 0 ≤ s mod t ∧ s mod t ≤ t - 1 ∧ s mod t ≤ s)
⊤ ▷ ⊢ ¬(t ≥ 1 ∧ s ≤ 0) ∨ (s ≤ s ÷ t ∧ s ÷ t ≤ 0 ∧ -(t - 1) ≤ s mod t ∧ s mod t ≤ 0 ∧ s mod t ≥ s)
⊤ ▷ ⊢ ¬(t ≤ -1 ∧ s ≥ 0) ∨ (-s ≤ s ÷ t ∧ s ÷ t ≤ 0 ∧ 0 ≤ s mod t ∧ s mod t ≤ -t - 1)
⊤ ▷ ⊢ ¬(t ≤ -1 ∧ s ≤ 0) ∨ (0 ≤ s ÷ t ∧ s ÷ t ≤ -s ∧ t + 1 ≤ s mod t ∧ s mod t ≤ 0)
⊤ ▷ ■ holds

control a9(s : i8, t : i8) ■ { holds }
⊤ ▷ ⊢ s * t ≥ -128 * t + -128 * s - -128 * -128
⊤ ▷ ⊢ s * t ≥ 127 * t + 127 * s - 127 * 127
⊤ ▷ ⊢ s * t ≤ 127 * t + -128 * s - 127 * -128
⊤ ▷ ⊢ s * t ≤ -128 * t + 127 * s - -128 * 127
⊤ ▷ ■ holds

control a9_unsigned(s : u16, t : u16) ■ { holds }
⊤ ▷ ⊢ s * t ≥ 0 ∧ s * t ≤ 65535 * t ∧ s * t ≤ 65535 * s
⊤ ▷ ■ holds
";

#[test]
fn the_axioms_hold_as_obligations_in_the_oracle() {
    let program = load(AXIOMS).expect("the program of the axioms");
    let config = Config::default();
    let mut rng = Rng::new(79);
    let edges_64 = [
        i64::MIN,
        i64::MIN + 1,
        -8,
        -7,
        -1,
        0,
        1,
        6,
        7,
        i64::MAX - 1,
        i64::MAX,
    ];
    let draw = |rng: &mut Rng, low: i128, high: i128, edges: &[i64]| {
        if rng.chance(1, 3) {
            let edge = i128::from(*rng.pick(edges));
            return edge.clamp(low, high);
        }
        if rng.chance(1, 2) {
            return rng.range(-9, 9).clamp(low, high);
        }
        rng.range(low, high)
    };
    let check = |entry: &str, arguments: Vec<i128>| {
        let values: Vec<Value> = arguments
            .iter()
            .map(|value| Value::Int(int(*value)))
            .collect();
        let outcome = run(&program, entry, values, &mut Script::default(), &config);
        assert!(
            matches!(outcome.end, End::Done { .. }),
            "{entry}{arguments:?}: {}",
            outcome.end
        );
    };
    let (low, high) = (i128::from(i64::MIN), i128::from(i64::MAX));
    for _ in 0..5_000 {
        check("a7", vec![draw(&mut rng, low, high, &edges_64)]);
        check(
            "a8",
            vec![
                draw(&mut rng, low, high, &edges_64),
                draw(&mut rng, low, high, &edges_64),
            ],
        );
        check(
            "a9",
            vec![
                draw(&mut rng, -128, 127, &[-128, -127, 127, 126]),
                draw(&mut rng, -128, 127, &[-128, -127, 127, 126]),
            ],
        );
        check(
            "a9_unsigned",
            vec![
                draw(&mut rng, 0, 65535, &[65535, 65534, 255, 256]),
                draw(&mut rng, 0, 65535, &[65535, 65534, 255, 256]),
            ],
        );
    }
}
