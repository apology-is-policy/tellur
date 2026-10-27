use tellur::{compile, ir::Value, manifest::Manifest, runtime::Scenario};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn run(source: &str, args: Vec<Value>) -> tellur::runtime::Execution {
    let p = compile(source, manifest()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: args,
        calls: vec![],
    };
    let result = p.run(&s).unwrap();
    assert_eq!(result, p.expanded("main").unwrap().run(&s).unwrap());
    result
}
fn rejects(source: &str, code: &str) {
    let e = compile(source, manifest()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}
#[test]
fn exclusive_control_parameter_updates_caller_storage() {
    let source = r#"control update(&mutable string text) exits done { [entry]: *text="changed"; exit done; }
    control main() exits auto { [entry]: mutable string s="before"; delta update(&mutable s) {done=>ready} [ready]: exit done {s}; }"#;
    assert_eq!(
        run(source, vec![]).fields,
        vec![Value::String("changed".into())]
    );
    rejects(
        &source.replace("mutable string s", "string s"),
        "E_MUTABILITY",
    );
}
#[test]
fn shared_reference_return_keeps_owner_borrowed() {
    let source = r#"control identity(&string text) exits view {&string result} { [entry]: exit view {text}; }
    control main() exits auto { [entry]: string s="hello"; delta identity(&s) {view {&string result}=>ready}
        [ready]: string copy=clone(result); drop(result); exit done {s,copy}; }"#;
    assert_eq!(
        run(source, vec![]).fields,
        vec![Value::String("hello".into()); 2]
    );
    rejects(
        &source.replace(
            "string copy=clone(result);",
            "auto moved=s; string copy=clone(result);",
        ),
        "E_LOAN",
    );
}
#[test]
fn exclusive_return_is_a_reborrow_and_parent_resumes_after_release() {
    let source = r#"control identity(&mutable string text) exits auto { [entry]: exit view {text}; }
    control main() exits auto { [entry]: mutable string s="hello"; auto parent=&mutable s;
        delta identity(parent) {view {auto child}=>ready}
        [ready]: *child="through child"; drop(child); *parent="through parent"; drop(parent); exit done {s}; }"#;
    assert_eq!(
        run(source, vec![]).fields,
        vec![Value::String("through parent".into())]
    );
    rejects(
        &source.replace("drop(child);", "*parent=\"conflict\"; emit(child);"),
        "E_LOAN",
    );
}
#[test]
fn references_cannot_escape_local_or_owned_parameter_storage() {
    rejects(
        r#"control main() exits auto { [entry]: string s="local"; exit done {&s}; }"#,
        "E_LIFETIME",
    );
    rejects(
        "control main(string s) exits auto { [entry]: exit done {&s}; }",
        "E_LIFETIME",
    );
}
#[test]
fn exclusive_borrow_conflicts_with_copy_reads_in_other_arguments() {
    rejects(
        "control mixed(&mutable u32 first,u32 second) exits done { [entry]: exit done; } control main() exits done { [entry]: mutable u32 n=1; delta mixed(&mutable n,n) {done=>ready} [ready]: exit done; }",
        "E_ALIAS",
    );
}
#[test]
fn reborrows_freeze_the_parent_and_release_on_scope_exit() {
    let source = r#"control main() exits auto { [entry]: mutable string s="hello"; auto parent=&mutable s;
        [entry] over &: { [entry]: auto child=&*parent; emit(child); }
        [entry]: *parent="after"; drop(parent); exit done {s}; }"#;
    assert_eq!(
        run(source, vec![]).fields,
        vec![Value::String("after".into())]
    );
    rejects(
        &source.replace("emit(child);", "*parent=\"conflict\"; emit(child);"),
        "E_LOAN",
    );
}
#[test]
fn conditional_reference_return_borrows_both_possible_owners() {
    let source = r#"control choose(bool flag,&string left,&string right) exits auto {
        [entry]: delta flag {true=>yes,false=>no}
        [yes]: exit chosen {left}; [no]: exit chosen {right};
    }
    control main(bool flag) exits auto { [entry]: string a="a"; string b="b";
        delta choose(flag,&a,&b) {chosen {auto view}=>ready}
        [ready]: string chosen=clone(view); drop(view); exit done {a,b,chosen}; }"#;
    assert_eq!(
        run(source, vec![Value::Bool(true)]).fields[2],
        Value::String("a".into())
    );
    assert_eq!(
        run(source, vec![Value::Bool(false)]).fields[2],
        Value::String("b".into())
    );
    rejects(
        &source
            .replace(
                "string chosen=clone(view);",
                "auto moved=a; string chosen=clone(view);",
            )
            .replace("emit(view);", "auto moved=s; emit(view);"),
        "E_LOAN",
    );
}
#[test]
fn field_borrows_flow_through_multiple_controls_and_remain_disjoint() {
    let mut m = manifest();
    m.types.push(tellur::manifest::TypeDef::Record {
        name: "text_pair".into(),
        fields: ["left", "right"]
            .map(|name| tellur::manifest::Field {
                name: name.into(),
                ty: "String".into(),
            })
            .to_vec(),
    });
    let source = r#"control split(&mutable text_pair pair) exits auto { [entry]: exit parts {&mutable (*pair).left,&mutable (*pair).right}; }
    control forward(&mutable text_pair pair) exits auto {
        [entry]: delta split(pair) {parts {auto left,auto right}=>ready}
        [ready]: exit parts {left,right};
    }
    control main() exits auto { [entry]: mutable text_pair pair=text_pair {"before left","before right"};
        delta forward(&mutable pair) {parts {auto left,auto right}=>ready}
        [ready]: *left="after left"; *right="after right"; drop(left); drop(right); exit done {pair}; }"#;
    let p = compile(source, m).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    let result = p.run(&s).unwrap();
    assert_eq!(result, p.expanded("main").unwrap().run(&s).unwrap());
    assert_eq!(
        result.fields,
        vec![Value::Construct {
            ty: "text_pair".into(),
            name: "text_pair".into(),
            fields: vec![
                Value::String("after left".into()),
                Value::String("after right".into())
            ]
        }]
    );
}
#[test]
fn returned_borrow_does_not_freeze_owner_on_an_outcome_without_it() {
    let source = r#"control maybe(bool flag,&string text) exits auto {
        [entry]: delta flag {true=>yes,false=>no}
        [yes]: exit found {text}; [no]: exit missing;
    }
    control main(bool flag) exits auto { [entry]: string s="value"; delta maybe(flag,&s) {found {auto view}=>yes,missing=>no}
        [yes]: emit(view); drop(view); [entry]: exit done {s}; }"#;
    for flag in [true, false] {
        assert_eq!(
            run(source, vec![Value::Bool(flag)]).fields,
            vec![Value::String("value".into())]
        );
    }
    rejects(
        &source
            .replace(
                "string chosen=clone(view);",
                "auto moved=a; string chosen=clone(view);",
            )
            .replace("emit(view);", "auto moved=s; emit(view);"),
        "E_LOAN",
    );
}
#[test]
fn duplicate_exclusive_returns_and_resource_overwrite_are_rejected() {
    rejects(
        "control bad(&mutable string text) exits auto { [entry]: exit done {text,text}; }",
        "E_LOAN",
    );
    rejects(
        "control bad(&mutable handle file,handle replacement) exits done { [entry]: *file=replacement; exit done; }",
        "E_RESOURCE",
    );
}
#[test]
fn native_borrow_mutation_and_return_match_interpreter_at_o0_o2() {
    let source = r#"control edit(&mutable string text) exits auto { [entry]: *text="changed"; exit view {text}; }
    control forward(&mutable string text) exits auto {
        [entry]: delta edit(text) {view {auto ref}=>ready} [ready]: exit view {ref};
    }
    control choose(bool flag,&string a,&string b) exits auto {
        [entry]: delta flag {true=>yes,false=>no} [yes]: exit view {a}; [no]: exit view {b};
    }
    control main(bool flag) exits auto { [entry]: mutable string owner="original";
        delta forward(&mutable owner) {view {auto ref}=>ready}
        [ready]: *ref="after"; drop(ref); string other="other";
        loop repeat(bool first=true) {
            [entry]: delta choose(flag,&owner,&other) {view {auto view}=>selected}
            [selected]: emit(view); drop(view); delta first {true=>again_path,false=>end}
            [again_path]: again repeat(false);
            [end]: exit done {clone(&owner),clone(&other)};
        }
    }"#;
    let p = compile(source, manifest()).unwrap();
    let root = std::env::temp_dir().join(format!("path-reference-native-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for expanded in [false, true] {
        let program = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let binary = root.join(format!("refs-{expanded}-{opt}"));
            program.build_native("main", &binary, opt).unwrap();
            for flag in [true, false] {
                let expected = p
                    .run(&Scenario {
                        entry: "main".into(),
                        arguments: vec![Value::Bool(flag)],
                        calls: vec![],
                    })
                    .unwrap();
                let actual = std::process::Command::new(&binary)
                    .args(["--json", "--", if flag { "true" } else { "false" }])
                    .output()
                    .unwrap();
                assert!(
                    actual.status.success(),
                    "{}",
                    String::from_utf8_lossy(&actual.stderr)
                );
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                    serde_json::to_value(expected).unwrap()
                );
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn local_lifetimes_end_at_last_use_without_explicit_drop() {
    let source = r#"control edit(&mutable string text) exits auto { [entry]: exit view {text}; }
    control main() exits auto { [entry]: mutable string owner="before";
        auto parent=&mutable owner;
        auto child=&*parent;
        string copy=clone(child);
        *parent="after";
        delta edit(parent) {view {auto returned}=>ready}
        [ready]: emit(returned); owner="reinitialized"; exit done {owner,copy}; }"#;
    assert_eq!(
        run(source, vec![]).fields,
        vec![
            Value::String("reinitialized".into()),
            Value::String("before".into())
        ]
    );
    rejects(
        &source.replace(
            "owner=\"reinitialized\";",
            "owner=\"reinitialized\"; emit(returned);",
        ),
        "E_LOAN",
    );
}
#[test]
fn future_use_in_nested_scope_keeps_a_reference_live() {
    rejects(
        r#"control main() exits done { [entry]: mutable string s="before"; auto ref=&s; s="after";
        [entry] over &: { [entry]: emit(ref); } [entry]: exit done; }"#,
        "E_LOAN",
    );
}
#[test]
fn copied_reborrow_keeps_the_unique_parent_frozen() {
    rejects(
        r#"control main() exits done { [entry]: mutable string s="before"; auto parent=&mutable s;
        auto child=&*parent; auto copy=child; *parent="after"; emit(copy); exit done; }"#,
        "E_LOAN",
    );
}
#[test]
fn loop_backedges_do_not_expire_outer_loans() {
    rejects(
        r#"control main() exits done { [entry]: mutable string s="before"; auto ref=&s;
        loop repeat() { [entry]: emit(ref); s="after"; again repeat(); } }"#,
        "E_LOAN",
    );
    let source = r#"control main() exits auto { [entry]: string start="value";
        loop repeat(string owner=start,bool first=true) {
            [entry]: auto ref=&owner; emit(ref); delta first {true=>yes,false=>no}
            [yes]: again repeat(owner,false); [no]: exit done {owner};
        } }"#;
    assert_eq!(
        run(source, vec![]).fields,
        vec![Value::String("value".into())]
    );
}
#[test]
fn argument_evaluation_respects_temporary_reads_and_live_exclusive_loans() {
    let source = r#"control mixed(string snapshot,&mutable string owner) exits auto { [entry]: *owner="after"; exit done {snapshot}; }
        control main() exits auto { [entry]: mutable string s="before"; delta mixed(clone(&s),&mutable s) {done {string snapshot}=>ready} [ready]: exit done {s,snapshot}; }"#;
    assert_eq!(
        run(source, vec![]).fields,
        vec![
            Value::String("after".into()),
            Value::String("before".into())
        ]
    );
    rejects(
        &source
            .replace(
                "string snapshot,&mutable string owner",
                "&mutable string owner,string snapshot",
            )
            .replace("mixed(clone(&s),&mutable s)", "mixed(&mutable s,clone(&s))"),
        "E_ALIAS",
    );
}
#[test]
fn borrow_contracts_do_not_allow_forged_entry_references() {
    let p = compile(
        "control entry(&string text) exits auto { [entry]: exit done {text}; }",
        manifest(),
    )
    .unwrap();
    let scenario = Scenario {
        entry: "entry".into(),
        arguments: vec![Value::String("not a reference".into())],
        calls: vec![],
    };
    assert!(
        p.run(&scenario)
            .unwrap_err()
            .message
            .contains("borrowed arguments")
    );
    assert!(
        serde_json::from_str::<Value>(
            r#"{"kind":"reference","value":{"ty":"&String","place":{"root":0,"fields":[]}}}"#
        )
        .is_err()
    );
}
#[test]
fn rust_scenario_api_cannot_disguise_a_reference_as_an_owned_value() {
    let p = compile(
        "control main(string value) exits auto { [entry]: exit done {value}; }",
        manifest(),
    )
    .unwrap();
    let forged = Value::Reference {
        ty: "String".into(),
        place: tellur::ir::Place {
            root: 0,
            fields: vec![],
            indirect: false,
        },
    };
    assert_eq!(
        p.run(&Scenario {
            entry: "main".into(),
            arguments: vec![forged],
            calls: vec![]
        })
        .unwrap_err()
        .code,
        "E_DRIVER"
    );
}
