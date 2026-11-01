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
    .unwrap()
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
    let result = p.run(&scenario()).unwrap();
    assert_eq!(
        result,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    result
        .events
        .into_iter()
        .filter_map(|e| match e {
            Event::Emit { message } => Some(message),
            _ => None,
        })
        .collect()
}
fn rejects(source: &str, code: &str) {
    let error = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(error.code, code, "{error}");
}
const PAIR: &str = r#"
struct text_pair['a, 'b] {
    ref string left <- 'a;
    ref string right <- 'b;
    constructor(ref string left, ref string right)[left, right] {
        self.left = left;
        self.right = right;
    }
}
control main() exits done {
    mutable string left = "left";
    mutable string right = "right";
    auto pair = text_pair(borrow left, borrow right);
    emit(pair.left);
    left = "released left";
    emit(pair.right);
    right = "released right";
    exit done;
}
"#;
#[test]
fn shared_constructor_results_keep_independent_field_origins() {
    assert_eq!(messages(PAIR), ["left", "right"]);
    assert_eq!(
        messages(&PAIR.replace("= text_pair(", "= move text_pair(")),
        ["left", "right"]
    );
    rejects(
        &PAIR.replace("emit(pair.left);", "left = \"bad\"; emit(pair.left);"),
        "E_LOAN",
    );
    rejects(
        &PAIR.replace("[left, right]", "[right, left]"),
        "E_LIFETIME",
    );
    rejects(&PAIR.replace("[left, right]", "[left]"), "E_ORIGIN");
    rejects(&PAIR.replace("[left, right]", ""), "E_ORIGIN");
    rejects(
        &PAIR.replace("[left, right]", "[absent, right]"),
        "E_ORIGIN",
    );
    let copied = PAIR
        .replace("emit(pair.left);", "auto retained = pair; emit(pair.left);")
        .replace(
            "left = \"released left\";",
            "left = \"released left\"; emit(retained.left);",
        );
    rejects(&copied, "E_LOAN");
    assert_eq!(
        messages(&copied.replace("left = \"released left\";", "")),
        ["left", "left", "right"]
    );
}

#[test]
fn reference_field_initialization_is_path_sensitive_and_cannot_escape_locals() {
    let source = r#"
struct view['a] {
    ref string text <- 'a;
    constructor(ref string input, bool first)[input] {
        delta first {true => first, false => second}
        [first]: self.text = input;
        [second]: self.text = input;
    }
}
control main() exits done {
    string owner = "shared";
    auto a = view(borrow owner, true);
    auto b = view(borrow owner, false);
    emit(a.text);
    emit(b.text);
    exit done;
}
"#;
    assert_eq!(messages(source), ["shared", "shared"]);
    rejects(
        &source.replace("[second]: self.text = input;", ""),
        "E_UNAVAILABLE",
    );
    rejects(
        &source.replace(
            "[second]: self.text = input;",
            "[entry]: self.text = input;",
        ),
        "E_REDEFINE",
    );
    rejects(
        &source.replace(
            "[first]: self.text = input;",
            "[first]: emit(self.text); self.text = input;",
        ),
        "E_UNAVAILABLE",
    );
    rejects(
        &source.replace(
            "self.text = input;",
            "string local = \"local\"; self.text = borrow local;",
        ),
        "E_LIFETIME",
    );
}

#[test]
fn exclusive_constructor_results_transfer_permissions() {
    let source = PAIR
        .replace("ref string", "ref mutable string")
        .replace("borrow left", "borrow mutable left")
        .replace("borrow right", "borrow mutable right")
        .replace(
            "emit(pair.left);",
            "deref pair.left = \"edited\"; emit(pair.left);",
        );
    assert_eq!(messages(&source), ["edited", "right"]);
    rejects(
        &source.replace("borrow mutable right)", "borrow mutable left)"),
        "E_ALIAS",
    );
}

const NESTED: &str = r#"
struct view['a] {
    ref string text <- 'a;
    constructor(ref string input)[input] { self.text = input; }
}
struct wrapper['a] {
    view['a] inner;
    constructor(ref string input)[input] { self.inner = view(input); }
}
struct borrowed_view['storage, 'text] {
    ref view['text] value <- 'storage;
    constructor(ref view input, bool first)[input, input['a]] {
        delta first { true => first, false => second }
        [first]: self.value = input;
        [second]: self.value = input;
        [entry]: emit(self.value.text);
    }
}
control unwrap(view input) exits ready { view[input['a]] value } {
    exit ready { input };
}
control main() exits done {
    mutable string owner = "nested";
    auto wrapped = wrapper(borrow owner);
    auto a = borrowed_view(borrow wrapped.inner, true);
    auto b = borrowed_view(borrow wrapped.inner, false);
    emit(a.value.text);
    emit(b.value.text);
    drop(a);
    drop(b);
    drop(wrapped);
    delta unwrap(view(borrow owner)) { ready { auto value } => ready }
    [ready]: emit(value.text);
    owner = "released";
    exit done;
}
"#;
#[test]
fn nested_construction_carrier_references_and_contained_arguments() {
    assert_eq!(messages(NESTED), ["nested"; 5]);
    rejects(
        &NESTED.replace("emit(a.value.text);", "drop(wrapped); emit(a.value.text);"),
        "E_LOAN",
    );
    rejects(
        &NESTED.replace(
            "emit(a.value.text);",
            "owner = \"bad\"; emit(a.value.text);",
        ),
        "E_LOAN",
    );
}

const OPTIONAL: &str = r#"
struct optional['a] {
    option<ref string>['a] value;
    constructor(ref string input, bool present)[input] {
        delta present { true => present, false => absent }
        [present]: self.value = some { input };
        [absent]: self.value = none {};
    }
}
control show(optional input) exits done {
    delta input.value { some { ref string text } => present, none => absent }
    [present]: emit(text);
    [entry]: exit done;
}
control main() exits done {
    mutable string owner = "present";
    show(optional(borrow owner, true));
    show(optional(borrow owner, false));
    owner = "released";
    exit done;
}
"#;
#[test]
fn optional_reference_fields_never_read_inactive_payloads() {
    assert_eq!(messages(OPTIONAL), ["present"]);
}

const OPTIONAL_CARRIER: &str = r#"
struct view['a] { ref string text <- 'a; }
struct optional_view['storage, 'text] {
    option<ref view>['storage, 'text] value;
    constructor(ref view input, bool present)[input, input['a]] {
        delta present { true => present, false => absent }
        [present]: self.value = some { input };
        [absent]: self.value = none {};
    }
}
control show(optional_view input) exits done {
    delta input.value { some { auto view } => present, none => absent }
    [present]: emit(view.text);
    [entry]: exit done;
}
control main() exits done {
    mutable string owner = "nested optional";
    view saved = view { borrow owner };
    show(optional_view(borrow saved, false));
    show(optional_view(borrow saved, true));
    drop(saved);
    owner = "released";
    exit done;
}
"#;
#[test]
fn optional_carriers_preserve_nested_origins_without_inactive_reads() {
    assert_eq!(messages(OPTIONAL_CARRIER), ["nested optional"]);
}

const EXCLUSIVE_CARRIER: &str = r#"
struct view['a] { ref mutable string text <- 'a; }
struct edit['storage, 'text] {
    ref mutable view['text] value <- 'storage;
    constructor(ref mutable view input)[input, input['a]] {
        self.value = input;
        deref self.value.text = "during construction";
    }
}
control main() exits done {
    mutable string owner = "before";
    mutable view saved = view { borrow mutable owner };
    auto editor = edit(borrow mutable saved);
    emit(editor.value.text);
    deref editor.value.text = "after construction";
    emit(editor.value.text);
    drop(editor);
    drop(saved);
    owner = "released";
    exit done;
}
"#;
#[test]
fn exclusive_carrier_fields_keep_storage_and_contained_permissions() {
    assert_eq!(
        messages(EXCLUSIVE_CARRIER),
        ["during construction", "after construction"]
    );
    rejects(
        &EXCLUSIVE_CARRIER.replace(
            "emit(editor.value.text);",
            "drop(saved);emit(editor.value.text);",
        ),
        "E_LOAN",
    );
}

#[test]
fn scopes_and_projected_exclusive_inputs_preserve_initialization_ownership() {
    let source = r#"
struct owners { string left; string right; }
struct split['a, 'b] {
    ref mutable string left <- 'a;
    ref mutable string right <- 'b;
    constructor(ref mutable owners input, bool first)[input, input] {
        delta first {true => first, false => second}
        [first]: { auto alias = borrow mutable input.left; self.left = move alias; }
        [second]: { self.left = borrow mutable input.left; }
        [entry]: self.right = borrow mutable input.right;
        deref self.left = "edited";
    }
}
control main() exits done {
    mutable owners owner = owners {"left", "right"};
    auto first = split(borrow mutable owner, true);
    emit(first.left);
    emit(first.right);
    drop(first);
    auto second = split(borrow mutable owner, false);
    emit(second.left);
    emit(second.right);
    drop(second);
    owner.left = "released";
    exit done;
}
"#;
    assert_eq!(messages(source), ["edited", "right", "edited", "right"]);
    rejects(
        &source.replace(
            "self.right = borrow mutable input.right;",
            "self.right = borrow mutable input.left;",
        ),
        "E_LOAN",
    );
}

#[test]
fn constructors_cannot_store_temporary_or_owned_parameter_borrows() {
    rejects(
        &PAIR.replace(
            "text_pair(borrow left, borrow right)",
            "text_pair(borrow \"temporary\", borrow right)",
        ),
        "E_TEMPORARY",
    );
    rejects(
        "struct view['a] {ref string text <- 'a; constructor(string input)[input] {self.text=borrow input;}}",
        "E_ORIGIN",
    );
    rejects(
        "struct view {ref string text; constructor(ref string input) {self.text=input;}}",
        "E_ORIGIN",
    );
    rejects(
        "struct view['a] {ref string text <- 'a; string owner; constructor(ref string input)[input] {self.owner=\"local\";self.text=borrow self.owner;}}",
        "E_LOAN",
    );
    rejects(
        "struct view['a] {ref mutable string text <- 'a; constructor(ref string input)[input] {self.text=input;}}",
        "E_ORIGIN",
    );
}

#[test]
fn private_and_public_fault_contracts_compose_with_origin_mappings() {
    let source = "pub struct view['a] {ref string text <- 'a; pub constructor(ref string input)[input] faults {self.text=input;fault \"failed\";}} control main() exits done {string owner=\"kept\";auto value=view(borrow owner);emit(value.text);exit done;}";
    let p = checked(source);
    let failure = p.run_detailed(&scenario(), 1000).unwrap_err();
    assert!(failure.events.is_empty());
    assert_eq!(
        failure.report.activations.last().unwrap().control,
        "view.constructor"
    );
    rejects(
        &source.replace("[input] faults", "[input]"),
        "E_FAULT_CONTRACT",
    );
}

#[test]
fn constructor_results_flow_through_exits_capture_and_adapters() {
    let source = r#"
struct view['a] {
    ref string text <- 'a;
    constructor(ref string input)[input] { self.text = input; }
}

control make(ref string input) exits ready { view[input] value } {
    exit ready { view(input) };
}
control main() exits done {
    mutable string owner = "captured";
    auto pending = capture make(borrow owner);
    delta pending { ready { auto value } => ready }
    [ready]: emit(value.text);
    drop(value);
    auto adapted = delta true {
        true => ready { view(borrow owner) },
        false => missing {}
    };
    delta adapted { ready { auto selected_view } => selected, missing => missing }
    [selected]: emit(selected_view.text);
    [entry]: owner = "released";
    exit done;
}
"#;
    assert_eq!(messages(source), ["captured", "captured"]);
}

#[test]
fn earlier_borrowed_constructor_arguments_protect_sources_from_later_calls() {
    let source = r#"
struct view['a] {
    ref string text <- 'a;
    constructor(ref string input)[input] { self.text = input; }
}
struct mark {
    bool done;
    constructor(ref mutable string input) { deref input = "changed"; self.done = true; }
}
struct both['a] {
    view['a] value;
    mark changed;
    constructor(view value, mark changed)[value['a]] {
        self.value = move value;
        self.changed = move changed;
    }
}
control main() exits done {
    mutable string owner = "protected";
    mutable string other = "separate";
    auto pair = both(view(borrow owner), mark(borrow mutable other));
    emit(pair.value.text);
    exit done;
}
"#;
    assert_eq!(messages(source), ["protected"]);
    rejects(
        &source.replace("mark(borrow mutable other)", "mark(borrow mutable owner)"),
        "E_ALIAS",
    );
}

#[test]
fn borrowed_constructors_match_native_and_expanded_execution() {
    use std::{fs, process::Command};
    let root = std::env::temp_dir().join(format!(
        "tellur-borrowed-construction-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    let moved_pair = PAIR.replace("= text_pair(", "= move text_pair(");
    for (name, source) in [
        ("pair", PAIR),
        ("moved-pair", moved_pair.as_str()),
        ("nested", NESTED),
        ("optional", OPTIONAL),
        ("optional-carrier", OPTIONAL_CARRIER),
        ("exclusive-carrier", EXCLUSIVE_CARRIER),
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
