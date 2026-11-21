//! `vn2`: the command line of the v2 oracle.

use std::process::ExitCode;
use villanelle_v2::host::{Arithmetic, Host, Script};
use villanelle_v2::int::Int;
use villanelle_v2::oracle::{Config, End, Value, run};
use villanelle_v2::{erased, load, script, text};

const USAGE: &str = "\
vn2 test FILE...                          carry out the directives of core text files
vn2 run FILE ENTRY [VALUE...] [OPTIONS]   run one control in the oracle
vn2 print FILE                            print a program in the glyph form

Options of `run`:
  --arithmetic   the host computes the checked arithmetic of the prelude
  --usize 32     the greatest usize is 2^32 - 1 (default: 2^64 - 1)
  --fuel N       stop after N steps (default: 1000000)
  --erased       run under the erased semantics, which checks nothing
";

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))
}

fn test(files: &[String]) -> Result<bool, String> {
    let mut failed = 0;
    let mut runs = 0;
    let mut compared = 0;
    for path in files {
        let report = script::check(&read(path)?);
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
    let program = load(&read(path)?)?;
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

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let result = match arguments.split_first() {
        Some((command, rest)) if command == "test" && !rest.is_empty() => test(rest),
        Some((command, rest)) if command == "run" => run_one(rest),
        Some((command, [path])) if command == "print" => read(path).and_then(|source| {
            let tree = text::parse(&source).map_err(|error| error.to_string())?;
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
