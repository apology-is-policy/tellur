use tellur::{compile, manifest::Manifest, runtime::Scenario};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn same(source: &str) -> tellur::CheckedProgram {
    let p = compile(source, manifest()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(
        p.run(&s).unwrap(),
        p.expanded("main").unwrap().run(&s).unwrap()
    );
    p
}
fn rejects(source: &str, code: &str) {
    let e = compile(source, manifest()).unwrap_err();
    assert_eq!(e.code, code, "{e}\n{source}");
}
const SOURCE: &str = r#"struct view['a] {ref mutable string text <- 'a;}
control lend(ref mutable view input) exits ready... {exit ready {input};}
control main() exits done {mutable string owner="before";mutable view saved=view {borrow mutable owner};
 auto pending=capture lend(borrow mutable saved);
 delta pending {ready {auto result}=>ready}
 [ready]:deref result.text="after";emit(result.text);drop(saved);owner="free";exit done;}
"#;
#[test]
fn captured_carrier_references_retain_both_layers_until_handled() {
    same(SOURCE);
    for action in [
        "owner=\"bad\";",
        "drop(saved);",
        "deref saved.text=\"bad\";",
    ] {
        rejects(
            &SOURCE.replace("delta pending", &format!("{action}delta pending")),
            "E_LOAN",
        );
    }
    rejects(
        &SOURCE.replace("deref result.text=", "drop(saved);deref result.text="),
        "E_LOAN",
    );
    rejects(&SOURCE.replace("delta pending {ready {auto result}=>ready}\n [ready]:deref result.text=\"after\";emit(result.text);drop(saved);owner=\"free\";",""),"E_OUTCOME");
}
#[test]
fn local_record_reference_fields_keep_contents_and_release_independently() {
    let source=SOURCE.replace("control main()","struct envelope {ref mutable view value;}\ncontrol main()")
 .replace("auto pending=capture lend(borrow mutable saved);\n delta pending {ready {auto result}=>ready}\n [ready]:", "envelope packet=envelope {borrow mutable saved};")
 .replace("result.text","packet.value.text");
    same(&source);
    rejects(
        &source.replace(
            "deref packet.value.text=",
            "owner=\"bad\";deref packet.value.text=",
        ),
        "E_LOAN",
    );
    rejects(
        &source.replace(
            "deref packet.value.text=",
            "drop(saved);deref packet.value.text=",
        ),
        "E_LOAN",
    );
}
#[test]
fn stored_adapters_and_nested_record_payloads_rebase_reference_addresses() {
    let source=SOURCE.replace("control main()","struct envelope {ref mutable view value;}\ncontrol main()")
 .replace("auto pending=capture lend(borrow mutable saved);","auto pending=delta lend(borrow mutable saved) {ready {auto lent}=>ready {envelope {lent}}};")
 .replace("result.text","result.value.text");
    same(&source);
    rejects(
        &source.replace("delta pending", "owner=\"bad\";delta pending"),
        "E_LOAN",
    );
}
#[test]
fn captured_sum_carriers_keep_tagged_referent_dependencies() {
    let source=SOURCE.replace("struct view['a] {ref mutable string text <- 'a;}\n","")
 .replace("ref mutable view input","ref mutable option<ref mutable string> input")
 .replace("mutable view saved=view {borrow mutable owner}","mutable option<ref mutable string> saved=some {borrow mutable owner}")
 .replace("[ready]:deref result.text=\"after\";emit(result.text);drop(saved);", "[ready]:delta result {some {auto text}=>present,none=>absent}[present]:deref text=\"after\";emit(text);[entry]:drop(saved);");
    same(&source);
    same(&source.replace("some {borrow mutable owner}", "none {}"));
    rejects(
        &source.replace("delta pending", "owner=\"bad\";delta pending"),
        "E_LOAN",
    );
    rejects(
        &source.replace("emit(text)", "drop(saved);emit(text)"),
        "E_LOAN",
    );
}
#[test]
fn shared_capture_copies_preserve_referent_sources_and_shared_authority() {
    let source=SOURCE.replace("control main()", "variant stored {ready {ref view value}}\ncontrol main()")
 .replace("ref mutable view input","ref view input")
 .replace("auto pending=capture lend(borrow mutable saved);", "stored pending=ready {borrow saved};")
 .replace("lend(borrow mutable saved)","lend(borrow saved)")
 .replace("delta pending", "auto second=pending;delta pending")
 .replace("deref result.text=\"after\";emit(result.text);", "emit(borrow deref result.text);delta second {ready {auto again}=>again}[again]:emit(borrow deref again.text);");
    same(&source);
    rejects(
        &source.replace(
            "emit(borrow deref result.text)",
            "deref result.text=\"bad\"",
        ),
        "E_MUTABILITY",
    );
    rejects(
        &source.replace("delta second", "drop(saved);delta second"),
        "E_LOAN",
    );
}
#[test]
fn nested_options_move_the_stored_reference_and_release_empty_branches() {
    let source=SOURCE.replace("auto pending=capture lend(borrow mutable saved);", "auto original=capture lend(borrow mutable saved);option<option<outcome<lend>>> packed=some {some {original}};\n delta packed {some {auto inner}=>outer,none=>absent}[outer]:delta inner {some {auto pending}=>inner,none=>empty}[inner]:")
 .replace("exit done;}","[entry]:exit done;}");
    same(&source);
}
#[test]
fn public_containers_of_carrier_references_remain_gated() {
    rejects(
        r#"struct view['a] {ref string text <- 'a;} struct envelope['a] {ref view value <- 'a;}
 control pass(envelope input) exits ready... {exit ready {input};}"#,
        "E_MANIFEST",
    );
}

#[test]
fn borrowed_stored_carrier_payloads_reborrow_both_layers() {
    let source=SOURCE.replace("auto pending=capture", "mutable auto pending=capture")
 .replace("delta pending {ready {auto result}=>ready}","delta borrow mutable pending {ready {auto result}=>ready}")
 .replace("drop(saved);owner=\"free\";", "delta pending {ready {auto handled}=>handled}[handled]:emit(handled.text);drop(saved);owner=\"free\";");
    same(&source);
    let shared = source
        .replace("delta borrow mutable pending", "delta borrow pending")
        .replace(
            "deref result.text=\"after\";emit(result.text);",
            "emit(borrow deref result.text);",
        );
    same(&shared);
    rejects(
        &shared.replace(
            "emit(borrow deref result.text)",
            "deref result.text=\"bad\"",
        ),
        "E_MUTABILITY",
    );
    rejects(
        &source.replace(
            "deref result.text=",
            "delta pending {ready {auto stolen}=>stolen}[stolen]:deref result.text=",
        ),
        "E_LOAN",
    );
}

#[test]
fn borrowed_record_payloads_preserve_reference_fields_with_borrowed_contents() {
    let source = r#"struct view['a] {ref mutable string text <- 'a;}
    struct job {ref mutable view target;}
    control main() exits done {mutable string owner="before";mutable view saved=view {borrow mutable owner};
    mutable option<job> queued=some {job {borrow mutable saved}};
    delta borrow mutable queued {some {auto work}=>some,none=>none}
    [some]:deref work.target.text="after";emit(work.target.text);
    [entry]:delta queued {some {auto final_job}=>present,none=>absent}
    [present]:emit(final_job.target.text);[entry]:drop(saved);owner="free";exit done;}"#;
    same(source);
    same(&source.replace("some {job {borrow mutable saved}}", "none {}"));
    rejects(
        &source.replace(
            "emit(work.target.text)",
            "drop(saved);emit(work.target.text)",
        ),
        "E_LOAN",
    );
}

#[test]
fn multiple_local_reference_layers_survive_adapter_scopes() {
    let source = r#"struct view['a] {ref mutable string text <- 'a;}
 struct inner {ref mutable view target;}
 struct outer {ref mutable inner target;}
 control main() exits done {mutable string owner="before";mutable view saved=view {borrow mutable owner};
 mutable inner middle=inner {borrow mutable saved};
 auto pending=delta true {true=>ready {outer {borrow mutable middle}},false=>empty {}};
 delta pending {ready {auto packet}=>ready,empty=>empty}
 [ready]:deref packet.target.target.text="after";emit(packet.target.target.text);
 [entry]:drop(middle);drop(saved);owner="free";exit done;}"#;
    same(source);
    rejects(
        &source.replace(
            "emit(packet.target.target.text)",
            "drop(middle);emit(packet.target.target.text)",
        ),
        "E_LOAN",
    );
    rejects(
        &source.replace(
            "emit(packet.target.target.text)",
            "owner=\"bad\";emit(packet.target.target.text)",
        ),
        "E_LOAN",
    );
}

#[test]
fn local_scope_escape_and_reference_to_reference_remain_rejected() {
    rejects(
        r#"struct view['a] {ref string text <- 'a;} struct holder {ref view value;}
 control make(ref string input) exits ready {view[input] value} {exit ready {view {input}};}
 control main() exits done {string owner="local";auto pending=delta make(borrow owner) {ready {auto carrier}=>ready {holder {borrow carrier}}};exit done;}"#,
        "E_LIFETIME",
    );
    rejects(
        r#"struct view['a] {ref string text <- 'a;} struct holder {ref view value;}
 control main() exits done {string owner="a";view saved=view {borrow owner};holder stored=holder {borrow saved};auto twice=borrow stored.value;exit done;}"#,
        "E_UNSUPPORTED",
    );
}

#[test]
fn branches_with_the_same_tag_union_contained_sources_after_unpacking() {
    let source = r#"struct view['a] {ref mutable string text <- 'a;}
 control main() exits done {mutable string a="a";mutable string b="b";
 mutable view left=view {borrow mutable a};mutable view right=view {borrow mutable b};
 auto pending=delta true {true=>ready {borrow mutable left},false=>ready {borrow mutable right}};
 delta pending {ready {auto result}=>ready}
 [ready]:deref result.text="edited";emit(result.text);drop(left);drop(right);a="free";b="free";exit done;}"#;
    same(source);
    same(&source.replace("delta true", "delta false"));
    for owner in ["a", "b"] {
        rejects(
            &source.replace(
                "deref result.text=",
                &format!("{owner}=\"bad\";deref result.text="),
            ),
            "E_LOAN",
        );
    }
}

#[test]
fn independent_shared_retainers_and_detached_fields_can_outlive_a_container() {
    let source = r#"struct view['a] {ref string text <- 'a;} struct holder {ref view value;}
 control main() exits done {mutable string owner="hello";view saved=view {borrow owner};
 holder first=holder {borrow saved};holder second=clone(borrow first);drop(first);
 ref string detached=second.value.text;drop(second);drop(saved);emit(detached);owner="free";exit done;}"#;
    same(source);
    rejects(
        &source.replace("ref string detached", "owner=\"bad\";ref string detached"),
        "E_LOAN",
    );
    rejects(
        &source.replace("emit(detached)", "owner=\"bad\";emit(detached)"),
        "E_LOAN",
    );
    let exclusive = source
        .replace("ref string text <-", "ref mutable string text <-")
        .replace("view {borrow owner}", "view {borrow mutable owner}")
        .replace(
            "detached=second.value.text",
            "detached=borrow deref second.value.text",
        );
    rejects(&exclusive, "E_LOAN");
}

#[test]
fn generic_local_wrappers_and_loop_local_captures_keep_lifetimes() {
    same(&SOURCE.replace("control main()", "struct envelope<t> {ref mutable t value;}\ncontrol main()")
 .replace("auto pending=capture lend(borrow mutable saved);", "auto pending=delta lend(borrow mutable saved) {ready {auto lent}=>ready {envelope<view> {lent}}};")
 .replace("result.text", "result.value.text"));
    same(
        r#"struct view['a] {ref string text <- 'a;}
 control lend(ref view input) exits ready... {exit ready {input};}
 control main() exits done {string owner="loop";view saved=view {borrow owner};
 loop repeat(bool again=true) {auto pending=capture lend(borrow saved);
 delta pending {ready {auto result}=>ready}[ready]:emit(result.text);
 delta again {true=>repeat,false=>finished}[repeat]:again repeat(false);[finished]:exit done;}}"#,
    );
}

#[test]
fn private_imported_carriers_can_be_captured_locally() {
    let root = std::env::temp_dir().join(format!(
        "tellur-stored-carrier-module-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("view.tellur"),
        r#"pub struct view['a] {ref string text <- 'a;}
 pub control make(ref string input) exits ready {view[input] value} {exit ready {view {input}};}
 pub control lend(ref view input) exits ready... {exit ready {input};}
 pub control display(ref view input) exits done {emit(input.text);exit done;}"#,
    )
    .unwrap();
    let source = r#"import "view.tellur" as v;
 control main() exits done {mutable string owner="module";
 delta v.make(borrow owner) {ready {auto saved}=>made}
 [made]:auto pending=capture v.lend(borrow saved);
 delta pending {ready {auto result}=>ready}[ready]:v.display(result);drop(saved);owner="free";exit done;}"#;
    std::fs::write(root.join("main.tellur"), source).unwrap();
    let p = tellur::compile_file(root.join("main.tellur"), manifest()).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(
        p.run(&scenario).unwrap(),
        p.expanded("main").unwrap().run(&scenario).unwrap()
    );
    std::fs::write(
        root.join("main.tellur"),
        source.replace("delta pending", "drop(saved);delta pending"),
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
fn expansion_certificates_reject_modified_stored_carrier_code() {
    let p = same(include_str!("fixtures/stored_carriers.tellur"));
    let original = p.expansion_artifact("main").unwrap();
    p.accept_expansion(&original).unwrap();
    let mut artifact = original;
    artifact.region.code.pop();
    assert_eq!(
        p.accept_expansion(&artifact).unwrap_err().code,
        "E_EXPANSION_CERTIFICATE"
    );
}

#[test]
fn native_stored_carriers_match_before_and_after_expansion() {
    let fixture = include_str!("fixtures/stored_carriers.tellur");
    let absent = fixture.replace("some { edit_job { borrow mutable saved } }", "none {}");
    let adapter = SOURCE
        .replace(
            "auto pending=capture lend(borrow mutable saved);",
            "auto pending=delta true {true=>ready {borrow mutable saved},false=>empty {}};",
        )
        .replace(
            "{ready {auto result}=>ready}",
            "{ready {auto result}=>ready,empty=>empty}",
        )
        .replace(
            "drop(saved);owner=\"free\";",
            "[entry]:drop(saved);owner=\"free\";",
        );
    for (case, source) in [SOURCE, fixture, absent.as_str(), adapter.as_str()]
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
            "tellur-stored-carriers-{}-{case}",
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
                let binary = root.join(format!("stored-{expanded}-{opt}"));
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
fn nested_shared_outer_borrows_never_grant_exclusive_field_access() {
    let source = r#"struct view['a] {ref mutable string text <- 'a;} struct holder {ref mutable view value;}
 control main() exits done {mutable string owner="hello";mutable view saved=view {borrow mutable owner};
 mutable option<holder> packet=some {holder {borrow mutable saved}};
 delta borrow packet {some {auto shared}=>some,none=>none}
 [some]:emit(borrow deref shared.value.text);[entry]:drop(packet);drop(saved);exit done;}"#;
    same(source);
    rejects(
        &source.replace(
            "emit(borrow deref shared.value.text)",
            "deref shared.value.text=\"bad\"",
        ),
        "E_MUTABILITY",
    );
    rejects(
        &source.replace(
            "emit(borrow deref shared.value.text)",
            "ref mutable string stolen=borrow mutable deref shared.value.text",
        ),
        "E_MUTABILITY",
    );
}

#[test]
fn inferred_public_results_cannot_bypass_nested_origin_contract_gates() {
    rejects(
        r#"struct view {ref string text;} control main() exits ready... {string owner="a";view saved=view {borrow owner};exit ready {borrow saved};}"#,
        "E_UNSUPPORTED",
    );
    rejects(
        r#"struct view['a] {ref string text <- 'a;} struct holder['storage] {ref view value <- 'storage;}
 control wrap(ref view input) exits ready... {exit ready {holder {input}};}"#,
        "E_MANIFEST",
    );
}

#[test]
fn captured_reference_to_named_outcome_preserves_both_handling_obligations() {
    let source = r#"outcome reply['a] {found {ref mutable string text <- 'a},empty}
 control lend(ref mutable reply input) exits ready... {exit ready {input};}
 control main() exits done {mutable string owner="before";mutable reply saved=found {borrow mutable owner};
 auto pending=capture lend(borrow mutable saved);
 delta pending {ready {auto result}=>ready}
 [ready]:delta result {found {auto text}=>found,empty=>empty}[found]:deref text="after";emit(text);
 [entry]:delta saved {found {auto final_text}=>handled,empty=>absent}[handled]:emit(final_text);[entry]:exit done;}"#;
    same(source);
    rejects(&source.replace("[entry]:delta saved {found {auto final_text}=>handled,empty=>absent}[handled]:emit(final_text);", ""),"E_OUTCOME");
}
