//! musl starts this Rust main so the runtime and Rust adapters receive process arguments.
unsafe extern "C" {
    fn tellur_program_main() -> i32;
}

fn main() -> std::process::ExitCode {
    // The checked entry returns a Villanelle process status in the range 0..=255.
    std::process::ExitCode::from(unsafe { tellur_program_main() } as u8)
}
