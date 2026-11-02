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
fn run(source: &str) -> tellur::CheckedProgram {
    let p = compile(source, manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    assert_eq!(
        p.expanded("start").unwrap().run(&scenario()).unwrap(),
        expected
    );
    p
}
const PAIR: &str = r#"
struct pair['a,'b] {ref string left <- 'a;ref string right <- 'b;}
async control first(pair input) exits ready {ref string value from input['a]} {exit ready {input.left};}
async control start() exits done {
    mutable string a="left";mutable string b="right";
    pair view=pair {borrow a,borrow b};
    auto pending=defer first(view);drop(view);
    auto moved=move pending;emit("created");
    delta await moved {ready {auto value}=>ready}
    [ready]:b="released";emit(value);a="free";exit done;
}
control main() exits done {forward run start();}
"#;
#[test]
fn owned_record_captures_keep_each_group_until_its_last_retainer() {
    run(PAIR);
    for source in [
        PAIR.replace("auto moved=", "b=\"bad\";auto moved="),
        PAIR.replace("b=\"released\";emit(value)", "a=\"bad\";emit(value)"),
    ] {
        assert_eq!(compile(&source, manifest()).unwrap_err().code, "E_LOAN");
    }
    let copies = PAIR
        .replace("drop(view);", "")
        .replace("a=\"free\";", "emit(view.right);drop(view);a=\"free\";");
    assert_eq!(compile(&copies, manifest()).unwrap_err().code, "E_LOAN");
    run(&copies.replace("b=\"released\";", ""));
}
const EXCLUSIVE: &str = r#"
struct view['a] {ref mutable string text <- 'a;}
async control relay(view input) exits ready {view[input['a]] value} {exit ready {input};}
async control start() exits done {
    mutable string owner="before";view input=view {borrow mutable owner};
    auto pending=defer relay(input);auto moved=move pending;
    delta await moved {ready {auto value}=>ready}
    [ready]:deref value.text="after";emit(value.text);drop(value);owner="free";exit done;
}
control main() exits done {forward run start();}
"#;
#[test]
fn exclusive_record_capture_moves_its_permission_into_the_result() {
    run(EXCLUSIVE);
    for (source, code) in [
        (
            EXCLUSIVE.replace("auto moved=", "emit(input.text);auto moved="),
            "E_MOVED",
        ),
        (
            EXCLUSIVE.replace("auto moved=", "owner=\"bad\";auto moved="),
            "E_LOAN",
        ),
        (
            EXCLUSIVE.replace("deref value.text=", "owner=\"bad\";deref value.text="),
            "E_LOAN",
        ),
    ] {
        assert_eq!(compile(&source, manifest()).unwrap_err().code, code);
    }
}
const OPTIONAL: &str = r#"
async control relay(option<ref mutable string> input)
    exits ready {option<ref mutable string>[input['source]] value} {exit ready {input};}
async control start() exits done {
    mutable string owner="before";
    option<ref mutable string> input=some {borrow mutable owner};
    auto pending=defer relay(input);
    auto result=capture await pending;
    delta result {ready {auto value}=>ready}
    [ready]:delta value {some {auto text}=>present,none=>absent}
        [present]:deref text="after";emit(text);owner="free";exit done;
        [absent]:owner="free";exit done;
}
control main() exits done {forward run start();}
"#;
#[test]
fn tagged_captures_transfer_or_release_only_the_selected_payload() {
    run(OPTIONAL);
    run(&OPTIONAL.replace("some {borrow mutable owner}", "none {}"));
    assert_eq!(
        compile(
            &OPTIONAL.replace("auto result=", "owner=\"bad\";auto result="),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
    assert_eq!(
        compile(
            &OPTIONAL.replace("[present]:deref", "[present]:owner=\"bad\";deref"),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
}
const CARRIER: &str = r#"
struct text_view['text] {ref mutable string text <- 'text;}
struct job['storage,'text] {ref mutable text_view['text] target <- 'storage;}
async control relay(job input) exits ready {job[input['storage],input['text]] value} {exit ready {input};}
async control start() exits done {
    mutable string owner="before";mutable text_view saved=text_view {borrow mutable owner};
    job input=job {borrow mutable saved};auto pending=defer relay(input);
    auto moved=move pending;
    delta await moved {ready {auto value}=>ready}
    [ready]:deref value.target.text="after";emit(value.target.text);drop(value);drop(saved);owner="free";exit done;
}
control main() exits done {forward run start();}
"#;
#[test]
fn contained_carrier_captures_keep_storage_guards_and_external_sources() {
    run(CARRIER);
    for action in [
        "drop(saved);",
        "owner=\"bad\";",
        "deref saved.text=\"bad\";",
    ] {
        for marker in ["auto moved=", "deref value.target.text="] {
            let source = CARRIER.replace(marker, &format!("{action}{marker}"));
            assert_eq!(compile(&source, manifest()).unwrap_err().code, "E_LOAN");
        }
    }
}
#[test]
fn nested_tags_and_generic_owned_captures_preserve_origin_paths() {
    let source = r#"
async control relay<t>(option<option<ref t>> input) exits ready... {exit ready {input};}
async control start() exits done {
    mutable string owner="nested";
    option<option<ref string>> input=some {some {borrow owner}};
    auto pending=defer relay<string>(input);drop(input);
    delta await pending {ready {auto value}=>ready}
    [ready]:delta value {some {auto inner}=>outer,none=>empty}
        [outer]:delta inner {some {auto text}=>present,none=>absent}
            [present]:emit(text);owner="free";exit done;
            [absent]:owner="free";exit done;
        [empty]:owner="free";exit done;
}
control main() exits done {forward run start();}
"#;
    run(source);
    assert_eq!(
        compile(
            &source.replace("[present]:emit(text)", "[present]:owner=\"bad\";emit(text)"),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
}
#[test]
fn captured_record_constructors_evaluate_now_and_exclude_later_conflicts() {
    let source = r#"
struct view['a] {ref string text <- 'a;constructor(ref string input)[input] {self.text=input;}}
struct marker {string text;constructor(ref mutable string input) {deref input="changed";self.text="marker";}}
async control show(view input,marker other) exits done {emit(input.text);exit done;}
async control start() exits done {
    mutable string owner="owner";mutable string scratch="scratch";
    auto pending=defer show(view(borrow owner),marker(borrow mutable scratch));
    emit(borrow scratch);await pending;owner="free";exit done;
}
control main() exits done {forward run start();}
"#;
    run(source);
    let bad = source.replace(
        "marker(borrow mutable scratch)",
        "marker(borrow mutable owner)",
    );
    assert!(matches!(
        compile(&bad, manifest()).unwrap_err().code,
        "E_ALIAS" | "E_LOAN"
    ));
}
#[test]
fn contained_pending_loans_survive_fault_paths_and_keep_existing_isolation() {
    let source = PAIR.replace(
        "auto moved=move pending;emit(\"created\");",
        "b=\"bad\";fault \"stop\";",
    );
    assert_eq!(compile(&source, manifest()).unwrap_err().code, "E_LOAN");
    let source=PAIR.replace("auto moved=move pending;emit(\"created\");", "fault boundary {delta await pending {ready {auto item}=>received}[received]:emit(item);} contingency [] {terminate process 1;} auto moved=move pending;");
    assert_eq!(
        compile(&source, manifest()).unwrap_err().code,
        "E_BOUNDARY_BORROW"
    );
}
#[test]
fn whole_carrier_borrows_are_supported_but_nested_task_storage_remains_gated() {
    let source = "struct view['a] {ref string text <- 'a;} async control show(ref view input) exits done {exit done;} async control start(ref view input) exits done {auto pending=defer show(input);await pending;exit done;}";
    compile(source, manifest()).unwrap();
    let source = PAIR.replace("auto moved=move pending;", "auto wrapped=some {pending};");
    assert!(
        compile(&source, manifest())
            .unwrap_err()
            .message
            .contains("borrowed tasks cannot yet be nested")
    );
}
#[test]
fn native_and_expanded_execution_retain_owned_contained_captures() {
    use std::{collections::BTreeMap, fs, process::Command};
    let root = std::env::temp_dir().join(format!("tellur-contained-tasks-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (name, source) in [
        ("record", PAIR),
        ("exclusive", EXCLUSIVE),
        ("option", OPTIONAL),
        ("carrier", CARRIER),
    ] {
        let p = run(source)
            .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
            .unwrap();
        let expected = serde_json::to_value(p.run(&scenario()).unwrap()).unwrap();
        for (index, p) in [p.clone(), p.expanded("start").unwrap()].iter().enumerate() {
            let binary = root.join(format!("{name}-{index}"));
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
                expected
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn carrier_capture_settlement_is_path_sensitive_even_without_future_handle_use() {
    let branched=CARRIER.replace("auto moved=move pending;", "delta true {true=>first,false=>second}\n[first]:delta await pending {ready {auto delivered}=>first_ready}\n[first_ready]:drop(delivered);drop(saved);owner=\"released\";exit done;\n[second]:auto moved=move pending;");
    run(&branched);
    for action in ["drop(saved);", "owner=\"bad\";"] {
        let faulted = CARRIER.replace(
            "auto moved=move pending;",
            &format!("{action}fault \"stop\";"),
        );
        assert_eq!(compile(&faulted, manifest()).unwrap_err().code, "E_LOAN");
    }
}
