use tellur::{compile, ir::Value, manifest::Manifest, runtime::Scenario};
fn m() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
fn check(s: &str) -> tellur::CheckedProgram {
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    p
}
fn rejects(s: &str, codes: &[&str]) {
    let e = compile(s, m()).unwrap_err();
    assert!(codes.contains(&e.code), "{e}");
}
#[test]
fn local_variants_preserve_shared_loans_and_copies() {
    let s = r#"variant view {found {ref string text},empty}
 control main() exits done {mutable string owner="hello";view saved=found {borrow owner};view copied=saved;
 delta saved {found {auto a}=>a,empty=>none}
 [a]:emit(a);[none]:emit("empty");
 [entry]:delta copied {found {auto b}=>b,empty=>missing}
 [b]:emit(b);[missing]:emit("missing");[entry]:owner="released";exit done;}"#;
    check(s);
    rejects(
        &s.replace("delta copied", "owner=\"bad\";delta copied"),
        &["E_LOAN"],
    );
}
#[test]
fn captured_results_keep_owner_until_handled_and_preserve_obligation() {
    let s = r#"control peek(ref string input,bool yes) exits found {ref string text from input},empty {exit delta yes {true=>found {input},false=>empty {}};}
 control main() exits done {mutable string owner="hello";auto saved=capture peek(borrow owner,true);
 delta saved {found {auto text}=>ready,empty=>missing}
 [ready]:emit(text);owner="released";exit done;
 [missing]:owner="unused";exit done;}"#;
    check(s);
    rejects(
        &s.replace("delta saved", "owner=\"bad\";delta saved"),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("emit(text);owner", "owner=\"bad\";emit(text);owner"),
        &["E_LOAN"],
    );
    let unhandled = s.split(" delta saved").next().unwrap().to_owned() + "exit done;}";
    rejects(&unhandled, &["E_OUTCOME"]);
    check(&s.replace("borrow owner,true", "borrow owner,false"));
}
#[test]
fn mutable_payload_moves_and_releases_on_empty_arm() {
    let s = r#"variant edit {found {ref mutable string text},empty}
 control main() exits done... {mutable string owner="before";edit saved=found {borrow mutable owner};edit moved=saved;
 delta moved {found {auto text}=>ready,empty=>missing}
 [ready]:deref text="after";exit done {owner};
 [missing]:exit done {owner};}"#;
    let p = check(s);
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("after".into())]
    );
    rejects(
        &s.replace("delta moved", "edit duplicate=saved;delta moved"),
        &["E_MOVED"],
    );
    check(&s.replace("found {borrow mutable owner}", "empty {}"));
}
#[test]
fn stored_adapters_transfer_borrowed_payloads() {
    let s = r#"outcome lookup {found {ref string text},empty}
 control main() exits done {mutable vector<string> words=["hello"];
 lookup saved=delta vector_get(borrow words,0) {found {auto word}=>found {word},out_of_bounds=>empty {}};
 auto moved=saved;
 delta moved {found {auto text}=>ready,empty=>missing}
 [ready]:emit(text);exit done;[missing]:exit done;}"#;
    check(s);
    rejects(
        &s.replace("auto moved=saved;", "drop(words);auto moved=saved;"),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("auto moved=saved;", "drop(saved);auto moved=saved;"),
        &["E_RESOURCE", "E_OUTCOME"],
    );
}
#[test]
fn adapters_reject_references_into_completed_arm_storage() {
    let s = r#"control make() exits ready {string text} {exit ready {"local"};}
 control main() exits done {auto saved=delta make() {ready {auto text}=>found {borrow text}};
 exit delta saved {found {auto text}=>done {}};}"#;
    rejects(s, &["E_LIFETIME"]);
    let s = r#"control main() exits done {mutable string owner="outer";
 auto saved=delta true {true=>found {borrow owner},false=>empty {}};
 exit delta saved {found {auto text}=>done {},empty=>done {}};}"#;
    check(s);
}
#[test]
fn record_payloads_preserve_exclusive_splits_and_guards() {
    let s = r#"struct pair['a,'b] {ref mutable string left <- 'a;ref mutable string right <- 'b;}
 variant saved {ready {pair value},empty}
 control main() exits done... {mutable string a="a";mutable string b="b";
 pair refs=pair {borrow mutable a,borrow mutable b};saved pending=ready {refs};
 delta pending {ready {auto pair}=>ready,empty=>empty}
 [ready]:deref pair.left="A";deref pair.right="B";exit done {a,b};
 [empty]:exit done {a,b};}"#;
    check(s);
    rejects(
        &s.replace("delta pending", "a=\"bad\";delta pending"),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("delta pending", "emit(refs.left);delta pending"),
        &["E_MOVED"],
    );
}
#[test]
fn exclusive_capture_forwards_and_shared_empty_payloads_have_no_loan() {
    let s = r#"control peek(ref mutable string input,bool yes) exits found {ref mutable string text from input},empty {exit delta yes {true=>found {input},false=>empty {}};}
 control main() exits done... {mutable string owner="hello";auto saved=capture peek(borrow mutable owner,true);
 auto mapped=delta saved {found {auto text}=>ready {text},empty=>empty {}};
 delta mapped {ready {auto text}=>ready,empty=>missing}
 [ready]:deref text="changed";exit done {owner};[missing]:exit done {owner};}"#;
    check(s);
    rejects(
        &s.replace("auto mapped=", "owner=\"bad\";auto mapped="),
        &["E_LOAN"],
    );
    check(&s.replace("borrow mutable owner,true", "borrow mutable owner,false"));
    let s = r#"variant maybe {found {ref string text},empty}
 control main() exits done {mutable string owner="unrelated";maybe saved=empty {};owner="free";
 delta saved {found {auto text}=>found,empty=>empty}[found]:emit(text);exit done;[empty]:exit done;}"#;
    check(s);
}
#[test]
fn optional_references_are_local_tagged_storage() {
    let s = r#"control main() exits done {mutable string owner="hello";option<ref string> saved=some {borrow owner};
 delta saved {some {auto text}=>found,none=>empty}[found]:emit(text);exit done;[empty]:exit done;}"#;
    check(s);
    rejects(
        &s.replace("delta saved", "owner=\"bad\";delta saved"),
        &["E_LOAN"],
    );
}
#[test]
fn generic_local_variants_and_scope_release_preserve_origins() {
    let s = r#"variant maybe<t> {found {ref t value},empty}
 control use<t>(ref t input) exits done {maybe<t> saved=found {input};delta saved {found {auto child}=>found,empty=>empty}[found]:drop(child);exit done;[empty]:exit done;}
 control main() exits done {string text="hello";use<string>(borrow text);exit done;}"#;
    check(s);
    let cloned = r#"variant view {found {ref string text},empty}
 control main() exits done {mutable string value="hello";view original=found {borrow value};view copied=clone(borrow original);drop(original);
 delta copied {found {auto text}=>ready,empty=>empty}[ready]:emit(text);exit done;[empty]:exit done;}"#;
    check(cloned);
    rejects(
        &cloned.replace("delta copied", "value=\"bad\";delta copied"),
        &["E_LOAN"],
    );
}
#[test]
fn borrowed_sum_boundaries_and_collection_elements_remain_gated() {
    for s in [
        "variant view {found {ref string text},empty} control invalid(view value) exits done {exit done;}",
        "variant view {found {ref string text},empty} control invalid(ref string input) exits ready {view value} {exit ready {found {input}};}",
        "control main() exits done {string x=\"x\";vector<ref string> values=[borrow x];exit done;}",
    ] {
        rejects(s, &["E_ORIGIN", "E_MANIFEST", "E_LIFETIME"]);
    }
    let s = r#"variant view {found {ref string text},empty}
 control main() exits auto {string x="x";view inner=found {borrow x};auto outer=delta true {true=>wrapped {inner},false=>empty {}};exit outer;}"#;
    rejects(s, &["E_ORIGIN"]);
}
#[test]
fn native_borrowed_outcomes_match_interpreter_and_expansion() {
    let p = check(include_str!("fixtures/borrowed_outcomes.tellur"));
    let expected = p.run(&scenario()).unwrap();
    assert_eq!(expected.fields, vec![Value::U32(2)]);
    let root = std::env::temp_dir().join(format!("path-borrowed-sums-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let binary = root.join(format!("sums-{expanded}-{opt}"));
            p.build_native("main", &binary, opt).unwrap();
            let output = std::process::Command::new(binary)
                .arg("--json")
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
    std::fs::remove_dir_all(root).unwrap();
    check(
        &include_str!("fixtures/borrowed_outcomes.tellur").replace("[\"A borrowed word\"]", "[]"),
    );
}
#[test]
fn captured_carrier_reborrows_retain_authorizing_storage() {
    let s = r#"struct editor['a] {ref mutable string value <- 'a;}
 control peek(ref mutable editor input) exits found {ref mutable string value from input['a]} {exit found {borrow mutable deref input.value};}
 control main() exits done {mutable string owner="x";mutable editor parent=editor {borrow mutable owner};auto saved=capture peek(borrow mutable parent);
 delta saved {found {auto child}=>found}[found]:deref child="child";deref parent.value="parent";exit done;}"#;
    check(s);
    rejects(
        &s.replace("delta saved", "drop(parent);delta saved"),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("deref child=", "deref parent.value=\"bad\";deref child="),
        &["E_LOAN"],
    );
    let duplicate=s.replace("delta saved {found {auto child}=>found}[found]:deref child=\"child\";deref parent.value=\"parent\";exit done;", "auto duplicate=delta saved {found {auto child}=>both {child,child}};exit duplicate;");
    rejects(&duplicate, &["E_LOAN", "E_MOVED"]);
}
#[test]
fn inferred_optional_references_and_captured_forwarding_preserve_origins() {
    let s = r#"control wrap(ref string input) exits found {ref string value from input} {auto pending=some {input};delta pending {some {auto child}=>some,none=>none}[some]:exit found {child};[none]:exit found {input};}
 control relay(ref string input) exits found {ref string value from input} {auto saved=capture wrap(input);exit saved;}
 control main() exits done {string value="x";auto saved=capture relay(borrow value);exit delta saved {found {auto child}=>done {}};}"#;
    check(s);
    let s = r#"control main() exits done {string x="x";auto saved=some {borrow x};exit delta saved {some {auto value}=>done {},none=>done {}};}"#;
    check(s);
}
#[test]
fn unpacking_releases_loans_for_other_constructors() {
    let s = r#"control main() exits done {mutable string a="a";mutable string b="b";
 auto saved=delta true {true=>left {borrow a},false=>right {borrow b}};
 delta saved {left {auto left}=>left,right {auto right}=>right}
 [left]:b="free";emit(left);exit done;
 [right]:a="free";emit(right);exit done;}"#;
    check(s);
    check(&s.replace("delta true", "delta false"));
    rejects(
        &s.replace("b=\"free\";emit(left)", "a=\"bad\";emit(left)"),
        &["E_LOAN"],
    );
}
#[test]
fn loop_local_borrowed_outcomes_are_recreated_each_iteration() {
    let s = r#"control peek(ref string input) exits found {ref string value from input} {exit found {input};}
 control main() exits done {string text="hello";
 loop count(bool repeat=true) {
 auto pending=capture peek(borrow text);delta pending {found {auto value}=>found}
 [found]:emit(value);delta repeat {true=>again,false=>finished}
 [again]:again count(false);[finished]:exit done;
 }}"#;
    check(s);
}

#[test]
fn nested_outcomes_and_records_preserve_independent_exclusive_loans() {
    let s = include_str!("fixtures/nested_borrows.tellur");
    let p = check(s);
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![
            Value::String("published title".into()),
            Value::String("revised body".into())
        ]
    );
    rejects(
        &s.replace("delta pending", "title = \"too soon\"; delta pending"),
        &["E_LOAN"],
    );
    rejects(
        &s.replace(
            "delta message.selected",
            "body = \"too soon\"; delta message.selected",
        ),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("title = \"published title\"", "body = \"too soon\""),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("delta pending", "drop(pending); delta pending"),
        &["E_RESOURCE", "E_OUTCOME"],
    );
    rejects(
        &s.replace("delta pending", "auto duplicate = pending; delta pending"),
        &["E_MOVED"],
    );
}

#[test]
fn deeply_nested_optional_references_keep_each_tag_layer() {
    let s = r#"control main() exits done {
        mutable string owner="hello";
        option<option<option<ref string>>> saved=some {some {some {borrow owner}}};
        delta saved {some {auto middle}=>middle,none=>absent}
        [middle]:delta middle {some {auto inner}=>inner,none=>empty}
        [inner]:delta inner {some {auto text}=>text,none=>none}
        [text]:emit(text);owner="free";exit done;
        [absent | empty | none]:owner="free";exit done;
    }"#;
    check(s);
    rejects(
        &s.replace("delta inner", "owner=\"bad\";delta inner"),
        &["E_LOAN"],
    );
    for constructor in ["none {}", "some {none {}}", "some {some {none {}}}"] {
        check(&s.replace("some {some {some {borrow owner}}}", constructor));
    }
}

#[test]
fn shared_nested_values_copy_clone_and_release_only_the_discarded_retainer() {
    let s = r#"variant view {found {ref string text},empty}
    struct box {view inner;}
    variant wrapped {boxed {box value},empty}
    control main() exits done {
        mutable string owner="hello";
        wrapped first=boxed {box {found {borrow owner}}};
        wrapped second=first;wrapped third=clone(borrow second);drop(first);drop(second);
        delta third {boxed {auto box}=>boxed,empty=>empty}
        [boxed]:delta box.inner {found {auto text}=>found,empty=>none}
        [found]:emit(text);owner="free";exit done;
        [empty | none]:owner="free";exit done;
    }"#;
    check(s);
    rejects(
        &s.replace("delta third", "owner=\"bad\";delta third"),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("delta box.inner", "owner=\"bad\";delta box.inner"),
        &["E_LOAN"],
    );
}

#[test]
fn sibling_nested_sums_with_identical_tags_keep_separate_field_paths() {
    let s = r#"variant edit {found {ref mutable string text},empty}
    struct pair {edit left;edit right;}
    variant wrapped {ready {pair value},empty}
    control main() exits done... {
        mutable string a="a";mutable string b="b";
        wrapped saved=ready {pair {found {borrow mutable a},found {borrow mutable b}}};
        delta saved {ready {auto pair}=>ready,empty=>empty}
        [ready]:drop(pair.left);a="free";delta pair.right {found {auto text}=>found,empty=>none}
        [found]:deref text="B";exit done {a,b};
        [empty | none]:exit done {a,b};
    }"#;
    let p = check(s);
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("free".into()), Value::String("B".into())]
    );
    rejects(&s.replace("a=\"free\"", "b=\"bad\""), &["E_LOAN"]);
    rejects(
        &s.replace("borrow mutable b", "borrow mutable a"),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("drop(pair.left);", "auto moved=pair.left;drop(pair.left);"),
        &["E_MOVED"],
    );
}

#[test]
fn nested_adapter_branches_release_other_constructor_origins() {
    let s = r#"variant view {found {ref string text},empty}
    control main() exits done {
        mutable string a="a";mutable string b="b";
        auto pending=delta true {true=>left {view.found {borrow a}},false=>right {view.found {borrow b}}};
        delta pending {left {auto l}=>left,right {auto r}=>right}
        [left]:b="free";delta l {found {auto text}=>found,empty=>empty}
        [found]:emit(text);exit done;[empty]:exit done;
        [right]:a="free";delta r {found {auto other}=>other,empty=>none}
        [other]:emit(other);exit done;[none]:exit done;
    }"#;
    check(s);
    check(&s.replace("delta true", "delta false"));
    rejects(&s.replace("b=\"free\"", "a=\"bad\""), &["E_LOAN"]);
}

#[test]
fn nested_adapters_cannot_retain_completed_arm_storage() {
    let s = r#"variant view {found {ref string text},empty}
    struct box {view value;}
    control text() exits ready {string text} {exit ready {"temporary"};}
    control main() exits done {
        auto pending=delta text() {ready {auto text}=>wrapped {box {found {borrow text}}}};
        exit delta pending {wrapped {auto box}=>done {}};
    }"#;
    rejects(s, &["E_LIFETIME"]);
}

#[test]
fn generic_nested_storage_preserves_capabilities_and_inferred_types() {
    let s = r#"variant view<t> {found {ref t value},empty}
    struct box<t> {option<view<t>> value;}
    control use<t>(ref t input) exits done {
        box<t> first=box<t> {some {view<t>.found {input}}};
        auto second=first;drop(first);
        delta second.value {some {auto inner}=>some,none=>none}
        [some]:delta inner {found {auto value}=>found,empty=>empty}
        [found]:drop(value);exit done;[none | empty]:exit done;
    }
    control main() exits done {string owner="hello";use<string>(borrow owner);exit done;}"#;
    check(s);
}

#[test]
fn nested_borrowed_outcomes_retain_mandatory_handling() {
    let s = r#"outcome value {ready {ref string text}}
    variant wrapped {some {value item},none}
    control main() exits done {string x="x";wrapped pending=some {value.ready {borrow x}};
        delta pending {some {auto item}=>some,none=>none}
        [some]:exit delta item {ready {auto text}=>done {}};
        [none]:exit done;
    }"#;
    check(s);
    rejects(
        &s.replace("delta pending", "drop(pending);delta pending"),
        &["E_OUTCOME", "E_RESOURCE"],
    );
    rejects(
        &s.replace(
            "exit delta item {ready {auto text}=>done {}};",
            "exit done;",
        ),
        &["E_OUTCOME"],
    );
}

#[test]
fn nested_adapter_carrier_guards_survive_all_unwrapping_layers() {
    let s = r#"struct editor['a] {ref mutable string value <- 'a;}
    variant edit {found {ref mutable string text},empty}
    control peek(ref mutable editor input) exits found {ref mutable string value from input['a]} {exit found {borrow mutable deref input.value};}
    control main() exits done {
        mutable string owner="before";mutable editor parent=editor {borrow mutable owner};
        auto boxed=delta peek(borrow mutable parent) {found {auto child}=>wrapped {edit.found {child}}};
        delta boxed {wrapped {auto inner}=>wrapped}
        [wrapped]:delta inner {found {auto child}=>found,empty=>empty}
        [found]:deref child="child";deref parent.value="parent";exit done;
        [empty]:exit done;
    }"#;
    check(s);
    rejects(
        &s.replace("delta boxed", "drop(parent);delta boxed"),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("delta inner", "drop(parent);delta inner"),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("deref child=", "deref parent.value=\"bad\";deref child="),
        &["E_LOAN"],
    );
}

#[test]
fn loop_local_nested_borrows_are_recreated_without_escaping_backedges() {
    let s = r#"variant view {found {ref string text},empty}
    control main() exits done {string owner="hello";
        loop repeat(bool again=true) {
            option<view> pending=some {view.found {borrow owner}};
            delta pending {some {auto inner}=>some,none=>none}
            [some]:delta inner {found {auto text}=>found,empty=>empty}
            [found]:emit(text);
            [found | empty | none]:delta again {true=>repeat,false=>finished}
            [repeat]:again repeat(false);[finished]:exit done;
        }
    }"#;
    check(s);
}

#[test]
fn nesting_does_not_enable_public_borrow_boundaries_or_mutable_storage_rebinding() {
    for s in [
        "variant view {found {ref string text},empty} struct box {view inner;} control invalid(box value) exits done {exit done;}",
        "variant view {found {ref string text},empty} struct box['a] {ref string direct <- 'a; view inner;} control invalid(ref string input) exits ready {box[input] value} {exit ready {box {input,found {input}}};}",
        "variant view {found {ref string text},empty} control invalid(ref string input) exits ready {option<view> value} {exit ready {some {found {input}}};}",
        "variant view {found {ref string text},empty} control invalid() exits done {string x=\"x\";mutable option<view> saved=some{found{borrow x}};saved=none{};exit done;}",
        "variant view {found {ref string text},empty} control invalid() exits done {string x=\"x\";vector<option<view>> values=[some{found{borrow x}}];exit done;}",
        "variant view {found {ref string text},empty} control invalid() exits done {string x=\"x\";option<view> saved=some{found{borrow x}};loop repeated(option<view> value=saved) {exit done;}}",
    ] {
        rejects(
            s,
            &["E_ORIGIN", "E_MANIFEST", "E_UNSUPPORTED", "E_LIFETIME"],
        );
    }
}

#[test]
fn native_nested_borrows_match_interpreter_and_expansion() {
    let source = include_str!("fixtures/nested_borrows.tellur");
    let programs = [source.to_string(),source.replace("found { editors {\n        title: borrow mutable title,\n        body: borrow mutable body\n    } }", "empty {}")];
    assert_ne!(programs[0], programs[1]);
    let root = std::env::temp_dir().join(format!("path-nested-borrows-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for (case, source) in programs.iter().enumerate() {
        let p = check(source);
        let expected = p.run(&scenario()).unwrap();
        for expanded in [false, true] {
            let p = if expanded {
                p.expanded("main").unwrap()
            } else {
                p.clone()
            };
            for opt in [0, 2] {
                let binary = root.join(format!("nested-{case}-{expanded}-{opt}"));
                p.build_native("main", &binary, opt).unwrap();
                let output = std::process::Command::new(binary)
                    .arg("--json")
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
    }
    std::fs::remove_dir_all(root).unwrap();
}
