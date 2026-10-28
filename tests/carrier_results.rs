use tellur::{compile, manifest::Manifest, runtime::Scenario};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn same(source: &str) -> tellur::CheckedProgram {
    let p = compile(source, manifest()).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(
        p.run(&scenario).unwrap(),
        p.expanded("main").unwrap().run(&scenario).unwrap()
    );
    p
}
fn rejects(source: &str, code: &str) {
    let e = compile(source, manifest()).unwrap_err();
    assert_eq!(e.code, code, "{e}\n{source}");
}
const RECORD: &str = r#"
struct view['a] {ref mutable string text <- 'a;}
control lend(ref mutable view input) exits ready {ref mutable view value from input} {exit ready {input};}
control main() exits done {
 mutable string owner="before";mutable view saved=view {borrow mutable owner};
 delta lend(borrow mutable saved) {ready {auto result}=>ready}
 [ready]:deref result.text="after";emit(result.text);drop(saved);owner="released";exit done;
}"#;
#[test]
fn exclusive_record_reference_results_keep_storage_and_contained_loans() {
    same(RECORD);
    rejects(
        &RECORD.replace("deref result.text=", "owner=\"bad\";deref result.text="),
        "E_LOAN",
    );
    rejects(
        &RECORD.replace("deref result.text=", "drop(saved);deref result.text="),
        "E_LOAN",
    );
    same(&RECORD.replace(
        "ref mutable view value from input",
        "ref mutable view[input['a]] value from input",
    ));
}
#[test]
fn shared_record_results_restrict_exclusive_fields() {
    let s = RECORD
        .replace("ref mutable view", "ref view")
        .replace("lend(borrow mutable saved)", "lend(borrow saved)")
        .replace("deref result.text=\"after\";", "")
        .replace("emit(result.text)", "emit(borrow deref result.text)");
    same(&s);
    rejects(
        &s.replace(
            "emit(borrow deref result.text)",
            "deref result.text=\"bad\"",
        ),
        "E_MUTABILITY",
    );
    same(&s.replace(
        "ref view value from input",
        "ref view[input['a]] value from input",
    ));
}
const SUM: &str = r#"
control lend(ref mutable option<ref mutable string> input)
 exits ready {ref mutable option<ref mutable string> value from input} {exit ready {input};}
control main() exits done {
 mutable string owner="before";mutable option<ref mutable string> saved=some {borrow mutable owner};
 delta lend(borrow mutable saved) {ready {auto result}=>ready}
 [ready]:delta result {some {auto text}=>some,none=>none}
 [some]:deref text="after";emit(text);[entry]:drop(saved);owner="released";exit done;
}"#;
#[test]
fn borrowed_sum_results_preserve_their_tagged_origins() {
    same(SUM);
    same(&SUM.replace("some {borrow mutable owner}", "none {}"));
    rejects(
        &SUM.replace("deref text=", "drop(saved);deref text="),
        "E_LOAN",
    );
    rejects(
        &SUM.replace("deref text=", "owner=\"bad\";deref text="),
        "E_LOAN",
    );
}
#[test]
fn projected_carriers_and_forwarded_results_keep_both_contract_layers() {
    let s = r#"struct view['a] {ref mutable string text <- 'a;}
 struct envelope['a] {view['a] inner;}
 control project(ref mutable envelope input) exits ready {ref mutable view value from input} {exit ready {borrow mutable input.inner};}
 control relay(ref mutable envelope input) exits ready... {forward project(input);}
 control main() exits done {mutable string owner="before";mutable envelope saved=envelope {view {borrow mutable owner}};
 delta relay(borrow mutable saved) {ready {auto result}=>ready}
 [ready]:deref result.text="after";emit(result.text);drop(saved);exit done;}
 "#;
    same(s);
    rejects(
        &s.replace("emit(result.text)", "drop(saved);emit(result.text)"),
        "E_LOAN",
    );
}
#[test]
fn returns_from_selected_sum_payloads_keep_the_enclosing_tag_alive() {
    let s = r#"struct view['a] {ref mutable string text <- 'a;}
 variant wrapped['a] {found {view['a] fields},empty}
 control project(ref mutable wrapped input) exits found {ref mutable view value from input},empty {
   exit delta input {found {auto fields}=>found {fields},empty=>empty {}};
 }
 control main() exits done {mutable string owner="before";mutable wrapped saved=found {view {borrow mutable owner}};
 delta project(borrow mutable saved) {found {auto result}=>found,empty=>empty}
 [found]:deref result.text="after";emit(result.text);[entry]:drop(saved);exit done;}
 "#;
    same(s);
    same(&s.replace(
        "saved=found {view {borrow mutable owner}}",
        "saved=empty {}",
    ));
    rejects(
        &s.replace("emit(result.text)", "drop(saved);emit(result.text)"),
        "E_LOAN",
    );
}
#[test]
fn returned_aliases_and_detached_references_preserve_carrier_authority() {
    same(
        &RECORD
            .replace(
                "[ready]:deref result",
                "[ready]:ref mutable view alias=result;deref alias",
            )
            .replace("emit(result.text)", "emit(alias.text)"),
    );
    let shared = r#"struct view['a] {ref string text <- 'a;}
 control lend(ref view input) exits ready... {exit ready {input};}
 control main() exits done {mutable string owner="hello";view saved=view {borrow owner};
 delta lend(borrow saved) {ready {auto result}=>ready}
 [ready]:ref string detached=result.text;drop(saved);emit(detached);owner="released";exit done;}
 "#;
    same(shared);
    rejects(
        &shared.replace("emit(detached)", "owner=\"bad\";emit(detached)"),
        "E_LOAN",
    );
    let exclusive = shared
        .replace("ref string text <-", "ref mutable string text <-")
        .replace("view {borrow owner}", "view {borrow mutable owner}")
        .replace("detached=result.text", "detached=borrow deref result.text");
    rejects(&exclusive, "E_LOAN");
}
#[test]
fn result_origins_and_exclusive_aliases_are_checked() {
    let wrong = r#"struct view['a] {ref string text <- 'a;}
 control wrong(ref view input,ref view other) exits ready {ref view[other['a]] value from input} {exit ready {input};}"#;
    rejects(wrong, "E_LIFETIME");
    rejects(
        &wrong.replace(
            "view[other['a]] value from input",
            "view[input['a]] value from other",
        ),
        "E_LIFETIME",
    );
    rejects(
        r#"struct view['a] {ref mutable string text <- 'a;} control duplicate(ref mutable view input) exits ready... {exit ready {input,input};}"#,
        "E_LOAN",
    );
    rejects(
        r#"struct view['a] {ref string text <- 'a;} control escape(ref string input) exits ready... {view local=view {input};exit ready {borrow local};}"#,
        "E_LIFETIME",
    );
}
#[test]
fn inference_and_generic_record_results_preserve_contents() {
    same(&RECORD.replace(
        "exits ready {ref mutable view value from input}",
        "exits auto",
    ));
    same(
        r#"struct view<t>['a] {ref t value <- 'a;}
 control lend<t>(ref view<t> input) exits ready {ref view<t> value from input} {exit ready {input};}
 control main() exits done {string owner="generic";view<string> saved=view<string> {borrow owner};
 delta lend<string>(borrow saved) {ready {auto result}=>ready}
 [ready]:emit(result.value);drop(saved);exit done;}
 "#,
    );
}
#[test]
fn conditional_carrier_results_retain_every_possible_owner() {
    let source = r#"struct view['a] {ref string text <- 'a;}
 control choose(ref view left,ref view right,bool first) exits ready... {
   exit delta first {true=>ready {left},false=>ready {right}};
 }
 control main() exits done {mutable string a="a";mutable string b="b";
 view left=view {borrow a};view right=view {borrow b};
 delta choose(borrow left,borrow right,true) {ready {auto result}=>ready}
 [ready]:emit(result.text);drop(left);drop(right);a="free";b="free";exit done;}
 "#;
    same(source);
    same(&source.replace("borrow right,true)", "borrow right,false)"));
    for change in ["a=\"bad\";", "b=\"bad\";", "drop(left);", "drop(right);"] {
        rejects(
            &source.replace("emit(result.text)", &format!("{change}emit(result.text)")),
            "E_LOAN",
        );
    }
}
#[test]
fn result_children_freeze_the_carrier_until_their_last_use() {
    let s=RECORD.replace("[ready]:deref result.text=\"after\";emit(result.text);", "[ready]:ref mutable string child=borrow mutable deref result.text;deref child=\"after\";emit(child);");
    same(&s);
    rejects(
        &s.replace(
            "emit(child)",
            "auto other=capture lend(borrow mutable saved);emit(child)",
        ),
        "E_LOAN",
    );
    rejects(
        &s.replace("emit(child)", "drop(result);emit(child)"),
        "E_LOAN",
    );
}
#[test]
fn captured_carrier_references_can_be_handled_later() {
    let source=RECORD.replace("delta lend(borrow mutable saved) {ready {auto result}=>ready}", "auto pending=capture lend(borrow mutable saved);delta pending {ready {auto result}=>ready}");
    same(&source);
}

#[test]
fn carrier_result_certificates_keep_both_lifetime_layers() {
    for source in [RECORD, SUM] {
        let p = same(source);
        let original = p.expansion_artifact("lend").unwrap();
        p.accept_expansion(&original).unwrap();
        assert_eq!(original.region.stored_returns.len(), 1);
        for mutation in 0..3 {
            let mut artifact = original.clone();
            match mutation {
                0 => artifact.region.stored_returns.clear(),
                1 => artifact.region.borrow_returns.clear(),
                _ => artifact.region.stored_returns[0].sources[0].parameter = 99,
            }
            assert_eq!(
                p.accept_expansion(&artifact).unwrap_err().code,
                "E_EXPANSION_CERTIFICATE"
            );
        }
    }
}

#[test]
fn dropping_an_independent_shared_copy_does_not_release_a_borrowed_holder() {
    same(
        r#"struct view['a] {ref string text <- 'a;}
 control lend(ref view input) exits ready... {exit ready {input};}
 control main() exits done {string owner="hello";view saved=view {borrow owner};view copy=saved;
 delta lend(borrow saved) {ready {auto result}=>ready}
 [ready]:drop(copy);emit(result.text);drop(saved);exit done;}"#,
    );
}

#[test]
fn imported_carrier_results_preserve_private_contained_origins() {
    let root = std::env::temp_dir().join(format!(
        "tellur-carrier-result-module-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("view.tellur"), r#"pub struct view['a] {ref string text <- 'a;}
 pub control make(ref string input) exits ready {view[input] value} {exit ready {view {input}};}
 pub control lend(ref view input) exits ready {ref view[input['a]] value from input} {exit ready {input};}
 pub control display(ref view input) exits done {emit(input.text);exit done;}"#).unwrap();
    let source = r#"import "view.tellur" as v;
 control main() exits done {mutable string owner="module";
 delta v.make(borrow owner) {ready {auto saved}=>made}
 [made]:delta v.lend(borrow saved) {ready {auto result}=>ready}
 [ready]:v.display(result);drop(saved);owner="free";exit done;}"#;
    std::fs::write(root.join("main.tellur"), source).unwrap();
    let p = tellur::compile_file(root.join("main.tellur"), manifest()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(
        p.run(&s).unwrap(),
        p.expanded("main").unwrap().run(&s).unwrap()
    );
    std::fs::write(
        root.join("main.tellur"),
        source.replace("v.display(result)", "owner=\"bad\";v.display(result)"),
    )
    .unwrap();
    assert_eq!(
        tellur::compile_file(root.join("main.tellur"), manifest())
            .unwrap_err()
            .diagnostic
            .code,
        "E_LOAN"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_carrier_results_match_interpreter_and_expansion() {
    let absent = SUM.replace("some {borrow mutable owner}", "none {}");
    for (case, source) in [
        RECORD,
        SUM,
        absent.as_str(),
        include_str!("fixtures/carrier_results.tellur"),
    ]
    .iter()
    .enumerate()
    {
        let p = same(source);
        let expected = p
            .run(&Scenario {
                entry: "main".into(),
                arguments: vec![],
                calls: vec![],
            })
            .unwrap();
        let root = std::env::temp_dir().join(format!(
            "tellur-carrier-results-{}-{case}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        for expanded in [false, true] {
            let program = if expanded {
                p.expanded("main").unwrap()
            } else {
                p.clone()
            };
            for opt in [0, 2] {
                let binary = root.join(format!("result-{expanded}-{opt}"));
                program.build_native("main", &binary, opt).unwrap();
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
}

#[test]
fn returned_named_outcome_borrows_preserve_the_owners_handling_obligation() {
    let source = r#"outcome reply['a] {found {ref mutable string text <- 'a},empty}
 control lend(ref mutable reply input) exits ready {ref mutable reply[input['a]] value from input} {exit ready {input};}
 control main() exits done {mutable string owner="before";mutable reply saved=found {borrow mutable owner};
 delta lend(borrow mutable saved) {ready {auto result}=>ready}
 [ready]:delta result {found {auto text}=>found,empty=>empty}
 [found]:deref text="after";emit(text);
 [entry]:delta saved {found {auto final_text}=>handled,empty=>absent}
 [handled]:emit(final_text);[entry]:exit done;}
 "#;
    same(source);
    same(&source.replace("saved=found {borrow mutable owner}", "saved=empty {}"));
    rejects(&source.replace("[entry]:delta saved {found {auto final_text}=>handled,empty=>absent}\n [handled]:emit(final_text);",""),"E_OUTCOME");
}

#[test]
fn aggregate_trait_reference_results_accept_explicit_origins() {
    compile(
        r#"struct view['a] {ref string text <- 'a;}
 trait lender {control lend(ref view input) exits ready {ref view[input['a]] value from input};}
 struct impl_type {u32 marker;}
 implement lender for impl_type {control lend(ref view input) exits ready {ref view[input['a]] value from input} {exit ready {input};}}"#,
        manifest(),
    ).unwrap();
}
