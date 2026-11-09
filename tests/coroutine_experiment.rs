//! E2.1 standalone LLVM coroutine experiment (`experiments/coroutines`). Hand-lowered
//! switched-resume coroutines, built with a pinned LLVM 22 at O0 and O2, must
//! produce the reference executor's events and process status for every
//! cancellation-specimen schedule. The production backend is unchanged.
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};
use tellur::{
    CheckedProgram, compile_file,
    ir::Value,
    manifest::Manifest,
    runtime::{Event, Scenario, ScriptCall},
};

const LLVM: &str = "/opt/homebrew/opt/llvm@22/bin";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
/// The pinned toolchain, or None when it is absent and not required.
fn clang() -> Option<PathBuf> {
    let clang = Path::new(LLVM).join("clang");
    if !clang.exists() {
        assert!(
            std::env::var_os("TELLUR_REQUIRE_COROUTINE_EXPERIMENT").is_none(),
            "{} is required",
            clang.display()
        );
        eprintln!("skipping: the pinned LLVM 22 toolchain is not installed at {LLVM}");
        return None;
    }
    let version = Command::new(Path::new(LLVM).join("llvm-config"))
        .arg("--version")
        .output()
        .unwrap();
    let version = String::from_utf8(version.stdout).unwrap();
    assert!(
        version.starts_with("22."),
        "pinned LLVM 22, found {version}"
    );
    Some(clang)
}
fn sysroot() -> String {
    let output = Command::new("xcrun")
        .arg("--show-sdk-path")
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn build(clang: &Path, specimen: &str, ir: &str, opt: u8) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tellur-coroutines-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join(format!("{specimen}-{:x}.ll", hash(ir)));
    std::fs::write(&source, ir).unwrap();
    let binary = dir.join(format!("{specimen}-{:x}-O{opt}", hash(ir)));
    let experiment = root().join("experiments/coroutines");
    let output = Command::new(clang)
        .arg("-isysroot")
        .arg(sysroot())
        .arg(format!("-O{opt}"))
        .args(["-Wall", "-Werror", "-Wno-override-module"])
        .arg(experiment.join("runtime.c"))
        .arg(experiment.join("intrinsics.ll"))
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}
fn hash(text: &str) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}
fn specimen_ir() -> String {
    std::fs::read_to_string(root().join("experiments/coroutines/cancellation.ll")).unwrap()
}
/// Both optimization levels of an unmodified specimen, built once.
fn clang_binaries(specimen: &'static str) -> Option<[PathBuf; 2]> {
    static BUILT: OnceLock<std::sync::Mutex<std::collections::BTreeMap<&str, [PathBuf; 2]>>> =
        OnceLock::new();
    let clang = clang()?;
    let mut built = BUILT.get_or_init(Default::default).lock().unwrap();
    let paths = built.entry(specimen).or_insert_with(|| {
        let ir =
            std::fs::read_to_string(root().join(format!("experiments/coroutines/{specimen}.ll")))
                .unwrap();
        [0, 2].map(|opt| build(&clang, specimen, &ir, opt))
    });
    Some(paths.clone())
}
/// Every build of a specimen: the pinned clang at O0/O2 and, with the `llvm`
/// feature, `tellurc llvm-smoke` (embedded LLVM and LLD) at O0/O2.
fn binaries(specimen: &'static str) -> Option<Vec<PathBuf>> {
    let all = clang_binaries(specimen)?.to_vec();
    #[cfg(feature = "llvm")]
    let all = [all, embedded_binaries(specimen).to_vec()].concat();
    Some(all)
}
#[cfg(feature = "llvm")]
fn embedded_binaries(specimen: &str) -> [PathBuf; 2] {
    let dir = std::env::temp_dir().join(format!("tellur-embedded-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let experiment = root().join("experiments/coroutines");
    // The C runtime is test tooling: lowered to IR once by the pinned clang, then
    // compiled with every specimen in-process.
    let runtime = dir.join("runtime.ll");
    if !runtime.exists() {
        let output = Command::new(Path::new(LLVM).join("clang"))
            .arg("-isysroot")
            .arg(sysroot())
            .args(["-S", "-emit-llvm", "-O0", "-Xclang", "-disable-O0-optnone"])
            .arg(experiment.join("runtime.c"))
            .arg("-o")
            .arg(&runtime)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    ["", "--release"].map(|opt| {
        let binary = dir.join(format!("{specimen}-embedded{opt}"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_tellurc"));
        command.arg("llvm-smoke");
        if !opt.is_empty() {
            command.arg(opt);
        }
        let output = command
            .arg(&binary)
            .arg(&runtime)
            .arg(experiment.join("intrinsics.ll"))
            .arg(experiment.join(format!("{specimen}.ll")))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        binary
    })
}

fn program() -> CheckedProgram {
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    compile_file(&root().join("playground/cancellation.tellur"), manifest).unwrap()
}
fn call(operation: &str, outcome: &str, fields: Vec<Value>, waits: u32) -> ScriptCall {
    ScriptCall {
        operation: operation.into(),
        outcome: outcome.into(),
        fields,
        waits,
    }
}
fn opened(token: &str) -> ScriptCall {
    let handle = Value::Opaque {
        ty: "handle".into(),
        token: token.into(),
    };
    call("open", "opened", vec![handle], 0)
}
fn data(text: &str, waits: u32) -> ScriptCall {
    call("receive", "data", vec![Value::Bytes(text.into())], waits)
}
fn decoded(text: &str) -> ScriptCall {
    call("decode_utf8", "text", vec![Value::String(text.into())], 0)
}
fn closed() -> ScriptCall {
    call("close", "closed", vec![], 0)
}
fn close_failed() -> ScriptCall {
    let error = Value::Opaque {
        ty: "close_error".into(),
        token: "disk".into(),
    };
    call("close", "failed", vec![error], 0)
}
fn schedules() -> Vec<Vec<ScriptCall>> {
    let mut all = vec![];
    // The slower receive is pending when cancelled.
    for waits in [1, 3, 9] {
        all.push(vec![
            opened("p"),
            opened("b"),
            data("hello", 0),
            data("late", waits),
            closed(),
            closed(),
            decoded("hello"),
        ]);
    }
    // Both completed before the request: the ordinary outcome wins the race.
    all.push(vec![
        opened("p"),
        opened("b"),
        data("hello", 0),
        data("late", 0),
        closed(),
        closed(),
        decoded("hello"),
    ]);
    // The primary ends without data, later than the backup answers.
    for waits in [0, 2] {
        all.push(vec![
            opened("p"),
            opened("b"),
            call("receive", "closed", vec![], waits),
            data("backup", 0),
            closed(),
            closed(),
            decoded("backup"),
        ]);
    }
    // Neither answers; the primary answers only after the backup is pending.
    all.push(vec![
        opened("p"),
        opened("b"),
        call("receive", "closed", vec![], 0),
        call("receive", "closed", vec![], 1),
        closed(),
        closed(),
    ]);
    all.push(vec![
        opened("p"),
        opened("b"),
        data("slow", 3),
        data("late", 5),
        closed(),
        closed(),
        decoded("slow"),
    ]);
    // Close failures after cancellation, and an unavailable second handle.
    for (first, second) in [
        (close_failed(), closed()),
        (closed(), close_failed()),
        (close_failed(), close_failed()),
    ] {
        all.push(vec![
            opened("p"),
            opened("b"),
            data("hello", 0),
            data("late", 4),
            first,
            second,
        ]);
    }
    all.push(vec![
        opened("p"),
        call("open", "not_found", vec![], 0),
        closed(),
    ]);
    all
}
/// The runtime's script format: `operation outcome waits [text payload]`.
fn script(calls: &[ScriptCall]) -> String {
    calls
        .iter()
        .map(|c| {
            let payload = match c.fields.as_slice() {
                [Value::Bytes(bytes)] => String::from_utf8(bytes.clone()).unwrap(),
                [Value::String(text)] => text.clone(),
                _ => String::new(),
            };
            format!("{} {} {} {payload}\n", c.operation, c.outcome, c.waits)
        })
        .collect()
}
fn trace(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            Event::Emit { message } => message.clone(),
            Event::Primitive { operation, .. } => operation.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}
fn execute(binary: &Path, calls: &[ScriptCall]) -> (Option<i32>, Vec<String>, String) {
    let path = binary.with_extension(format!("{:x}.txt", hash(&script(calls))));
    std::fs::write(&path, script(calls)).unwrap();
    let output = Command::new(binary).arg(&path).output().unwrap();
    let lines = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    (
        output.status.code(),
        lines,
        String::from_utf8_lossy(&output.stderr).into(),
    )
}

#[test]
fn hand_lowered_coroutines_match_the_reference_executor_at_o0_and_o2() {
    let Some(binaries) = binaries("cancellation") else {
        return;
    };
    let p = program();
    for calls in schedules() {
        let s = Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: calls.clone(),
        };
        let result = p.run(&s).unwrap_or_else(|e| panic!("{e}\n{calls:?}"));
        let status = match result.outcome.as_str() {
            "answered" => 0,
            "unanswered" => 3,
            "unavailable" => 2,
            _ => 1,
        };
        for binary in &binaries {
            let (code, events, stderr) = execute(binary, &calls);
            assert_eq!(
                code,
                Some(status),
                "{}\n{stderr}\n{calls:?}",
                binary.display()
            );
            assert_eq!(
                events,
                trace(&result.events),
                "{}\n{calls:?}",
                binary.display()
            );
        }
    }
}

#[test]
fn the_runtime_rejects_a_close_before_the_borrowing_reader_is_joined() {
    // The checker rejects this program (E_LOAN, tests/cancellation_specimen.rs);
    // a lowering that closed early would be caught by the runtime's loan check.
    let Some(clang) = clang() else { return };
    let ir = specimen_ir();
    let from = "  br label %drive\ndrive:";
    assert_eq!(ir.matches(from).count(), 1);
    let early = ir.replacen(
        from,
        "  %early = call i32 @rt_close(i32 %primary)\n  br label %drive\ndrive:",
        1,
    );
    let binary = build(&clang, "early-close", &early, 0);
    let (code, _, stderr) = execute(&binary, &schedules()[0]);
    assert_eq!(code, Some(70), "{stderr}");
    assert!(stderr.contains("loan violated"), "{stderr}");
}

#[test]
fn the_runtime_rejects_an_unjoined_cancelled_child() {
    // Skipping the join of the cancelled backup leaves its frame alive and its loan
    // held, so the later close of the backup handle is caught.
    let Some(clang) = clang() else { return };
    let ir = specimen_ir();
    let from = "  call void @rt_cancel(i32 %second)\n  br label %join_second\n";
    assert_eq!(ir.matches(from).count(), 1);
    let abandoned = ir.replacen(
        from,
        "  call void @rt_cancel(i32 %second)\n  br label %answered_block\n",
        1,
    );
    let binary = build(&clang, "abandoned", &abandoned, 2);
    let (code, _, stderr) = execute(&binary, &schedules()[0]);
    assert_eq!(code, Some(70), "{stderr}");
    assert!(stderr.contains("loan violated"), "{stderr}");
}

/// `experiments/coroutines/sets.tellur` with the async `tick` test primitive.
fn sets_program() -> CheckedProgram {
    let mut config: serde_json::Value =
        serde_json::from_str(include_str!("../playground/manifest.json")).unwrap();
    config["primitives"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "name": "tick", "async": true,
            "parameters": [{"name": "label", "type": "string", "mode": "consume"}],
            "outcomes": [{"name": "ticked", "fields": [{"name": "label", "type": "string"}]}],
            "effects": ["clock.tick"], "returns": true
        }));
    let manifest = Manifest::from_json(&config.to_string()).unwrap();
    compile_file(&root().join("experiments/coroutines/sets.tellur"), manifest).unwrap()
}

#[test]
fn a_cancelled_task_set_drains_in_the_reference_completion_order() {
    let Some(binaries) = binaries("sets") else {
        return;
    };
    let p = sets_program();
    // Every combination of readiness delays, including members that completed
    // before the set was cancelled.
    for waits in 0..64u32 {
        let delay = |shift: u32| [0, 1, 2, 4][(waits >> shift & 3) as usize];
        let calls = ["a", "b", "c"]
            .iter()
            .zip([0, 2, 4])
            .map(|(label, shift)| {
                call(
                    "tick",
                    "ticked",
                    vec![Value::String((*label).into())],
                    delay(shift),
                )
            })
            .collect::<Vec<_>>();
        let s = Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: calls.clone(),
        };
        let result = p.run(&s).unwrap_or_else(|e| panic!("{e}\n{calls:?}"));
        assert_eq!(result.outcome, "done");
        for binary in &binaries {
            let (code, events, stderr) = execute(binary, &calls);
            assert_eq!(code, Some(0), "{stderr}\n{calls:?}");
            assert_eq!(
                events,
                trace(&result.events),
                "{}\n{calls:?}",
                binary.display()
            );
        }
    }
}

#[test]
fn a_sibling_fault_stops_pending_work_and_destroys_every_suspended_frame() {
    let Some(binaries) = binaries("fault") else {
        return;
    };
    let mut config: serde_json::Value =
        serde_json::from_str(include_str!("../playground/manifest.json")).unwrap();
    config["primitives"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "name": "tick", "async": true,
            "parameters": [{"name": "label", "type": "string", "mode": "consume"}],
            "outcomes": [{"name": "ticked", "fields": [{"name": "label", "type": "string"}]}],
            "effects": ["clock.tick"], "returns": true
        }));
    let manifest = Manifest::from_json(&config.to_string()).unwrap();
    let p = compile_file(
        &root().join("experiments/coroutines/fault.tellur"),
        manifest,
    )
    .unwrap();
    // Pending at the fault (no completion event), or completed first. The runtime
    // also requires every frame, suspended or completed, to be released once.
    for waits in 0..6 {
        let calls = vec![call(
            "tick",
            "ticked",
            vec![Value::String("one".into())],
            waits,
        )];
        let s = Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: calls.clone(),
        };
        let failure = p.run_detailed(&s, 10_000).unwrap_err();
        assert!(failure.diagnostic.message.contains("sibling failed"));
        for binary in &binaries {
            let (code, events, stderr) = execute(binary, &calls);
            assert_eq!(code, Some(1), "{stderr}");
            assert!(stderr.contains("fault: sibling failed"), "{stderr}");
            assert_eq!(events, trace(&failure.events), "waits {waits}");
        }
    }
}
