use tellur::{
    compile,
    ir::{Action, Expr, Value},
    manifest::Manifest,
    runtime::Scenario,
};
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
const ORDER: &str = r#"
struct pair {string first;string second;}
control make(string owner) exits ready {pair value} {
    exit ready {pair {second: clone(&owner), first: owner}};
}
control main() exits auto {
    exit delta make("Ada") {ready {auto value}=>done {value.first,value.second}};
}
"#;
#[test]
fn written_order_preserves_clone_before_move_and_declared_layout() {
    let p = compile(ORDER, m()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    assert_eq!(expected.fields, vec![Value::String("Ada".into()); 2]);
    assert_eq!(
        expected,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    let bad = ORDER.replace(
        "second: clone(&owner), first: owner",
        "first: owner, second: clone(&owner)",
    );
    assert_eq!(compile(&bad, m()).unwrap_err().code, "E_MOVED");
}
#[test]
fn nested_fields_infer_literals_and_allow_trailing_commas() {
    let source = r#"struct inner {vector<string> words;option<u32> number;}
struct outer {inner data;bool flag;}
control main() exits auto {
    auto value=outer {flag: true, data: inner {number: none,words: ["one","two"],},};
    exit done {value.flag,value.data.words};
}"#;
    let p = compile(source, m()).unwrap();
    let result = p.run(&scenario()).unwrap();
    assert_eq!(result.fields[0], Value::Bool(true));
    assert_eq!(
        result,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn field_errors_identify_missing_duplicate_unknown_and_mistyped_initializers() {
    for (init, code, fragment) in [
        ("a: 1", "E_FIELD", "b"),
        ("a: 1,a: 2,b: true", "E_FIELD", "more than once"),
        ("c: 1,b: true", "E_FIELD", "no field `c`"),
        ("b: 1,a: 2", "E_TYPE", "Bool"),
        ("a: 1,true", "E_PARSE", ":"),
        ("1,b: true", "E_PARSE", "}"),
    ] {
        let source = format!(
            "struct pair {{u32 a;bool b;}} control main() exits done {{auto value=pair {{{init}}};exit done;}}"
        );
        let e = compile(&source, m()).unwrap_err();
        assert_eq!(e.code, code, "{e}");
        assert!(e.message.contains(fragment), "{e}");
    }
    let e=compile("variant choice {yes {u32 value},no} control main() exits done {auto x=choice.yes {value:1};exit done;}",m()).unwrap_err();
    assert_eq!(e.code, "E_CONSTRUCT");
}
#[test]
fn names_do_not_relax_nominality_or_resource_obligations() {
    let source = "struct owned {handle file;u32 id;} control main(handle file) exits done {owned value=owned {id:1,file:file};exit done;}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_RESOURCE");
    let source = source
        .replace("exits done", "exits ready {owned value}")
        .replace("exit done;", "exit ready {value};");
    compile(&source, m()).unwrap();
    let source = "struct a {u32 x;} struct b {u32 x;} control main() exits done {b value=a {x:1};exit done;}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_TYPE");
}
#[test]
fn nested_initializers_keep_borrows_live_and_reject_overlapping_call_arguments() {
    let good = r#"struct pair {string a;string b;}
control accept(&string borrowed,pair owned) exits done {exit done;}
control main() exits done {string owner="x";auto ref=&owner;pair value=pair {b:clone(ref),a:"other"};auto moved=owner;exit done;}"#;
    compile(good, m()).unwrap();
    let bad = good.replace("b:clone(ref),a:\"other\"", "a:owner,b:clone(ref)");
    assert_eq!(compile(&bad, m()).unwrap_err().code, "E_LOAN");
    let bad = good.replace(
        "pair value=pair {b:clone(ref),a:\"other\"};auto moved=owner;exit done;",
        "forward accept(&owner,pair {b:\"other\",a:owner});",
    );
    assert_eq!(compile(&bad, m()).unwrap_err().code, "E_ALIAS");
}
#[test]
fn expansion_certificate_preserves_named_initializer_order_and_indices() {
    let p = compile(ORDER, m()).unwrap();
    let artifact = p.expansion_artifact("main").unwrap();
    let encoded = serde_json::to_string(&artifact).unwrap();
    let decoded = serde_json::from_str(&encoded).unwrap();
    p.accept_expansion(&decoded).unwrap();
    for mutation in 0..3 {
        let mut a = artifact.clone();
        let mut changed = false;
        for i in &mut a.region.code {
            // Expansion completes a callee by assigning its payload to caller slots.
            if let Action::Bind(_, Expr::Record(_, fields)) = &mut i.action {
                match mutation {
                    0 => fields.reverse(),
                    1 => fields[0].0 = fields[1].0,
                    _ => fields[0].0 = 99,
                }
                changed = true;
                break;
            }
        }
        if !changed {
            for i in &mut a.region.code {
                if let Action::Complete {
                    bindings: fields, ..
                } = &mut i.action
                {
                    for (_, e) in fields {
                        if let Expr::Record(_, fields) = e {
                            match mutation {
                                0 => fields.reverse(),
                                1 => fields[0].0 = fields[1].0,
                                _ => fields[0].0 = 99,
                            }
                            changed = true;
                            break;
                        }
                    }
                }
            }
        }
        assert!(changed);
        assert!(p.accept_expansion(&a).is_err());
    }
}
#[test]
fn native_named_fields_match_reference_before_and_after_expansion_at_o0_o2() {
    use std::{fs, process::Command};
    let p = compile(ORDER, m()).unwrap();
    let expected = serde_json::to_value(p.run(&scenario()).unwrap()).unwrap();
    let root = std::env::temp_dir().join(format!("path-named-fields-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let s = root.join("scenario.json");
    fs::write(&s, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (name, program) in [
        ("original", p.clone()),
        ("expanded", p.expanded("main").unwrap()),
    ] {
        for opt in [0, 2] {
            let bin = root.join(format!("{name}-{opt}"));
            program.build_native("main", &bin, opt).unwrap();
            let out = Command::new(bin)
                .arg("--scenario")
                .arg(&s)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap(),
                expected
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn runnable_example_and_tail_extraction_preserve_named_fields() {
    let p = compile(include_str!("../playground/named_fields.tellur"), m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    let source = "struct pair {string first;string second;}\ncontrol main() exits auto {\n string owner=\"Ada\";\n auto value=pair {second:clone(&owner),first:owner};\n exit done {value};\n}";
    let extracted = tellur::extract::extract_tail(source, m(), "main", 4, "finish").unwrap();
    assert_eq!(
        compile(source, m()).unwrap().run(&scenario()).unwrap(),
        compile(&extracted.source, m())
            .unwrap()
            .run(&scenario())
            .unwrap()
    );
}
