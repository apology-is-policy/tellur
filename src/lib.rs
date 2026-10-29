//! Experimental Tellur front end. Checked programs originate in the front end or validated expansion artifacts.
pub mod algebra;
pub mod bdd;
pub mod certificate;
mod check;
mod collection_support;
mod collections;
pub mod contracts;
pub mod diagnostics;
mod expand;
pub mod extract;
pub mod format;
mod generic_controls;
mod generics;
mod history;
mod inference;
pub mod ir;
pub mod launch;
mod lifetimes;
mod liveness;
pub mod lsp;
pub mod manifest;
pub mod modules;
pub mod native;
mod origins;
pub mod protocol;
pub mod runtime;
pub mod rust_host;
pub mod syntax;
mod traits;
mod verify;

pub use check::{check_source, compile};
pub use diagnostics::{CheckResult, Diagnostics};
pub use ir::CheckedProgram;
pub use modules::{check_file, compile_file};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}
/// Half-open source range in one-based Unicode scalar coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceRange {
    pub start: Span,
    pub end: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticNote {
    pub span: Span,
    pub path: Option<std::path::PathBuf>,
    pub message: String,
}
/// A satisfying structural history, not evidence of a feasible concrete input.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiagnosticWitness {
    StructuralMayReach {
        control: String,
        selected_paths: Vec<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub span: Span,
    /// Optional exclusive end; boxed to keep error results small on the success path.
    pub end: Option<Box<Span>>,
    pub message: String,
    pub notes: Vec<DiagnosticNote>,
    pub witness: Option<Box<DiagnosticWitness>>,
}
impl Diagnostic {
    pub fn new(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            end: None,
            message: message.into(),
            notes: vec![],
            witness: None,
        }
    }
    pub fn with_range(mut self, range: SourceRange) -> Self {
        self.span = range.start;
        self.end = Some(Box::new(range.end));
        self
    }
}
impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: {}: {}",
            self.span.line, self.span.column, self.code, self.message
        )?;
        for note in &self.notes {
            write!(f, "\n  note: ")?;
            if let Some(path) = &note.path {
                write!(f, "{}:", path.display())?;
            }
            write!(
                f,
                "{}:{}: {}",
                note.span.line, note.span.column, note.message
            )?;
        }
        Ok(())
    }
}
impl std::error::Error for Diagnostic {}
pub type Result<T> = std::result::Result<T, Diagnostic>;
