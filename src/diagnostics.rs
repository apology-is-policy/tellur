//! Bounded diagnostic batches. Failed checks never expose a partial checked program.
use crate::Diagnostic;

pub const MAX_DIAGNOSTICS: usize = 32;

#[derive(Clone, Debug)]
pub struct Diagnostics<E = Diagnostic> {
    pub errors: Vec<E>,
    /// Collection stopped at the budget; remaining statements or controls may be unchecked.
    pub limit_reached: bool,
}
impl<E> From<E> for Diagnostics<E> {
    fn from(error: E) -> Self {
        Self {
            errors: vec![error],
            limit_reached: false,
        }
    }
}
impl<E> Diagnostics<E> {
    pub fn first(self) -> E {
        self.errors
            .into_iter()
            .next()
            .expect("a failed check has a diagnostic")
    }
    pub fn map<F>(self, transform: impl FnMut(E) -> F) -> Diagnostics<F> {
        Diagnostics {
            errors: self.errors.into_iter().map(transform).collect(),
            limit_reached: self.limit_reached,
        }
    }
}
impl<E: std::fmt::Display> std::fmt::Display for Diagnostics<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, error) in self.errors.iter().enumerate() {
            if index != 0 {
                writeln!(f)?;
            }
            write!(f, "{error}")?;
        }
        if self.limit_reached {
            write!(
                f,
                "\nnote: diagnostic limit reached; remaining statements or controls may be unchecked"
            )?;
        }
        Ok(())
    }
}
impl<E: std::error::Error> std::error::Error for Diagnostics<E> {}

pub type CheckResult<T, E = Diagnostic> = std::result::Result<T, Diagnostics<E>>;

/// Collect independent checks within one preparation phase. A failed phase
/// discards every successful value and prevents all dependent phases from running.
pub(crate) fn collect_checks<T, E>(
    checks: impl IntoIterator<Item = std::result::Result<T, E>>,
) -> CheckResult<Vec<T>, E> {
    let mut values = vec![];
    let mut errors = vec![];
    for result in checks {
        match result {
            Ok(value) => values.push(value),
            Err(error) => {
                errors.push(error);
                if errors.len() == MAX_DIAGNOSTICS {
                    return Err(Diagnostics {
                        errors,
                        limit_reached: true,
                    });
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(values)
    } else {
        Err(Diagnostics {
            errors,
            limit_reached: false,
        })
    }
}
