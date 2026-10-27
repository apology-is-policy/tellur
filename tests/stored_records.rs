use tellur::{compile, ir::Value, manifest::Manifest, runtime::Scenario};

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
fn rejects(source: &str, code: &str) {
    let error = compile(source, manifest()).unwrap_err();
    assert_eq!(error.code, code, "{error}");
}
const SOURCE: &str = r#"
struct message {string text;}
struct view {ref message item;string label;}
struct pair {view left;ref mutable string right;}
control append(ref mutable string text) exits done {deref text="changed";exit done;}
control main() exits done {string left,string right} {
    message owner=message {"hello"};
    mutable string other="before";
    pair first=pair {view {borrow owner,"label"},borrow mutable other};
    pair second=first;
    view left=second.left;
    ref mutable string right=second.right;
    append(right);
    ref message item=left.item;
    exit done {clone(borrow item.text),other};
}
"#;

#[test]
fn nested_records_transfer_loans_with_whole_and_partial_moves() {
    let p = compile(SOURCE, manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    assert_eq!(
        expected.fields,
        vec![
            Value::String("hello".into()),
            Value::String("changed".into())
        ]
    );
    assert_eq!(
        expected,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &SOURCE.replace("pair second=first;", "pair second=first;drop(first);"),
        "E_MOVED",
    );
    rejects(
        &SOURCE.replace("append(right);", "append(second.right);append(right);"),
        "E_MOVED",
    );
}

#[test]
fn shared_records_copy_and_clone_keep_independent_loans() {
    let source = r#"struct view {ref string text;}
    control main() exits done {string text} {
      mutable string owner="hello";
      view first=view {borrow owner};view second=first;view third=clone(borrow second);
      drop(first);drop(second);ref string text=third.text;emit(text);
      owner="after";exit done {owner};
    }"#;
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("after".into())]
    );
    rejects(
        &source.replace(
            "drop(first);drop(second);",
            "drop(first);drop(second);owner=\"bad\";",
        ),
        "E_LOAN",
    );
    rejects(
        &source.replace("emit(text);", "owner=\"bad\";emit(text);"),
        "E_LOAN",
    );
}

#[test]
fn exclusive_fields_move_and_cannot_be_copied_or_cloned() {
    let source = r#"struct editor {ref mutable string text;}
    control main() exits done {string text} {
      mutable string owner="before";ref mutable string original=borrow mutable owner;
      editor first=editor {original};editor second=first;
      deref second.text="after";drop(second);exit done {owner};
    }"#;
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("after".into())]
    );
    rejects(
        &source.replace("editor second=first;", "editor second=clone(borrow first);"),
        "E_CLONE",
    );
    rejects(
        &source.replace(
            "editor second=first;",
            "editor second=first;deref original=\"bad\";",
        ),
        "E_MOVED",
    );
    rejects(
        &source.replace("deref second.text=", "deref first.text="),
        "E_MOVED",
    );
    rejects(
        &source.replace("drop(second);", "owner=\"bad\";drop(second);"),
        "E_LOAN",
    );
}

#[test]
fn construction_checks_every_argument_in_written_order() {
    rejects(
        r#"struct view {ref string text;}
    control main() exits done {mutable string text="x";ref mutable string editor=borrow mutable text;view v=view {editor};exit done;}"#,
        "E_TYPE",
    );
    rejects(
        r#"struct bad {ref string view;string owner;}
    control main() exits done {string text="x";bad v=bad {borrow text,text};exit done;}"#,
        "E_LOAN",
    );
    rejects(
        r#"struct bad {string owner;ref string view;}
    control main() exits done {string text="x";bad v=bad {text,borrow text};exit done;}"#,
        "E_MOVED",
    );
    rejects(
        r#"struct pair {ref mutable string left;ref mutable string right;}
    control main() exits done {mutable string text="x";pair p=pair {right:borrow mutable text,left:borrow mutable text};exit done;}"#,
        "E_LOAN",
    );
    rejects(
        r#"struct view {ref string text;}
    control main() exits done {view v=view {borrow "temporary"};exit done;}"#,
        "E_LIFETIME",
    );
}

#[test]
fn field_permissions_and_disjoint_owner_fields_are_preserved() {
    let source = r#"struct message {string left;string right;}struct view {ref message item;}
    control main() exits done {mutable message owner=message {"left","right"};view v=view {borrow owner};
    emit(borrow v.item.left);exit done;}"#;
    compile(source, manifest()).unwrap();
    rejects(
        &source.replace("emit(borrow v.item.left);", "v.item.left=\"bad\";"),
        "E_MUTABILITY",
    );
    rejects(
        &source.replace("emit(borrow v.item.left);", "string stolen=v.item.left;"),
        "E_BORROW_MOVE",
    );
    let disjoint = r#"struct message {string left;string right;}struct view {ref string text;}
    control main() exits done {mutable message owner=message {"left","right"};view v=view {borrow owner.left};
    owner.right="fine";ref string text=v.text;emit(text);exit done;}"#;
    compile(disjoint, manifest()).unwrap();
    rejects(&disjoint.replace("owner.right=", "owner.left="), "E_LOAN");
}

#[test]
fn stored_origins_survive_borrowed_control_results_and_local_scope() {
    let source = r#"struct view {ref string text;}
    control peek(ref string input) exits found {ref string text from input} {
      view v=view {input};exit found {v.text};
    }
    control main() exits done {string text} {
      string owner="hello";
      delta peek(borrow owner) {found {ref string text}=>available}
      [available]:view v=view {text};ref string local=v.text;emit(local);exit done {owner};
    }"#;
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &source.replace(
            "view v=view {input};",
            "string temp=\"bad\";view v=view {borrow temp};",
        ),
        "E_LIFETIME",
    );
}

#[test]
fn unimplemented_boundaries_are_rejected_before_losing_provenance() {
    // Local concrete provenance no longer requires a public origin schema.
    let local = r#"struct view {ref string text;}
      control main() exits done {string owner="x";view v=view {borrow owner};
        auto copy=borrow v;emit(copy.text);exit done;}"#;
    let program = compile(local, manifest()).unwrap();
    assert_eq!(
        program.run(&scenario()).unwrap(),
        program.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &local.replace("emit(copy.text);", "drop(owner);emit(copy.text);"),
        "E_LOAN",
    );
    for operation in [
        "v=view {borrow other};",
        "exit done {v};",
        "vector<view> all=[v];",
    ] {
        let source = format!(
            "struct view {{ref string text;}} control main() exits auto {{string owner=\"x\";string other=\"y\";mutable view v=view {{borrow owner}};{operation} exit done;}}"
        );
        assert!(
            compile(&source, manifest()).is_err(),
            "accepted unsupported boundary: {operation}"
        );
    }
    rejects(
        "struct view {ref string text;} control use(view v) exits done {exit done;}",
        "E_MANIFEST",
    );
}

#[test]
fn paths_and_scope_exit_release_only_the_selected_storage() {
    let source = r#"struct view {ref string text;}
    control main() exits done {
      mutable string owner="hello";view v=view {borrow owner};
      delta true {true=>yes,false=>no}
      [yes]:drop(v);owner="changed";exit done;
      [no]:ref string text=v.text;emit(text);exit done;
    }"#;
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &source.replace("[no]:ref string", "[no]:owner=\"bad\";ref string"),
        "E_LOAN",
    );
    rejects(&source.replace("drop(v);", "drop(v);drop(v);"), "E_MOVED");
}

#[test]
fn generic_record_layouts_and_field_reborrows_keep_owner_identity() {
    let source = r#"struct view<t> {ref t item;}
    struct editor {ref mutable string item;}
    control main() exits done {string text} {
      mutable string owner="hello";editor e=editor {borrow mutable owner};
      view<string> v=view<string> {borrow deref e.item};
      ref string text=v.item;emit(text);
      deref e.item="changed";drop(e);exit done {owner};
    }"#;
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("changed".into())]
    );
    rejects(
        &source.replace(
            "ref string text=v.item;",
            "deref e.item=\"bad\";ref string text=v.item;",
        ),
        "E_LOAN",
    );
    rejects(
        &source.replace("ref string text=v.item;", "drop(e);ref string text=v.item;"),
        "E_LOAN",
    );
}

#[test]
fn loop_local_storage_is_recreated_and_outer_exclusive_storage_cannot_be_transferred() {
    let source = r#"struct editor {ref mutable string text;}
    control main() exits done {string text} {
      mutable string owner="before";
      loop repeat(bool first=true) {
        editor current=editor {borrow mutable owner};deref current.text="after";
        delta first {true=>next,false=>finished}
        [next]:again repeat(false);
        [finished]:exit done {clone(borrow owner)};
      }
    }"#;
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("after".into())]
    );
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        r#"struct editor {ref mutable string text;}
    control main() exits done {mutable string owner="x";editor outside=editor {borrow mutable owner};
    loop repeat(bool first=true) {auto inside=outside;deref inside.text="bad";again repeat(false);}}"#,
        "E_LOOP_CAPTURE",
    );
}

#[test]
fn native_record_loans_match_interpreter_and_certified_expansion() {
    let p = compile(SOURCE, manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    let root = std::env::temp_dir().join(format!("path-stored-records-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let binary = root.join(format!("records-{expanded}-{opt}"));
            p.build_native("main", &binary, opt).unwrap();
            let actual = std::process::Command::new(binary)
                .arg("--json")
                .output()
                .unwrap();
            assert!(
                actual.status.success(),
                "{}",
                String::from_utf8_lossy(&actual.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
