use std::{collections::BTreeMap, fs, process::Command};
use tellur::{
    CheckedProgram, compile,
    ir::{Action, Operation, Value},
    manifest::Manifest,
    runtime::{ExecutionProgress, Scenario},
};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn checked(source: &str) -> CheckedProgram {
    compile(source, manifest()).unwrap_or_else(|e| panic!("{e}\n{source}"))
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
const SOURCE: &str = r#"
async control lend(ref mutable string input) exits lent {ref mutable string value from input} {
    emit("child");
    exit lent {input};
}
async control startup(ref mutable string input) exits done {
    emit("before");
    delta await lend(input) {lent {auto value}=>lent}
    [lent]:deref value="changed";
    emit(value);
    exit done;
}
control wrapper(ref mutable string input) exits done {forward run startup(input);}
control main() exits done {
    mutable string text="initial";
    wrapper(borrow mutable text);
    text="settled";
    emit(borrow text);
    exit done;
}
"#;
#[test]
fn direct_await_preserves_caller_storage_loans_and_sequential_order() {
    let p = checked(SOURCE);
    assert!(p.region("startup").unwrap().asynchronous);
    assert!(!p.region("startup").unwrap().contingent);
    assert!(!p.region("wrapper").unwrap().asynchronous);
    let expected = p.run(&scenario()).unwrap();
    assert_eq!(expected.outcome, "done");
    let text = serde_json::to_string(&expected.events).unwrap();
    let mut cursor = 0;
    for word in ["before", "child", "changed", "settled"] {
        cursor += text[cursor..].find(word).unwrap() + word.len();
    }
    let s = scenario();
    let mut execution = p.start_execution(&s, 200);
    for _ in 0..200 {
        assert!(matches!(execution.advance(0), ExecutionProgress::Paused));
        match execution.advance(1) {
            ExecutionProgress::Paused => {}
            ExecutionProgress::Complete(result) => {
                assert_eq!(
                    serde_json::to_value(result.unwrap()).unwrap()["events"],
                    serde_json::to_value(&expected.events).unwrap()
                );
                assert!(matches!(execution.advance(1), ExecutionProgress::Finished));
                return;
            }
            ExecutionProgress::Finished => panic!("completion lost"),
        }
    }
    panic!("execution failed to finish");
}
#[test]
fn invocation_permissions_survive_argument_staging_parentheses_and_capture() {
    let p = checked(
        r#"
struct message {string text;constructor(string text) {self.text=move text;}}
async control length(ref string text, message message) exits sized {u32 length} {
    emit(text);emit(borrow message.text);exit sized {9};
}
async control outer() exits auto {
    auto saved=capture (await length(borrow "temporary", message("owned")));
    exit delta saved {sized {u32 length}=>sized {length}};
}
control main() exits auto {forward run outer();}
"#,
    );
    assert_eq!(p.run(&scenario()).unwrap().fields, vec![Value::U32(9)]);
    assert!(p.region("outer").unwrap().code.iter().any(|i| matches!(
        &i.action,
        Action::Decide {
            operation: Operation::Await(_),
            ..
        }
    )));
}
#[test]
fn permission_errors_are_explicit_and_cannot_hide_in_value_or_handler_contexts() {
    let prefix =
        "async control work() exits done {exit done;}\ncontrol plain() exits done {exit done;}\n";
    for body in [
        "control main() exits done {work();exit done;}",
        "control main() exits done {await work();exit done;}",
        "async control main() exits done {run work();exit done;}",
        "async control main() exits done {work();exit done;}",
        "control main() exits done {run plain();exit done;}",
        "async control main() exits done {await plain();exit done;}",
        "control main() exits done {auto value=run work();exit done;}",
        "control main() exits done {emit(run work());exit done;}",
        "control main() exits done {auto value=(run work());exit done;}",
        "@contingent async control main() exits done {exit done;}",
        "control main() exits done {fault boundary {fault \"test\";} contingency [] {run work();terminate process 1;} exit done;}",
        "async control main() exits done {fault boundary {fault \"test\";} contingency [] {await work();terminate process 1;} exit done;}",
    ] {
        let error = compile(&format!("{prefix}{body}"), manifest()).unwrap_err();
        assert_eq!(error.code, "E_ASYNC", "{body}: {error}");
    }
    for expr in ["run true", "await pending", "run (await work())"] {
        let error = compile(
            &format!("{prefix}control main() exits done {{delta {expr};exit done;}}"),
            manifest(),
        )
        .unwrap_err();
        assert_eq!(error.code, "E_ASYNC", "{expr}: {error}");
    }
}
#[test]
fn source_async_controls_cannot_be_implicit_host_entries() {
    let p = checked("async control main() exits done {exit done;}");
    assert!(
        p.run(&scenario())
            .unwrap_err()
            .message
            .contains("using run")
    );
    assert!(
        p.native_source("main")
            .unwrap_err()
            .message
            .contains("using run")
    );
    assert!(
        p.entry_exit_codes("main")
            .unwrap_err()
            .message
            .contains("using run")
    );
}
#[test]
fn async_faults_keep_the_existing_fault_channel_and_boundary_handover() {
    let source = r#"
async control fail() faults {emit("before fault");fault "broken";}
async control child() faults {await fail();}
control main() faults {run child();}
"#;
    let p = checked(source);
    let failure = p.run_detailed(&scenario(), 200).unwrap_err();
    assert_eq!(failure.diagnostic.code, "E_FAULT");
    assert_eq!(
        failure
            .report
            .activations
            .iter()
            .map(|a| a.control.as_str())
            .collect::<Vec<_>>(),
        ["main", "child", "fail"]
    );
    assert_eq!(failure.events.len(), 1);
    let handled=source.replace("control main() faults {run child();}", "control main() exits done {fault boundary {run child();} contingency [] {emit(\"contingency\");terminate process 23;}}");
    let result = checked(&handled).run_completion(&scenario(), 200).unwrap();
    let text = serde_json::to_value(result).unwrap();
    assert!(text.to_string().contains("contingency"), "{text}");
    assert!(text.to_string().contains("23"), "{text}");
    let missing = source.replace("control main() faults", "pub control main() exits done");
    assert_eq!(
        compile(&missing, manifest()).unwrap_err().code,
        "E_FAULT_CONTRACT"
    );
}
#[test]
fn generic_and_trait_contracts_keep_async_permission() {
    let source = r#"
trait reader {async control read(ref self input) exits found {ref string value from input};}
struct message {string text;}
implement reader for message {
    async control read(ref self input) exits found {ref string value from input} {
        exit found {borrow (deref input).text};
    }
}
async control read_generic<t:reader>(ref t input) exits auto {forward await reader<t>.read(input);}
control main() exits done {
    message owned=message {text:"hello"};
    delta run read_generic<message>(borrow owned) {found {auto text}=>found}
    [found]:emit(text);exit done;
}
"#;
    assert_eq!(checked(source).run(&scenario()).unwrap().events.len(), 1);
    let mismatch = source.replacen(
        "    async control read(ref self",
        "    control read(ref self",
        1,
    );
    assert_eq!(compile(&mismatch, manifest()).unwrap_err().code, "E_ASYNC");
    let hidden = source.replace(
        "forward await reader<t>.read(input)",
        "forward reader<t>.read(input)",
    );
    assert_eq!(compile(&hidden, manifest()).unwrap_err().code, "E_ASYNC");
}
#[test]
fn lifting_keeps_async_boundaries_and_certificates_reject_permission_erasure() {
    let p = checked(SOURCE);
    let artifact = p.expansion_artifact("main").unwrap();
    let expanded = p.accept_expansion(&artifact).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        expanded.run(&scenario()).unwrap()
    );
    assert!(expanded.region("main").unwrap().code.iter().any(|i|
        matches!(&i.action,Action::Decide {operation:Operation::Run(name),..} if name=="startup")));
    let mut tampered = serde_json::to_value(&artifact).unwrap();
    fn erase(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                if let Some(name) = fields.remove("Run") {
                    fields.insert("Region".into(), name);
                }
                for v in fields.values_mut() {
                    erase(v);
                }
            }
            serde_json::Value::Array(values) => {
                for v in values {
                    erase(v)
                }
            }
            _ => {}
        }
    }
    erase(&mut tampered);
    assert_ne!(tampered, serde_json::to_value(&artifact).unwrap());
    assert!(
        p.accept_expansion(&serde_json::from_value(tampered).unwrap())
            .is_err()
    );
}
#[test]
fn native_execution_agrees_with_reference_before_and_after_lifting() {
    let root = std::env::temp_dir().join(format!("tellur-async-entry-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    let p = checked(SOURCE)
        .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
        .unwrap();
    for (index, program) in [p.clone(), p.expanded("main").unwrap()].iter().enumerate() {
        let binary = root.join(format!("program-{index}"));
        program.build_native("main", &binary, 0).unwrap();
        let output = Command::new(&binary)
            .arg("--scenario")
            .arg(&input)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            serde_json::to_value(program.run(&scenario()).unwrap()).unwrap()
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn lifting_sync_helpers_inside_async_controls_preserves_explicit_runtime_entry() {
    let p = checked(
        r#"
async control child() exits done {emit("nested");exit done;}
control enter() exits done {forward run child();}
control helper() exits done {forward enter();}
async control startup() exits done {forward helper();}
control run() exits done {forward run startup();}
control main() exits done {forward run();}
"#,
    );
    let expanded = p.expanded("startup").unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        expanded.run(&scenario()).unwrap()
    );
    assert!(expanded.region("startup").unwrap().code.iter().any(
        |i| matches!(&i.action,Action::Decide {operation:Operation::Run(name),..} if name=="child")
    ));
}

#[test]
fn async_edges_are_included_in_recursion_checks_and_remain_opaque_to_protocols() {
    let source = "async control first() exits done {forward await second();}\nasync control second() exits done {forward await first();}";
    assert_eq!(compile(source, manifest()).unwrap_err().code, "E_RECURSION");
    let p = checked(SOURCE);
    assert!(
        p.protocol("main", 1000)
            .unwrap_err()
            .message
            .contains("run or await")
    );
}
