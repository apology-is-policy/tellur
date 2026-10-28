use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};
use tellur::{compile_file, manifest::Manifest, runtime::Scenario};
const USAGE: &str = "Tellur compiler\n  tellurc check SOURCE [MANIFEST]\n  tellurc lock-native SOURCE [MANIFEST]  (explicitly resolve Rust adapter dependencies)\n  tellurc run SOURCE MANIFEST SCENARIO [--expanded]  (reference interpreter)\n  tellurc compare SOURCE MANIFEST OTHER_SOURCE SCENARIOS  (finite scenario evidence)\n  tellurc expand SOURCE MANIFEST ENTRY\n  tellurc extract-tail SOURCE MANIFEST CONTROL LINE HELPER [--report]\n  tellurc protocol SOURCE MANIFEST ENTRY [PREFIX_JSON]  (structural abstraction)\n  tellurc protocol-compare SOURCE MANIFEST OTHER_SOURCE ENTRY\n  tellurc certify-expansion SOURCE MANIFEST ENTRY  (JSON graph and correspondence)\n  tellurc verify-expansion SOURCE MANIFEST ARTIFACT\n  tellurc inspect SOURCE MANIFEST ENTRY [--expanded]\n  tellurc build SOURCE [MANIFEST] [--entry main] [--out FILE] [--release] [--expanded]\n  tellurc exec SOURCE [MANIFEST] [build options] [-- PROGRAM_ARGUMENTS]\nBuild writes a native executable, LLVM IR (.ll), and generated source (.tellur-build/).\nNative programs accept --help, --json, --scenario FILE, and --fuel N.\nRust/Cargo with its LLVM backend is required to build; executables are standalone.";
fn discover(source: &Path) -> Option<PathBuf> {
    source
        .parent()?
        .ancestors()
        .map(|p| p.join("manifest.json"))
        .find(|p| p.is_file())
}
fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() || args[0] == "--help" || args[0] == "help" {
        println!("{USAGE}");
        return Ok(());
    }
    if args[0] == "--version" {
        println!(
            "tellurc {} (v0.1 core, v0.2 syntax preview)",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if args.len() < 2 {
        return Err(USAGE.into());
    }
    let source = PathBuf::from(&args[1]);
    let mut index = 2;
    let manifest = if args.get(index).is_some_and(|s| !s.starts_with("--")) {
        index += 1;
        PathBuf::from(&args[index - 1])
    } else {
        discover(&source).ok_or("no manifest.json found; supply a manifest path")?
    };
    let manifest_text = fs::read_to_string(&manifest)?;
    let program = compile_file(&source, Manifest::from_json_at(&manifest_text, &manifest)?)?;
    let rest = &args[index..];
    match args[0].as_str() {
        "lock-native" if rest.is_empty() => {
            let output = source
                .parent()
                .unwrap_or(Path::new("."))
                .join("build")
                .join(source.file_stem().ok_or("invalid source name")?);
            println!(
                "Locked native dependencies: {}",
                program.lock_native("main", &output)?.display()
            );
        }
        "check" if rest.is_empty() => {
            if program.generic_controls().is_empty() {
                println!("checked {} control(s)", program.regions().count());
            } else {
                println!(
                    "checked {} concrete control(s) and {} generic definition(s)",
                    program.regions().count(),
                    program.generic_controls().len()
                );
            }
        }
        "extract-tail" if rest.len() == 3 || (rest.len() == 4 && rest[3] == "--report") => {
            let extraction = tellur::extract::extract_tail(
                &fs::read_to_string(&source)?,
                Manifest::from_json_at(&manifest_text, &manifest)?,
                &rest[0],
                rest[1].parse()?,
                &rest[2],
            )?;
            if rest.len() == 4 {
                println!("{}", serde_json::to_string_pretty(&extraction)?);
            } else {
                print!("{}", extraction.source);
            }
        }
        "protocol-compare" if rest.len() == 2 => {
            let other = compile_file(&rest[0], Manifest::from_json_at(&manifest_text, &manifest)?)?;
            let before = program.protocol(&rest[1], 20_000)?;
            let after = other.protocol(&rest[1], 20_000)?;
            let before_in_after = before.included_in(&after, 20_000)?;
            let after_in_before = after.included_in(&before, 20_000)?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"before_in_after":before_in_after,"after_in_before":after_in_before})
                )?
            );
            if !before_in_after.included || !after_in_before.included {
                return Err("completed structural trace languages differ; witnesses are abstract possibilities, not concrete executions".into());
            }
        }
        "protocol" if rest.len() == 1 || rest.len() == 2 => {
            let model = program.protocol(&rest[0], 20_000)?;
            if rest.len() == 2 {
                let prefix: Vec<tellur::protocol::Symbol> =
                    serde_json::from_str(&fs::read_to_string(&rest[1])?)?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&model.residual(&prefix))?
                );
            } else {
                println!("{}", serde_json::to_string_pretty(&model)?);
            }
        }
        "certify-expansion" if rest.len() == 1 => {
            println!(
                "{}",
                serde_json::to_string_pretty(&program.expansion_artifact(&rest[0])?)?
            );
        }
        "verify-expansion" if rest.len() == 1 => {
            let artifact: tellur::certificate::ExpansionArtifact =
                serde_json::from_str(&fs::read_to_string(&rest[0])?)?;
            program.accept_expansion(&artifact)?;
            println!(
                "verified expansion of {}: {} call substitution(s); rewrite conformance, not a general equivalence proof",
                artifact.entry,
                artifact.calls.len()
            );
        }
        "compare" if rest.len() == 2 => {
            let other = compile_file(&rest[0], Manifest::from_json_at(&manifest_text, &manifest)?)?;
            let cases: Vec<Scenario> = serde_json::from_str(&fs::read_to_string(&rest[1])?)?;
            let report = tellur::algebra::compare_scenarios(&program, &other, &cases, 100_000)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !report.observations_match || !report.declared_outcome_shapes_equal {
                return Err("comparison found different observations or declared outcome shapes; see report".into());
            }
        }
        "run" if rest.len() == 1 || (rest.len() == 2 && rest[1] == "--expanded") => {
            let s: Scenario = serde_json::from_str(&fs::read_to_string(&rest[0])?)?;
            let p = if rest.len() == 2 {
                program.expanded(&s.entry)?
            } else {
                program
            };
            println!("{}", serde_json::to_string_pretty(&p.run(&s)?)?);
        }
        "expand" | "inspect" if rest.len() == 1 || (rest.len() == 2 && rest[1] == "--expanded") => {
            let p = if args[0] == "expand" || rest.len() == 2 {
                program.expanded(&rest[0])?
            } else {
                program
            };
            print!("{}", p.inspect(&rest[0])?);
        }
        "build" | "exec" => {
            let mut entry = "main".to_string();
            let mut output = source
                .parent()
                .unwrap_or(Path::new("."))
                .join("build")
                .join(source.file_stem().ok_or("invalid source name")?);
            let mut opt = 0;
            let mut expanded = false;
            let mut runtime_args = &[][..];
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--" => {
                        runtime_args = &rest[i + 1..];
                        break;
                    }
                    "--entry" => {
                        i += 1;
                        entry = rest.get(i).ok_or("--entry needs a control name")?.clone();
                    }
                    "--out" => {
                        i += 1;
                        output = PathBuf::from(rest.get(i).ok_or("--out needs a filename")?);
                    }
                    "--release" => opt = 2,
                    "--expanded" => expanded = true,
                    other => return Err(format!("unknown build option {other}").into()),
                };
                i += 1;
            }
            let p = if expanded {
                program.expanded(&entry)?
            } else {
                program
            };
            let build = p.build_native(&entry, &output, opt)?;
            if args[0] == "exec" {
                let status = Command::new(&build.executable)
                    .args(runtime_args)
                    .status()?;
                if !status.success() {
                    return Err(format!("program exited with {status}").into());
                }
            } else {
                println!(
                    "Built {}\nLLVM IR: {}\nGenerated source: {}",
                    build.executable.display(),
                    build.llvm.display(),
                    build.generated.display()
                );
            }
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
