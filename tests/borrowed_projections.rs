mod support;
use tellur::{
    compile,
    manifest::Manifest,
    runtime::{Event, Scenario},
};
fn checked(source: &str) -> tellur::CheckedProgram {
    compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_or_else(|e| panic!("{e}\n{source}"))
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
fn messages(source: &str) -> Vec<String> {
    let p = checked(source);
    let report = p.run(&scenario()).unwrap();
    assert_eq!(
        report,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    report
        .events
        .into_iter()
        .filter_map(|e| match e {
            Event::Emit { message } => Some(message),
            _ => None,
        })
        .collect()
}
fn rejects(source: &str, code: &str) {
    let e = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(e.code, code, "{e}");
}

const SHARED: &str = r#"
struct view['a] {
    ref string text <- 'a;
    constructor(ref string input)[input] {emit("made");self.text=input;}
}
control main() exits done {
    mutable string owner="owner";
    auto first=view(borrow owner).text;
    emit(first);
    ref auto second=move view(borrow owner).text;
    string copy=clone(view(borrow owner).text);
    emit(second);
    emit(move copy);
    owner="released";
    emit(view {borrow owner}.text);
    exit done;
}
"#;
#[test]
fn shared_fields_bind_clone_and_emit_without_repeating_construction() {
    assert_eq!(
        messages(SHARED),
        [
            "made", "owner", "made", "made", "owner", "owner", "released"
        ]
    );
    rejects(
        &SHARED.replace("emit(first);", "owner=\"bad\";emit(first);"),
        "E_LOAN",
    );
    rejects(
        &SHARED.replace("ref auto second", "ref mutable auto second"),
        "E_TYPE",
    );
    rejects(
        &SHARED.replace(
            "auto first=view(borrow owner).text;",
            "auto first=borrow view(borrow owner);",
        ),
        "E_LIFETIME",
    );
}

const NESTED: &str = r#"
struct view<t>['a] {ref t text <- 'a;constructor(ref t input)[input]{self.text=input;}}
struct packet['a] {
    view<string>['a] inner;
    option<view<string>>['a] optional;
    string owned;
    constructor(ref string input)[input] {
        self.inner=view<string>(input);
        self.optional=some {view<string>(input)};
        self.owned="owned";
    }
}
control relay(ref string input) exits found {ref string text from input} {
    exit found {packet(input).inner.text};
}
control main() exits done {
    mutable string owner="nested";
    auto inner=packet(borrow owner).inner;
    emit(inner.text);
    emit(packet(borrow owner).owned);
    delta packet(borrow owner).optional {some {auto selected}=>some,none=>none}
    [some]: emit(selected.text);
    [entry]: auto pending=capture relay(packet(borrow owner).inner.text);
    delta pending {found {auto text}=>found}
    [found]: emit(text);
    owner="released";
    exit done;
}
"#;
#[test]
fn nested_generic_records_options_and_returned_borrows_keep_origins() {
    assert_eq!(messages(NESTED), ["nested", "owned", "nested", "nested"]);
    rejects(
        &NESTED.replace("emit(text);", "owner=\"bad\";emit(text);"),
        "E_LOAN",
    );
    rejects(
        &NESTED.replace(
            "packet(input).inner.text",
            "packet(borrow \"temporary\").inner.text",
        ),
        "E_TEMPORARY",
    );
    rejects(
        &NESTED.replace("packet(borrow owner).owned", "packet(borrow owner).absent"),
        "E_FIELD",
    );
}

const EXCLUSIVE: &str = r#"
struct edit['a] {ref mutable string text <- 'a;constructor(ref mutable string input)[input]{self.text=input;}}
control relay(ref mutable string input) exits found {ref mutable string text from input} {
    exit found {edit(input).text};
}
control main() exits done {
    mutable string owner="before";
    ref mutable auto first=edit(borrow mutable owner).text;
    deref first="one";
    emit(first);
    deref edit(borrow mutable owner).text="two";
    emit(borrow owner);
    delta relay(edit(borrow mutable owner).text) {found {auto text}=>found}
    [found]: deref text="three";
    emit(text);
    owner="released";
    exit done;
}
"#;
#[test]
fn exclusive_projections_transfer_and_can_access_the_external_pointee() {
    assert_eq!(messages(EXCLUSIVE), ["one", "two", "three"]);
    rejects(
        &EXCLUSIVE.replace("emit(first);", "owner=\"bad\";emit(first);"),
        "E_LOAN",
    );
    rejects(
        &EXCLUSIVE.replace("borrow mutable owner", "borrow owner"),
        "E_BORROW",
    );
}

const CARRIER: &str = r#"
struct view['a] {ref mutable string text <- 'a;}
struct link['storage,'a] {
    ref mutable view['a] value <- 'storage;
    constructor(ref mutable view input)[input,input['a]] {self.value=input;}
}
control main() exits done {
    mutable string owner="before";
    mutable view saved=view {borrow mutable owner};
    auto reference=link(borrow mutable saved).value;
    deref reference.text="carrier";
    emit(reference.text);
    drop(saved);
    owner="released";
    exit done;
}
"#;
#[test]
fn projected_carrier_references_keep_storage_and_contained_permissions() {
    assert_eq!(messages(CARRIER), ["carrier"]);
    rejects(
        &CARRIER.replace("emit(reference.text);", "drop(saved);emit(reference.text);"),
        "E_LOAN",
    );
    rejects(
        &CARRIER.replace(
            "emit(reference.text);",
            "owner=\"bad\";emit(reference.text);",
        ),
        "E_LOAN",
    );
}

#[test]
fn selection_preserves_outcome_obligations_at_every_owned_level() {
    let source = r#"
outcome completion {done}
struct packet['a] {
    completion pending;
    ref string text <- 'a;
    constructor(ref string input)[input]{self.pending=done {};self.text=input;}
}
control main() exits done {
    mutable string owner="owner";
    delta packet(borrow owner).pending {done=>done}
    [done]: owner="released";
    exit done;
}
"#;
    assert_eq!(messages(source), Vec::<String>::new());
    rejects(
        &source.replace(
            "delta packet(borrow owner).pending {done=>done}\n    [done]:",
            "auto text=packet(borrow owner).text;emit(text);",
        ),
        "E_TEMPORARY",
    );
    let nested=source.replace("control main()", "struct outer['a] {packet['a] inner;constructor(ref string input)[input]{self.inner=packet(input);}}control main()")
        .replace("packet(borrow owner).pending","outer(borrow owner).inner.pending");
    checked(&nested);
    rejects(
        &nested.replace(
            "delta outer(borrow owner).inner.pending {done=>done}\n    [done]:",
            "auto text=outer(borrow owner).inner.text;emit(text);",
        ),
        "E_TEMPORARY",
    );
}

#[test]
fn projections_preserve_evaluation_order_and_selected_arm_execution() {
    let source = r#"
struct marked['a] {
    ref string text <- 'a;
    string owned;
    constructor(ref string input,string label)[input] {emit(borrow label);self.text=input;self.owned=move label;}
}
control both(ref string left,ref string right) exits done {emit(left);emit(right);exit done;}
control choose(ref string input,bool yes) exits ready {ref string text from input},missing {
    exit delta yes {true=>ready {marked(input,"selected").text},false=>missing {}};
}
control main() exits done {
    string owner="owner";
    both(marked(borrow owner,"first").text,marked(borrow owner,"second").text);
    delta choose(borrow owner,false) {ready {auto text}=>unexpected,missing=>missing}
    [unexpected]: emit(text);
    [missing]: auto pending=capture choose(borrow owner,true);
    delta pending {ready {auto selected}=>selected,missing=>absent}
    [selected]: emit(selected);
    [entry]: exit done;
}
"#;
    assert_eq!(
        messages(source),
        ["first", "second", "owner", "owner", "selected", "owner"]
    );
    let nested = source.replace(
        "both(marked(borrow owner,\"first\").text,marked(borrow owner,\"second\").text);",
        "emit(concat(marked(borrow owner,\"first\").owned,marked(borrow owner,\"second\").owned));",
    );
    assert_eq!(
        messages(&nested),
        ["first", "second", "firstsecond", "selected", "owner"]
    );
}

#[test]
fn projection_does_not_create_a_lifetime_for_owned_temporary_storage() {
    let source = "struct mixed['a] {ref string text <- 'a;string owned;constructor(ref string input)[input]{self.text=input;self.owned=\"owned\";}} control main() exits done {string owner=\"owner\";auto reference=borrow mixed(borrow owner).owned;emit(reference);exit done;}";
    rejects(source, "E_LIFETIME");
    assert_eq!(
        messages(&source.replace(
            "auto reference=borrow mixed(borrow owner).owned;emit(reference);",
            "auto owned=mixed(borrow owner).owned;emit(borrow owned);"
        )),
        ["owned"]
    );
}

#[test]
fn fresh_borrowed_projections_match_native_and_expanded_execution() {
    use std::{fs, process::Command};
    let root = std::env::temp_dir().join(format!(
        "tellur-borrowed-projections-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    let absent = NESTED.replace("some {view<string>(input)}", "none {}");
    assert_ne!(absent, NESTED);
    for (name, source) in [
        ("absent", absent.as_str()),
        ("shared", SHARED),
        ("nested", NESTED),
        ("exclusive", EXCLUSIVE),
        ("carrier", CARRIER),
    ] {
        let original = checked(source);
        for expanded in [false, true] {
            let p = if expanded {
                original.expanded("main").unwrap()
            } else {
                original.clone()
            };
            for opt in [0, 2] {
                let binary = root.join(format!("{name}-{expanded}-{opt}"));
                support::scenario_executable(&p)
                    .build_native("main", &binary, opt)
                    .unwrap();
                let output = Command::new(binary)
                    .arg("--scenario")
                    .arg(&input)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
                    serde_json::to_value(p.run(&scenario()).unwrap()).unwrap()
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unselected_references_release_independently_and_projected_collection_refs_work() {
    let source = r#"
struct pair['a,'b] {
    ref mutable string left <- 'a;
    ref mutable string right <- 'b;
    constructor(ref mutable string left,ref mutable string right)[left,right] {self.left=left;self.right=right;}
}
struct list_view['a] {ref vector<string> values <- 'a;constructor(ref vector<string> input)[input]{self.values=input;}}
control main() exits done {
    mutable string left="left";
    mutable string right="right";
    auto first=pair(borrow mutable left,borrow mutable right).left;
    right="released right";
    deref first="edited";
    emit(first);
    left="released left";
    vector<string> items=["one","two"];
    emit(u32_text(vector_length(list_view(borrow items).values)));
    exit done;
}
"#;
    assert_eq!(messages(source), ["edited", "2"]);
    rejects(
        &source.replace("right=\"released right\";", "left=\"bad\";"),
        "E_LOAN",
    );
}
