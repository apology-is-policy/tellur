//! Experimental Tellur front end. Checked programs originate in the front end or validated expansion artifacts.
pub mod algebra;
pub mod bdd;
pub mod certificate;
mod check;
mod collection_support;
mod collections;
mod expand;
pub mod extract;
mod generic_controls;
mod generics;
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

pub use check::compile;
pub use ir::CheckedProgram;
pub use modules::compile_file;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Span {
    pub line: usize,
    pub column: usize,
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
    pub message: String,
    pub notes: Vec<DiagnosticNote>,
    pub witness: Option<Box<DiagnosticWitness>>,
}
impl Diagnostic {
    pub fn new(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
            notes: vec![],
            witness: None,
        }
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
