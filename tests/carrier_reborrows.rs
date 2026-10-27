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
fn rejects(s: &str, codes: &[&str]) {
    let e = compile(s, m()).unwrap_err();
    assert!(codes.contains(&e.code), "{e}");
}
const SCHEMA: &str =
    "struct editors['a,'b] {ref mutable string left <- 'a;ref mutable string right <- 'b;}";
#[test]
fn shared_carrier_can_read_but_cannot_mutate_or_extract_exclusive_fields() {
    let s = format!(
        r#"{SCHEMA}
 control inspect(ref editors input) exits done {{emit(input.left);emit(input.right);exit done;}}
 control main() exits done {{mutable string a="a";mutable string b="b";editors p=editors {{borrow mutable a,borrow mutable b}};inspect(borrow p);deref p.left="again";exit done;}}"#
    );
    let p = compile(&s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace("emit(input.left);", "deref input.left=\"bad\";"),
        &["E_MUTABILITY"],
    );
    rejects(
        &s.replace("emit(input.left);", "ref mutable string taken=input.left;"),
        &["E_MUTABILITY", "E_BORROW_MOVE"],
    );
}
#[test]
fn exclusive_child_returns_retain_carrier_and_restore_parent_at_last_use() {
    let s = format!(
        r#"{SCHEMA}
 control first(ref mutable editors input) exits found {{ref mutable string value from input['a]}} {{exit found {{borrow mutable deref input.left}};}}
 control main() exits done... {{mutable string a="a";mutable string b="b";mutable editors p=editors {{borrow mutable a,borrow mutable b}};
 delta first(borrow mutable p) {{found {{auto child}}=>ready}}
 [ready]: deref child="child";deref p.left="parent";deref p.right="other";exit done {{a,b}};
 }}"#
    );
    let p = compile(&s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![
            Value::String("parent".into()),
            Value::String("other".into())
        ]
    );
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace("deref child=", "deref p.left=\"bad\";deref child="),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("deref child=", "drop(p);deref child="),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("ref mutable editors input", "ref editors input"),
        &["E_LIFETIME", "E_MUTABILITY"],
    );
}
#[test]
fn shared_child_from_exclusive_field_cannot_outlive_local_carrier() {
    let s = format!(
        r#"{SCHEMA}
 control peek(ref editors input) exits found {{ref string value from input['a]}} {{exit found {{borrow deref input.left}};}}
 control main() exits done {{mutable string a="a";mutable string b="b";editors p=editors {{borrow mutable a,borrow mutable b}};
 delta peek(borrow p) {{found {{auto child}}=>ready}} [ready]:emit(child);drop(p);exit done;
 }}"#
    );
    let p = compile(&s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace("emit(child);drop(p)", "drop(p);emit(child)"),
        &["E_LOAN"],
    );
    rejects(
        &s.replace(
            "emit(child);drop(p)",
            "deref p.left=\"bad\";emit(child);drop(p)",
        ),
        &["E_LOAN"],
    );
    let escape = format!(
        r#"{SCHEMA}
 control bad(ref mutable string a,ref mutable string b) exits found... {{editors p=editors {{a,b}};ref editors alias=borrow p;exit found {{borrow deref alias.left}};}}"#
    );
    rejects(&escape, &["E_LIFETIME"]);
}
#[test]
fn local_carrier_aliases_suspend_conflicting_parent_access() {
    let s = format!(
        r#"{SCHEMA} control main() exits done {{mutable string a="a";mutable string b="b";mutable editors p=editors {{borrow mutable a,borrow mutable b}};
 ref mutable editors alias=borrow mutable p;deref alias.left="through alias";deref p.left="restored";exit done;}}"#
    );
    let p = compile(&s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace(
            "deref alias.left=",
            "deref p.left=\"bad\";deref alias.left=",
        ),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("deref alias.left=", "emit(p.left);deref alias.left="),
        &["E_LOAN"],
    );
    rejects(
        &s.replace(
            "ref mutable editors alias=borrow mutable p",
            "ref editors alias=borrow p",
        ),
        &["E_MUTABILITY"],
    );
}
#[test]
fn disjoint_exclusive_children_forward_in_returned_records() {
    let s = format!(
        r#"{SCHEMA}
 control both(ref mutable editors input) exits ready {{editors[input['a],input['b]] value}} {{exit ready {{editors {{borrow mutable deref input.left,borrow mutable deref input.right}}}};}}
 control relay(ref mutable editors input) exits ready... {{forward both(input);}}
 control main() exits done... {{mutable string a="a";mutable string b="b";mutable editors p=editors {{borrow mutable a,borrow mutable b}};
 delta relay(borrow mutable p) {{ready {{auto children}}=>ready}}
 [ready]:deref children.left="left child";deref p.left="left parent";deref children.right="right child";deref p.right="right parent";exit done {{a,b}};}}"#
    );
    let p = compile(&s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace(
            "deref children.right=",
            "deref p.right=\"bad\";deref children.right=",
        ),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("deref children.left=", "drop(p);deref children.left="),
        &["E_LOAN"],
    );
    let mut artifact = p.expansion_artifact("relay").unwrap();
    p.accept_expansion(&artifact).unwrap();
    artifact.region.stored_returns[0]
        .sources
        .retain(|s| !s.guard);
    assert_eq!(
        p.accept_expansion(&artifact).unwrap_err().code,
        "E_EXPANSION_CERTIFICATE"
    );
}
#[test]
fn shared_children_may_be_copied_but_all_copies_retain_authorization() {
    let s = format!(
        r#"{SCHEMA} control main() exits done {{mutable string a="a";mutable string b="b";editors p=editors {{borrow mutable a,borrow mutable b}};
 ref editors carrier=borrow p;ref string child=borrow deref carrier.left;ref string copy=child;emit(child);emit(copy);drop(p);exit done;}}"#
    );
    let p = compile(&s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace("emit(copy);drop(p)", "drop(p);emit(copy)"),
        &["E_LOAN"],
    );
}
#[test]
fn whole_carrier_call_aliases_include_exclusive_contained_permissions() {
    for (params, args) in [
        (
            "ref mutable editors p,ref string value",
            "borrow mutable p,p.left",
        ),
        (
            "ref string value,ref mutable editors p",
            "p.left,borrow mutable p",
        ),
    ] {
        let s = format!(
            r#"{SCHEMA} control use({params}) exits done {{emit(value);deref p.left="changed";exit done;}}
  control main() exits done {{mutable string a="a";mutable string b="b";mutable editors p=editors {{borrow mutable a,borrow mutable b}};use({args});exit done;}}"#
        );
        rejects(&s, &["E_ALIAS", "E_LOAN"]);
    }
    let s = format!(
        r#"{SCHEMA} control use(ref editors one,ref editors two) exits done {{emit(one.left);emit(two.left);exit done;}}
 control main() exits done {{mutable string a="a";mutable string b="b";editors p=editors {{borrow mutable a,borrow mutable b}};use(borrow p,borrow p);exit done;}}"#
    );
    compile(&s, m()).unwrap();
}
#[test]
fn nested_generic_carriers_keep_guards_through_forwarding() {
    let s = r#"struct editor<t>['a] {ref mutable t value <- 'a;}
 struct wrapper<t>['a] {editor<t>['a] inner;}
 control peek<t>(ref wrapper<t> input) exits found {ref t value from input['a]} {exit found {borrow deref input.inner.value};}
 control relay<t>(ref wrapper<t> input) exits found... {forward peek<t>(input);}
 control main() exits done {mutable string a="a";wrapper<string> p=wrapper<string> {editor<string> {borrow mutable a}};
 delta relay<string>(borrow p) {found {auto child}=>ready} [ready]:emit(child);drop(p);exit done;}
 "#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace("emit(child);drop(p)", "drop(p);emit(child)"),
        &["E_LOAN"],
    );
    rejects(
        r#"struct editor<t>['a] {ref mutable t value <- 'a;} control bad<t>(ref editor<t> input) exits found... {exit found {borrow mutable deref input.value};}"#,
        &["E_MUTABILITY"],
    );
}
#[test]
fn collection_elements_keep_the_exclusive_carrier_guard() {
    let s = r#"struct editor['a] {ref mutable vector<string> words <- 'a;}
 control first(ref editor input) exits found {ref string word from input['a]},missing {
   exit delta vector_get(input.words,0) {found {auto word}=>found {word},out_of_bounds=>missing {}};
 }
 control main() exits done {mutable vector<string> words=["word"];editor p=editor {borrow mutable words};
 delta first(borrow p) {found {auto child}=>found,missing=>missing}
 [found]:emit(child);drop(p);exit done;[missing]:exit done;}
 "#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace("emit(child);drop(p)", "drop(p);emit(child)"),
        &["E_LOAN"],
    );
}
#[test]
fn inferred_contracts_and_moved_children_keep_guard_dependencies() {
    let s = format!(
        r#"{SCHEMA}
 control first(ref mutable editors input) exits found... {{exit found {{borrow mutable deref input.left}};}}
 control main() exits done {{mutable string a="a";mutable string b="b";mutable editors p=editors {{borrow mutable a,borrow mutable b}};
 delta first(borrow mutable p) {{found {{auto child}}=>ready}}
 [ready]:ref mutable string moved=child;deref moved="child";drop(p);exit done;}}"#
    );
    let p = compile(&s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace("deref moved=", "drop(p);deref moved="),
        &["E_LOAN"],
    );
    rejects(
        &s.replace("deref moved=", "deref child=\"bad\";deref moved="),
        &["E_MOVED"],
    );
}
#[test]
fn native_carrier_reborrows_preserve_execution_and_certified_expansion() {
    let p = compile(include_str!("fixtures/carrier_reborrows.tellur"), m()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    assert_eq!(
        expected.fields,
        vec![
            Value::String("First parent restored".into()),
            Value::String("Second parent restored".into())
        ]
    );
    let root = std::env::temp_dir().join(format!("path-carrier-reborrows-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        assert_eq!(p.run(&scenario()).unwrap(), expected);
        for opt in [0, 2] {
            let binary = root.join(format!("reborrows-{expanded}-{opt}"));
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
}
#[test]
fn legacy_spelling_cannot_copy_exclusive_carrier_permissions() {
    let s = format!(
        r#"{SCHEMA} region main() exits done {{[Entry]:mutable string a="a";mutable string b="b";editors p=editors {{borrow mutable a,borrow mutable b}};auto moved=p;deref moved.left="valid";exit done;}}"#
    );
    compile(&s, m()).unwrap();
    rejects(
        &s.replace("deref moved.left=", "auto duplicate=p;deref moved.left="),
        &["E_MOVED"],
    );
}
