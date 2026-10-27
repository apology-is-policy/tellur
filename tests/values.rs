use tellur::{compile, ir::Value, manifest::Manifest, runtime::Scenario};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn result(source: &str, args: Vec<Value>) -> tellur::runtime::Execution {
    let p = compile(source, manifest()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: args,
        calls: vec![],
    };
    let actual = p.run(&s).unwrap();
    assert_eq!(actual, p.expanded("main").unwrap().run(&s).unwrap());
    actual
}
fn rejects(source: &str, code: &str) {
    let e = compile(source, manifest()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}
#[test]
fn owning_strings_move_but_scalars_copy() {
    rejects(
        r#"control main() exits done { [entry]: string a="hello"; auto b=a; emit(a); exit done; }"#,
        "E_MOVED",
    );
    assert_eq!(
        result(
            "control main() exits auto { [entry]: u32 a=42; auto b=a; exit done {a,b}; }",
            vec![]
        )
        .fields,
        vec![Value::U32(42); 2]
    );
}
#[test]
fn clone_is_explicit_and_shared_borrow_preserves_owner() {
    let r = result(
        r#"control main() exits auto { [entry]: string a="hello"; string b=clone(&a); emit(&a); exit done {a,b}; }"#,
        vec![],
    );
    assert_eq!(r.fields, vec![Value::String("hello".into()); 2]);
    rejects(
        "control main(handle file) exits auto { [entry]: auto copy=clone(&file); exit done {file,copy}; }",
        "E_CLONE",
    );
    rejects(
        r#"control main() exits done { [entry]: string a="x"; auto b=clone(a); exit done; }"#,
        "E_BORROW",
    );
}
#[test]
fn mutable_bindings_reinitialize_owned_values() {
    let r = result(
        r#"control main() exits auto { [entry]: mutable string a="old"; string taken=a; a="new"; exit done {taken,a}; }"#,
        vec![],
    );
    assert_eq!(
        r.fields,
        vec![Value::String("old".into()), Value::String("new".into())]
    );
    rejects(
        "control main() exits done { [entry]: u32 a=1; a=2; exit done; }",
        "E_MUTABILITY",
    );
    rejects(
        "control main() exits done { [entry]: mutable u32 a=1; a=true; exit done; }",
        "E_TYPE",
    );
    rejects(
        "control main(handle first,handle second) exits auto { [entry]: mutable handle file=first; file=second; exit done {file}; }",
        "E_RESOURCE",
    );
}
#[test]
fn mutations_and_moves_are_path_sensitive() {
    let source = r#"control main(bool flag) exits auto {
        [entry]: mutable string value="before"; delta flag {true=>yes,false=>no}
        [yes]: string taken=value; value="after";
        [entry]: exit done {value};
    }"#;
    assert_eq!(
        result(source, vec![Value::Bool(true)]).fields,
        vec![Value::String("after".into())]
    );
    assert_eq!(
        result(source, vec![Value::Bool(false)]).fields,
        vec![Value::String("before".into())]
    );
    rejects(&source.replace("value=\"after\";", ""), "E_MOVED");
}
#[test]
fn shared_control_parameters_cannot_be_consumed_and_calls_require_ampersands() {
    let source = r#"control main() exits auto { [entry]: string owner="hello"; delta duplicate(&owner) {copied {string copy}=>ready} [ready]: exit done {owner,copy}; }
        control duplicate(&string text) exits auto { [entry]: exit copied {clone(text)}; }"#;
    assert_eq!(
        result(source, vec![]).fields,
        vec![Value::String("hello".into()); 2]
    );
    rejects(
        &source.replace("duplicate(&owner)", "duplicate(owner)"),
        "E_BORROW",
    );
    rejects(&source.replace("clone(text)", "*text"), "E_BORROW_MOVE");
}
#[test]
fn overlapping_borrow_and_move_in_one_call_is_rejected() {
    rejects(
        r#"control main() exits auto { [entry]: string s="x"; delta mixed(&s,s) {done=>finished} [finished]: exit done; }
    control mixed(&string a,string b) exits done { [entry]: emit(a); emit(b); exit done; }"#,
        "E_ALIAS",
    );
}
#[test]
fn ordinary_drop_is_allowed_but_protocol_obligations_cannot_be_dropped() {
    result(
        r#"control main() exits done { [entry]: string a="x"; drop(a); exit done; }"#,
        vec![],
    );
    rejects(
        r#"control main() exits done { [entry]: string a="x"; drop(a); emit(a); exit done; }"#,
        "E_MOVED",
    );
    rejects(
        "control main(handle file) exits done { [entry]: drop(file); exit done; }",
        "E_RESOURCE",
    );
}
#[test]
fn noncopy_records_move_by_field_and_allow_reinitialization() {
    let mut m = manifest();
    m.types.push(tellur::manifest::TypeDef::Record {
        name: "pair".into(),
        fields: vec![
            tellur::manifest::Field {
                name: "left".into(),
                ty: "String".into(),
            },
            tellur::manifest::Field {
                name: "right".into(),
                ty: "String".into(),
            },
        ],
    });
    let source = r#"control main() exits auto { [entry]: mutable pair p=pair {"a","b"}; string first=p.left; p.left="c"; exit done {first,p}; }"#;
    let p = compile(source, m.clone()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(
        p.run(&s).unwrap(),
        p.expanded("main").unwrap().run(&s).unwrap()
    );
    assert_eq!(
        compile(&source.replace("p.left=\"c\";", ""), m)
            .unwrap_err()
            .code,
        "E_PARTIAL_MOVE"
    );
}
#[test]
fn legacy_region_bindings_retain_implicit_copy_compatibility() {
    result(
        r#"region main() exits done { [entry]: string a="hello"; auto b=a; emit(a); emit(b); exit done; }"#,
        vec![],
    );
}

#[test]
fn shared_local_references_copy_and_hold_their_owner() {
    let source = r#"control main() exits auto { [entry]:
        string owner="hello";
        &string first=&owner;
        auto second=first;
        emit(&*first);
        drop(first);
        string copy=clone(&*second);
        drop(second);
        exit done {owner,copy};
    }"#;
    assert_eq!(
        result(source, vec![]).fields,
        vec![Value::String("hello".into()); 2]
    );
    rejects(
        &source.replace(
            "string copy=clone(&*second);",
            "auto moved=owner; string copy=clone(&*second);",
        ),
        "E_LOAN",
    );
    rejects(
        &source.replace("string copy=clone(&*second);", "string copy=*second;"),
        "E_BORROW_MOVE",
    );
    rejects(
        &source.replace("drop(second);", "drop(second); emit(&*second);"),
        "E_MOVED",
    );
}
#[test]
fn exclusive_local_references_move_and_update_the_owner() {
    let source = r#"control main() exits auto { [entry]:
        mutable string owner="before";
        &mutable string first=&mutable owner;
        auto second=first;
        *second="after";
        string snapshot=clone(&*second);
        drop(second);
        exit done {owner,snapshot};
    }"#;
    assert_eq!(
        result(source, vec![]).fields,
        vec![Value::String("after".into()); 2]
    );
    rejects(&source.replace("*second=", "*first="), "E_MOVED");
    rejects(
        &source.replace(
            "auto second=first;",
            "auto second=&mutable owner; emit(first);",
        ),
        "E_LOAN",
    );
    rejects(
        &source.replace("mutable string owner", "string owner"),
        "E_MUTABILITY",
    );
    rejects(
        &source.replace("auto second=first;", "emit(&owner); auto second=first;"),
        "E_LOAN",
    );
}
#[test]
fn shared_reference_excludes_mutation_and_exclusive_borrow() {
    rejects(
        r#"control main() exits done { [entry]: mutable string s="x"; auto r=&s; s="y"; emit(r); exit done; }"#,
        "E_LOAN",
    );
    rejects(
        r#"control main() exits done { [entry]: mutable string s="x"; auto r=&s; *r="y"; exit done; }"#,
        "E_MUTABILITY",
    );
    rejects(
        r#"control main() exits done { [entry]: mutable string s="x"; auto r=&s; auto w=&mutable s; emit(r); emit(w); exit done; }"#,
        "E_LOAN",
    );
}
#[test]
fn reference_lifetimes_are_scoped_and_path_sensitive() {
    let source = r#"control main(bool flag) exits auto {
        [entry]: mutable string s="before"; delta flag {true=>yes,false=>no}
        [yes]: auto r=&s; emit(&*r); drop(r);
        [no]: s="after";
        [entry]: exit done {s};
    }"#;
    assert_eq!(
        result(source, vec![Value::Bool(true)]).fields,
        vec![Value::String("before".into())]
    );
    assert_eq!(
        result(source, vec![Value::Bool(false)]).fields,
        vec![Value::String("after".into())]
    );
    rejects(
        &source.replace("emit(&*r);", "s=\"conflict\"; emit(&*r);"),
        "E_LOAN",
    );
    result(
        r#"control main() exits auto { [entry]: mutable string s="old"; [entry] over &: { [entry]: auto r=&s; emit(&*r); } [entry]: s="new"; exit done {s}; }"#,
        vec![],
    );
}
#[test]
fn local_references_cannot_escape_or_borrow_temporaries() {
    rejects(
        r#"control main() exits auto { [entry]: string s="x"; auto r=&s; exit done {r}; }"#,
        "E_LIFETIME",
    );
    rejects(
        r#"control main() exits auto { [entry]: string s="x"; exit done {&s}; }"#,
        "E_LIFETIME",
    );
    rejects(
        r#"control main() exits done { [entry]: auto r=&"x"; exit done; }"#,
        "E_LIFETIME",
    );
}
#[test]
fn local_reference_can_be_passed_to_a_shared_parameter() {
    let source = r#"control main() exits auto { [entry]: string s="hello"; auto r=&s; delta duplicate(r) { copied {string copy}=>ready } [ready]: drop(r); exit done {s,copy}; }
    control duplicate(&string text) exits auto { [entry]: exit copied {clone(text)}; }"#;
    assert_eq!(
        result(source, vec![]).fields,
        vec![Value::String("hello".into()); 2]
    );
}
#[test]
fn loops_carry_owned_values_instead_of_repeatedly_consuming_captures() {
    rejects(
        r#"control main() exits done { [entry]: string s="x"; loop repeat() { [entry]: emit(s); again repeat(); } }"#,
        "E_LOOP_CAPTURE",
    );
    let source = r#"control main() exits auto { [entry]: string s="x"; loop repeat(string value=s, bool first=true) {
        [entry]: delta first {true=>again_path,false=>finish}
        [again_path]: auto ref=&value; emit(&*ref); drop(ref); again repeat(value,false);
        [finish]: exit done {value};
    } }"#;
    assert_eq!(
        result(source, vec![]).fields,
        vec![Value::String("x".into())]
    );
}

#[test]
fn record_field_loans_are_disjoint_and_parent_moves_conflict() {
    let mut m = manifest();
    m.types.push(tellur::manifest::TypeDef::Record {
        name: "pair".into(),
        fields: vec![
            tellur::manifest::Field {
                name: "left".into(),
                ty: "String".into(),
            },
            tellur::manifest::Field {
                name: "right".into(),
                ty: "String".into(),
            },
        ],
    });
    let source = r#"control main() exits auto { [entry]:
        mutable pair p=pair {"a","b"}; auto left=&mutable p.left; auto right=&p.right;
        *left="changed"; string copy=clone(right); drop(left); drop(right); exit done {p,copy};
    }"#;
    let p = compile(source, m.clone()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    let actual = p.run(&s).unwrap();
    assert_eq!(actual, p.expanded("main").unwrap().run(&s).unwrap());
    assert_eq!(actual.fields[1], Value::String("b".into()));
    assert_eq!(
        compile(
            &source.replace(
                "string copy=clone(right);",
                "auto moved=p; string copy=clone(right);"
            ),
            m.clone()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
    assert_eq!(
        compile(&source.replace("auto right=&p.right", "auto right=&p"), m)
            .unwrap_err()
            .code,
        "E_LOAN"
    );
}
#[test]
fn copy_capabilities_propagate_through_options_and_bytes_are_owned() {
    result(
        "control main() exits auto { [entry]: auto a=some {42}; auto b=a; exit done {a,b}; }",
        vec![],
    );
    rejects(
        r#"control main() exits auto { [entry]: auto a=some {"hello"}; auto b=a; exit done {a,b}; }"#,
        "E_MOVED",
    );
    rejects(
        r#"control main() exits auto { [entry]: bytes a=bytes"abcd"; auto b=a; exit done {a,b}; }"#,
        "E_MOVED",
    );
    let actual = result(
        r#"control main() exits auto { [entry]: bytes a=bytes"abcd"; auto b=clone(&a); exit done {a,b}; }"#,
        vec![],
    );
    assert_eq!(actual.fields, vec![Value::Bytes(vec![0xab, 0xcd]); 2]);
}
#[test]
fn borrowed_resource_still_requires_owner_completion() {
    let source = "control main(handle file) exits auto { [entry]: auto view=&file; drop(view); exit done {file}; }";
    result(
        source,
        vec![Value::Opaque {
            ty: "handle".into(),
            token: "owned".into(),
        }],
    );
    rejects(
        &source.replace("drop(view);", "auto moved=file; drop(view);"),
        "E_LOAN",
    );
    rejects(
        &source.replace("exit done {file}", "exit done"),
        "E_RESOURCE",
    );
}

#[test]
fn nested_owned_construction_cannot_hide_an_argument_move() {
    let mut m = manifest();
    m.types.push(tellur::manifest::TypeDef::Record {
        name: "wrapper".into(),
        fields: vec![tellur::manifest::Field {
            name: "text".into(),
            ty: "String".into(),
        }],
    });
    let source = r#"control main() exits done { [entry]: string s="text"; delta mixed(&s,wrapper {s}) {done=>ready} [ready]: exit done; }
        control mixed(&string first,wrapper second) exits done { [entry]: emit(first); drop(second); exit done; }"#;
    assert_eq!(compile(source, m).unwrap_err().code, "E_ALIAS");
}
#[test]
fn reference_rebinding_is_still_rejected_explicitly() {
    rejects(
        r#"control main() exits done { [entry]: string s="x"; auto a=&s; a=&s; exit done; }"#,
        "E_UNSUPPORTED",
    );
}
