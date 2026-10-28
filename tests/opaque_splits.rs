mod support;
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
fn rejects(s: &str) {
    let e = compile(s, m()).unwrap_err();
    assert!(
        [
            "E_LOAN",
            "E_ALIAS",
            "E_MOVED",
            "E_MUTABILITY",
            "E_BORROW_MOVE"
        ]
        .contains(&e.code),
        "{e}"
    );
}
const BASE: &str = r#"
struct owner {string left;string right;}
control split(ref mutable owner input) exits ready {ref mutable string left from input,ref mutable string right from input} {
    exit ready {borrow mutable input.left,borrow mutable input.right};
}
control write(ref mutable string a,ref mutable string b) exits done {deref a="L";deref b="R";exit done;}
control main() exits done... {
    mutable owner value=owner {"left","right"};
    delta split(borrow mutable value) {ready {auto a,auto b}=>split}
    [split]: write(a,b);emit(a);emit(b);exit done {value.left,value.right};
}
"#;
#[test]
fn opaque_siblings_coexist_but_owner_remains_protected() {
    let p = compile(BASE, m()).unwrap();
    let result = p.run(&scenario()).unwrap();
    assert_eq!(
        result.fields,
        vec![Value::String("L".into()), Value::String("R".into())]
    );
    assert_eq!(
        result,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    for edit in [
        "value.left=\"bad\";write(a,b);",
        "drop(value);write(a,b);",
        "delta split(borrow mutable value) {ready {auto c,auto d}=>again} [again]:write(a,b);",
        "write(a,a);write(a,b);",
    ] {
        rejects(&BASE.replace("write(a,b);", edit));
    }
    // The explicit contract hides the layout: even the released left field remains
    // protected while the right child is live.
    rejects(&BASE.replace("emit(a);emit(b);", "emit(a);value.left=\"bad\";emit(b);"));
}
#[test]
fn implementation_cannot_assert_exclusivity_into_existence() {
    rejects(&BASE.replace("borrow mutable input.right", "borrow mutable input.left"));
    rejects(
        &BASE
            .replace(
                "ref mutable string right from input",
                "ref string right from input",
            )
            .replace("borrow mutable input.right", "borrow input.left"),
    );
    rejects(&BASE.replace("exits ready {ref mutable string left from input,ref mutable string right from input}","exits ready...").replace("borrow mutable input.right","borrow mutable input.left"));
}
#[test]
fn sibling_permission_survives_reborrow_forward_and_moving() {
    let s=BASE.replace("control write",r#"control forward(ref mutable string input) exits found {ref mutable string value from input} {exit found {input};}
 control write"#).replace("write(a,b);",r#"delta forward(a) {found {auto child}=>child}
 [child]:ref mutable string moved=child;write(moved,b);deref a="parent restored";"#);
    let p = compile(&s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(&s.replace("write(moved,b);", "deref a=\"bad\";write(moved,b);"));
    rejects(&s.replace("write(moved,b);", "emit(child);write(moved,b);"));
}
#[test]
fn branches_can_swap_private_projections_without_losing_separation() {
    let s = BASE
        .replace(
            "ref mutable owner input)",
            "ref mutable owner input,bool reverse)",
        )
        .replace(
            "exit ready {borrow mutable input.left,borrow mutable input.right};",
            r#"
 delta reverse {true=>reversed,false=>original}
 [original]:exit ready {borrow mutable input.left,borrow mutable input.right};
 [reversed]:exit ready {borrow mutable input.right,borrow mutable input.left};"#,
        )
        .replace(
            "split(borrow mutable value)",
            "split(borrow mutable value,true)",
        );
    for s in [
        s.clone(),
        s.replace(
            "exits ready {ref mutable string left from input,ref mutable string right from input}",
            "exits ready...",
        ),
    ] {
        let p = compile(&s, m()).unwrap();
        assert_eq!(
            p.run(&scenario()).unwrap().fields,
            vec![Value::String("R".into()), Value::String("L".into())]
        );
        assert_eq!(
            p.run(&scenario()).unwrap(),
            p.expanded("main").unwrap().run(&scenario()).unwrap()
        );
    }
}
#[test]
fn shared_aliases_remain_shared_and_copies_retain_owner() {
    let s = r#"struct owner {string left;string right;}
 control split(ref mutable owner input) exits ready {ref string a from input,ref string b from input,ref mutable string c from input} {exit ready {borrow input.left,borrow input.left,borrow mutable input.right};}
 control main() exits done {mutable owner value=owner {"l","r"};delta split(borrow mutable value) {ready {auto a,auto b,auto c}=>ready}
 [ready]:ref string copy=a;emit(a);deref c="changed";emit(b);emit(copy);value.left="released";exit done;}"#;
    compile(s, m()).unwrap();
    rejects(&s.replace(
        "emit(b);emit(copy);",
        "emit(b);value.left=\"bad\";emit(copy);",
    ));
    rejects(&s.replace("borrow mutable input.right", "borrow mutable input.left"));
    rejects(&s.replace("emit(a);", "deref a=\"bad\";emit(a);"));
}
#[test]
fn grouped_leaves_share_lifetimes_without_sharing_exclusive_permissions() {
    let s = r#"struct owner {string left;string right;}
 struct pair['a] {ref mutable string left <- 'a;ref mutable string right <- 'a;}
 control split(ref mutable owner input) exits ready {pair[input] value} {exit ready {pair {borrow mutable input.left,borrow mutable input.right}};}
 control relay(pair input) exits ready {pair[input['a]] value} {exit ready {input};}
 control children(ref mutable pair input) exits ready {pair[input['a]] value} {exit ready {pair {borrow mutable deref input.left,borrow mutable deref input.right}};}
 control write(ref mutable string a,ref mutable string b) exits done {deref a="L";deref b="R";exit done;}
 control main() exits done... {mutable owner value=owner {"l","r"};
 delta split(borrow mutable value) {ready {auto original}=>split}
 [split]:delta relay(original) {ready {auto received}=>relayed}
 [relayed]:mutable pair parents=received;delta children(borrow mutable parents) {ready {auto kids}=>children}
 [children]:write(kids.left,kids.right);emit(kids.left);emit(kids.right);write(parents.left,parents.right);exit done {value.left,value.right};}"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(&s.replace(
        "write(kids.left,kids.right);",
        "write(parents.left,parents.right);write(kids.left,kids.right);",
    ));
    rejects(&s.replace("emit(kids.left);", "drop(parents);emit(kids.left);"));
    rejects(&s.replace("borrow mutable input.right", "borrow mutable input.left"));
}
#[test]
fn trait_contracts_require_disjoint_implementations_and_preserve_them_generically() {
    let s = r#"struct owner {string left;string right;}
 trait split {control parts(ref mutable self input) exits ready {ref mutable string left from input,ref mutable string right from input};}
 implement split for owner {control parts(ref mutable self input) exits ready {ref mutable string left from input,ref mutable string right from input} {exit ready {borrow mutable input.left,borrow mutable input.right};}}
 control parts<t:split>(ref mutable t input) exits ready {ref mutable string left from input,ref mutable string right from input} {forward split<t>.parts(input);}
 control main() exits done... {mutable owner value=owner {"l","r"};delta parts<owner>(borrow mutable value) {ready {auto a,auto b}=>ready} [ready]:deref a="L";deref b="R";emit(a);emit(b);exit done {value.left,value.right};}"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(&s.replace("borrow mutable input.right", "borrow mutable input.left"));
    rejects(&s.replace("emit(a);", "drop(value);emit(a);"));
    // Even an unused generic body cannot duplicate the abstract permission it receives.
    rejects(
        &(s.to_owned()
            + "control bad<t:split>(ref mutable t input) exits ready... {delta split<t>.parts(input) {ready {auto a,auto b}=>ready} [ready]:exit ready {a,a};}"),
    );
}
#[test]
fn native_opaque_splits_match_interpreter_and_expansion() {
    let p = compile(include_str!("fixtures/opaque_splits.tellur"), m()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    assert_eq!(
        expected.fields,
        vec![
            Value::String("Final title".into()),
            Value::String("A new body".into())
        ]
    );
    let root = std::env::temp_dir().join(format!("path-opaque-splits-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        assert_eq!(p.run(&scenario()).unwrap(), expected);
        for opt in [0, 2] {
            let binary = root.join(format!("split-{expanded}-{opt}"));
            support::scenario_executable(&p)
                .build_native("main", &binary, opt)
                .unwrap();
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
fn independent_invocations_do_not_create_false_disjointness() {
    // Shared aliases may overlap. Copies must still retain the parent permission.
    let s = BASE.replace(
        "write(a,b);",
        r#"ref string first=borrow deref a;
 ref string second=borrow deref a;emit(first);emit(second);write(a,b);"#,
    );
    compile(&s, m()).unwrap();
    rejects(&s.replace(
        "emit(first);emit(second);",
        "deref a=\"bad\";emit(first);emit(second);",
    ));
    // Separate opaque calls over concrete distinct subobjects stay independent.
    let s=BASE.replace("control main()", "struct outer {owner left;owner right;} control main()").replace("mutable owner value=owner {\"left\",\"right\"};", "mutable outer all=outer {owner {\"left\",\"right\"},owner {\"x\",\"y\"}};")
 .replace("borrow mutable value", "borrow mutable all.left")
 .replace("write(a,b);", "delta split(borrow mutable all.right) {ready {auto c,auto d}=>again} [again]:write(a,c);write(b,d);")
 .replace("value.left,value.right", "all.left.left,all.left.right");
    compile(&s, m()).unwrap();
    rejects(&s.replace(
        "split(borrow mutable all.right)",
        "split(borrow mutable all.left)",
    ));
}
#[test]
fn projections_split_access_without_narrowing_the_opaque_owner_anchor() {
    let s = r#"struct pair {string left;string right;} struct owner {pair data;string unused;}
 control view(ref mutable owner input) exits found {ref mutable pair value from input} {exit found {borrow mutable input.data};}
 control update(ref mutable string a,ref mutable string b) exits done {deref a="A";deref b="B";exit done;}
 control main() exits done... {mutable owner value=owner {pair {"a","b"},"u"};
 delta view(borrow mutable value) {found {auto parent}=>found}
 [found]:ref mutable string a=borrow mutable parent.left;ref mutable string b=borrow mutable parent.right;
 update(a,b);emit(a);parent.left="restored";emit(b);exit done {value.data.left,value.data.right};}"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(&s.replace("borrow mutable parent.right", "borrow mutable parent.left"));
    rejects(&s.replace("update(a,b);", "parent.left=\"bad\";update(a,b);"));
    rejects(&s.replace("emit(b);", "value.unused=\"bad\";emit(b);"));
}
