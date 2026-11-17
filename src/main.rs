#[cfg(target_os = "thylacine")]
#[path = "pouch_process.rs"]
mod pouch_process;

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};
use tellur::{compile_file, manifest::Manifest, runtime::Scenario};
const USAGE: &str = "Villanelle compiler\n  tellurc lsp  (language server over stdio)\n  tellurc explain SOURCE [MANIFEST] (--at LINE:COLUMN | --control NAME --outcome NAME) [--json]\n  tellurc absorption-snapshot SOURCE [MANIFEST]  (bounded outcome contributor JSON)\n  tellurc absorption-diff BEFORE_JSON AFTER_JSON [--check]\n  tellurc contract-snapshot SOURCE [MANIFEST]  (checked public interface JSON)\n  tellurc contract-diff BEFORE_JSON AFTER_JSON [--check]  (exact contract differences)\n  tellurc format SOURCE [--check | --write] [--width N]  (stdout by default; no manifest needed)\n  tellurc check SOURCE [MANIFEST] [--json]\n  tellurc pack-adapter SOURCE [MANIFEST] --out FILE [--target TRIPLE] [--release]\n  tellurc lock-native SOURCE [MANIFEST]  (explicitly resolve Rust adapter dependencies)\n  tellurc run SOURCE MANIFEST SCENARIO [--expanded] [--json]  (reference interpreter)\n  tellurc compare SOURCE MANIFEST OTHER_SOURCE SCENARIOS  (finite scenario evidence)\n  tellurc expand SOURCE MANIFEST ENTRY\n  tellurc extract-tail SOURCE MANIFEST CONTROL LINE HELPER [--report]\n  tellurc protocol SOURCE MANIFEST ENTRY [PREFIX_JSON]  (structural abstraction)\n  tellurc protocol-compare SOURCE MANIFEST OTHER_SOURCE ENTRY\n  tellurc certify-expansion SOURCE MANIFEST ENTRY  (JSON graph and correspondence)\n  tellurc verify-expansion SOURCE MANIFEST ARTIFACT\n  tellurc inspect SOURCE MANIFEST ENTRY [--expanded]\n  tellurc build SOURCE [MANIFEST] [--entry main] [--out FILE] [--release] [--expanded] [--backend rust|llvm] [--target TRIPLE] [--adapter-pack FILE]\n  tellurc exec SOURCE [MANIFEST] [build options] [-- PROGRAM_ARGUMENTS]\nBuild writes a native executable, LLVM IR (.ll), and generated source (.tellur-build/).\nA Linux target triple selects a bundled static musl pack (x86_64 or aarch64).\nNative programs accept --help, --json, --scenario FILE, and --fuel N.\nThe native distributions embed the LLVM runtime and system link packs for supported targets. Cargo is required to compile source adapters or use --backend rust. An LLVM build can reuse a matching --adapter-pack without Cargo.\nCompilers with an embedded native target pack default to LLVM. Source builds with --no-default-features use Rust. Pass --backend rust to use generated Rust on any build. See docs/reference/llvm-backend.md for supported operations. Executables are standalone.";
fn discover(source: &Path) -> Option<PathBuf> {
    let source = if source.is_absolute() {
        source.to_owned()
    } else {
        env::current_dir().ok()?.join(source)
    };
    source
        .parent()?
        .ancestors()
        .map(|p| p.join("manifest.json"))
        .find(|p| p.is_file())
}
fn run(
    args: Vec<String>,
    compiler_name: &str,
) -> std::result::Result<ExitCode, Box<dyn std::error::Error>> {
    let usage = || USAGE.replace("tellurc ", &format!("{compiler_name} "));
    if args.is_empty() || args[0] == "--help" || args[0] == "help" {
        println!("{}", usage());
        return Ok(ExitCode::SUCCESS);
    }
    if args[0] == "--version" {
        println!(
            "{compiler_name} {} (v0.1 core, v0.2 syntax preview)",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(ExitCode::SUCCESS);
    }
    if args[0] == "lsp" {
        if args.len() != 1 {
            return Err("usage: vnc lsp".into());
        }
        return Ok(tellur::lsp::serve_stdio());
    }
    if args.len() < 2 {
        return Err(usage().into());
    }
    // Development command of the embedded LLVM backend (feature `llvm`):
    // `llvm-smoke [--release] OUT INPUT.ll...` links a host executable in-process.
    #[cfg(feature = "llvm")]
    if args[0] == "llvm-smoke" {
        let mut rest: Vec<&String> = args[1..].iter().collect();
        let opt = if rest.first().is_some_and(|a| *a == "--release") {
            rest.remove(0);
            2
        } else {
            0
        };
        let (output, inputs) = rest
            .split_first()
            .ok_or("usage: llvm-smoke [--release] OUT INPUT.ll...")?;
        let inputs: Vec<&Path> = inputs.iter().map(|s| Path::new(s.as_str())).collect();
        tellur::llvm::smoke(&inputs, Path::new(output.as_str()), opt)?;
        return Ok(ExitCode::SUCCESS);
    }
    if args[0] == "absorption-diff" {
        if args.len() < 3 || args.len() > 4 || (args.len() == 4 && args[3] != "--check") {
            return Err("usage: vnc absorption-diff BEFORE_JSON AFTER_JSON [--check]".into());
        }
        let before = tellur::absorption::Snapshot::from_json(&fs::read_to_string(&args[1])?)?;
        let after = tellur::absorption::Snapshot::from_json(&fs::read_to_string(&args[2])?)?;
        let difference = before.compare(&after)?;
        println!("{}", serde_json::to_string_pretty(&difference)?);
        return Ok(
            if args.len() == 4
                && (!difference.public_contracts_equal
                    || !difference.recorded_evidence_equal
                    || difference.truncated)
            {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            },
        );
    }
    if args[0] == "contract-diff" {
        if args.len() < 3 || args.len() > 4 || (args.len() == 4 && args[3] != "--check") {
            return Err("usage: vnc contract-diff BEFORE_JSON AFTER_JSON [--check]".into());
        }
        let before = tellur::contracts::Snapshot::from_json(&fs::read_to_string(&args[1])?)?;
        let after = tellur::contracts::Snapshot::from_json(&fs::read_to_string(&args[2])?)?;
        let difference = before.compare(&after)?;
        println!("{}", serde_json::to_string_pretty(&difference)?);
        return Ok(if args.len() == 4 && !difference.equal {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        });
    }
    let source = PathBuf::from(&args[1]);
    if args[0] == "format" {
        let mut mode = "stdout";
        let mut options = tellur::format::Options::default();
        let mut width_seen = false;
        let mut flags = args[2..].iter();
        while let Some(flag) = flags.next() {
            match flag.as_str() {
                "--check" | "--write" if mode == "stdout" => mode = flag,
                "--width" if !width_seen => {
                    options.max_width = flags
                        .next()
                        .ok_or("--width requires a number from 40 through 240")?
                        .parse()?;
                    width_seen = true;
                }
                _ => {
                    return Err("usage: vnc format SOURCE [--check | --write] [--width N]".into());
                }
            }
        }
        let original = fs::read_to_string(&source)?;
        let formatted = tellur::format::source(&original, options)?;
        match mode {
            "--check" if original != formatted => {
                eprintln!("{} needs formatting", source.display());
                return Ok(ExitCode::FAILURE);
            }
            "--write" if original != formatted => fs::write(&source, formatted)?,
            "stdout" => print!("{formatted}"),
            _ => {}
        }
        return Ok(ExitCode::SUCCESS);
    }
    let mut index = 2;
    let manifest = if args.get(index).is_some_and(|s| !s.starts_with("--")) {
        index += 1;
        PathBuf::from(&args[index - 1])
    } else {
        discover(&source).ok_or("no manifest.json found; supply a manifest path")?
    };
    let manifest_text = fs::read_to_string(&manifest)?;
    let checked_manifest = Manifest::from_json_at(&manifest_text, &manifest)?;
    if args[0] == "explain" {
        let mut rest = &args[index..];
        let json = rest.last().is_some_and(|s| s == "--json");
        if json {
            rest = &rest[..rest.len() - 1];
        }
        let usage = "usage: vnc explain SOURCE [MANIFEST] (--at LINE:COLUMN | --control NAME --outcome NAME) [--json]";
        let query = match rest {
            [flag, at] if flag == "--at" => {
                let (line, column) = at.split_once(':').ok_or(usage)?;
                tellur::explain::Query::Selection {
                    line: line.parse().map_err(|_| usage)?,
                    column: column.parse().map_err(|_| usage)?,
                }
            }
            [flag, control, next, outcome] if flag == "--control" && next == "--outcome" => {
                tellur::explain::Query::Outcome {
                    control: control.clone(),
                    outcome: outcome.clone(),
                }
            }
            _ => return Err(usage.into()),
        };
        let report = tellur::explain::file(&source, checked_manifest, query)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print!("{}", report.markdown);
        }
        return Ok(ExitCode::SUCCESS);
    }
    if args[0] == "absorption-snapshot" {
        if index != args.len() {
            return Err("usage: vnc absorption-snapshot SOURCE [MANIFEST]".into());
        }
        let snapshot = tellur::modules::absorption_snapshot(&source, checked_manifest)?;
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
        return Ok(ExitCode::SUCCESS);
    }
    if args[0] == "contract-snapshot" {
        if index != args.len() {
            return Err("usage: vnc contract-snapshot SOURCE [MANIFEST]".into());
        }
        let snapshot = tellur::modules::contract_snapshot(&source, checked_manifest)?;
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
        return Ok(ExitCode::SUCCESS);
    }
    let program = if args[0] == "check" {
        tellur::check_file(&source, checked_manifest)?
    } else {
        compile_file(&source, checked_manifest)?
    };
    let rest = &args[index..];
    match args[0].as_str() {
        "pack-adapter" => {
            #[cfg(feature = "llvm")]
            {
                let mut options = rest.iter();
                let mut output = None;
                let mut target = None;
                let mut opt = 0;
                while let Some(option) = options.next() {
                    match option.as_str() {
                        "--out" => {
                            output =
                                Some(Path::new(options.next().ok_or("--out needs a filename")?))
                        }
                        "--target" => {
                            target = Some(
                                options
                                    .next()
                                    .ok_or("--target needs a target triple")?
                                    .as_str(),
                            )
                        }
                        "--release" => opt = 2,
                        other => return Err(format!("unknown pack-adapter option {other}").into()),
                    }
                }
                let output = output.ok_or("pack-adapter requires --out FILE")?;
                program.pack_llvm_adapter(output, opt, target)?;
                println!("Packed Rust adapter at {}", output.display());
            }
            #[cfg(not(feature = "llvm"))]
            return Err("pack-adapter requires a compiler built with the llvm feature".into());
        }
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
            if !report.observations_match
                || !report.declared_outcome_shapes_equal
                || !report.fault_permissions_equal
            {
                return Err(
                    "comparison found different observations or declared contracts; see report"
                        .into(),
                );
            }
        }
        "run" if !rest.is_empty() => {
            let mut expanded = false;
            let mut json = false;
            for flag in &rest[1..] {
                match flag.as_str() {
                    "--expanded" if !expanded => expanded = true,
                    "--json" if !json => json = true,
                    _ => {
                        return Err(
                            "usage: vnc run SOURCE MANIFEST SCENARIO [--expanded] [--json]".into(),
                        );
                    }
                }
            }
            let s: Scenario = serde_json::from_str(&fs::read_to_string(&rest[0])?)?;
            let p = if expanded {
                program.expanded(&s.entry)?
            } else {
                program
            };
            match p.run_completion(&s, 1_000_000) {
                Ok(result) => {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                    if let tellur::runtime::RunCompletion::Process(result) = result {
                        return Ok(ExitCode::from(result.status));
                    }
                }
                Err(failure) => {
                    if json {
                        eprintln!("{}", serde_json::to_string(&failure.report)?);
                    } else {
                        eprintln!("{}", failure.report);
                    }
                    return Ok(ExitCode::FAILURE);
                }
            }
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
            let mut backend = None;
            let mut target = None;
            let mut adapter_pack = None;
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
                    "--target" => {
                        i += 1;
                        target = Some(
                            rest.get(i)
                                .ok_or("--target needs a target triple")?
                                .as_str(),
                        );
                    }
                    "--adapter-pack" => {
                        i += 1;
                        adapter_pack = Some(Path::new(
                            rest.get(i).ok_or("--adapter-pack needs a filename")?,
                        ));
                    }
                    "--backend" => {
                        i += 1;
                        let selected = rest.get(i).ok_or("--backend needs rust or llvm")?.as_str();
                        backend = Some(selected);
                        if !matches!(selected, "rust" | "llvm") {
                            return Err(format!("unknown backend {selected}").into());
                        }
                    }
                    other => return Err(format!("unknown build option {other}").into()),
                };
                i += 1;
            }
            let p = if expanded {
                program.expanded(&entry)?
            } else {
                program
            };
            let backend = backend.unwrap_or(if target.is_some() || adapter_pack.is_some() {
                "llvm"
            } else {
                tellur::native::DEFAULT_BACKEND
            });
            if target.is_some() && backend == "rust" {
                return Err("--target requires the LLVM backend".into());
            }
            if adapter_pack.is_some() && backend == "rust" {
                return Err("--adapter-pack requires the LLVM backend".into());
            }
            let build = if backend == "llvm" {
                #[cfg(feature = "llvm")]
                {
                    p.build_llvm_with_adapter(&entry, &output, opt, target, adapter_pack)?
                }
                #[cfg(not(feature = "llvm"))]
                {
                    return Err(
                        "LLVM backend requires a compiler built with the llvm feature".into(),
                    );
                }
            } else {
                p.build_native(&entry, &output, opt)?
            };
            if args[0] == "exec" {
                #[cfg(target_os = "thylacine")]
                let status = pouch_process::run(&build.executable, runtime_args)?;
                #[cfg(not(target_os = "thylacine"))]
                let status = std::process::Command::new(&build.executable)
                    .args(runtime_args)
                    .status()?;
                let code = status
                    .code()
                    .ok_or_else(|| format!("program terminated with {status}"))?;
                return Ok(ExitCode::from(u8::try_from(code)?));
            } else {
                println!(
                    "Built {}\nLLVM IR: {}\nGenerated source: {}",
                    build.executable.display(),
                    build.llvm.display(),
                    build.generated.display()
                );
            }
        }
        _ => return Err(usage().into()),
    }
    Ok(ExitCode::SUCCESS)
}
fn main() -> ExitCode {
    let executable = env::args_os().next().map(PathBuf::from).unwrap_or_default();
    let compiler_name = match executable.file_stem().and_then(|name| name.to_str()) {
        Some("tellur-lsp" | "path-lsp") => return tellur::lsp::serve_stdio(),
        Some("tellurc" | "pathc") => "tellurc",
        Some("tellur") => "tellur",
        _ => "vnc",
    };
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|a| a == "check") && args[1..].iter().any(|a| a == "--json") {
        return json_check(&args[1..]);
    }
    match run(args, compiler_name) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Versioned check envelope with ranges and bounded independent body diagnostics.
fn json_check(args: &[String]) -> ExitCode {
    use serde_json::{Value, json};
    use tellur::{Diagnostic, Span};
    struct Failure {
        path: Option<PathBuf>,
        diagnostic: Diagnostic,
    }
    fn failure(
        code: &'static str,
        path: Option<PathBuf>,
        message: impl Into<String>,
    ) -> tellur::Diagnostics<Failure> {
        Failure {
            path,
            diagnostic: Diagnostic::new(code, Span::default(), message),
        }
        .into()
    }
    fn absolute(path: &Path) -> PathBuf {
        // Keep the caller's spelling: a source/manifest symlink must discover and
        // resolve configuration exactly as a normal textual check does.
        if path.is_absolute() {
            path.to_owned()
        } else {
            env::current_dir().map_or_else(|_| path.to_owned(), |cwd| cwd.join(path))
        }
    }
    fn position(span: Span) -> Value {
        if span.line == 0 || span.column == 0 {
            Value::Null
        } else {
            json!({"line":span.line,"column":span.column})
        }
    }
    let checked: tellur::CheckResult<_, Failure> = (|| {
        let mut files = vec![];
        let mut flags = 0;
        for arg in args {
            if arg == "--json" {
                flags += 1;
            } else if arg.starts_with("--") {
                return Err(failure(
                    "E_USAGE",
                    None,
                    format!("unknown check option {arg}"),
                ));
            } else {
                files.push(PathBuf::from(arg));
            }
        }
        if flags != 1 || !(1..=2).contains(&files.len()) {
            return Err(failure(
                "E_USAGE",
                None,
                "tellurc check SOURCE [MANIFEST] --json",
            ));
        }
        let source = absolute(&files[0]);
        let manifest = files
            .get(1)
            .cloned()
            .or_else(|| discover(&source))
            .ok_or_else(|| {
                failure(
                    "E_CONFIG",
                    Some(source.clone()),
                    "no manifest.json found; supply a manifest path",
                )
            })?;
        let manifest = absolute(&manifest);
        let text = fs::read_to_string(&manifest)
            .map_err(|e| failure("E_IO", Some(manifest.clone()), e.to_string()))?;
        let manifest = Manifest::from_json_at(&text, &manifest).map_err(|diagnostic| Failure {
            path: Some(manifest.clone()),
            diagnostic,
        })?;
        let program = tellur::check_file(&source, manifest).map_err(|errors| {
            errors.map(|e| Failure {
                path: Some(e.path),
                diagnostic: e.diagnostic,
            })
        })?;
        Ok((program.regions().count(), program.generic_controls().len()))
    })();
    let mut report = json!({
        "schema":"tellur.check.v1", "ok":checked.is_ok(),
        "position_encoding":"one_based_unicode_scalars",
        "diagnostics":[], "diagnostics_limited":false, "summary":null,
    });
    let success = match checked {
        Ok((controls, generic_definitions)) => {
            report["summary"] =
                json!({"controls":controls,"generic_definitions":generic_definitions});
            true
        }
        Err(errors) => {
            report["diagnostics_limited"] = json!(errors.limit_reached);
            report["diagnostics"] = errors.errors.into_iter().map(|Failure { path, diagnostic }| {
            let notes: Vec<_> = diagnostic.notes.iter().map(|note| {
                let path = note.path.as_ref().or(path.as_ref());
                json!({"path":path.map(|p| p.to_string_lossy()),"position":position(note.span),"range":note.end.as_deref().map(|end| json!({"start":position(note.span),"end":position(*end)})),"message":note.message})
            }).collect();
            json!({
                "code":diagnostic.code, "message":diagnostic.message,
                "path":path.map(|p| p.to_string_lossy().into_owned()),
                "position":position(diagnostic.span),
                "range":diagnostic.end.map(|end| json!({"start":position(diagnostic.span),"end":position(*end)})),
                "notes":notes,
                "witness":diagnostic.witness,
            })
            }).collect::<Vec<_>>().into();
            false
        }
    };
    println!(
        "{}",
        serde_json::to_string(&report).expect("JSON report values are serializable")
    );
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
