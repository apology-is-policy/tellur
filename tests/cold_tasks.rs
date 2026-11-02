use tellur::{compile, manifest::Manifest, runtime::Scenario};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn source(body: &str) -> String {
    format!(
        r#"
async control work(string text) exits ready {{string text}} {{emit("body");exit ready {{text}};}}
async control startup() exits done {{ {body} }}
control main() exits done {{forward run startup();}}
"#
    )
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
#[test]
fn deferred_body_runs_only_after_await_and_handles_move_once() {
    let text = source(
        r#"auto pending=defer work("payload"); emit("between"); auto moved=move pending;
 delta await moved {ready {string text}=>ready} [ready]:emit(borrow text);exit done;"#,
    );
    let p = compile(&text, manifest()).unwrap();
    let result = p.run(&scenario()).unwrap();
    let messages = result
        .events
        .iter()
        .map(|e| match e {
            tellur::runtime::Event::Emit { message } => message.as_str(),
            _ => panic!(),
        })
        .collect::<Vec<_>>();
    assert_eq!(messages, ["between", "body", "payload"]);
    assert_eq!(
        p.expanded("main").unwrap().run(&scenario()).unwrap(),
        result
    );
}
#[test]
fn pending_task_cannot_be_discarded_cloned_or_reused() {
    for body in [
        r#"auto pending=defer work("payload"); exit done;"#,
        r#"auto pending=defer work("payload"); drop(pending); exit done;"#,
        r#"auto pending=defer work("payload"); auto twin=clone(borrow pending); exit done;"#,
        r#"auto pending=defer work("payload"); auto saved=capture await pending; auto second=capture await pending;exit done;"#,
    ] {
        assert!(compile(&source(body), manifest()).is_err(), "{body}");
    }
}
#[test]
fn cold_handles_stay_local_with_whole_carrier_borrows() {
    let borrowed = "struct view['a] {ref string text <- 'a;} async control work(ref view input) exits done {exit done;} async control main() exits done {string text=\"a\";view v=view {borrow text};auto pending=defer work(borrow v);await pending;exit done;}";
    compile(borrowed, manifest()).unwrap();
    let escape = "async control work() exits done {exit done;} async control factory() exits auto {auto pending=defer work();exit ready {pending};}";
    assert_eq!(compile(escape, manifest()).unwrap_err().code, "E_ASYNC");
}
#[test]
fn single_completion_and_forwarding_can_await_local_handles() {
    let text = r#"async control work() exits done {emit("body");exit done;}
 async control startup() exits auto {auto pending=defer work(); forward await pending;}
 control main() exits auto {forward run startup();}"#;
    let p = compile(text, manifest()).unwrap();
    assert_eq!(p.run(&scenario()).unwrap().events.len(), 1);
    let text = text.replace("forward await pending;", "await pending;exit done;");
    assert_eq!(
        compile(&text, manifest())
            .unwrap()
            .run(&scenario())
            .unwrap()
            .events
            .len(),
        1
    );
}

#[test]
fn pending_capture_obligations_do_not_disappear_behind_nullary_results() {
    let prefix = r#"async control finish(handle owned) exits done {
        delta close(owned) {closed=>closed,failed {close_error error}=>failed}
        [closed|failed]:exit done;
    }"#;
    for body in [
        "auto pending=defer finish(owned);fault \"interrupted\";",
        "auto pending=defer finish(owned);auto moved=move pending;fault \"interrupted\";",
    ] {
        let source = format!("{prefix} async control caller(handle owned) faults {{{body}}}");
        assert_eq!(
            compile(&source, manifest()).unwrap_err().code,
            "E_FAULT_RESOURCE"
        );
    }
    let source = format!(
        "{prefix} async control caller(handle owned) exits done {{auto pending=defer finish(owned);await pending;exit done;}}"
    );
    compile(&source, manifest()).unwrap();
}
#[test]
fn body_fault_permission_is_required_at_await_not_at_creation() {
    let source = r#"
async control fail() faults {emit("body");fault "deferred";}
pub async control startup() exits done {
    supervised loop attempts(mutable bool first=true) {
        delta first {true=>try_work,false=>finished}
        [try_work]:first=false;
            auto pending=defer fail();
            emit("created");
            fault boundary {await pending;} contingency [] {recover again attempts;}
        [finished]:exit done;
    }
}
control main() exits done {forward run startup();}
"#;
    let p = compile(source, manifest()).unwrap();
    assert!(!p.region("startup").unwrap().faults);
    let result = p.run(&scenario()).unwrap();
    assert_eq!(result.outcome, "done");
    let messages = result
        .events
        .iter()
        .filter_map(|e| match e {
            tellur::runtime::Event::Emit { message } => Some(message.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(messages, ["created", "body"]);
}
#[test]
fn owned_generic_captures_and_constructor_effects_keep_written_order() {
    let source = r#"
struct message {string text;constructor(string text){emit(borrow text);self.text=move text;}}
async control work<t>(t input, message second) exits ready {t value} {
    emit("body");exit ready {input};
}
async control wrapper<t>(t input) exits auto {
    auto pending=defer work<t>(input,message("argument"));
    emit("after capture");
    auto saved=capture await pending;
    exit saved;
}
control main() exits done {
    delta run wrapper<string>("value") {ready {string text}=>ready}
    [ready]:emit(borrow text);exit done;
}
"#;
    let p = compile(source, manifest()).unwrap();
    let result = p.run(&scenario()).unwrap();
    let messages = result
        .events
        .iter()
        .map(|e| match e {
            tellur::runtime::Event::Emit { message } => message.as_str(),
            _ => panic!(),
        })
        .collect::<Vec<_>>();
    assert_eq!(messages, ["argument", "after capture", "body", "value"]);
}
#[test]
fn native_cold_tasks_match_reference_and_expansion() {
    use std::{collections::BTreeMap, fs, process::Command};
    let text = source(
        r#"auto pending=defer work("payload");emit("between");
        delta await pending {ready {string text}=>ready} [ready]:emit(borrow text);exit done;"#,
    );
    let p = compile(&text, manifest())
        .unwrap()
        .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
        .unwrap();
    let root = std::env::temp_dir().join(format!("tellur-cold-native-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (index, p) in [p.clone(), p.expanded("startup").unwrap()]
        .iter()
        .enumerate()
    {
        let binary = root.join(format!("program-{index}"));
        p.build_native("main", &binary, 0).unwrap();
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
            serde_json::to_value(p.run(&scenario()).unwrap()).unwrap()
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn zero_capture_tasks_and_disjoint_paths_retain_the_await_obligation() {
    let prefix = "async control work() exits done {emit(\"body\");exit done;}";
    let accepted = format!(
        "{prefix} async control start(bool flag) exits done {{
        auto pending=defer work();
        delta flag {{true=>yes,false=>no}}
        [yes]:await pending;exit done;
        [no]:auto moved=move pending;await moved;exit done;
    }} control main() exits done {{forward run start(false);}}"
    );
    assert_eq!(
        compile(&accepted, manifest())
            .unwrap()
            .run(&scenario())
            .unwrap()
            .events
            .len(),
        1
    );
    for body in [
        "auto pending=defer work();exit done;",
        "auto pending=defer work();drop(pending);exit done;",
        "mutable auto pending=defer work();await pending;exit done;",
    ] {
        assert!(
            compile(
                &format!("{prefix} async control start() exits done {{{body}}}"),
                manifest()
            )
            .is_err()
        );
    }
    let nested_escape = format!(
        "{prefix} async control start() exits auto {{auto pending=defer work();auto wrapped=some {{pending}};exit ready {{wrapped}};}}"
    );
    assert_eq!(
        compile(&nested_escape, manifest()).unwrap_err().code,
        "E_ASYNC"
    );
}

#[test]
fn deferred_argument_faults_are_immediate_and_preserve_prefix_owners() {
    let prefix = r#"
struct message {string text;constructor(string text) faults {fault "argument failed";}}
async control work(handle owned, message input) exits done {
    delta close(owned) {closed=>closed,failed {close_error error}=>failed}
    [closed|failed]:exit done;
}
"#;
    let text = format!(
        "{prefix} async control start(handle owned) faults {{auto pending=defer work(owned,message(\"text\"));await pending;}}"
    );
    assert_eq!(
        compile(&text, manifest()).unwrap_err().code,
        "E_FAULT_RESOURCE"
    );
    let text = r#"
struct message {string text;constructor(string text) faults {emit("argument");fault "argument failed";}}
async control work(string owned, message input) exits done {emit("body");exit done;}
async control start() exits done faults {auto pending=defer work("text",message("text"));await pending;exit done;}
control main() exits done faults {forward run start();}
"#;
    let p = compile(text, manifest()).unwrap();
    let failure = p.run_detailed(&scenario(), 200).unwrap_err();
    let json = serde_json::to_string(&failure.report).unwrap();
    assert!(json.contains("argument failed"), "{json}");
    assert!(!json.contains("\"body\""), "{json}");
}

#[test]
fn stepping_and_serialized_expansion_keep_creation_separate_from_execution() {
    use tellur::runtime::ExecutionProgress;
    let p = compile(&source(r#"auto pending=defer work("text");emit("between");delta await pending {ready {string text}=>ready} [ready]:emit(borrow text);exit done;"#), manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    let artifact = p.expansion_artifact("startup").unwrap();
    let serialized = serde_json::to_string(&artifact).unwrap();
    assert!(serialized.contains("Defer") && serialized.contains("AwaitTask"));
    let expanded = p
        .accept_expansion(&serde_json::from_str(&serialized).unwrap())
        .unwrap();
    assert_eq!(expanded.run(&scenario()).unwrap(), expected);
    let mut tampered: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    fn erase(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(fields) => {
                if let Some(name) = fields.remove("Defer") {
                    fields.insert("Await".into(), name);
                }
                for v in fields.values_mut() {
                    erase(v);
                }
            }
            serde_json::Value::Array(values) => {
                for v in values {
                    erase(v);
                }
            }
            _ => {}
        }
    }
    erase(&mut tampered);
    assert!(
        p.accept_expansion(&serde_json::from_value(tampered).unwrap())
            .is_err()
    );
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
    panic!("execution did not finish");
}

#[test]
fn inline_await_adapters_handle_outcomes_without_erasing_task_creation() {
    let text = source(
        r#"auto pending=defer work("text");exit delta await pending {ready {string text}=>done {}};"#,
    );
    let p = compile(&text, manifest()).unwrap();
    assert_eq!(p.run(&scenario()).unwrap().outcome, "done");
    for text in [
        "async control work() exits done {exit done;} control main() exits done {auto pending=defer work();await pending;exit done;}",
        "async control work() exits done {exit done;} async control main() exits done {auto pending=defer work();auto value=await pending;exit done;}",
        "async control work() exits done {exit done;} async control main() exits done {fault boundary {fault \"fail\";} contingency [] {auto pending=defer work();terminate process 1;}exit done;}",
    ] {
        assert_eq!(
            compile(text, manifest()).unwrap_err().code,
            "E_ASYNC",
            "{text}"
        );
    }
}

#[test]
fn raw_carrier_temporary_does_not_implicitly_become_a_deferred_borrow() {
    let text = "struct view['a] {ref string text <- 'a;} async control work(ref view input) exits done {exit done;} async control main(ref string text) exits done {auto pending=defer work(view {text});await pending;exit done;}";
    assert_eq!(compile(text, manifest()).unwrap_err().code, "E_BORROW");
}

#[test]
fn queued_nested_await_fault_reports_match_native_and_expansion() {
    use std::{fs, process::Command};
    let source = r#"
async control leaf(ref string text) faults { emit(text); fault "child failed"; }
async control relay(ref string text) faults {
    auto inner = defer leaf(text); await inner;
}
async control start() faults {
    string text = "captured";
    auto pending = defer relay(borrow text); await pending;
}
control main() faults { run start(); }
"#;
    let p = compile(source, manifest()).unwrap();
    let root = std::env::temp_dir().join(format!("tellur-queued-fault-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (index, p) in [p.clone(), p.expanded("start").unwrap()].iter().enumerate() {
        let expected = p.run_detailed(&scenario(), 200).unwrap_err();
        assert_eq!(
            expected
                .report
                .activations
                .iter()
                .map(|a| a.control.as_str())
                .collect::<Vec<_>>(),
            ["main", "start", "relay", "leaf"]
        );
        let binary = root.join(format!("fault-{index}"));
        p.build_native("main", &binary, 0).unwrap();
        let output = Command::new(binary)
            .args(["--scenario", input.to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let report: tellur::fault::Report = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(report, expected.report);
    }
    fs::remove_dir_all(root).unwrap();
}
