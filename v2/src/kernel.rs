//! The independent certificate kernel (`docs/v2/facts.md`, section 8,
//! `[V1]` to `[V4]`).
//!
//! The compiler accepts an entailment only after [`validate`] checks its
//! [`Certificate`]. This module shares no search code with [`crate::facts`]:
//! it checks that every [`ProofStep`] in every [`TheoryLemma`] is sound by
//! exact integer arithmetic and that the conjunction of the query, the axiom
//! instances and the lemmas reduces to `FALSE` in the binary decision diagram.

use crate::bdd;
use crate::facts::{Certificate, ProofStep, Row, TheoryLemma, div_floor};
use crate::int::Int;
use std::collections::BTreeMap;

/// Validates a [`TheoryLemma`] against `theory_atoms` ([V3]).
pub fn validate_lemma(lemma: &TheoryLemma, theory_atoms: &[Row]) -> Result<(), String> {
    if lemma.steps.is_empty() {
        return Err("a lemma proof cannot be empty".to_string());
    }
    let mut derived: Vec<Row> = Vec::with_capacity(lemma.steps.len());
    let mut closed = false;

    for (index, step) in lemma.steps.iter().enumerate() {
        match step {
            ProofStep::Row(lit_idx) => {
                let Some(&(atom_id, polarity)) = lemma.literals.get(*lit_idx) else {
                    return Err(format!(
                        "step {index}: literal index {lit_idx} out of range"
                    ));
                };
                let Some(atom) = theory_atoms.get(atom_id) else {
                    return Err(format!("step {index}: theory atom {atom_id} out of range"));
                };
                let row = if polarity {
                    atom.clone()
                } else {
                    atom.complement()
                };
                derived.push(row);
            }
            ProofStep::Combine(terms) => {
                if terms.is_empty() {
                    return Err(format!("step {index}: empty Combine step"));
                }
                let mut coeffs: BTreeMap<usize, Int> = BTreeMap::new();
                let mut rhs = Int::zero();
                for (src, multiplier) in terms {
                    if *src >= derived.len() {
                        return Err(format!("step {index}: Combine refers to future step {src}"));
                    }
                    if multiplier.is_negative() {
                        return Err(format!(
                            "step {index}: Combine multiplier {multiplier} is negative"
                        ));
                    }
                    let row = &derived[*src];
                    for (&var, coeff) in &row.coeffs {
                        let sum = coeffs
                            .get(&var)
                            .cloned()
                            .unwrap_or_default()
                            .plus(&coeff.times(multiplier));
                        if sum.is_zero() {
                            coeffs.remove(&var);
                        } else {
                            coeffs.insert(var, sum);
                        }
                    }
                    rhs = rhs.plus(&row.rhs.times(multiplier));
                }
                derived.push(Row { coeffs, rhs });
            }
            ProofStep::Round { source, divisor } => {
                if *source >= derived.len() {
                    return Err(format!(
                        "step {index}: Round refers to future step {source}"
                    ));
                }
                if *divisor < Int::from_u64(1) {
                    return Err(format!(
                        "step {index}: Round divisor {divisor} is not positive"
                    ));
                }
                let src_row = &derived[*source];
                let mut coeffs = BTreeMap::new();
                for (&var, coeff) in &src_row.coeffs {
                    let Some((q, r)) = coeff.divide(divisor) else {
                        return Err(format!("step {index}: zero divisor in Round"));
                    };
                    if !r.is_zero() {
                        return Err(format!(
                            "step {index}: Round divisor {divisor} does not divide coefficient {coeff}"
                        ));
                    }
                    if !q.is_zero() {
                        coeffs.insert(var, q);
                    }
                }
                let rhs = div_floor(&src_row.rhs, divisor);
                derived.push(Row { coeffs, rhs });
            }
            ProofStep::Merge { le_row, ge_row } => {
                if *le_row >= derived.len() || *ge_row >= derived.len() {
                    return Err(format!("step {index}: Merge refers to future step"));
                }
                let a = &derived[*le_row];
                let b = &derived[*ge_row];
                let sum = a.combine(&Int::from_u64(1), b, &Int::from_u64(1));
                if !sum.coeffs.is_empty() || !sum.rhs.is_zero() {
                    return Err(format!(
                        "step {index}: Merge rows are not opposite inequalities"
                    ));
                }
                derived.push(a.clone());
            }
            ProofStep::Close(source) => {
                if *source >= derived.len() {
                    return Err(format!(
                        "step {index}: Close refers to future step {source}"
                    ));
                }
                let row = &derived[*source];
                if !row.coeffs.is_empty() || !row.rhs.is_negative() {
                    return Err(format!(
                        "step {index}: Close row is not `0 ≤ b` with `b < 0`"
                    ));
                }
                closed = true;
                derived.push(row.clone());
            }
        }
    }

    if !closed {
        return Err("the lemma proof does not end with a valid Close step".to_string());
    }
    Ok(())
}

/// Validates a complete [`Certificate`] ([V1] to [V4]).
pub fn validate(certificate: &Certificate) -> Result<(), String> {
    const VALID_CLAUSES: [&str; 6] = ["A1", "A2", "A7", "A8", "A9", "I10"];
    for instance in &certificate.instances {
        if !VALID_CLAUSES.contains(&instance.clause) {
            return Err(format!(
                "`{}` is not a recognized axiom clause",
                instance.clause
            ));
        }
    }

    for (index, lemma) in certificate.lemmas.iter().enumerate() {
        validate_lemma(lemma, &certificate.theory_atoms)
            .map_err(|err| format!("lemma {index}: {err}"))?;
    }

    // [V2]: Recompute the Boolean step: the conjunction of the query, the
    // instances and the negated lemmas must be FALSE.
    let mut bdd = certificate.bdd.clone();
    let mut total = certificate.query;
    for instance in &certificate.instances {
        total = bdd.and(total, instance.formula);
    }
    for lemma in &certificate.lemmas {
        let mut conj = bdd::TRUE;
        for &(atom_id, polarity) in &lemma.literals {
            let lit = bdd.literal(certificate.first_theory_var + atom_id, polarity);
            conj = bdd.and(conj, lit);
        }
        let not_conj = bdd.not(conj);
        total = bdd.and(total, not_conj);
    }
    if total != bdd::FALSE {
        return Err("the conjunction of the query, instances and lemmas is not FALSE".to_string());
    }
    Ok(())
}
