//! Test schedules over the exact production callback emitter.
use super::*;

fn source(program: &CheckedProgram, entry: &str) -> Result<String> {
    let mut source = program.native_source(entry)?;
    source = source.replace("run_entry(host,args,&MODEL_", "drive(host,args,&MODEL_");
    // Exercise the exact shared implementation embedded by normal generation.
    // Only compiled fixture entry points and explicit test schedules are injected.
    assert_eq!(source.matches(EXECUTION).count(), 1);
    assert_eq!(source.matches(CONTEXTS).count(), 1);
    source = source.replacen(
        CONTEXTS,
        &format!(
            "{CONTEXTS}\n{}\n{}\nmod sibling_model {{\n{}\n}}\nmod result_model {{\n{}\n}}\npub use result_model::*;\npub use sibling_model::*;\n",
            include_str!("compiled_model.rs"),
            include_str!("context_model.rs"),
            include_str!("sibling_model.rs"),
            include_str!("result_model.rs")
        ),
        1,
    );
    source = source.replacen(
        "pub use contexts::{ContextKey, ContextPoll, Contexts, Readiness, run_entry};",
        "pub use contexts::*;",
        1,
    );
    Ok(source)
}

#[cfg(test)]
#[path = "resume_model_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "resume_model_resource_tests.rs"]
mod resource_tests;

#[cfg(test)]
#[path = "context_model_tests.rs"]
mod context_tests;

#[cfg(test)]
#[path = "sibling_model_tests.rs"]
mod sibling_tests;

#[cfg(test)]
#[path = "result_model_tests.rs"]
mod result_tests;

#[cfg(test)]
#[path = "mixed_await_model_tests.rs"]
mod mixed_await_tests;
