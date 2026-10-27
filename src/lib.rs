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
mod lifetimes;
mod liveness;
pub mod lsp;
pub mod manifest;
pub mod modules;
pub mod native;
mod origins;
pub mod protocol;
pub mod runtime;
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
pub struct Diagnostic {
    pub code: &'static str,
    pub span: Span,
    pub message: String,
}
impl Diagnostic {
    pub fn new(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: {}: {}",
            self.span.line, self.span.column, self.code, self.message
        )
    }
}
impl std::error::Error for Diagnostic {}
pub type Result<T> = std::result::Result<T, Diagnostic>;
