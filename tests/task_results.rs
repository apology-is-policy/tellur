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
fn program(body: &str) -> String {
    format!(
        r#"
async control lend(ref string left, ref string right) exits ready {{ref string value from left}} {{exit ready {{left}};}}
async control lend_mut(ref mutable string input) exits ready {{ref mutable string value from input}} {{exit ready {{input}};}}
async control start() exits done {{{body}}}
control main() exits done {{forward run start();}}
"#
    )
}
#[test]
fn await_keeps_selected_origins_and_releases_unrelated_captures() {
    let source = program(
        r#"mutable string left="left";mutable string right="right";
        auto pending=defer lend(borrow left,borrow right);auto moved=move pending;
        delta await moved {ready {auto value}=>ready}
        [ready]:right="free";emit(value);left="free too";emit(borrow left);exit done;"#,
    );
    let p = compile(&source, manifest()).unwrap();
    let result = p.run(&scenario()).unwrap();
    assert_eq!(
        serde_json::to_value(&result).unwrap()["events"][0]["message"],
        "left"
    );
    assert_eq!(
        p.expanded("start").unwrap().run(&scenario()).unwrap(),
        result
    );
    let bad = source.replace(
        "right=\"free\";emit(value)",
        "left=\"too soon\";emit(value)",
    );
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
}
#[test]
fn returned_exclusive_reference_keeps_reborrow_ancestry_and_move_semantics() {
    let source = program(
        r#"mutable string text="old";ref mutable string alias=borrow mutable text;
        auto pending=defer lend_mut(alias);
        delta await pending {ready {auto value}=>ready}
        [ready]:deref value="via result";emit(value);deref alias="via parent";emit(alias);exit done;"#,
    );
    let p = compile(&source, manifest()).unwrap();
    assert_eq!(p.run(&scenario()).unwrap().events.len(), 2);
    let bad = source.replace(
        "deref value=\"via result\";",
        "deref alias=\"too soon\";deref value=\"via result\";",
    );
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
    let transferred = source.replace("defer lend_mut(alias)", "defer lend_mut(move alias)");
    assert_eq!(
        compile(&transferred, manifest()).unwrap_err().code,
        "E_MOVED"
    );
    compile(
        &transferred.replace(
            "deref alias=\"via parent\";emit(alias);",
            "text=\"owner\";emit(borrow text);",
        ),
        manifest(),
    )
    .unwrap();
}
#[test]
fn completion_paths_keep_separate_capture_permissions() {
    let source = r#"
async control choose(bool choose_left, ref string left, ref string right)
    exits left {ref string value from left}, right {ref string value from right} {
    delta choose_left {true=>yes,false=>no}
    [yes]:exit left {left};
    [no]:exit right {right};
}
async control start() exits done {
    mutable string a="a";mutable string b="b";
    auto pending=defer choose(true,borrow a,borrow b);
    delta true {true=>first,false=>second}
    [first]:auto moved=move pending;delta await moved {left {auto x}=>a_result,right {auto y}=>b_result}
        [a_result]:b="free";emit(x);exit done;
        [b_result]:a="free";emit(y);exit done;
    [second]:delta await pending {left {auto z}=>other_a,right {auto w}=>other_b}
        [other_a]:b="free";emit(z);exit done;
        [other_b]:a="free";emit(w);exit done;
}
control main() exits done {forward run start();}
"#;
    compile(source, manifest())
        .unwrap()
        .run(&scenario())
        .unwrap();
    for bad in [
        source.replace("b=\"free\";emit(x)", "a=\"bad\";emit(x)"),
        source.replace("a=\"free\";emit(w)", "b=\"bad\";emit(w)"),
    ] {
        assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
    }
}
const STORED: &str = r#"
struct text_pair['a, 'b] {ref string left <- 'a;ref string right <- 'b;}
async control pair(ref string a, ref string b) exits ready {text_pair[a,b] value} {
    exit ready {text_pair {a,b}};
}
async control optional(ref string a) exits ready {option<ref string>[a] value} {
    exit ready {some {a}};
}
async control start() exits done {
    mutable string a="left";mutable string b="right";
    auto pending=defer pair(borrow a,borrow b);
    delta await pending {ready {auto view}=>ready}
    [ready]:ref string left=view.left;drop(view);b="released";emit(left);
        a="later";auto maybe=defer optional(borrow a);
        delta await maybe {ready {auto result}=>received}
        [received]:delta result {some {auto item}=>present,none=>absent}
            [present]:emit(item);a="settled";exit done;
            [absent]:a="settled";exit done;
}
control main() exits done {forward run start();}
"#;
#[test]
fn borrowed_record_and_sum_completions_preserve_leaf_origins() {
    let p = compile(STORED, manifest()).unwrap();
    let result = p.run(&scenario()).unwrap();
    assert_eq!(
        serde_json::to_value(&result).unwrap()["events"][1]["message"],
        "later"
    );
    assert_eq!(
        p.expanded("start").unwrap().run(&scenario()).unwrap(),
        result
    );
    let bad = STORED.replace("b=\"released\";emit(left)", "a=\"too soon\";emit(left)");
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
    let bad = STORED.replace(
        "[received]:delta result",
        "[received]:a=\"too soon\";delta result",
    );
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
}
#[test]
fn stored_adapters_forwarding_and_generic_producers_transfer_borrowed_results() {
    let source = r#"
async control lend<t>(ref t input) exits ready {ref t value from input} {exit ready {input};}
async control wrapped(ref string input) exits ready {ref string value from input} {
    auto pending=defer lend<string>(input);forward await pending;
}
async control start() exits done {
    mutable string text="borrowed";auto pending=defer wrapped(borrow text);
    auto result=capture await pending;
    delta result {ready {auto value}=>ready}
    [ready]:emit(value);text="free";exit done;
}
control main() exits done {forward run start();}
"#;
    let p = compile(source, manifest()).unwrap();
    assert_eq!(p.run(&scenario()).unwrap().events.len(), 1);
    let bad = source.replace("delta result {", "text=\"bad\";delta result {");
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
    let inline = source.replace(
        "auto result=capture await pending;",
        "auto result=delta await pending {ready {auto x}=>ready {x}};",
    );
    compile(&inline, manifest())
        .unwrap()
        .run(&scenario())
        .unwrap();
}
#[test]
fn native_and_instruction_stepping_retain_borrowed_completion_storage() {
    use std::{collections::BTreeMap, fs, process::Command};
    use tellur::runtime::ExecutionProgress;
    let p = compile(STORED, manifest())
        .unwrap()
        .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
        .unwrap();
    let expected = p.run(&scenario()).unwrap();
    let input = scenario();
    let mut execution = p.start_execution(&input, 400);
    let mut complete = false;
    for _ in 0..400 {
        match execution.advance(1) {
            ExecutionProgress::Paused => {}
            ExecutionProgress::Complete(result) => {
                assert_eq!(
                    result.unwrap(),
                    tellur::runtime::RunCompletion::Outcome(expected.clone())
                );
                complete = true;
                break;
            }
            ExecutionProgress::Finished => panic!("completion lost"),
        }
    }
    assert!(complete);
    let root = std::env::temp_dir().join(format!("tellur-task-results-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (i, p) in [p.clone(), p.expanded("start").unwrap()].iter().enumerate() {
        let binary = root.join(format!("program-{i}"));
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
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn inferred_field_projections_and_opaque_split_permissions_survive_await() {
    let source = r#"
struct pair {string left;string right;}
async control left(ref pair input) exits auto {exit ready {borrow (deref input).left};}
async control split(ref mutable pair input)
    exits ready {ref mutable string left from input, ref mutable string right from input} {
    exit ready {borrow mutable (deref input).left,borrow mutable (deref input).right};
}
async control start() exits done {
    mutable pair data=pair {"left","right"};
    auto pending=defer left(borrow data);
    delta await pending {ready {auto selected}=>ready}
    [ready]:data.right="independent";emit(selected);
        auto both=defer split(borrow mutable data);
        delta await both {ready {auto a,auto b}=>split}
        [split]:deref a="first";deref b="second";emit(a);emit(b);data.left="free";exit done;
}
control main() exits done {forward run start();}
"#;
    let p = compile(source, manifest()).unwrap();
    assert_eq!(p.run(&scenario()).unwrap().events.len(), 3);
    let bad = source.replace("data.right=\"independent\"", "data.left=\"bad\"");
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
    let bad = source.replace("deref a=\"first\"", "data.left=\"bad\";deref a=\"first\"");
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
}
#[test]
fn trait_contracts_and_union_origins_remain_conservative() {
    let source = r#"
trait choice {async control choose(ref self left,ref self right,bool flag) exits ready {ref self value from left};}
implement choice for string {
    async control choose(ref self left,ref self right,bool flag) exits ready {ref self value from left} {
        exit ready {left};
    }
}
async control start() exits done {
    mutable string a="a";mutable string b="b";
    auto pending=defer choice<string>.choose(borrow a,borrow b,true);
    delta await pending {ready {auto value}=>ready}
    [ready]:emit(value);a="free";b="free";exit done;
}
control main() exits done {forward run start();}
"#;
    compile(source, manifest())
        .unwrap()
        .run(&scenario())
        .unwrap();
    let union = source.replace("defer choice<string>.choose", "defer union");
    let union = format!(
        "async control union(ref string left,ref string right,bool flag) exits auto {{exit delta flag {{true=>ready {{left}},false=>ready {{right}}}};}}{union}"
    );
    compile(&union, manifest())
        .unwrap()
        .run(&scenario())
        .unwrap();
    for owner in ["a", "b"] {
        let bad = union.replace(
            "[ready]:emit(value)",
            &format!("[ready]:{owner}=\"bad\";emit(value)"),
        );
        assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
    }
}
#[test]
fn borrowed_results_cannot_escape_a_task_owned_value_or_local_caller_storage() {
    for source in [
        "async control bad(string text) exits auto {exit ready {borrow text};} async control main() exits done {auto pending=defer bad(\"owned\");delta await pending {ready {auto value}=>ready}[ready]:emit(value);exit done;}",
        "async control lend(ref string text) exits ready {ref string value from text} {exit ready {text};} async control bad() exits auto {string text=\"local\";auto pending=defer lend(borrow text);forward await pending;}",
    ] {
        assert_eq!(compile(source, manifest()).unwrap_err().code, "E_LIFETIME");
    }
}

#[test]
fn same_producer_tasks_keep_distinct_capture_instances() {
    let source = program(
        r#"mutable string a="a";mutable string b="b";
        auto first=defer lend(borrow a,borrow b);
        auto second=defer lend(borrow b,borrow a);
        delta await first {ready {auto x}=>first_ready}
        [first_ready]:delta await second {ready {auto y}=>second_ready}
        [second_ready]:emit(x);a="free";emit(y);b="free";exit done;"#,
    );
    let p = compile(&source, manifest()).unwrap();
    let result = serde_json::to_value(p.run(&scenario()).unwrap()).unwrap();
    assert_eq!(result["events"][0]["message"], "a");
    assert_eq!(result["events"][1]["message"], "b");
    for bad in [
        source.replace("[first_ready]:delta", "[first_ready]:b=\"bad\";delta"),
        source.replace("[second_ready]:emit(x)", "[second_ready]:a=\"bad\";emit(x)"),
    ] {
        assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
    }
}
