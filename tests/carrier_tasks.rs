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
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("start").unwrap().run(&scenario()).unwrap()
    );
    p
}
fn rejects(source: &str, code: &str) {
    let error = compile(source, manifest()).unwrap_err();
    assert_eq!(error.code, code, "{error}\n{source}");
}
const SHARED: &str = r#"
struct pair['a,'b] {ref string left <- 'a;ref string right <- 'b;}
async control first(ref pair input) exits ready {ref string value from input['a]} {exit ready {input.left};}
async control start() exits done {
    mutable string a="left";mutable string b="right";
    pair saved=pair {borrow a,borrow b};auto pending=defer first(borrow saved);
    auto moved=move pending;emit("created");
    delta await moved {ready {auto value}=>ready}
    [ready]:drop(saved);b="released";emit(value);a="free";exit done;
}
control main() exits done {forward run start();}
"#;
#[test]
fn shared_carrier_storage_is_retained_then_detached_results_keep_only_selected_sources() {
    run(SHARED);
    for action in ["drop(saved);", "a=\"bad\";", "b=\"bad\";"] {
        rejects(
            &SHARED.replace("auto moved=", &format!("{action}auto moved=")),
            "E_LOAN",
        );
    }
    rejects(
        &SHARED.replace("emit(value);", "a=\"bad\";emit(value);"),
        "E_LOAN",
    );
}
const EXCLUSIVE: &str = r#"
struct view['a] {ref mutable string text <- 'a;}
async control lend(ref mutable view input) exits ready {ref mutable view value from input} {exit ready {input};}
async control start() exits done {
    mutable string owner="before";mutable view saved=view {borrow mutable owner};
    auto pending=defer lend(borrow mutable saved);auto moved=move pending;
    delta await moved {ready {auto value}=>ready}
    [ready]:deref value.text="after";emit(value.text);drop(saved);owner="free";exit done;
}
control main() exits done {forward run start();}
"#;
#[test]
fn exclusive_carrier_results_keep_storage_and_nested_permission() {
    run(EXCLUSIVE);
    for action in [
        "drop(saved);",
        "owner=\"bad\";",
        "deref saved.text=\"bad\";",
    ] {
        for marker in ["auto moved=", "deref value.text="] {
            rejects(
                &EXCLUSIVE.replace(marker, &format!("{action}{marker}")),
                "E_LOAN",
            );
        }
    }
}
#[test]
fn shared_outer_authority_cannot_mutate_or_transfer_exclusive_contents() {
    let shared = EXCLUSIVE
        .replace("ref mutable view", "ref view")
        .replace("lend(borrow mutable saved)", "lend(borrow saved)")
        .replace("deref value.text=\"after\";", "")
        .replace("emit(value.text)", "emit(borrow deref value.text)");
    run(&shared);
    rejects(
        &shared.replace("emit(borrow deref value.text)", "deref value.text=\"bad\""),
        "E_MUTABILITY",
    );
    assert!(
        compile(
            &shared.replace(
                "emit(borrow deref value.text)",
                "ref mutable string alias=value.text"
            ),
            manifest()
        )
        .is_err()
    );
}
#[test]
fn carrier_alias_reborrow_and_explicit_transfer_have_distinct_consumption() {
    let alias = EXCLUSIVE
        .replace(
            "auto pending=",
            "ref mutable view alias=borrow mutable saved;auto pending=",
        )
        .replace("lend(borrow mutable saved)", "lend(alias)")
        .replace("drop(saved);", "deref alias.text=\"again\";drop(saved);");
    run(&alias);
    rejects(
        &alias.replace("auto moved=", "deref alias.text=\"bad\";auto moved="),
        "E_LOAN",
    );
    let moved = alias.replace("lend(alias)", "lend(move alias)");
    rejects(&moved, "E_MOVED");
    run(&moved.replace("deref alias.text=\"again\";", ""));
}
const NESTED: &str = r#"
struct view['a] {ref mutable string text <- 'a;}
struct job['storage,'text] {ref mutable view['text] target <- 'storage;}
async control first(ref mutable job input) exits ready... {exit ready {borrow mutable deref input.target.text};}
async control start() exits done {
    mutable string owner="before";mutable view saved=view {borrow mutable owner};
    mutable job work=job {borrow mutable saved};auto pending=defer first(borrow mutable work);
    auto moved=move pending;
    delta await moved {ready {auto value}=>ready}
    [ready]:deref value="after";emit(value);drop(work);drop(saved);owner="free";exit done;
}
control main() exits done {forward run start();}
"#;
#[test]
fn nested_carrier_contents_preserve_through_paths_and_storage_guards() {
    run(NESTED);
    for action in [
        "drop(work);",
        "drop(saved);",
        "owner=\"bad\";",
        "deref saved.text=\"bad\";",
    ] {
        for marker in ["auto moved=", "deref value="] {
            rejects(
                &NESTED.replace(marker, &format!("{action}{marker}")),
                "E_LOAN",
            );
        }
    }
    let shared = NESTED
        .replace("first(ref mutable job", "first(ref job")
        .replace(
            "borrow mutable deref input.target.text",
            "borrow deref input.target.text",
        )
        .replace("first(borrow mutable work)", "first(borrow work)")
        .replace("deref value=\"after\";", "");
    run(&shared);
    rejects(
        &shared.replace("emit(value);", "deref value=\"bad\";emit(value);"),
        "E_MUTABILITY",
    );
}
const SUM: &str = r#"
async control first<t>(ref mutable option<option<ref mutable t>> input) exits ready... {exit ready {input};}
async control start() exits done {
    mutable string owner="before";
    mutable option<option<ref mutable string>> saved=some {some {borrow mutable owner}};
    auto pending=defer first<string>(borrow mutable saved);
    auto result=capture await pending;
    delta result {ready {auto value}=>ready}
    [ready]:delta value {some {auto inner}=>outer,none=>empty}
        [outer]:delta inner {some {auto text}=>present,none=>absent}
            [present]:deref text="after";emit(text);
    [entry]:drop(saved);owner="free";exit done;
}
control main() exits done {forward run start();}
"#;
#[test]
fn generic_borrowed_nested_sums_preserve_active_tags_and_empty_alternatives() {
    run(SUM);
    run(&SUM.replace("some {some {borrow mutable owner}}", "some {none {}}"));
    run(&SUM.replace("some {some {borrow mutable owner}}", "none {}"));
    for marker in ["auto result=", "deref text="] {
        for action in ["drop(saved);", "owner=\"bad\";"] {
            rejects(&SUM.replace(marker, &format!("{action}{marker}")), "E_LOAN");
        }
    }
}
#[test]
fn carrier_capture_loans_survive_faults_and_selected_settlement() {
    for action in ["drop(saved);", "owner=\"bad\";"] {
        rejects(
            &EXCLUSIVE.replace(
                "auto moved=move pending;",
                &format!("{action}fault \"stop\";"),
            ),
            "E_LOAN",
        );
    }
    let branches=EXCLUSIVE.replace("auto moved=move pending;", "delta true {true=>first,false=>second}\n[first]:delta await pending {ready {auto delivered}=>received}\n[received]:emit(delivered.text);drop(saved);owner=\"free\";exit done;\n[second]:auto moved=move pending;");
    run(&branches);
}

#[test]
fn disjoint_carrier_captures_and_projected_results_keep_instance_identity() {
    let source = r#"
struct view['a] {ref mutable string text <- 'a;}
struct pair['a,'b] {view['a] left;view['b] right;}
async control lend(ref mutable view input) exits ready... {exit ready {input};}
async control project(ref mutable pair input) exits ready... {exit ready {borrow mutable input.left};}
async control start() exits done {
    mutable string a="a";mutable string b="b";
    mutable pair saved=pair {view {borrow mutable a},view {borrow mutable b}};
    auto left=defer lend(borrow mutable saved.left);auto right=defer lend(borrow mutable saved.right);
    delta await left {ready {auto l}=>left}
    [left]:deref l.text="left";emit(l.text);
    delta await right {ready {auto r}=>right}
    [right]:deref r.text="right";emit(r.text);
    auto projected=defer project(borrow mutable saved);
    delta await projected {ready {auto value}=>projected}
    [projected]:deref value.text="projected";emit(value.text);drop(saved);a="free";b="free";exit done;
}
control main() exits done {forward run start();}
"#;
    run(source);
    rejects(
        &source.replace(
            "delta await right",
            "deref saved.right.text=\"bad\";delta await right",
        ),
        "E_LOAN",
    );
    rejects(
        &source.replace("deref value.text=", "drop(saved);deref value.text="),
        "E_LOAN",
    );
}

#[test]
fn whole_carrier_tasks_preserve_isolation_and_temporary_lifetime_limits() {
    let isolated=EXCLUSIVE.replace("auto moved=move pending;", "fault boundary {delta await pending {ready {auto result}=>received}[received]:emit(result.text);} contingency [] {terminate process 1;}auto moved=move pending;");
    rejects(&isolated, "E_BOUNDARY_BORROW");
    let temporary = r#"
struct view['a] {ref string text <- 'a;constructor(ref string input)[input] {self.text=input;}}
async control show(ref view input) exits done {emit(input.text);exit done;}
async control start() exits done {string owner="text";auto pending=defer show(borrow view(borrow owner));await pending;exit done;}
"#;
    rejects(temporary, "E_ASYNC");
}

#[test]
fn returned_carrier_parameters_can_be_captured_again_in_nested_async_calls() {
    let source=EXCLUSIVE.replace("async control start()", "async control relay(ref mutable view input) exits ready... {auto child=defer lend(input);forward await child;}\nasync control start()")
        .replace("defer lend(borrow mutable saved)", "defer relay(borrow mutable saved)");
    run(&source);
    rejects(
        &source.replace("deref value.text=", "drop(saved);deref value.text="),
        "E_LOAN",
    );
    let shared = source
        .replace("ref mutable view", "ref view")
        .replace("relay(borrow mutable saved)", "relay(borrow saved)")
        .replace("deref value.text=\"after\";", "")
        .replace("emit(value.text)", "emit(borrow deref value.text)");
    run(&shared);
    rejects(
        &shared.replace("emit(borrow deref value.text)", "deref value.text=\"bad\""),
        "E_MUTABILITY",
    );
}

#[test]
fn native_expansion_and_step_partitions_preserve_carrier_task_observations() {
    use std::{collections::BTreeMap, fs, process::Command};
    use tellur::runtime::ExecutionProgress;
    let root = std::env::temp_dir().join(format!("tellur-carrier-tasks-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (name, source) in [
        ("shared", SHARED),
        ("exclusive", EXCLUSIVE),
        ("nested", NESTED),
        ("sum", SUM),
    ] {
        let p = run(source)
            .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
            .unwrap();
        let expected = p.run(&scenario()).unwrap();
        let input_scenario = scenario();
        let mut execution = p.start_execution(&input_scenario, 1000);
        let mut complete = false;
        for _ in 0..1000 {
            match execution.advance(1) {
                ExecutionProgress::Paused => {}
                ExecutionProgress::Complete(report) => {
                    assert_eq!(
                        report.unwrap(),
                        tellur::runtime::RunCompletion::Outcome(expected.clone())
                    );
                    complete = true;
                    break;
                }
                ExecutionProgress::Finished => panic!("completion lost"),
            }
        }
        assert!(complete);
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
                serde_json::to_value(&expected).unwrap()
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}
