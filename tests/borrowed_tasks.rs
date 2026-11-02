use tellur::{compile, manifest::Manifest, runtime::Scenario};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
fn source(body: &str) -> String {
    format!(
        r#"
async control inspect_text(ref string input) exits done {{emit(input);exit done;}}
async control update_text(ref mutable string input) exits done {{deref input="written";exit done;}}
async control start() exits done {{{body}}}
control main() exits done {{forward run start();}}
"#
    )
}
#[test]
fn direct_shared_and_exclusive_captures_protect_storage_until_await() {
    let text = source(
        r#"mutable string text="initial";
        auto first=defer inspect_text(borrow text);
        auto second=defer inspect_text(borrow text);
        emit("between");await first;await second;
        text="available";
        auto pending=defer update_text(borrow mutable text);
        auto moved=move pending;
        await moved;emit(borrow text);exit done;"#,
    );
    let p = compile(&text, manifest()).unwrap();
    let result = p.run(&scenario()).unwrap();
    let json = serde_json::to_value(&result).unwrap();
    let messages = json["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["message"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(messages, ["between", "initial", "initial", "written"]);
    assert_eq!(
        p.expanded("start").unwrap().run(&scenario()).unwrap(),
        result
    );
}
#[test]
fn pending_borrows_reject_conflicting_uses_and_do_not_expire_on_fault_paths() {
    for body in [
        r#"auto pending=defer inspect_text(borrow text);text="changed";await pending;exit done;"#,
        r#"auto pending=defer update_text(borrow mutable text);emit(borrow text);await pending;exit done;"#,
        r#"auto pending=defer inspect_text(borrow text);auto moved=move pending;text="changed";await moved;exit done;"#,
        r#"auto pending=defer inspect_text(borrow text);drop(text);await pending;exit done;"#,
        r#"auto pending=defer inspect_text(borrow text);text="changed";fault "stop";"#,
        r#"auto pending=defer update_text(borrow mutable text);auto other=defer inspect_text(borrow text);await pending;await other;exit done;"#,
    ] {
        let text = source(&format!("mutable string text=\"initial\";{body}"));
        let error = compile(&text, manifest()).unwrap_err();
        assert_eq!(error.code, "E_LOAN", "{body}: {error:?}");
    }
}
#[test]
fn reference_arguments_reborrow_unless_explicitly_moved() {
    let text = source(
        r#"mutable string text="initial";ref mutable string alias=borrow mutable text;
        auto pending=defer update_text(alias);await pending;deref alias="after";emit(alias);exit done;"#,
    );
    let p = compile(&text, manifest()).unwrap();
    assert_eq!(
        serde_json::to_value(p.run(&scenario()).unwrap()).unwrap()["events"][0]["message"],
        "after"
    );
    let illegal = text.replace("await pending;deref alias", "deref alias");
    assert_eq!(compile(&illegal, manifest()).unwrap_err().code, "E_LOAN");
    let moved = text.replace("defer update_text(alias)", "defer update_text(move alias)");
    assert_eq!(compile(&moved, manifest()).unwrap_err().code, "E_MOVED");
    let settled = moved.replace(
        "deref alias=\"after\";emit(alias);",
        "text=\"after\";emit(borrow text);",
    );
    compile(&settled, manifest())
        .unwrap()
        .run(&scenario())
        .unwrap();
}
#[test]
fn disjoint_branches_release_only_the_task_they_await() {
    let text = source(
        r#"mutable string text="initial";auto pending=defer inspect_text(borrow text);
        delta true {true=>yes,false=>no}
        [yes]:await pending;text="yes";exit done;
        [no]:auto moved=move pending;await moved;text="no";exit done;"#,
    );
    compile(&text, manifest())
        .unwrap()
        .run(&scenario())
        .unwrap();
    let bad = text.replace("await moved;text=\"no\";", "text=\"no\";await moved;");
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
}
#[test]
fn borrowed_task_storage_and_result_contracts_remain_bounded() {
    let contained = "struct view['a] {ref string text <- 'a;} async control inspect_text(ref view input) exits done {exit done;} async control main(ref view input) exits done {auto pending=defer inspect_text(input);await pending;exit done;}";
    compile(contained, manifest()).unwrap();
    let nested = source(
        r#"string text="initial";auto pending=defer inspect_text(borrow text);auto wrapped=some {pending};exit done;"#,
    );
    assert!(
        compile(&nested, manifest())
            .unwrap_err()
            .message
            .contains("borrowed tasks cannot yet be nested")
    );
    let temporary = r#"struct message {string text;constructor(string text){self.text=move text;}}
        async control inspect_text(ref message input) exits done {exit done;}
        async control main() exits done {auto pending=defer inspect_text(borrow message("x"));await pending;exit done;}"#;
    assert!(compile(temporary, manifest()).is_err());
}
#[test]
fn constructor_effects_keep_capture_borrows_live_through_later_initializers() {
    let prefix = r#"struct message {string text;constructor(ref mutable string input){deref input="changed";self.text="owned";}}
        async control inspect(ref string input,message other) exits done {emit(input);exit done;}"#;
    let text = format!(
        "{prefix} async control start() exits done {{mutable string text=\"initial\";auto pending=defer inspect(borrow text,message(borrow mutable text));await pending;exit done;}}"
    );
    assert_eq!(compile(&text, manifest()).unwrap_err().code, "E_ALIAS");
}

const NATIVE_SOURCE: &str = r#"
struct pair {string left;string right;}
async control replace(ref mutable string text) exits done {deref text="updated";exit done;}
async control observe<t>(ref t input) exits done {exit done;}
async control start() exits done {
    mutable pair data=pair {"left","right"};
    auto first=defer replace(borrow mutable data.left);
    auto second=defer replace(borrow mutable data.right);
    await first;data.left="settled";
    auto moved=move second;await moved;
    emit(borrow data.left);emit(borrow data.right);
    auto observe_later=defer observe<pair>(borrow data);
    await observe_later;
    exit done;
}
control main() exits done {forward run start();}
"#;
#[test]
fn field_borrows_generic_captures_and_native_execution_preserve_backing_storage() {
    use std::{collections::BTreeMap, fs, process::Command};
    let p = compile(NATIVE_SOURCE, manifest())
        .unwrap()
        .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
        .unwrap();
    let result = p.run(&scenario()).unwrap();
    let events = serde_json::to_value(&result.events).unwrap();
    assert_eq!(events[0]["message"], "settled");
    assert_eq!(events[1]["message"], "updated");
    let root = std::env::temp_dir().join(format!("tellur-borrowed-task-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (index, p) in [p.clone(), p.expanded("start").unwrap()].iter().enumerate() {
        let binary = root.join(format!("program-{index}"));
        p.build_native("main", &binary, 0).unwrap();
        let output = Command::new(binary)
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
            serde_json::to_value(&result).unwrap()
        );
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn stored_reference_sources_and_trait_calls_keep_their_origins() {
    let text = r#"
struct view['a] {ref string text <- 'a;}
struct message {string text;}
trait viewer {async control inspect(ref self input) exits done;}
implement viewer for message {async control inspect(ref self input) exits done {emit(borrow (deref input).text);exit done;}}
async control display(ref string input) exits done {emit(input);exit done;}
async control start() exits done {
    mutable string text="original";view saved=view {borrow text};
    auto pending=defer display(saved.text);drop(saved);
    await pending;text="available";
    message message=message {text};
    auto show=defer viewer<message>.inspect(borrow message);await show;exit done;
}
control main() exits done {forward run start();}
"#;
    let p = compile(text, manifest()).unwrap();
    let result = p.run(&scenario()).unwrap();
    assert_eq!(result.events.len(), 2);
    let bad = text.replace(
        "await pending;text=\"available\";",
        "text=\"available\";await pending;",
    );
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
}
#[test]
fn instruction_pauses_do_not_release_a_captured_exclusive_reference() {
    use tellur::runtime::ExecutionProgress;
    let p = compile(NATIVE_SOURCE, manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    let input = scenario();
    let mut execution = p.start_execution(&input, 400);
    for _ in 0..400 {
        match execution.advance(1) {
            ExecutionProgress::Paused => {}
            ExecutionProgress::Complete(result) => {
                assert_eq!(
                    serde_json::to_value(result.unwrap()).unwrap()["events"],
                    serde_json::to_value(expected.events).unwrap()
                );
                return;
            }
            ExecutionProgress::Finished => panic!("completion lost"),
        }
    }
    panic!("execution did not finish");
}

#[test]
fn borrowed_tasks_preserve_boundary_isolation_and_settle_local_captures() {
    let text = r#"
async control fail(ref string text) faults {emit(text);fault "failed";}
async control start() exits done {
    mutable string text="borrowed";
    supervised loop attempts(mutable bool first=true) {
        delta first {true=>try_work,false=>finished}
        [try_work]:first=false;auto pending=defer fail(borrow text);
            fault boundary {await pending;} contingency [] {recover again attempts;}
        [finished]:text="settled";emit(borrow text);exit done;
    }
}
control main() exits done {forward run start();}
"#;
    assert_eq!(
        compile(text, manifest()).unwrap_err().code,
        "E_BOUNDARY_BORROW"
    );
    let local = text.replace(
        "auto pending=defer fail(borrow text);\n            fault boundary {await pending;}",
        "fault boundary {string local=\"borrowed\";auto pending=defer fail(borrow local);await pending;}"
    );
    assert_ne!(local, text);
    let p = compile(&local, manifest()).unwrap();
    let result = p.run(&scenario()).unwrap();
    let messages = result
        .events
        .iter()
        .filter_map(|event| match event {
            tellur::runtime::Event::Emit { message } => Some(message.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(messages, ["borrowed", "settled"]);
    assert_eq!(
        p.expanded("start").unwrap().run(&scenario()).unwrap(),
        result
    );
}
