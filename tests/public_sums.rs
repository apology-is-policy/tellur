use tellur::{compile, ir::Value, manifest::Manifest, runtime::Scenario};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
const DECL: &str = r#"
variant choice['a, 'b] {
    left { ref string text <- 'a },
    right { ref string text <- 'b },
    empty
}
control choose(ref string a, ref string b, bool flag)
    exits ready { choice[a, b] value }
{
    exit delta flag {
        true => ready { choice.left { a } },
        false => ready { choice.right { b } }
    };
}
"#;
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
#[test]
fn returned_variants_keep_tagged_origins_and_release_other_alternatives() {
    let source = format!(
        r#"{DECL}
control main() exits done {{ string a, string b }} {{
    mutable string a="a"; mutable string b="b";
    delta choose(borrow a, borrow b, true) {{ ready {{ auto selected }} => selected }}
    [selected]: delta selected {{left {{ref string left}}=>left, right {{ref string right}}=>right, empty=>empty}}
    [left]: b="other owner released"; emit(left);
    [right]: a="other owner released"; emit(right);
    [entry]: a="finished a"; b="finished b"; exit done {{a,b}};
}}"#
    );
    let p = compile(&source, manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    assert_eq!(
        expected.fields,
        vec![
            Value::String("finished a".into()),
            Value::String("finished b".into())
        ]
    );
    assert_eq!(
        expected,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    assert!(
        p.region("choose")
            .unwrap()
            .stored_returns
            .iter()
            .all(|r| r.sum_leaf.is_some())
    );
    let bad = source.replace(
        "b=\"other owner released\"; emit(left)",
        "a=\"too early\"; emit(left)",
    );
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
}
#[test]
fn owned_variant_inputs_return_selected_contained_origins() {
    let source = format!(
        r#"{DECL}
control take(choice input) exits found {{ref string text from input['a]}}, empty {{
    delta input {{left {{auto left}}=>left,right {{auto right}}=>right,empty=>empty}}
    [left]: exit found {{left}};
    [right | empty]: exit empty;
}}
control main() exits done {{
    mutable string a="a"; mutable string b="b";
    delta choose(borrow a,borrow b,true) {{ready {{auto v}}=>chosen}}
    [chosen]: delta take(v) {{found {{auto text}}=>found,empty=>empty}}
    [found]: b="released"; emit(text);
    [entry]: a="released"; exit done;
}}"#
    );
    let p = compile(&source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn absence_and_exclusive_payloads_cross_boundaries_without_inactive_projection() {
    for empty in [false, true] {
        let source = format!(
            r#"
variant editor['a] {{ready {{ref mutable string text <- 'a}},empty}}
control make(ref mutable string input) exits ready {{editor[input] value}} {{exit ready {{{}}};}}
control relay(editor input) exits ready {{editor[input['a]] value}} {{exit ready {{input}};}}
control main() exits done {{string result}} {{
    mutable string owner="before";
    delta make(borrow mutable owner) {{ready {{auto value}}=>made}}
    [made]: delta relay(value) {{ready {{auto second}}=>relayed}}
    [relayed]: delta second {{ready {{auto text}}=>present,empty=>absent}}
    [present]: deref text="changed";
    [entry]: exit done {{owner}};
}}"#,
            if empty {
                "editor.empty {}"
            } else {
                "editor.ready {input}"
            }
        );
        let p = compile(&source, manifest()).unwrap();
        let result = p.run(&scenario()).unwrap();
        assert_eq!(
            result.fields,
            vec![Value::String(
                if empty { "before" } else { "changed" }.into()
            )]
        );
        assert_eq!(
            result,
            p.expanded("main").unwrap().run(&scenario()).unwrap()
        );
    }
}

#[test]
fn nested_record_and_variant_boundaries_keep_independent_exclusive_permissions() {
    let source = include_str!("fixtures/public_variants.tellur");
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    assert_eq!(
        compile(
            &source.replace(
                "emit(fields.title);",
                "title=\"too early\";emit(fields.title);"
            ),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
    assert_eq!(
        compile(
            &source.replace(
                "deref fields.body =",
                "body=\"too early\";deref fields.body ="
            ),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
}

#[test]
fn schemas_and_declared_origins_reject_missing_invented_and_wrong_dependencies() {
    for source in [
        "variant bad['a] {some {ref string text},none}",
        "variant bad['a] {some {ref string text <- 'unknown},none}",
        "variant bad['a,'a] {some {ref string text <- 'a},none}",
        "variant bad['a] {some {string text <- 'a},none}",
        "variant bad {some {ref string text <- 'a},none}",
        "variant bad<t>['a] {some {ref t text <- 'missing},none}",
    ] {
        assert_eq!(
            compile(source, manifest()).unwrap_err().code,
            "E_ORIGIN",
            "{source}"
        );
    }
    assert_eq!(
        compile(&DECL.replace("choice[a, b]", "choice[b, a]"), manifest())
            .unwrap_err()
            .code,
        "E_LIFETIME"
    );
    assert_eq!(
        compile(&DECL.replace("choice[a, b]", "choice[a]"), manifest())
            .unwrap_err()
            .code,
        "E_ORIGIN"
    );
    let source = "variant view['a] {some {ref string text <- 'a},none} control invalid() exits ready... {string local=\"x\";exit ready {view.some {borrow local}};}";
    assert_eq!(compile(source, manifest()).unwrap_err().code, "E_LIFETIME");
}

#[test]
fn unproduced_alternatives_need_explicit_maps_and_grouped_borrows_are_valid() {
    let source = "variant view['a] {some {ref string text <- 'a},none} control absent(ref string input) exits ready... {exit ready {view.none {}};}";
    assert_eq!(compile(source, manifest()).unwrap_err().code, "E_ORIGIN");
    compile(
        &source.replace("exits ready...", "exits ready {view[input] result}"),
        manifest(),
    )
    .unwrap();
    let source = "variant view['a] {some {ref string text <- 'a},none} control invalid(ref view input) exits done {exit done;}";
    compile(source, manifest()).unwrap();
}

#[test]
fn generic_variant_schemas_nested_tags_capture_and_forwarding_compose() {
    let source = r#"
variant view<t>['a] {present {ref t value <- 'a},absent}
variant outer<t>['a] {wrapped {view<t>['a] value},empty}
control make<t>(ref t input) exits ready {outer<t>[input] value} {exit ready {outer<t>.wrapped {view<t>.present {input}}};}
control relay<t>(ref t input) exits ready... {forward make<t>(input);}
control main() exits done {
    mutable string owner="value";
    auto saved=capture relay<string>(borrow owner);
    delta saved {ready {auto outside}=>ready}
    [ready]: delta outside {wrapped {auto inside}=>wrapped,empty=>empty}
    [wrapped]: delta inside {present {auto value}=>present,absent=>absent}
    [present]: emit(value);
    [entry]: owner="released";exit done;
}"#;
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    assert_eq!(
        compile(
            &source.replace("emit(value)", "owner=\"too early\";emit(value)"),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
}

#[test]
fn exclusive_payloads_cannot_be_duplicated_or_escape_a_completed_adapter_arm() {
    let source = r#"variant edit['a] {ready {ref mutable string first <- 'a,ref mutable string second <- 'a},empty}
control invalid(ref mutable string input) exits ready {edit[input] result} {exit ready {edit.ready {input,input}};}"#;
    assert!(compile(source, manifest()).is_err());
    let source = r#"variant view['a] {ready {ref string text <- 'a},empty}
control own() exits ready {string text} {exit ready {"temporary"};}
control main() exits done {auto saved=delta own() {ready {string text}=>stored {view.ready {borrow text}}};exit done;}"#;
    assert_eq!(compile(source, manifest()).unwrap_err().code, "E_LIFETIME");
}

#[test]
fn empty_known_input_does_not_invent_retained_references() {
    let source = r#"variant view['a] {present {ref string text <- 'a},absent}
control take(view input) exits found {ref string value from input['a]},empty {
    exit delta input {present {auto value}=>found {value},absent=>empty {}};
}
control main() exits done {view input=view.absent {};delta take(input) {found {auto value}=>found,empty=>empty}[found]:emit(value);[entry]:exit done;}"#;
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}

#[test]
fn native_public_variant_boundaries_match_both_branches_and_expansion() {
    let root = std::env::temp_dir().join(format!("path-public-variants-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for present in [true, false] {
        let source = include_str!("fixtures/public_variants.tellur")
            .replace("body, true)", &format!("body, {present})"));
        let p = compile(&source, manifest()).unwrap();
        let expected = p.run(&scenario()).unwrap();
        for expanded in [false, true] {
            let p = if expanded {
                p.expanded("main").unwrap()
            } else {
                p.clone()
            };
            for opt in [0, 2] {
                let binary = root.join(format!("variant-{present}-{expanded}-{opt}"));
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
                let actual: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(actual, serde_json::to_value(&expected).unwrap());
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn surviving_shared_copies_and_nested_completion_obligations_are_retained() {
    let source = r#"variant view['a] {found {ref string text <- 'a},empty}
control make(ref string input) exits ready {view[input] value} {exit ready {view.found {input}};}
control main() exits done {mutable string owner="value";
 delta make(borrow owner) {ready {auto first}=>made}
 [made]:view second=first;drop(first);owner="too early";
 delta second {found {auto text}=>found,empty=>empty}
 [found]:emit(text);[entry]:exit done;}"#;
    assert_eq!(compile(source, manifest()).unwrap_err().code, "E_LOAN");
    compile(&source.replace("owner=\"too early\";", ""), manifest()).unwrap();
    let source = r#"outcome receipt {done}
variant work['a] {pending {ref string text <- 'a,receipt receipt},empty}
control relay(work input) exits ready... {exit ready {input};}
control main() exits done {string owner="value";work value=work.pending {borrow owner,receipt.done {}};
 delta relay(value) {ready {auto result}=>relayed}
 [relayed]:delta result {pending {auto text,auto receipt}=>pending,empty=>empty}
 [pending]:emit(text);delta receipt {done=>handled}
 [entry]:exit done;}"#;
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    assert_eq!(
        compile(
            &source.replace("delta receipt {done=>handled}", ""),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_OUTCOME"
    );
}

#[test]
fn returned_variant_reborrows_keep_authorizing_carrier_storage_alive() {
    let source = r#"
struct editor['a] {ref mutable string text <- 'a;}
variant view['a] {found {ref mutable string text <- 'a},empty}
control child(ref mutable editor input) exits ready {view[input['a]] value} {
    exit ready {view.found {borrow mutable deref input.text}};
}
control main() exits done {
    mutable string owner="value";mutable editor parent=editor {borrow mutable owner};
    delta child(borrow mutable parent) {ready {auto saved}=>ready}
    [ready]:delta saved {found {auto text}=>found,empty=>empty}
    [found]:deref text="child";deref parent.text="parent";
    [entry]:emit(parent.text);exit done;
}"#;
    for source in [
        source.to_owned(),
        source.replace("exits ready {view[input['a]] value}", "exits ready..."),
    ] {
        let p = compile(&source, manifest()).unwrap();
        assert_eq!(
            p.run(&scenario()).unwrap(),
            p.expanded("main").unwrap().run(&scenario()).unwrap()
        );
        assert!(
            p.region("child").unwrap().stored_returns[0]
                .sources
                .iter()
                .any(|s| s.guard)
        );
        assert_eq!(
            compile(
                &source.replace("delta saved", "drop(parent);delta saved"),
                manifest()
            )
            .unwrap_err()
            .code,
            "E_LOAN"
        );
    }
}
