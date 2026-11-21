//! The fact logic (`docs/v2/facts.md`): linear integer arithmetic, equality,
//! uninterpreted symbols, axiom instantiation (`[A1]` to `[A9]`, `[I10]`),
//! the decision procedure (`[J1]` to `[J6]`), certificates (`[V1]` to `[V5]`)
//! and counterexamples (`[M1]` to `[M6]`).

use crate::bdd::{self, Bdd, Formula};
use crate::int::Int;
use crate::typed::{ControlId, IntType};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

pub type TermId = usize;
pub type TheoryAtomId = usize;

/// Greatest common divisor of two magnitudes, always non-negative.
pub fn gcd(a: &Int, b: &Int) -> Int {
    let mut x = if a.is_negative() {
        a.negate()
    } else {
        a.clone()
    };
    let mut y = if b.is_negative() {
        b.negate()
    } else {
        b.clone()
    };
    while !y.is_zero() {
        let (_, r) = x.divide(&y).expect("non-zero divisor");
        x = y;
        y = r;
    }
    x
}

/// Floor quotient `⌊a / d⌋` toward `-∞`, for `d > 0`.
pub fn div_floor(a: &Int, d: &Int) -> Int {
    let (q, r) = a.divide(d).expect("positive divisor");
    if a.is_negative() && !r.is_zero() {
        q.minus(&Int::from_u64(1))
    } else {
        q
    }
}

/// Ceiling quotient `⌈a / d⌉` toward `+∞`, for `d > 0`.
pub fn div_ceil(a: &Int, d: &Int) -> Int {
    let (q, r) = a.divide(d).expect("positive divisor");
    if !a.is_negative() && !r.is_zero() {
        q.plus(&Int::from_u64(1))
    } else {
        q
    }
}

/// An atomic integer term (`[S6]`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AtomTerm {
    /// A logical name. `is_value_term` is true when `[A1]` gives it the range
    /// of `ty`.
    Var {
        name: String,
        ty: Option<IntType>,
        is_value_term: bool,
    },
    /// The constant `usize.max` ([A2]).
    UsizeMax,
    /// `t ÷ k` for a literal `k ≥ 1` ([A7]).
    DivConst(LinearTerm, Int),
    /// `div(s, t)` for a non-constant divisor ([A8]).
    Div(LinearTerm, LinearTerm),
    /// `rem(s, t)` for a non-constant divisor ([A8]).
    Rem(LinearTerm, LinearTerm),
    /// `mul(s, t)` with ordered operands and their declared intervals ([A9]).
    Mul {
        left: LinearTerm,
        right: LinearTerm,
        left_bounds: (Option<Int>, Option<Int>),
        right_bounds: (Option<Int>, Option<Int>),
    },
    /// Uninterpreted alternative `alt_f(args)` of a pure operation ([I10]).
    PureAlt(ControlId, Vec<LinearTerm>),
    /// Uninterpreted payload `pay_{f, alt, field}(args)` of a pure operation ([I10]).
    PurePay(ControlId, usize, usize, Vec<LinearTerm>),
}

/// A linear combination `constant + Σ coeff_i · u_i` ([S6]).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct LinearTerm {
    pub constant: Int,
    pub coeffs: BTreeMap<TermId, Int>,
}

impl LinearTerm {
    pub fn constant(value: Int) -> LinearTerm {
        LinearTerm {
            constant: value,
            coeffs: BTreeMap::new(),
        }
    }

    pub fn from_i128(value: i128) -> LinearTerm {
        LinearTerm::constant(Int::from_i128(value))
    }

    pub fn var(id: TermId) -> LinearTerm {
        let mut coeffs = BTreeMap::new();
        coeffs.insert(id, Int::from_u64(1));
        LinearTerm {
            constant: Int::zero(),
            coeffs,
        }
    }

    pub fn as_constant(&self) -> Option<Int> {
        if self.coeffs.is_empty() {
            Some(self.constant.clone())
        } else {
            None
        }
    }

    pub fn negate(&self) -> LinearTerm {
        LinearTerm {
            constant: self.constant.negate(),
            coeffs: self
                .coeffs
                .iter()
                .map(|(id, c)| (*id, c.negate()))
                .collect(),
        }
    }

    pub fn add(&self, other: &LinearTerm) -> LinearTerm {
        let mut coeffs = self.coeffs.clone();
        for (id, c) in &other.coeffs {
            let sum = coeffs.get(id).cloned().unwrap_or_default().plus(c);
            if sum.is_zero() {
                coeffs.remove(id);
            } else {
                coeffs.insert(*id, sum);
            }
        }
        LinearTerm {
            constant: self.constant.plus(&other.constant),
            coeffs,
        }
    }

    pub fn sub(&self, other: &LinearTerm) -> LinearTerm {
        self.add(&other.negate())
    }

    pub fn scale(&self, factor: &Int) -> LinearTerm {
        if factor.is_zero() {
            return LinearTerm::default();
        }
        LinearTerm {
            constant: self.constant.times(factor),
            coeffs: self
                .coeffs
                .iter()
                .map(|(id, c)| (*id, c.times(factor)))
                .collect(),
        }
    }

    pub fn eval(&self, model: &BTreeMap<TermId, Int>) -> Int {
        let mut sum = self.constant.clone();
        for (id, coeff) in &self.coeffs {
            let val = model.get(id).cloned().unwrap_or_default();
            sum = sum.plus(&coeff.times(&val));
        }
        sum
    }
}

/// A linear inequality row `Σ a_i · u_i ≤ rhs` (`docs/v2/facts.md`, section 7).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Row {
    pub coeffs: BTreeMap<TermId, Int>,
    pub rhs: Int,
}

impl Row {
    /// Builds the row `term ≤ 0`.
    pub fn le_zero(term: &LinearTerm) -> Row {
        Row {
            coeffs: term.coeffs.clone(),
            rhs: term.constant.negate(),
        }
    }

    /// Builds `left ≤ right`.
    pub fn le(left: &LinearTerm, right: &LinearTerm) -> Row {
        Row::le_zero(&left.sub(right))
    }

    /// Builds `left < right` (`left + 1 ≤ right`).
    pub fn lt(left: &LinearTerm, right: &LinearTerm) -> Row {
        Row::le(&left.add(&LinearTerm::from_i128(1)), right)
    }

    /// The negation `¬(Σ a_i · u_i ≤ rhs)` over the integers:
    /// `Σ (-a_i) · u_i ≤ -rhs - 1`.
    pub fn complement(&self) -> Row {
        Row {
            coeffs: self
                .coeffs
                .iter()
                .map(|(id, c)| (*id, c.negate()))
                .collect(),
            rhs: self.rhs.negate().minus(&Int::from_u64(1)),
        }
    }

    /// The positive common factor of all coefficients, or `1` if `coeffs` is
    /// empty.
    pub fn coeff_gcd(&self) -> Int {
        let mut g = Int::zero();
        for coeff in self.coeffs.values() {
            g = if g.is_zero() {
                if coeff.is_negative() {
                    coeff.negate()
                } else {
                    coeff.clone()
                }
            } else {
                gcd(&g, coeff)
            };
        }
        if g.is_zero() { Int::from_u64(1) } else { g }
    }

    /// Divides all coefficients by their GCD `g ≥ 1` and replaces `rhs` with
    /// `⌊rhs / g⌋`.
    pub fn normalize(&self) -> (Row, Int) {
        let g = self.coeff_gcd();
        if g <= Int::from_u64(1) {
            return (self.clone(), Int::from_u64(1));
        }
        let coeffs = self
            .coeffs
            .iter()
            .map(|(id, c)| (*id, c.divide(&g).expect("divisible").0))
            .collect();
        let rhs = div_floor(&self.rhs, &g);
        (Row { coeffs, rhs }, g)
    }

    pub fn combine(&self, mult_self: &Int, other: &Row, mult_other: &Int) -> Row {
        let mut coeffs = BTreeMap::new();
        for (id, c) in &self.coeffs {
            let prod = c.times(mult_self);
            if !prod.is_zero() {
                coeffs.insert(*id, prod);
            }
        }
        for (id, c) in &other.coeffs {
            let sum = coeffs
                .get(id)
                .cloned()
                .unwrap_or_default()
                .plus(&c.times(mult_other));
            if sum.is_zero() {
                coeffs.remove(id);
            } else {
                coeffs.insert(*id, sum);
            }
        }
        Row {
            coeffs,
            rhs: self.rhs.times(mult_self).plus(&other.rhs.times(mult_other)),
        }
    }

    pub fn holds(&self, model: &BTreeMap<TermId, Int>) -> bool {
        let mut lhs = Int::zero();
        for (id, coeff) in &self.coeffs {
            let val = model.get(id).cloned().unwrap_or_default();
            lhs = lhs.plus(&coeff.times(&val));
        }
        lhs <= self.rhs
    }
}

/// One step in the proof of a theory lemma (`docs/v2/facts.md`, [V3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProofStep {
    /// A row from literal `index` of the lemma's literal list.
    Row(usize),
    /// A non-negative integer linear combination of earlier derived rows.
    Combine(Vec<(usize, Int)>),
    /// Chvátal–Gomory rounding of an earlier row by common factor `divisor ≥ 1`.
    Round { source: usize, divisor: Int },
    /// Equality `s = t` merged from `s - t ≤ 0` and `t - s ≤ 0`.
    Merge { le_row: usize, ge_row: usize },
    /// Contradiction `0 ≤ b` with `b < 0` on the given row.
    Close(usize),
}

/// A theory lemma: a conjunction of theory literals that cannot hold together,
/// with its [V3] proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TheoryLemma {
    pub literals: Vec<(TheoryAtomId, bool)>,
    pub steps: Vec<ProofStep>,
}

/// An axiom instance added to a query ([J3], [V2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AxiomInstance {
    pub clause: &'static str,
    pub term: TermId,
    pub formula: Formula,
}

/// The certificate of a proved entailment ([V1] to [V5]).
#[derive(Clone, Debug)]
pub struct Certificate {
    pub bdd: Bdd,
    pub first_theory_var: usize,
    pub theory_atoms: Vec<Row>,
    pub query: Formula,
    pub instances: Vec<AxiomInstance>,
    pub lemmas: Vec<TheoryLemma>,
}

impl fmt::Display for Certificate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            formatter,
            "certificate (atoms: {}, instances: {}, lemmas: {})",
            self.theory_atoms.len(),
            self.instances.len(),
            self.lemmas.len()
        )?;
        for (idx, instance) in self.instances.iter().enumerate() {
            writeln!(
                formatter,
                "  instance {idx}: [{}] on term u{}",
                instance.clause, instance.term
            )?;
        }
        for (idx, lemma) in self.lemmas.iter().enumerate() {
            writeln!(
                formatter,
                "  lemma {idx} ({} literals, {} steps)",
                lemma.literals.len(),
                lemma.steps.len()
            )?;
        }
        Ok(())
    }
}

/// A validated counterexample model when an entailment fails ([M1] to [M6]).
#[derive(Clone, Debug)]
pub struct Counterexample {
    /// The history atoms that hold in the counterexample (`(site_name, alt_name)`).
    pub history: Vec<(String, String)>,
    /// Values of program names in the counterexample (`name -> value`).
    pub values: BTreeMap<String, String>,
    /// Remark when the failure depends on an operator the logic does not
    /// interpret completely ([M3]).
    pub assumed_remark: Option<String>,
    /// Whether the obligation is inside a loop body ([M4]).
    pub in_loop: bool,
    /// Raw Boolean assignment `h` and integer valuation `ν`.
    pub bool_model: BTreeMap<usize, bool>,
    pub int_model: BTreeMap<TermId, Int>,
}

impl fmt::Display for Counterexample {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.history.is_empty() {
            let path: Vec<String> = self
                .history
                .iter()
                .map(|(site, alt)| format!("{site}.{alt}"))
                .collect();
            let _ = write!(formatter, "on history [{}]", path.join(", "));
        } else {
            let _ = write!(formatter, "on the entry history");
        }
        if !self.values.is_empty() {
            let bindings: Vec<String> = self
                .values
                .iter()
                .map(|(name, val)| format!("{name} = {val}"))
                .collect();
            let _ = write!(formatter, " with {}", bindings.join(", "));
        }
        if let Some(remark) = &self.assumed_remark {
            let _ = write!(formatter, " ({remark})");
        }
        if self.in_loop {
            let _ = write!(
                formatter,
                " [values at the start of an iteration; the loop states nothing that excludes them]"
            );
        }
        Ok(())
    }
}

pub enum Decision {
    Proved(Certificate),
    Refuted(Box<Counterexample>),
}

#[derive(Clone, Debug)]
pub enum DefRhs {
    Linear(LinearTerm),
    BoolFormula(Formula),
}

/// The solver state for one control body.
#[derive(Clone, Debug)]
pub struct Solver {
    pub bdd: Bdd,
    /// History atom BDD variables `0..first_theory_var`, named `(site, alt)`.
    pub history_names: Vec<(String, String)>,
    pub first_theory_var: usize,
    /// Interned atomic integer terms.
    pub terms: Vec<AtomTerm>,
    term_ids: HashMap<AtomTerm, TermId>,
    pub usize_max_term: TermId,
    /// Canonical positive linear inequality rows, each mapped to BDD variable
    /// `first_theory_var + id`.
    pub theory_atoms: Vec<Row>,
    atom_ids: HashMap<Row, TheoryAtomId>,
    /// Structural formula `Ω` ([K4], [K5], [K8]).
    pub structure: Vec<Formula>,
    /// Conditional facts `G ⇒ φ` in `Ψ` ([N4]).
    pub facts: Vec<Formula>,
    /// Conditional definitions `G ⇒ u_id = rhs` for model completion.
    pub definitions: Vec<(Formula, TermId, DefRhs)>,
    /// Axiom instances instantiated so far ([J3]).
    pub instances: Vec<AxiomInstance>,
    instantiated_terms: HashSet<TermId>,
    instantiated_pairs: HashSet<(TermId, TermId)>,
    /// Theory lemmas learned across queries of this body.
    pub lemmas: Vec<TheoryLemma>,
}

impl Solver {
    pub fn new(history_names: Vec<(String, String)>) -> Solver {
        let first_theory_var = history_names.len();
        let mut solver = Solver {
            bdd: Bdd::new(),
            history_names,
            first_theory_var,
            terms: Vec::new(),
            term_ids: HashMap::new(),
            usize_max_term: 0,
            theory_atoms: Vec::new(),
            atom_ids: HashMap::new(),
            structure: Vec::new(),
            facts: Vec::new(),
            definitions: Vec::new(),
            instances: Vec::new(),
            instantiated_terms: HashSet::new(),
            instantiated_pairs: HashSet::new(),
            lemmas: Vec::new(),
        };
        solver.usize_max_term = solver.intern_term(AtomTerm::UsizeMax);
        solver
    }

    pub fn intern_term(&mut self, term: AtomTerm) -> TermId {
        if let Some(id) = self.term_ids.get(&term) {
            return *id;
        }
        let id = self.terms.len();
        self.terms.push(term.clone());
        self.term_ids.insert(term, id);
        id
    }

    pub fn fresh_var(&mut self, name: String, ty: Option<IntType>, is_value_term: bool) -> TermId {
        let unique = format!("{name}#{}", self.terms.len());
        let id = self.intern_term(AtomTerm::Var {
            name: unique,
            ty,
            is_value_term,
        });
        self.instantiate_all();
        id
    }

    /// Returns the BDD formula for `row` (`Σ a_i · u_i ≤ rhs`), interning its
    /// canonical positive form as a theory atom.
    pub fn row_formula(&mut self, row: Row) -> Formula {
        if row.coeffs.is_empty() {
            return if row.rhs >= Int::zero() {
                bdd::TRUE
            } else {
                bdd::FALSE
            };
        }
        let (norm, _) = row.normalize();
        let first_coeff = norm.coeffs.values().next().expect("non-empty");
        let (canonical, positive) = if !first_coeff.is_negative() {
            (norm, true)
        } else {
            (norm.complement(), false)
        };
        let atom_id = if let Some(id) = self.atom_ids.get(&canonical) {
            *id
        } else {
            let id = self.theory_atoms.len();
            self.theory_atoms.push(canonical.clone());
            self.atom_ids.insert(canonical, id);
            id
        };
        let bdd_var = self.first_theory_var + atom_id;
        self.bdd.literal(bdd_var, positive)
    }

    pub fn le(&mut self, left: &LinearTerm, right: &LinearTerm) -> Formula {
        self.row_formula(Row::le(left, right))
    }

    pub fn lt(&mut self, left: &LinearTerm, right: &LinearTerm) -> Formula {
        self.row_formula(Row::lt(left, right))
    }

    pub fn eq(&mut self, left: &LinearTerm, right: &LinearTerm) -> Formula {
        let a = self.le(left, right);
        let b = self.le(right, left);
        self.bdd.and(a, b)
    }

    pub fn ne(&mut self, left: &LinearTerm, right: &LinearTerm) -> Formula {
        let eq = self.eq(left, right);
        self.bdd.not(eq)
    }

    pub fn add_structure(&mut self, formula: Formula) {
        self.structure.push(formula);
    }

    pub fn add_fact(&mut self, guard: Formula, fact: Formula) {
        let imp = self.bdd.implies(guard, fact);
        self.facts.push(imp);
    }

    pub fn add_def_linear(&mut self, guard: Formula, var_id: TermId, rhs: LinearTerm) {
        let eq = self.eq(&LinearTerm::var(var_id), &rhs);
        self.add_fact(guard, eq);
        self.definitions.push((guard, var_id, DefRhs::Linear(rhs)));
    }

    pub fn add_def_bool(&mut self, guard: Formula, var_id: TermId, phi: Formula) {
        let is_one = self.le(&LinearTerm::from_i128(1), &LinearTerm::var(var_id));
        let iff = self.bdd.iff(is_one, phi);
        self.add_fact(guard, iff);
        self.definitions
            .push((guard, var_id, DefRhs::BoolFormula(phi)));
    }

    /// Instantiates `[A1]`, `[A2]`, `[A7]`, `[A8]`, `[A9]` and `[I10]` for all
    /// terms currently in `self.terms` ([J3]).
    pub fn instantiate_all(&mut self) {
        let mut idx = 0;
        while idx < self.terms.len() {
            if self.instantiated_terms.insert(idx) {
                let term = self.terms[idx].clone();
                let u = LinearTerm::var(idx);
                match term {
                    AtomTerm::Var {
                        ty: Some(int_ty),
                        is_value_term: true,
                        ..
                    } => {
                        let formula = if int_ty == IntType::Usize {
                            let low = self.le(&LinearTerm::from_i128(0), &u);
                            let umax = LinearTerm::var(self.usize_max_term);
                            let high = self.le(&u, &umax);
                            self.bdd.and(low, high)
                        } else {
                            let (lo, hi) = int_ty.literal_bounds();
                            let low = self.le(&LinearTerm::constant(lo), &u);
                            let high = self.le(&u, &LinearTerm::constant(hi));
                            self.bdd.and(low, high)
                        };
                        self.instances.push(AxiomInstance {
                            clause: "A1",
                            term: idx,
                            formula,
                        });
                    }
                    AtomTerm::UsizeMax => {
                        let lo = Int::power_of_two(32).minus(&Int::from_u64(1));
                        let hi = Int::power_of_two(64).minus(&Int::from_u64(1));
                        let low = self.le(&LinearTerm::constant(lo), &u);
                        let high = self.le(&u, &LinearTerm::constant(hi));
                        let formula = self.bdd.and(low, high);
                        self.instances.push(AxiomInstance {
                            clause: "A2",
                            term: idx,
                            formula,
                        });
                    }
                    AtomTerm::DivConst(t, k) => {
                        // [A7]:
                        // t ≥ 0 ⇒ k·q ≤ t ≤ k·q + (k - 1)
                        // t ≤ 0 ⇒ k·q - (k - 1) ≤ t ≤ k·q
                        let zero = LinearTerm::from_i128(0);
                        let kq = u.scale(&k);
                        let slack = LinearTerm::constant(k.minus(&Int::from_u64(1)));

                        let t_ge_0 = self.le(&zero, &t);
                        let pos_lo = self.le(&kq, &t);
                        let pos_hi = self.le(&t, &kq.add(&slack));
                        let pos_both = self.bdd.and(pos_lo, pos_hi);
                        let pos_imp = self.bdd.implies(t_ge_0, pos_both);

                        let t_le_0 = self.le(&t, &zero);
                        let neg_lo = self.le(&kq.sub(&slack), &t);
                        let neg_hi = self.le(&t, &kq);
                        let neg_both = self.bdd.and(neg_lo, neg_hi);
                        let neg_imp = self.bdd.implies(t_le_0, neg_both);

                        let formula = self.bdd.and(pos_imp, neg_imp);
                        self.instances.push(AxiomInstance {
                            clause: "A7",
                            term: idx,
                            formula,
                        });
                    }
                    AtomTerm::Div(s, t) => {
                        let formula = self.instantiate_a8_div(&u, &s, &t);
                        self.instances.push(AxiomInstance {
                            clause: "A8",
                            term: idx,
                            formula,
                        });
                    }
                    AtomTerm::Rem(s, t) => {
                        let formula = self.instantiate_a8_rem(&u, &s, &t);
                        self.instances.push(AxiomInstance {
                            clause: "A8",
                            term: idx,
                            formula,
                        });
                    }
                    AtomTerm::Mul {
                        left: s,
                        right: t,
                        left_bounds: (a, big_a),
                        right_bounds: (b, big_b),
                    } => {
                        let mut conj = bdd::TRUE;
                        // w ≥ a·t + b·s - a·b
                        if let (Some(a), Some(b)) = (&a, &b) {
                            let plane = t
                                .scale(a)
                                .add(&s.scale(b))
                                .sub(&LinearTerm::constant(a.times(b)));
                            let c = self.le(&plane, &u);
                            conj = self.bdd.and(conj, c);
                        }
                        // w ≥ A·t + B·s - A·B
                        if let (Some(big_a), Some(big_b)) = (&big_a, &big_b) {
                            let plane = t
                                .scale(big_a)
                                .add(&s.scale(big_b))
                                .sub(&LinearTerm::constant(big_a.times(big_b)));
                            let c = self.le(&plane, &u);
                            conj = self.bdd.and(conj, c);
                        }
                        // w ≤ A·t + b·s - A·b
                        if let (Some(big_a), Some(b)) = (&big_a, &b) {
                            let plane = t
                                .scale(big_a)
                                .add(&s.scale(b))
                                .sub(&LinearTerm::constant(big_a.times(b)));
                            let c = self.le(&u, &plane);
                            conj = self.bdd.and(conj, c);
                        }
                        // w ≤ a·t + B·s - a·B
                        if let (Some(a), Some(big_b)) = (&a, &big_b) {
                            let plane = t
                                .scale(a)
                                .add(&s.scale(big_b))
                                .sub(&LinearTerm::constant(a.times(big_b)));
                            let c = self.le(&u, &plane);
                            conj = self.bdd.and(conj, c);
                        }
                        self.instances.push(AxiomInstance {
                            clause: "A9",
                            term: idx,
                            formula: conj,
                        });
                    }
                    AtomTerm::Var { .. }
                    | AtomTerm::PureAlt(_, _)
                    | AtomTerm::PurePay(_, _, _, _) => {}
                }
            }
            idx += 1;
        }

        // Congruence instances for uninterpreted applications ([S5], [I10]).
        let len = self.terms.len();
        for i in 0..len {
            for j in (i + 1)..len {
                if self.instantiated_pairs.contains(&(i, j)) {
                    continue;
                }
                let args_pair: Option<(Vec<LinearTerm>, Vec<LinearTerm>)> =
                    match (&self.terms[i], &self.terms[j]) {
                        (AtomTerm::Div(s1, t1), AtomTerm::Div(s2, t2))
                        | (AtomTerm::Rem(s1, t1), AtomTerm::Rem(s2, t2))
                        | (
                            AtomTerm::Mul {
                                left: s1,
                                right: t1,
                                ..
                            },
                            AtomTerm::Mul {
                                left: s2,
                                right: t2,
                                ..
                            },
                        ) => Some((vec![s1.clone(), t1.clone()], vec![s2.clone(), t2.clone()])),
                        (AtomTerm::PureAlt(c1, a1), AtomTerm::PureAlt(c2, a2)) if c1 == c2 => {
                            Some((a1.clone(), a2.clone()))
                        }
                        (
                            AtomTerm::PurePay(c1, alt1, f1, a1),
                            AtomTerm::PurePay(c2, alt2, f2, a2),
                        ) if c1 == c2 && alt1 == alt2 && f1 == f2 => Some((a1.clone(), a2.clone())),
                        _ => None,
                    };
                if let Some((args1, args2)) = args_pair {
                    self.instantiated_pairs.insert((i, j));
                    let mut eq_args = bdd::TRUE;
                    for (a, b) in args1.iter().zip(&args2) {
                        let e = self.eq(a, b);
                        eq_args = self.bdd.and(eq_args, e);
                    }
                    let eq_res = self.eq(&LinearTerm::var(i), &LinearTerm::var(j));
                    let formula = self.bdd.implies(eq_args, eq_res);
                    self.instances.push(AxiomInstance {
                        clause: "I10",
                        term: i,
                        formula,
                    });
                }
            }
        }
    }

    fn instantiate_a8_div(&mut self, q: &LinearTerm, s: &LinearTerm, t: &LinearTerm) -> Formula {
        let zero = LinearTerm::from_i128(0);
        let one = LinearTerm::from_i128(1);
        let neg_one = LinearTerm::from_i128(-1);
        let neg_s = s.negate();

        let t_ge_1 = self.le(&one, t);
        let t_le_neg1 = self.le(t, &neg_one);
        let s_ge_0 = self.le(&zero, s);
        let s_le_0 = self.le(s, &zero);

        // Row 1: t ≥ 1 ∧ s ≥ 0 ⇒ 0 ≤ q ≤ s
        let c1 = self.bdd.and(t_ge_1, s_ge_0);
        let r1_a = self.le(&zero, q);
        let r1_b = self.le(q, s);
        let r1 = self.bdd.and(r1_a, r1_b);
        let imp1 = self.bdd.implies(c1, r1);

        // Row 2: t ≥ 1 ∧ s ≤ 0 ⇒ s ≤ q ≤ 0
        let c2 = self.bdd.and(t_ge_1, s_le_0);
        let r2_a = self.le(s, q);
        let r2_b = self.le(q, &zero);
        let r2 = self.bdd.and(r2_a, r2_b);
        let imp2 = self.bdd.implies(c2, r2);

        // Row 3: t ≤ -1 ∧ s ≥ 0 ⇒ -s ≤ q ≤ 0
        let c3 = self.bdd.and(t_le_neg1, s_ge_0);
        let r3_a = self.le(&neg_s, q);
        let r3_b = self.le(q, &zero);
        let r3 = self.bdd.and(r3_a, r3_b);
        let imp3 = self.bdd.implies(c3, r3);

        // Row 4: t ≤ -1 ∧ s ≤ 0 ⇒ 0 ≤ q ≤ -s
        let c4 = self.bdd.and(t_le_neg1, s_le_0);
        let r4_a = self.le(&zero, q);
        let r4_b = self.le(q, &neg_s);
        let r4 = self.bdd.and(r4_a, r4_b);
        let imp4 = self.bdd.implies(c4, r4);

        let a = self.bdd.and(imp1, imp2);
        let b = self.bdd.and(imp3, imp4);
        self.bdd.and(a, b)
    }

    fn instantiate_a8_rem(&mut self, r: &LinearTerm, s: &LinearTerm, t: &LinearTerm) -> Formula {
        let zero = LinearTerm::from_i128(0);
        let one = LinearTerm::from_i128(1);
        let neg_one = LinearTerm::from_i128(-1);
        let t_minus_1 = t.sub(&one);
        let one_minus_t = one.sub(t);
        let neg_t_minus_1 = t.negate().sub(&one);
        let t_plus_1 = t.add(&one);

        let t_ge_1 = self.le(&one, t);
        let t_le_neg1 = self.le(t, &neg_one);
        let s_ge_0 = self.le(&zero, s);
        let s_le_0 = self.le(s, &zero);

        // Row 1: t ≥ 1 ∧ s ≥ 0 ⇒ 0 ≤ r ≤ t - 1 ∧ r ≤ s
        let c1 = self.bdd.and(t_ge_1, s_ge_0);
        let r1_a = self.le(&zero, r);
        let r1_b = self.le(r, &t_minus_1);
        let r1_c = self.le(r, s);
        let r1_ab = self.bdd.and(r1_a, r1_b);
        let r1 = self.bdd.and(r1_ab, r1_c);
        let imp1 = self.bdd.implies(c1, r1);

        // Row 2: t ≥ 1 ∧ s ≤ 0 ⇒ -(t - 1) ≤ r ≤ 0 ∧ r ≥ s
        let c2 = self.bdd.and(t_ge_1, s_le_0);
        let r2_a = self.le(&one_minus_t, r);
        let r2_b = self.le(r, &zero);
        let r2_c = self.le(s, r);
        let r2_ab = self.bdd.and(r2_a, r2_b);
        let r2 = self.bdd.and(r2_ab, r2_c);
        let imp2 = self.bdd.implies(c2, r2);

        // Row 3: t ≤ -1 ∧ s ≥ 0 ⇒ 0 ≤ r ≤ -t - 1
        let c3 = self.bdd.and(t_le_neg1, s_ge_0);
        let r3_a = self.le(&zero, r);
        let r3_b = self.le(r, &neg_t_minus_1);
        let r3 = self.bdd.and(r3_a, r3_b);
        let imp3 = self.bdd.implies(c3, r3);

        // Row 4: t ≤ -1 ∧ s ≤ 0 ⇒ t + 1 ≤ r ≤ 0
        let c4 = self.bdd.and(t_le_neg1, s_le_0);
        let r4_a = self.le(&t_plus_1, r);
        let r4_b = self.le(r, &zero);
        let r4 = self.bdd.and(r4_a, r4_b);
        let imp4 = self.bdd.implies(c4, r4);

        let a = self.bdd.and(imp1, imp2);
        let b = self.bdd.and(imp3, imp4);
        self.bdd.and(a, b)
    }

    pub fn lemma_formula(&mut self, lemma: &TheoryLemma) -> Formula {
        let mut conj = bdd::TRUE;
        for &(atom_id, polarity) in &lemma.literals {
            let lit = self.bdd.literal(self.first_theory_var + atom_id, polarity);
            conj = self.bdd.and(conj, lit);
        }
        self.bdd.not(conj)
    }

    fn complete_int_model(
        &self,
        candidate_literals: &[(TheoryAtomId, bool)],
        assignment: &BTreeMap<usize, bool>,
        int_model: &mut BTreeMap<TermId, Int>,
    ) {
        let mut constrained = HashSet::new();
        for &(atom_id, _) in candidate_literals {
            for &var in self.theory_atoms[atom_id].coeffs.keys() {
                constrained.insert(var);
            }
        }
        let u32_max = Int::power_of_two(32).minus(&Int::from_u64(1));

        for _ in 0..8 {
            let mut changed = false;
            for (id, term) in self.terms.iter().enumerate() {
                if constrained.contains(&id) {
                    continue;
                }
                let val = match term {
                    AtomTerm::UsizeMax => Some(u32_max.clone()),
                    AtomTerm::DivConst(t, k) => {
                        Some(t.eval(int_model).divide(k).map_or(Int::zero(), |p| p.0))
                    }
                    AtomTerm::Div(s, t) => Some(
                        s.eval(int_model)
                            .divide(&t.eval(int_model))
                            .map_or(Int::zero(), |p| p.0),
                    ),
                    AtomTerm::Rem(s, t) => Some(
                        s.eval(int_model)
                            .divide(&t.eval(int_model))
                            .map_or(Int::zero(), |p| p.1),
                    ),
                    AtomTerm::Mul { left, right, .. } => {
                        Some(left.eval(int_model).times(&right.eval(int_model)))
                    }
                    AtomTerm::Var { .. }
                    | AtomTerm::PureAlt(_, _)
                    | AtomTerm::PurePay(_, _, _, _) => None,
                };
                if let Some(v) = val
                    && int_model.get(&id) != Some(&v)
                {
                    int_model.insert(id, v);
                    changed = true;
                }
            }

            let full = self.complete_bool_model(assignment, int_model);
            for (guard, var_id, rhs) in &self.definitions {
                if constrained.contains(var_id) || !self.bdd.evaluate(*guard, &full) {
                    continue;
                }
                let v = match rhs {
                    DefRhs::Linear(term) => term.eval(int_model),
                    DefRhs::BoolFormula(phi) => {
                        let b = self.bdd.evaluate(*phi, &full);
                        Int::from_u64(u64::from(b))
                    }
                };
                if int_model.get(var_id) != Some(&v) {
                    int_model.insert(*var_id, v);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn complete_bool_model(
        &self,
        assignment: &BTreeMap<usize, bool>,
        int_model: &BTreeMap<TermId, Int>,
    ) -> BTreeMap<usize, bool> {
        let mut full = BTreeMap::new();
        for var in 0..self.first_theory_var {
            full.insert(var, assignment.get(&var).copied().unwrap_or(false));
        }
        for (atom_id, atom) in self.theory_atoms.iter().enumerate() {
            let var = self.first_theory_var + atom_id;
            let holds = atom.holds(int_model);
            full.insert(var, holds);
        }
        full
    }

    /// Decides whether `negated_goal` (`¬F` for [N1], `G ∧ ¬φ` for [N2]) is
    /// unsatisfiable under `Ω` and `Ψ` (`docs/v2/facts.md`, section 7).
    pub fn decide(
        &mut self,
        negated_goal: Formula,
        visible_vars: &[(String, TermId, bool)],
        in_loop: bool,
    ) -> Decision {
        self.instantiate_all();
        let mut query = negated_goal;
        let mut working = negated_goal;
        let mut added_structure = vec![false; self.structure.len()];
        let mut added_facts = vec![false; self.facts.len()];
        let mut added_instances = vec![false; self.instances.len()];
        let mut added_lemmas = vec![false; self.lemmas.len()];
        let mut used_instances = Vec::new();
        let mut used_lemmas = Vec::new();

        for (idx, &s) in self.structure.iter().enumerate() {
            query = self.bdd.and(query, s);
            working = self.bdd.and(working, s);
            added_structure[idx] = true;
        }

        'search: while let Some(assignment) = self.bdd.witness(working) {
            for (idx, added) in added_lemmas.iter_mut().enumerate() {
                if *added {
                    continue;
                }
                let matches_all = self.lemmas[idx].literals.iter().all(|&(atom_id, pol)| {
                    assignment.get(&(self.first_theory_var + atom_id)).copied() == Some(pol)
                });
                if matches_all {
                    *added = true;
                    let lemma = self.lemmas[idx].clone();
                    let lf = self.lemma_formula(&lemma);
                    working = self.bdd.and(working, lf);
                    used_lemmas.push(lemma);
                    continue 'search;
                }
            }

            let mut candidate_literals = Vec::new();
            for (&bdd_var, &polarity) in &assignment {
                if bdd_var >= self.first_theory_var {
                    candidate_literals.push((bdd_var - self.first_theory_var, polarity));
                }
            }

            match solve_candidate(&candidate_literals, &self.theory_atoms) {
                CandidateResult::Unsat(lemma) => {
                    let lf = self.lemma_formula(&lemma);
                    working = self.bdd.and(working, lf);
                    self.lemmas.push(lemma.clone());
                    added_lemmas.push(true);
                    used_lemmas.push(lemma);
                }
                CandidateResult::Sat(mut int_model) => {
                    self.complete_int_model(&candidate_literals, &assignment, &mut int_model);
                    let full_bool_model = self.complete_bool_model(&assignment, &int_model);

                    for (idx, &s) in self.structure.iter().enumerate() {
                        if !added_structure[idx] && !self.bdd.evaluate(s, &full_bool_model) {
                            added_structure[idx] = true;
                            query = self.bdd.and(query, s);
                            working = self.bdd.and(working, s);
                            continue 'search;
                        }
                    }

                    for (idx, &f) in self.facts.iter().enumerate() {
                        if !added_facts[idx] && !self.bdd.evaluate(f, &full_bool_model) {
                            added_facts[idx] = true;
                            query = self.bdd.and(query, f);
                            working = self.bdd.and(working, f);
                            continue 'search;
                        }
                    }

                    for (idx, added) in added_instances.iter_mut().enumerate() {
                        if !*added
                            && !self
                                .bdd
                                .evaluate(self.instances[idx].formula, &full_bool_model)
                        {
                            *added = true;
                            let inst = self.instances[idx].clone();
                            working = self.bdd.and(working, inst.formula);
                            used_instances.push(inst);
                            continue 'search;
                        }
                    }

                    let cex = self.build_counterexample(
                        query,
                        full_bool_model,
                        int_model,
                        visible_vars,
                        in_loop,
                    );
                    return Decision::Refuted(Box::new(cex));
                }
            }
        }

        Decision::Proved(Certificate {
            bdd: self.bdd.clone(),
            first_theory_var: self.first_theory_var,
            theory_atoms: self.theory_atoms.clone(),
            query,
            instances: used_instances,
            lemmas: used_lemmas,
        })
    }

    fn build_counterexample(
        &self,
        query: Formula,
        bool_model: BTreeMap<usize, bool>,
        int_model: BTreeMap<TermId, Int>,
        visible_vars: &[(String, TermId, bool)],
        in_loop: bool,
    ) -> Counterexample {
        // [M1]: Verify that the query evaluates to true in the model.
        assert!(
            self.bdd.evaluate(query, &bool_model),
            "counterexample must satisfy the Boolean query"
        );
        for (&bdd_var, &polarity) in &bool_model {
            if bdd_var >= self.first_theory_var {
                let atom = &self.theory_atoms[bdd_var - self.first_theory_var];
                let row = if polarity {
                    atom.clone()
                } else {
                    atom.complement()
                };
                assert!(
                    row.holds(&int_model),
                    "counterexample integer valuation must satisfy every active theory literal"
                );
            }
        }

        let mut history = Vec::new();
        for (idx, (site, alt)) in self.history_names.iter().enumerate() {
            if bool_model.get(&idx).copied().unwrap_or(false) && !site.starts_with('_') {
                history.push((site.clone(), alt.clone()));
            }
        }

        let mut values = BTreeMap::new();
        for (name, term_id, is_bool) in visible_vars {
            if name.starts_with('_') {
                continue;
            }
            let val = int_model.get(term_id).cloned().unwrap_or_default();
            let shown = if *is_bool {
                (!val.is_zero()).to_string()
            } else {
                val.to_string()
            };
            values.insert(name.clone(), shown);
        }

        // [M3]: Check if any nonlinear symbol (`Mul`, `Div`, `Rem`) was given
        // an assumed value in `int_model` that differs from the real operation
        // on the model's arguments.
        let mut assumed_remark = None;
        for (id, term) in self.terms.iter().enumerate() {
            let Some(chosen) = int_model.get(&id) else {
                continue;
            };
            match term {
                AtomTerm::Mul { left, right, .. } => {
                    let real = left.eval(&int_model).times(&right.eval(&int_model));
                    if *chosen != real {
                        assumed_remark = Some(
                            "depends on `*`, which the linear logic bounds only by declared intervals"
                                .to_string(),
                        );
                        break;
                    }
                }
                AtomTerm::Div(s, t) => {
                    let denom = t.eval(&int_model);
                    if let Some((q, _)) = s.eval(&int_model).divide(&denom)
                        && *chosen != q
                    {
                        assumed_remark = Some(
                            "depends on `÷` by a variable, which the linear logic bounds only by sign and magnitude"
                                .to_string(),
                        );
                        break;
                    }
                }
                AtomTerm::Rem(s, t) => {
                    let denom = t.eval(&int_model);
                    if let Some((_, r)) = s.eval(&int_model).divide(&denom)
                        && *chosen != r
                    {
                        assumed_remark = Some(
                            "depends on `mod` by a variable, which the linear logic bounds only by sign and magnitude"
                                .to_string(),
                        );
                        break;
                    }
                }
                _ => {}
            }
        }

        Counterexample {
            history,
            values,
            assumed_remark,
            in_loop,
            bool_model,
            int_model,
        }
    }
}

enum CandidateResult {
    Unsat(TheoryLemma),
    Sat(BTreeMap<TermId, Int>),
}

/// Decides whether `literals` can hold simultaneously over the integers.
/// Either produces a pruned [`TheoryLemma`] with its `[V3]` proof or a
/// satisfying integer valuation `ν`.
fn solve_candidate(literals: &[(TheoryAtomId, bool)], theory_atoms: &[Row]) -> CandidateResult {
    let mut steps = Vec::new();
    let mut active: Vec<(Row, usize)> = Vec::new();
    let mut seen = HashSet::new();

    for (idx, &(atom_id, polarity)) in literals.iter().enumerate() {
        let base = &theory_atoms[atom_id];
        let row = if polarity {
            base.clone()
        } else {
            base.complement()
        };
        if let Some(close_step) =
            push_derived_row(row, ProofStep::Row(idx), &mut steps, &mut active, &mut seen)
        {
            let _ = close_step;
            return CandidateResult::Unsat(prune_lemma(literals, &steps));
        }
    }

    let mut eliminated: Vec<(TermId, Vec<Row>, Vec<Row>)> = Vec::new();

    while active.iter().any(|(row, _)| !row.coeffs.is_empty()) {
        // Pick a variable to eliminate: prefer one with 0 upper or 0 lower
        // bounds, then one with unit coefficients and fewest pairs.
        let mut vars = BTreeSet::new();
        for (row, _) in &active {
            for &var in row.coeffs.keys() {
                vars.insert(var);
            }
        }
        let chosen_var = vars
            .into_iter()
            .min_by_key(|&var| {
                let mut pos = 0usize;
                let mut neg = 0usize;
                let mut all_unit = true;
                for (row, _) in &active {
                    if let Some(c) = row.coeffs.get(&var) {
                        if c.is_negative() {
                            neg += 1;
                            if *c != Int::from_i128(-1) {
                                all_unit = false;
                            }
                        } else {
                            pos += 1;
                            if *c != Int::from_u64(1) {
                                all_unit = false;
                            }
                        }
                    }
                }
                let pairs = pos * neg;
                let unit_penalty = usize::from(!all_unit && pairs > 0);
                (pairs > 0, unit_penalty, pairs, var)
            })
            .expect("at least one variable");

        let mut pos = Vec::new();
        let mut neg = Vec::new();
        let mut next_active = Vec::new();

        for (row, step_id) in active {
            match row.coeffs.get(&chosen_var) {
                None => next_active.push((row, step_id)),
                Some(c) if c.is_negative() => neg.push((row, step_id)),
                Some(_) => pos.push((row, step_id)),
            }
        }

        for (p_row, p_id) in &pos {
            let c_pos = p_row.coeffs[&chosen_var].clone();
            for (n_row, n_id) in &neg {
                let c_neg = n_row.coeffs[&chosen_var].negate();
                let g = gcd(&c_pos, &c_neg);
                let m_pos = c_neg.divide(&g).expect("divisible").0;
                let m_neg = c_pos.divide(&g).expect("divisible").0;
                let combined = p_row.combine(&m_pos, n_row, &m_neg);
                let step = ProofStep::Combine(vec![(*p_id, m_pos), (*n_id, m_neg)]);
                if push_derived_row(combined, step, &mut steps, &mut next_active, &mut seen)
                    .is_some()
                {
                    return CandidateResult::Unsat(prune_lemma(literals, &steps));
                }
            }
        }

        eliminated.push((
            chosen_var,
            pos.into_iter().map(|(r, _)| r).collect(),
            neg.into_iter().map(|(r, _)| r).collect(),
        ));
        active = next_active;
    }

    // Back-substitute to construct an integer valuation `ν` ([M1], [M6]).
    let mut model = BTreeMap::new();
    while let Some((var, pos_rows, neg_rows)) = eliminated.pop() {
        let mut upper: Option<Int> = None;
        for row in &pos_rows {
            let c_pos = &row.coeffs[&var];
            let mut rest = Int::zero();
            for (&other_var, coeff) in &row.coeffs {
                if other_var != var {
                    let val = model.get(&other_var).cloned().unwrap_or_default();
                    rest = rest.plus(&coeff.times(&val));
                }
            }
            let bound = div_floor(&row.rhs.minus(&rest), c_pos);
            upper = Some(match upper {
                None => bound,
                Some(prev) => prev.min(bound),
            });
        }

        let mut lower: Option<Int> = None;
        for row in &neg_rows {
            let c_neg = row.coeffs[&var].negate();
            let mut rest = Int::zero();
            for (&other_var, coeff) in &row.coeffs {
                if other_var != var {
                    let val = model.get(&other_var).cloned().unwrap_or_default();
                    rest = rest.plus(&coeff.times(&val));
                }
            }
            let bound = div_ceil(&rest.minus(&row.rhs), &c_neg);
            lower = Some(match lower {
                None => bound,
                Some(prev) => prev.max(bound),
            });
        }

        let chosen = match (lower, upper) {
            (Some(lo), Some(hi)) => {
                if Int::zero() >= lo && Int::zero() <= hi {
                    Int::zero()
                } else if Int::zero() < lo {
                    lo
                } else {
                    hi
                }
            }
            (Some(lo), None) => lo.max(Int::zero()),
            (None, Some(hi)) => hi.min(Int::zero()),
            (None, None) => Int::zero(),
        };
        model.insert(var, chosen);
    }

    CandidateResult::Sat(model)
}

fn push_derived_row(
    row: Row,
    step: ProofStep,
    steps: &mut Vec<ProofStep>,
    active: &mut Vec<(Row, usize)>,
    seen: &mut HashSet<Row>,
) -> Option<usize> {
    let step_id = steps.len();
    steps.push(step);
    if row.coeffs.is_empty() {
        if row.rhs.is_negative() {
            let close_id = steps.len();
            steps.push(ProofStep::Close(step_id));
            return Some(close_id);
        }
        return None;
    }

    let (rounded, divisor) = row.normalize();
    let (final_row, final_id) = if divisor > Int::from_u64(1) {
        let round_id = steps.len();
        steps.push(ProofStep::Round {
            source: step_id,
            divisor,
        });
        (rounded, round_id)
    } else {
        (row, step_id)
    };

    if final_row.coeffs.is_empty() {
        if final_row.rhs.is_negative() {
            let close_id = steps.len();
            steps.push(ProofStep::Close(final_id));
            return Some(close_id);
        }
        return None;
    }

    if seen.insert(final_row.clone()) {
        active.push((final_row, final_id));
    }
    None
}

/// Extracts only the steps and input literals that participate in deriving the
/// final `ProofStep::Close` step.
fn prune_lemma(literals: &[(TheoryAtomId, bool)], steps: &[ProofStep]) -> TheoryLemma {
    let last = steps.len() - 1;
    let mut used = vec![false; steps.len()];
    used[last] = true;
    for idx in (0..steps.len()).rev() {
        if !used[idx] {
            continue;
        }
        match &steps[idx] {
            ProofStep::Row(_) => {}
            ProofStep::Combine(items) => {
                for &(src, _) in items {
                    used[src] = true;
                }
            }
            ProofStep::Round { source, .. } | ProofStep::Close(source) => {
                used[*source] = true;
            }
            ProofStep::Merge { le_row, ge_row } => {
                used[*le_row] = true;
                used[*ge_row] = true;
            }
        }
    }

    let mut lit_remap: BTreeMap<usize, usize> = BTreeMap::new();
    let mut pruned_literals = Vec::new();
    let mut step_remap = vec![0usize; steps.len()];
    let mut pruned_steps = Vec::new();

    for (idx, step) in steps.iter().enumerate() {
        if !used[idx] {
            continue;
        }
        let new_id = pruned_steps.len();
        step_remap[idx] = new_id;
        let mapped = match step {
            ProofStep::Row(lit_idx) => {
                let new_lit = *lit_remap.entry(*lit_idx).or_insert_with(|| {
                    let id = pruned_literals.len();
                    pruned_literals.push(literals[*lit_idx]);
                    id
                });
                ProofStep::Row(new_lit)
            }
            ProofStep::Combine(items) => ProofStep::Combine(
                items
                    .iter()
                    .map(|(src, mult)| (step_remap[*src], mult.clone()))
                    .collect(),
            ),
            ProofStep::Round { source, divisor } => ProofStep::Round {
                source: step_remap[*source],
                divisor: divisor.clone(),
            },
            ProofStep::Merge { le_row, ge_row } => ProofStep::Merge {
                le_row: step_remap[*le_row],
                ge_row: step_remap[*ge_row],
            },
            ProofStep::Close(source) => ProofStep::Close(step_remap[*source]),
        };
        pruned_steps.push(mapped);
    }

    TheoryLemma {
        literals: pruned_literals,
        steps: pruned_steps,
    }
}
