//! Native runtime linked into Tellur executables through the versioned C ABI.
#![allow(dead_code)]

#[path = "../src/native/runtime.rs"]
pub mod runtime;
use runtime::{R, ReadinessBackend};

#[cfg(feature = "polled")]
use runtime::Descriptor;
#[cfg(feature = "polled")]
include!("../src/native/mio_readiness.rs");

#[cfg(not(feature = "polled"))]
fn rust_readiness() -> R<Option<Box<dyn ReadinessBackend>>> {
    Ok(None)
}
