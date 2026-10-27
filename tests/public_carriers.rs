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
struct view['text] {ref mutable string text <- 'text;}
struct job['storage, 'text] {ref mutable view['text] target <- 'storage;}
control pack(ref mutable view input) exits ready {job[input, input['text]] value} {
    exit ready {job {input}};
}
control relay(job input) exits ready {job[input['storage], input['text]] value} {exit ready {input};}
control main() exits done {string text} {
    mutable string owner="before";mutable view saved=view {borrow mutable owner};
    delta pack(borrow mutable saved) {ready {auto packed}=>packed}
    [packed]:delta relay(packed) {ready {auto result}=>ready}
    [ready]:deref result.target.text="after";emit(result.target.text);drop(result);drop(saved);exit done {owner};
}"#;
#[test]
fn records_cross_owned_boundaries_with_both_origin_layers() {
    same(RECORD);
    rejects(
        &RECORD.replace(
            "deref result.target.text=",
            "drop(saved);deref result.target.text=",
        ),
        "E_LOAN",
    );
    rejects(
        &RECORD.replace(
            "deref result.target.text=",
            "owner=\"bad\";deref result.target.text=",
        ),
        "E_LOAN",
    );
}
#[test]
fn inferred_and_forwarded_results_preserve_layers() {
    same(
        &RECORD
            .replace(
                "exits ready {job[input, input['text]] value}",
                "exits ready...",
            )
            .replace(
                "exits ready {job[input['storage], input['text]] value}",
                "exits ready...",
            ),
    );
    let forwarded = RECORD
        .replace(
            "control main()",
            "control again(job input) exits ready... {forward relay(input);}\ncontrol main()",
        )
        .replace("relay(packed)", "again(packed)");
    same(&forwarded);
}
#[test]
fn shared_outer_authority_never_grants_exclusive_inner_access() {
    let s = RECORD
        .replace("ref mutable view", "ref view")
        .replace("pack(borrow mutable saved)", "pack(borrow saved)")
        .replace("deref result.target.text=\"after\";", "")
        .replace(
            "emit(result.target.text)",
            "emit(borrow deref result.target.text)",
        );
    same(&s);
    rejects(
        &s.replace(
            "emit(borrow deref result.target.text)",
            "deref result.target.text=\"bad\"",
        ),
        "E_MUTABILITY",
    );
}
#[test]
fn missing_nested_maps_and_local_storage_escape_are_rejected() {
    rejects(
        &RECORD.replace("view['text] target", "view target"),
        "E_ORIGIN",
    );
    rejects(
        &RECORD.replace(
            "job[input, input['text]] value",
            "job[input['text], input] value",
        ),
        "E_LIFETIME",
    );
    rejects(
        &RECORD.replace(
            "exit ready {job {input}};",
            "mutable view local=view {borrow mutable deref input.text};exit ready {job {borrow mutable local}};",
        ),
        "E_LIFETIME",
    );
}
#[test]
fn nested_origin_selection_and_borrowed_whole_inputs_keep_storage_guards() {
    let s = RECORD.replace("control main()", r#"control text(ref mutable job input) exits found {ref mutable string value from input['text]} {exit found {borrow mutable deref input.target.text};}
control main()"#).replace("[ready]:deref result.target.text=\"after\";emit(result.target.text);", "[ready]:delta text(borrow mutable result) {found {auto child}=>child}[child]:deref child=\"after\";emit(child);");
    let s = s
        .replace(
            "[ready]:delta text",
            "[ready]:mutable job working=result;delta text",
        )
        .replace(
            "text(borrow mutable result)",
            "text(borrow mutable working)",
        )
        .replace("drop(result);", "drop(working);");
    same(&s);
    rejects(
        &s.replace("deref child=", "drop(saved);deref child="),
        "E_LOAN",
    );
    rejects(
        &s.replace("deref child=", "drop(working);deref child="),
        "E_LOAN",
    );
}
const SUM: &str = r#"
struct view['text] {ref mutable string text <- 'text;}
variant packet['storage, 'text] {present {ref mutable view['text] target <- 'storage}, empty}
control pack(ref mutable view input, bool choose) exits ready {packet[input,input['text]] value} {
    exit delta choose {true=>ready {present {input}},false=>ready {empty {}}};
}
control relay(packet input) exits ready... {exit ready {input};}
control main() exits done {string text} {
    mutable string owner="before";mutable view saved=view {borrow mutable owner};
    delta pack(borrow mutable saved,true) {ready {auto packed}=>packed}
    [packed]:delta relay(packed) {ready {auto result}=>ready}
    [ready]:delta result {present {auto target}=>present,empty=>empty}
    [present]:deref target.text="after";emit(target.text);
    [entry]:drop(saved);exit done {owner};
}"#;
#[test]
fn sum_contracts_keep_each_layer_guarded_by_its_tag() {
    same(SUM);
    same(&SUM.replace("saved,true", "saved,false"));
    rejects(
        &SUM.replace("deref target.text=", "drop(saved);deref target.text="),
        "E_LOAN",
    );
    same(&SUM.replace("variant packet", "outcome packet"));
}
#[test]
fn option_reference_schemas_and_named_completion_contracts() {
    let s=SUM.replace("variant packet['storage, 'text] {present {ref mutable view['text] target <- 'storage}, empty}", "")
        .replace("packet[input,input['text]]", "option<ref mutable view>[input,input['text]]")
        .replace("packet input", "option<ref mutable view> input")
        .replace("present", "some").replace("empty", "none");
    same(&s);
    same(&s.replace("saved,true", "saved,false"));
    let s=RECORD.replace("control pack", "outcome answer['storage,'text] {ready {ref mutable view['text] target <- 'storage}}\ncontrol pack")
      .replace("exits ready {job[input, input['text]] value}", "exits answer[input,input['text]]")
      .replace("exit ready {job {input}};", "exit ready {input};")
      .replace("relay(packed)", "relay(local)").replace("[packed]:delta", "[packed]:job local=job {packed};delta");
    same(&s);
    same(&s.replace("exits answer[input,input['text]]", "exits answer"));
}
#[test]
fn three_reference_layers_survive_capture_and_owned_calls() {
    let s=RECORD.replace("control main()", r#"struct envelope['outer,'storage,'text] {ref mutable job['storage,'text] inner <- 'outer;}
control wrap(ref mutable job input) exits ready... {exit ready {envelope {input}};}
control pass(envelope input) exits ready... {exit ready {input};}
control main()"#).replace("[ready]:deref result.target.text=\"after\";emit(result.target.text);", "[ready]:mutable job working=result;auto pending=capture wrap(borrow mutable working);delta pending {ready {auto wrapped}=>wrapped}[wrapped]:delta pass(wrapped) {ready {auto delivered}=>delivered}[delivered]:deref delivered.inner.target.text=\"after\";emit(delivered.inner.target.text);drop(delivered);");
    let s = s.replace("drop(result);", "drop(working);");
    same(&s);
}
#[test]
fn borrowed_public_sum_inputs_and_whole_reference_results() {
    let s = SUM.replace("control main()", r#"
control lend(ref mutable packet input) exits ready... {exit ready {input};}
control touch(ref mutable packet input) exits done {
    delta input {present {auto target}=>present,empty=>empty}
    [present]:deref target.text="touched";[entry]:exit done;
}
control main()"#).replace("[ready]:delta result", "[ready]:mutable packet working=result;touch(borrow mutable working);delta lend(borrow mutable working) {ready {auto borrowed}=>borrowed}[borrowed]:delta borrowed")
      .replace("[entry]:drop(saved);", "[entry]:drop(working);drop(saved);");
    same(&s);
    same(&s.replace("saved,true", "saved,false"));
    rejects(
        &s.replace(
            "deref target.text=\"after\"",
            "drop(working);deref target.text=\"after\"",
        ),
        "E_LOAN",
    );
}
#[test]
fn groups_at_different_field_positions_do_not_conflate_storage_with_sources() {
    let s = RECORD
        .replace(
            "{ref mutable string text <- 'text;}",
            "{u32 count;ref mutable string text <- 'text;}",
        )
        .replace(
            "view {borrow mutable owner}",
            "view {7,borrow mutable owner}",
        )
        .replace(
            "{ref mutable view['text] target <- 'storage;}",
            "{u32 count;ref mutable view['text] target <- 'storage;}",
        )
        .replace("job {input}", "job {9,input}");
    same(&s);
    let s=s.replace("control main()", "control peek(ref mutable job input) exits found {ref mutable string value from input['text]} {exit found {borrow mutable deref input.target.text};}\ncontrol main()");
    same(&s);
}
#[test]
fn generic_nested_containers_are_checked_after_layout_substitution() {
    let s = RECORD
        .replace(
            "struct job['storage, 'text]",
            "struct job<t>['storage, 'text]",
        )
        .replace(
            "{ref mutable view['text] target",
            "{t metadata;ref mutable view['text] target",
        )
        .replace("job[input", "job<u32>[input")
        .replace("job input", "job<u32> input")
        .replace("job {input}", "job<u32> {7,input}");
    same(&s);
}
#[test]
fn shared_container_copies_and_detached_shared_fields_keep_independent_lifetimes() {
    let s=RECORD.replace("ref mutable", "ref").replace("borrow mutable saved", "borrow saved").replace("borrow mutable owner", "borrow owner")
      .replace("deref result.target.text=\"after\";emit(result.target.text);drop(result);", "job copy=result;ref string detached=copy.target.text;drop(result);drop(copy);drop(saved);emit(detached);")
      .replace("emit(detached);drop(saved);", "emit(detached);");
    same(&s);
    rejects(
        &s.replace("emit(detached);", "owner=\"bad\";emit(detached);"),
        "E_LOAN",
    );
}
#[test]
fn detached_exclusive_child_prevents_lending_its_parent_again() {
    let s=RECORD.replace("deref result.target.text=\"after\";emit(result.target.text);", "ref mutable string child=borrow mutable deref result.target.text;deref child=\"after\";emit(child);");
    same(&s);
    rejects(
        &s.replace(
            "deref child=",
            "ref mutable view duplicate=borrow mutable deref result.target;deref child=",
        ),
        "E_LOAN",
    );
    rejects(
        &s.replace("deref child=", "drop(result);deref child="),
        "E_LOAN",
    );
}
#[test]
fn named_outcome_and_option_group_selectors_relay_nested_dependencies() {
    let s=SUM.replace("variant packet['storage, 'text] {present {ref mutable view['text] target <- 'storage}, empty}", "")
        .replace("packet[input,input['text]]", "option<ref mutable view>[input,input['text]]")
        .replace("control relay(packet input) exits ready...", "control relay(option<ref mutable view> input) exits ready {option<ref mutable view>[input['source],input['referent_text]] value}")
        .replace("present", "some").replace("empty", "none");
    same(&s);
}
const NESTED_SUM: &str = r#"variant view['text] {found {ref mutable string text <- 'text},empty}
struct job['storage,'text] {ref mutable view['text] target <- 'storage;}
control pack(ref mutable view input) exits ready... {exit ready {job {input}};}
control relay(job input) exits ready... {exit ready {input};}
control main() exits done {string text} {mutable string owner="before";mutable view saved=found {borrow mutable owner};
delta pack(borrow mutable saved) {ready {auto packed}=>packed}[packed]:delta relay(packed) {ready {auto result}=>ready}
[ready]:delta result.target {found {auto text}=>found,empty=>empty}[found]:deref text="after";emit(text);[entry]:drop(result);drop(saved);exit done {owner};}"#;
#[test]
fn nested_variant_referents_do_not_read_inactive_payloads() {
    let s = NESTED_SUM;
    same(s);
    same(&s.replace("saved=found {borrow mutable owner}", "saved=empty {}"));
}
#[test]
fn native_nested_public_contracts_match_original_and_expanded_execution() {
    for (case, source) in [
        include_str!("fixtures/public_carriers.tellur").to_owned(),
        RECORD.to_owned(),
        SUM.to_owned(),
        SUM.replace("saved,true", "saved,false"),
        NESTED_SUM.to_owned(),
        NESTED_SUM.replace("saved=found {borrow mutable owner}", "saved=empty {}"),
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
            "tellur-public-carriers-{}-{case}",
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
                let binary = root.join(format!("public-{expanded}-{opt}"));
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
fn private_imported_carriers_expose_groups_without_exposing_fields() {
    let root = std::env::temp_dir().join(format!(
        "tellur-public-carrier-module-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("views.tellur"),
        r#"pub struct view['text] {ref mutable string text <- 'text;}
pub struct job['storage,'text] {ref mutable view['text] target <- 'storage;}
pub control make(ref mutable string input) exits ready... {exit ready {view {input}};}
pub control pack(ref mutable view input) exits ready... {exit ready {job {input}};}
pub control edit(job input) exits done {deref input.target.text="module";exit done;}
"#,
    )
    .unwrap();
    let source = r#"import "views.tellur" as v;control main() exits done {string text} {
mutable string owner="before";delta v.make(borrow mutable owner) {ready {auto view}=>view}
[view]:mutable v.view saved=view;delta v.pack(borrow mutable saved) {ready {auto job}=>job}
[job]:v.edit(job);drop(saved);exit done {owner};}"#;
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
        source.replace("v.edit(job);", "emit(job.target.text);v.edit(job);"),
    )
    .unwrap();
    assert_eq!(
        tellur::compile_file(root.join("main.tellur"), manifest())
            .unwrap_err()
            .diagnostic
            .code,
        "E_VISIBILITY"
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn unioned_nested_result_origins_retain_every_possible_source() {
    let s = r#"struct view['text] {ref string text <- 'text;}
struct job['storage,'text] {ref view['text] target <- 'storage;}
control choose(ref view a,ref view b,bool first) exits ready... {exit delta first {true=>ready {job {a}},false=>ready {job {b}}};}
control main() exits done {mutable string a="left";mutable string b="right";view av=view {borrow a};view bv=view {borrow b};
delta choose(borrow av,borrow bv,true) {ready {auto result}=>ready}[ready]:emit(result.target.text);drop(result);drop(av);drop(bv);exit done;}"#;
    same(s);
    same(&s.replace("bv,true", "bv,false"));
    for owner in ["a", "b"] {
        rejects(
            &s.replace(
                "emit(result.target.text);",
                &format!("{owner}=\"bad\";emit(result.target.text);"),
            ),
            "E_LOAN",
        );
    }
}
