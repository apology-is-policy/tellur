//! Villanelle v2.
//!
//! The specification is in `docs/v2/`. This crate is built beside v1 and
//! shares no code with it. It grows by slices; each slice passes through the
//! oracle, the checker and the backend before the next begins.
//!
//! | Module | Holds |
//! | --- | --- |
//! | `int` | Mathematical integers |
//! | `syntax`, `text` | The core text form: its tree, printer and reader |
//! | `typed`, `resolve` | The typed core, and the way to it from the tree |
//! | `oracle`, `host` | The instrumented semantics and the hosts of a run |
//! | `script` | Test directives written beside a program |

pub mod host;
pub mod int;
pub mod oracle;
pub mod resolve;
pub mod script;
pub mod syntax;
pub mod text;
pub mod typed;

/// Reads and resolves a program in the core text form.
pub fn load(source: &str) -> Result<typed::Program, String> {
    let tree = text::parse(source).map_err(|error| error.to_string())?;
    resolve::resolve(&tree).map_err(|error| error.to_string())
}
