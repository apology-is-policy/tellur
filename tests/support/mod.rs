// Differential tests compare Tellur outcomes, not application status policy.
// Explicitly configure their standalone harness entries to report completion as 0.
pub fn scenario_executable(program: &tellur::CheckedProgram) -> tellur::CheckedProgram {
    let mut configured = program.clone();
    for region in program
        .regions()
        .filter(|r| !r.parameters.iter().any(|p| p.ty.starts_with('&')))
    {
        let codes = region.exits.iter().map(|o| (o.name.clone(), 0)).collect();
        configured = configured
            .with_entry_exit_codes(&region.name, codes)
            .unwrap();
    }
    configured
}
