use tellur::{
    compile,
    manifest::Manifest,
    runtime::{ExecutionProgress, Scenario},
};

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
async control inspect_text(ref string text) exits done {{yield;emit(text);exit done;}}
async control edit(ref mutable string text) exits done {{yield;deref text="edited";exit done;}}
async control startup() exits done {{{body}}}
control main() exits done {{forward run startup();}}
"#
    )
}
#[test]
fn direct_spawn_loans_preserve_explicit_schedules_and_disjoint_fields() {
    let p = compile(
        include_str!("../playground/borrowed_spawn.tellur"),
        manifest(),
    )
    .unwrap();
    let expected = p.run_completion(&scenario(), 1000).unwrap();
    let value = serde_json::to_value(&expected).unwrap();
    let messages: Vec<_> = value["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["message"].as_str())
        .collect();
    assert_eq!(
        messages,
        [
            "queued",
            "first",
            "second",
            "parent",
            "shared",
            "shared",
            "left edited",
            "right edited"
        ]
    );
    for p in [p.clone(), p.expanded("main").unwrap()] {
        for quota in [1, 3, usize::MAX] {
            let s = scenario();
            let mut session = p.start_execution(&s, 1000);
            assert!(matches!(session.advance(0), ExecutionProgress::Paused));
            loop {
                match session.advance(quota) {
                    ExecutionProgress::Complete(result) => {
                        assert_eq!(result.unwrap(), expected);
                        break;
                    }
                    ExecutionProgress::Paused => {}
                    other => panic!("{other:?}"),
                }
            }
        }
    }
}
#[test]
fn captures_protect_queued_running_and_unjoined_storage() {
    for handoffs in ["", "yield;", "yield;yield;yield;"] {
        for (capture, invalid) in [
            ("inspect_text(borrow text)", "text=\"changed\";"),
            ("inspect_text(borrow text)", "drop(text);"),
            ("edit(borrow mutable text)", "emit(borrow text);"),
            (
                "edit(borrow mutable text)",
                "auto peer=spawn inspect_text(borrow text);await peer;",
            ),
        ] {
            let s = source(&format!(
                "mutable string text=\"original\";auto task=spawn {capture};auto moved=move task;{handoffs}{invalid}await moved;exit done;"
            ));
            let error = compile(&s, manifest()).unwrap_err();
            assert_eq!(error.code, "E_LOAN", "{s}: {error:?}");
            let repaired = s.replace(
                &format!("{invalid}await moved;"),
                &format!("await moved;{invalid}"),
            );
            compile(&repaired, manifest())
                .unwrap()
                .run(&scenario())
                .unwrap();
        }
    }
}
#[test]
fn shared_capture_joins_release_only_the_consumed_loan() {
    let s = source(
        "mutable string text=\"original\";auto first=spawn inspect_text(borrow text);auto second=spawn inspect_text(borrow text);yield;await first;text=\"changed\";await second;exit done;",
    );
    assert_eq!(compile(&s, manifest()).unwrap_err().code, "E_LOAN");
    let repaired = s.replace(
        "text=\"changed\";await second;",
        "await second;text=\"changed\";",
    );
    compile(&repaired, manifest())
        .unwrap()
        .run(&scenario())
        .unwrap();
}
#[test]
fn reference_reborrows_moves_and_path_joins_preserve_authority() {
    let base = "mutable string text=\"original\";ref mutable string alias=borrow mutable text;auto task=spawn edit(alias);auto child=move task;await child;deref alias=\"after\";emit(alias);exit done;";
    let s = source(base);
    let wrapped = s.replace("auto child=move task;", "auto wrapped=some {task};delta wrapped {some {auto child}=>ready,none=>empty} [empty]:exit done;[ready]:");
    assert_eq!(
        compile(&wrapped, manifest()).unwrap_err().code,
        "E_MANIFEST"
    );
    compile(&s, manifest()).unwrap().run(&scenario()).unwrap();
    assert_eq!(
        compile(
            &s.replace("await child;deref alias", "deref alias"),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
    assert_eq!(
        compile(
            &s.replace("spawn edit(alias)", "spawn edit(move alias)"),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_MOVED"
    );
    let paths = source(
        "mutable string text=\"original\";auto task=spawn edit(borrow mutable text);delta true {true=>yes,false=>no} [yes]:await task;text=\"yes\";exit done;[no]:auto moved=move task;await moved;text=\"no\";exit done;",
    );
    compile(&paths, manifest())
        .unwrap()
        .run(&scenario())
        .unwrap();
    assert_eq!(
        compile(
            &paths.replace("await moved;text=\"no\";", "text=\"no\";await moved;"),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
}
#[test]
fn borrowed_spawn_does_not_admit_temporary_carrier_or_result_references() {
    for s in [
        "struct view['a] {ref string text <- 'a;} async control work(ref view value) exits done {exit done;} async control caller(ref view value) exits done {auto task=spawn work(value);await task;exit done;}",
        "struct view['a] {ref string text <- 'a;} struct nested['storage,'a] {ref view['a] value <- 'storage;} async control work(nested value) exits done {exit done;} async control caller(nested value) exits done {auto task=spawn work(value);await task;exit done;}",
        "struct message {string text;constructor(string text){self.text=move text;}} async control work(ref message value) exits done {exit done;} async control caller() exits done {auto task=spawn work(borrow message(\"temporary\"));await task;exit done;}",
    ] {
        assert_eq!(compile(s, manifest()).unwrap_err().code, "E_ASYNC", "{s}");
    }
}
#[test]
fn sibling_fault_settles_direct_borrowers_without_nested_handlers() {
    let s = include_str!("fixtures/source_borrowed_task_fault.tellur");
    for text in [
        s.to_owned(),
        s.replace(
            "auto failure = spawn fail();",
            "auto failure = spawn fail();auto moved = move second;",
        )
        .replace("await second;", "await moved;"),
    ] {
        let p = compile(&text, manifest()).unwrap();
        for p in [p.clone(), p.expanded("main").unwrap()] {
            let result = p.run_completion(&scenario(), 1000).unwrap();
            let value = serde_json::to_value(result).unwrap();
            assert_eq!(value["status"], 23);
            let messages: Vec<_> = value["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|event| event["message"].as_str())
                .collect();
            assert_eq!(messages, ["queued", "reader started", "owner contingency"]);
        }
    }
}

#[test]
fn concrete_calls_and_stored_reference_sources_retain_capture_origins() {
    let text = r#"
struct view['a] {ref string text <- 'a;}
struct message {string text;}
trait viewer {async control inspect(ref self input) exits done;}
implement viewer for message {async control inspect(ref self input) exits done {yield;emit(borrow (deref input).text);exit done;}}
async control inspect_value<t>(ref t input) exits done {yield;exit done;}
async control display(ref string input) exits done {yield;emit(input);exit done;}
async control start() exits done {
    mutable string text="original";view saved=view {borrow text};
    auto pending=spawn display(saved.text);drop(saved);
    auto observer=spawn inspect_value<string>(borrow text);
    await pending;await observer;text="available";
    message value=message {text};
    auto show=spawn viewer<message>.inspect(borrow value);await show;exit done;
}
control main() exits done {forward run start();}
"#;
    let p = compile(text, manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    assert_eq!(expected.events.len(), 2);
    assert_eq!(
        p.expanded("main").unwrap().run(&scenario()).unwrap(),
        expected
    );
    let bad = text.replace(
        "await pending;await observer;text=\"available\";",
        "await pending;text=\"available\";await observer;",
    );
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
}
