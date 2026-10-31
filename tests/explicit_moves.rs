mod support;
use tellur::{compile, ir::Value, manifest::Manifest, runtime::Scenario};

fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn run(source: &str) -> tellur::runtime::Execution {
    let p = compile(source, manifest()).unwrap();
    p.verify().unwrap();
    let case = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    let r = p.run(&case).unwrap();
    assert_eq!(r, p.expanded("main").unwrap().run(&case).unwrap());
    r
}
fn rejects(source: &str, code: &str) {
    let e = compile(source, manifest()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}

const NESTED: &str = r#"
struct image {string text; bytes pixels;}
struct box<t> {t value;}
control relay<t>(t value) exits ready... {exit ready {move value};}
control main() exits auto {
    string text="pixels";
    delta relay<box<image>>(box<image>(image(move text, bytes "0102")));
    [ready]: exit done {move value.value};
}
"#;

#[test]
fn owned_temporaries_nest_and_generic_forwarding_does_not_clone() {
    let r = run(NESTED);
    assert_eq!(
        r.fields[0],
        Value::Construct {
            ty: "image".into(),
            name: "image".into(),
            fields: vec![Value::String("pixels".into()), Value::Bytes(vec![1, 2])]
        }
    );
    rejects(
        &NESTED.replace("[ready]: exit done", "[ready]: emit(text); exit done"),
        "E_MOVED",
    );
    let r = run(
        "control main() exits auto {u32 count=7;auto copied=move count;exit done {count,copied};}",
    );
    assert_eq!(r.fields, vec![Value::U32(7); 2]);
}

#[test]
fn explicit_moves_preserve_partial_ownership_and_loan_checks() {
    let s = r#"struct pair {string left;string right;}
control main() exits auto {pair p=pair {"left","right"};auto left=move p.left;exit done {move left,move p.right};}"#;
    assert_eq!(
        run(s).fields,
        vec![Value::String("left".into()), Value::String("right".into())]
    );
    rejects(&s.replace("move p.right", "move p"), "E_PARTIAL_MOVE");
    rejects(
        r#"control main() exits done {string s="s";auto r=borrow s;auto taken=move s;emit(r);exit done;}"#,
        "E_LOAN",
    );
    rejects(
        r#"control main() exits done {string s="s";auto r=borrow s;auto taken=move deref r;exit done;}"#,
        "E_BORROW_MOVE",
    );
    rejects(
        r#"control main() exits done {string s="s";auto c=clone(borrow move s);exit done;}"#,
        "E_MOVE",
    );
}

#[test]
fn moving_reference_carriers_preserves_contained_origins() {
    let s = r#"struct view['a] {ref string text <- 'a;}
control main() exits auto {
 string s="source";view v=view {borrow s};auto packed=some {move v};
 delta move packed {some {auto selected}=>ready,none=>exit empty}
 [ready]: string text=clone(selected.text);exit done {move s,move text};
}"#;
    assert_eq!(run(s).fields, vec![Value::String("source".into()); 2]);
    rejects(
        &s.replace("string text=clone", "auto taken=move s;string text=clone"),
        "E_LOAN",
    );
}

#[test]
fn moving_exclusive_reference_into_call_consumes_it_but_implicit_reborrow_does_not() {
    let s = r#"control write(ref mutable string text) exits done {deref text="changed";exit done;}
control main() exits auto {mutable string s="before";auto r=borrow mutable s;write(move r);exit done {move s};}"#;
    assert_eq!(run(s).fields, vec![Value::String("changed".into())]);
    rejects(
        &s.replace("exit done {move s};", "write(r);exit done {move s};"),
        "E_MOVED",
    );
    run(&s.replace("write(move r);", "write(r);write(r);"));
    run(&s.replace(
        "write(move r);",
        "auto transferred=move r;write(transferred);",
    ));
}

#[test]
fn moved_reference_arguments_can_return_their_loan() {
    let s = r#"control identity(ref mutable string text) exits ready {ref mutable string value from text} {exit ready {move text};}
control main() exits auto {mutable string s="before";auto r=borrow mutable s;
 delta identity(move r);[ready]:deref value="after";exit done {move s};}"#;
    assert_eq!(run(s).fields, vec![Value::String("after".into())]);
    rejects(
        &s.replace("deref value=", "emit(r);deref value="),
        "E_MOVED",
    );
}

#[test]
fn argument_aliasing_and_completion_obligations_survive_move_syntax() {
    rejects(
        r#"control both(ref string a,string b) exits done {exit done;}
control main() exits done {string s="s";both(borrow s,move s);exit done;}"#,
        "E_ALIAS",
    );
    rejects(
        "control main(handle h) exits done {drop(move h);exit done;}",
        "E_RESOURCE",
    );
    rejects(
        "control main(handle h) exits done {auto next=move h;exit done;}",
        "E_RESOURCE",
    );
}

const TEMPORARIES: &str = r#"
struct box<t> {t value;}
control display<t>(ref box<t> input) exits done {exit done;}
control show(ref string input) exits done {emit(input);exit done;}
control update(ref mutable string input) exits done {deref input="updated";emit(input);exit done;}
control main() exits done {
    show(borrow "temporary");
    update(borrow mutable "before");
    string owned="nested";
    display<string>(borrow box<string>(move owned));
    delta false {true=>unused,false=>used}
    [unused]: show(borrow "wrong path");exit done;
    [used]: exit done;
}
"#;

#[test]
fn borrowed_rvalues_have_call_duration_storage_and_selected_evaluation() {
    let r = run(TEMPORARIES);
    assert_eq!(
        r.events,
        vec![
            tellur::runtime::Event::Emit {
                message: "temporary".into()
            },
            tellur::runtime::Event::Emit {
                message: "updated".into()
            }
        ]
    );
    rejects(
        &TEMPORARIES.replace("delta false", "emit(owned);delta false"),
        "E_MOVED",
    );
    rejects(
        r#"control view(ref string input) exits ready {ref string value from input} {exit ready {input};}
control main() exits done {delta view(borrow "temporary");[ready]:emit(value);exit done;}"#,
        "E_TEMPORARY",
    );
    rejects(
        r#"control main() exits done {ref string value=borrow "temporary";emit(value);exit done;}"#,
        "E_LIFETIME",
    );
    rejects(
        r#"struct resource {handle value;}
control inspect(ref resource input) exits done {exit done;}
control main(handle h) exits done {inspect(borrow resource(move h));exit done;}"#,
        "E_TEMPORARY",
    );
}

#[test]
fn staging_temporary_arguments_preserves_left_to_right_ownership() {
    rejects(
        r#"struct boxed {string value;}
control both(string first,ref boxed second) exits done {exit done;}
control main() exits done {string s="s";both(move s,borrow boxed(move s));exit done;}"#,
        "E_ALIAS",
    );
    rejects(
        r#"struct boxed {string value;}
control both(ref boxed first,string second) exits done {exit done;}
control main() exits done {string s="s";both(borrow boxed(move s),move s);exit done;}"#,
        "E_ALIAS",
    );
    let s = r#"struct boxed {string value;}
control both(string first,ref boxed second) exits done {exit done;}
control main() exits done {string s="s";both(move s,borrow boxed(clone(borrow s)));exit done;}"#;
    rejects(s, "E_MOVED");
    run(
        r#"control both(ref string a,ref string b) exits done {exit done;}
control main() exits done {string s="s";auto r=borrow s;both(move r,r);emit(r);exit done;}"#,
    );
}

#[test]
fn native_moves_and_temporary_borrows_match_reference_before_and_after_expansion() {
    use std::{fs, process::Command};
    let root = std::env::temp_dir().join(format!("tellur-moves-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    for (name, source) in [("owned", NESTED), ("borrowed", TEMPORARIES)] {
        let p = compile(source, manifest()).unwrap();
        let case = Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![],
        };
        let expected = serde_json::to_value(p.run(&case).unwrap()).unwrap();
        for (opt, program) in [(0, p.clone()), (2, p.expanded("main").unwrap())] {
            let binary = root.join(format!("{name}-{opt}"));
            support::scenario_executable(&program)
                .build_native("main", &binary, opt)
                .unwrap();
            let input = root.join("scenario.json");
            fs::write(&input, serde_json::to_vec(&case).unwrap()).unwrap();
            let output = Command::new(&binary)
                .args(["--scenario", input.to_str().unwrap()])
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
