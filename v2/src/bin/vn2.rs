//! `vn2`: the command line of the v2 oracle.

use std::process::ExitCode;
use villanelle_v2::host::{Arithmetic, Host, Script};
use villanelle_v2::int::Int;
use villanelle_v2::oracle::{Config, End, Value, run};
use villanelle_v2::{check, erased, load, load_surface, script, surface, text};

const USAGE: &str = "\
vn2 test FILE...                          carry out the directives of core or Rails files
vn2 check FILE... [--certificates]        check core or Rails files in the Phase 3 checker
vn2 format FILE... [--write | --check]    format Rails files into canonical Rails glyph form
vn2 run FILE ENTRY [VALUE...] [OPTIONS]   run one control in the oracle
vn2 print FILE                            print a core or Rails program in the core glyph form

Options of `run`:
  --arithmetic   the host computes the checked arithmetic of the prelude
  --usize 32     the greatest usize is 2^32 - 1 (default: 2^64 - 1)
  --fuel N       stop after N steps (default: 1000000)
  --erased       run under the erased semantics, which checks nothing
";

fn is_core_file(path: &str) -> bool {
    path.ends_with(".core")
}

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))
}

fn test(files: &[String]) -> Result<bool, String> {
    let mut failed = 0;
    let mut runs = 0;
    let mut compared = 0;
    for path in files {
        let source = read(path)?;
        let report = if is_core_file(path) {
            script::check(&source)
        } else {
            script::check_surface(&source)
        };
        runs += report.runs;
        compared += report.compared;
        for failure in &report.failures {
            println!("{path}: {failure}");
        }
        failed += report.failures.len();
    }
    println!(
        "{} files, {runs} runs, {compared} of them compared with the erased semantics, {failed} failures",
        files.len()
    );
    Ok(failed == 0)
}

fn value(text: &str) -> Result<Value, String> {
    match text {
        "true" => Ok(Value::Bool(true)),
        "false" => Ok(Value::Bool(false)),
        digits => Int::parse(digits)
            .map(Value::Int)
            .ok_or(format!("`{digits}` is not a value")),
    }
}

fn run_one(arguments: &[String]) -> Result<bool, String> {
    let [path, entry, rest @ ..] = arguments else {
        return Err(USAGE.to_string());
    };
    let source = read(path)?;
    let program = if is_core_file(path) {
        load(&source)?
    } else {
        load_surface(&source)?
    };
    let mut config = Config::default();
    let mut arithmetic = false;
    let mut erase = false;
    let mut values = Vec::new();
    let mut rest = rest.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--arithmetic" => arithmetic = true,
            "--erased" => erase = true,
            "--usize" => {
                let bits = rest
                    .next()
                    .and_then(|bits| bits.parse().ok())
                    .ok_or("--usize 32 or 64")?;
                config.usize_max = Config::with_usize_bits(bits).usize_max;
            }
            "--fuel" => {
                config.fuel = rest
                    .next()
                    .and_then(|fuel| fuel.parse().ok())
                    .ok_or("--fuel N")?;
            }
            other => values.push(value(other)?),
        }
    }
    let mut host = Arithmetic(Script::default());
    let host: &mut dyn Host = if arithmetic { &mut host } else { &mut host.0 };
    if erase {
        let outcome = erased::run(&program, entry, values, host, &config);
        for event in &outcome.trace {
            println!("{event}");
        }
        println!("{}", outcome.end);
        return Ok(matches!(outcome.end, erased::End::Done { .. }));
    }
    let outcome = run(&program, entry, values, host, &config);
    for event in &outcome.trace {
        println!("{event}");
    }
    println!("{}", outcome.end);
    Ok(matches!(outcome.end, End::Done { .. }))
}

fn check_files(arguments: &[String]) -> Result<bool, String> {
    let mut show_certificates = false;
    let mut files = Vec::new();
    for argument in arguments {
        if argument == "--certificates" {
            show_certificates = true;
        } else {
            files.push(argument);
        }
    }
    if files.is_empty() {
        return Err(USAGE.to_string());
    }
    let mut failed = 0usize;
    let mut total_certificates = 0usize;
    for path in &files {
        let source = read(path)?;
        let program = if is_core_file(path) {
            load(&source)
        } else {
            load_surface(&source)
        };
        let program = match program {
            Ok(program) => program,
            Err(error) => {
                println!("{path}: {error}");
                failed += 1;
                continue;
            }
        };
        match check::check(&program) {
            Ok(report) => {
                total_certificates += report.certificates.len();
                if show_certificates {
                    for (control, line, clause, certificate) in &report.certificates {
                        println!("{path}:{line} ({control} [{clause}]):");
                        print!("{certificate}");
                    }
                }
            }
            Err(error) => {
                println!("{path}: {error}");
                failed += 1;
            }
        }
    }
    println!(
        "{} files checked, {total_certificates} certificates validated, {failed} failures",
        files.len()
    );
    Ok(failed == 0)
}

fn format_files(arguments: &[String]) -> Result<bool, String> {
    let mut write_in_place = false;
    let mut check_only = false;
    let mut files = Vec::new();
    for argument in arguments {
        match argument.as_str() {
            "--write" => write_in_place = true,
            "--check" => check_only = true,
            _ => files.push(argument),
        }
    }
    if files.is_empty() || (write_in_place && check_only) {
        return Err(USAGE.to_string());
    }
    let mut ok = true;
    for path in &files {
        let source = read(path)?;
        let formatted = match surface::format(&source) {
            Ok(text) => text,
            Err(error) => {
                println!("{path}:{error}");
                ok = false;
                continue;
            }
        };
        if check_only {
            if formatted != source {
                println!("{path}: not in canonical Rails form");
                ok = false;
            }
        } else if write_in_place {
            if formatted != source {
                std::fs::write(path, formatted).map_err(|error| format!("{path}: {error}"))?;
            }
        } else {
            print!("{formatted}");
        }
    }
    Ok(ok)
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let result = match arguments.split_first() {
        Some((command, rest)) if command == "test" && !rest.is_empty() => test(rest),
        Some((command, rest)) if command == "check" && !rest.is_empty() => check_files(rest),
        Some((command, rest)) if command == "format" && !rest.is_empty() => format_files(rest),
        Some((command, rest)) if command == "run" => run_one(rest),
        Some((command, [path])) if command == "print" => read(path).and_then(|source| {
            let tree = if is_core_file(path) {
                text::parse(&source).map_err(|error| error.to_string())?
            } else {
                surface::elaborate(&source).map_err(|error| error.to_string())?
            };
            print!("{}", tree.print());
            Ok(true)
        }),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
