use std::path::Path;
use tellur::{CheckedProgram, Result, native::NativeBuild};

#[allow(dead_code)]
pub trait BuildTestExecutable {
    fn build_for_test(&self, entry: &str, output: &Path, opt: u8) -> Result<NativeBuild>;
}

#[allow(dead_code)]
pub fn test_backend() -> &'static str {
    match std::env::var("TELLUR_TEST_BACKEND").as_deref() {
        Ok("rust") => "rust",
        Err(std::env::VarError::NotPresent) => tellur::native::DEFAULT_BACKEND,
        #[cfg(feature = "llvm")]
        Ok("llvm") => "llvm",
        #[cfg(not(feature = "llvm"))]
        Ok("llvm") => panic!("TELLUR_TEST_BACKEND=llvm requires cargo test --features llvm"),
        value => panic!("expected rust or llvm for TELLUR_TEST_BACKEND, got {value:?}"),
    }
}

impl BuildTestExecutable for CheckedProgram {
    fn build_for_test(&self, entry: &str, output: &Path, opt: u8) -> Result<NativeBuild> {
        match test_backend() {
            "rust" => self.build_native(entry, output, opt),
            #[cfg(feature = "llvm")]
            "llvm" => self.build_llvm(entry, output, opt),
            _ => unreachable!("test_backend checks the selected backend"),
        }
    }
}

// Differential tests compare Villanelle outcomes, not application status policy.
// Explicitly configure their standalone harness entries to report completion as 0.
#[allow(dead_code)]
pub fn scenario_executable(program: &tellur::CheckedProgram) -> tellur::CheckedProgram {
    let mut configured = program.clone();
    for region in program.regions().filter(|r| {
        r.boundary.is_none()
            && !r.is_constructor()
            && !r.contingency
            && !r.parameters.iter().any(|p| p.ty.starts_with('&'))
    }) {
        let codes = region.exits.iter().map(|o| (o.name.clone(), 0)).collect();
        configured = configured
            .with_entry_exit_codes(&region.name, codes)
            .unwrap();
    }
    configured
}
