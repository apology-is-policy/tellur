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

const ORDER: &str = r#"
struct marker {
    string label;
    constructor(ref mutable string state, string label) {
        emit(borrow label);
        deref state = clone(borrow label);
        self.label = move label;
    }
}
struct bundle['a] {
    string snapshot;
    marker later;
    ref string text <- 'a;
    marker earlier;
}
control main() exits done {
    string owner = "borrowed";
    mutable string first = "before";
    mutable string second = "other";
    auto value = bundle {
        snapshot: clone(borrow first),
        earlier: marker(borrow mutable first, "first"),
        text: borrow owner,
        later: marker(borrow mutable second, "second")
    };
    emit(borrow value.snapshot);
    emit(borrow value.earlier.label);
    emit(value.text);
    emit(borrow value.later.label);
    exit done;
}
"#;
#[test]
fn named_fields_evaluate_in_written_order_and_freeze_earlier_values() {
    assert_eq!(
        messages(ORDER),
        ["first", "second", "before", "first", "borrowed", "second"]
    );
    rejects(
        &ORDER.replace(
            "text: borrow owner,",
            "text: borrow owner, text: borrow owner,",
        ),
        "E_FIELD",
    );
    rejects(&ORDER.replace("text: borrow owner,", ""), "E_ARITY");
}

const RECORDS: &str = r#"
struct view<t>['a] {
    ref t text <- 'a;
    constructor(ref t input)[input] { self.text = input; }
}
struct pair['a, 'b] { view<string>['a] left; view<string>['b] right; }
control pair_of(ref string left, ref string right)
    exits ready { pair[left, right] value }
{
    exit ready { pair {right: view<string>(right), left: move view<string>(left)} };
}
control main() exits done {
    mutable string left = "left";
    mutable string right = "right";
    auto pending = capture pair_of(borrow left, borrow right);
    delta pending {ready {auto value} => ready}
    [ready]: emit(value.left.text);
    left = "released left";
    emit(value.right.text);
    right = "released right";
    exit done;
}
"#;
#[test]
fn generic_nested_records_cross_factory_and_capture_with_independent_origins() {
    assert_eq!(messages(RECORDS), ["left", "right"]);
    let local = RECORDS.replace(
        "auto pending = capture pair_of(borrow left, borrow right);\n    delta pending {ready {auto value} => ready}\n    [ready]:",
        "auto value = move pair {view<string>(borrow left), view<string>(borrow right)};",
    );
    assert_eq!(messages(&local), ["left", "right"]);
    let retained = local
        .replace(
            "emit(value.left.text);",
            "auto copy=value;emit(value.left.text);",
        )
        .replace(
            "left = \"released left\";",
            "left = \"released left\";emit(copy.left.text);",
        );
    rejects(&retained, "E_LOAN");
    assert_eq!(
        messages(&retained.replace("left = \"released left\";", "")),
        ["left", "left", "right"]
    );
    rejects(
        &RECORDS.replace(
            "emit(value.left.text);",
            "left = \"too early\"; emit(value.left.text);",
        ),
        "E_LOAN",
    );
    rejects(
        &RECORDS.replace("pair[left, right]", "pair[right, left]"),
        "E_LIFETIME",
    );
    rejects(
        &RECORDS.replace("view<string>(right)", "view<string>(borrow \"temporary\")"),
        "E_TEMPORARY",
    );
}

const SUMS: &str = r#"
struct view['a] {
    ref string text <- 'a;
    constructor(ref string input)[input] {self.text=input;}
}
struct packet['a] { option<view>['a] content; }
variant choice['a] { present {packet['a] value}, absent }
outcome report['a] { ready {choice['a] value}, missing }
control show(report input) exits done {
    delta input {ready {auto value} => ready, missing => missing}
    [ready]: delta value {present {auto selected} => present, absent => absent}
    [present]: delta selected.content {some {auto item} => some, none => none}
    [some]: emit(item.text);
    [entry]: exit done;
}
control main() exits done {
    mutable string owner = "nested tags";
    report first = ready {present {packet {some {view(borrow owner)}}}};
    show(move first);
    report second = ready {absent {}};
    show(move second);
    report third = missing {};
    show(move third);
    owner = "released";
    exit done;
}
"#;
#[test]
fn nested_options_variants_and_named_outcomes_preserve_tagged_origins() {
    assert_eq!(messages(SUMS), ["nested tags"]);
    rejects(
        &SUMS.replace("show(move first);", "owner=\"too early\";show(move first);"),
        "E_LOAN",
    );
}

const EXCLUSIVE: &str = r#"
struct edit['a] {
    ref mutable string text <- 'a;
    constructor(ref mutable string input)[input] {self.text=input;}
}
struct pair['a, 'b] {edit['a] left; option<edit>['b] right;}
control main() exits done {
    mutable string left = "left";
    mutable string right = "right";
    auto value = pair {edit(borrow mutable left), some {edit(borrow mutable right)}};
    deref value.left.text = "edited left";
    emit(value.left.text);
    left = "released left";
    delta value.right {some {auto selected} => present, none => absent}
    [present]: deref selected.text = "edited right";
    emit(selected.text);
    [entry]: right = "released right";
    exit done;
}
"#;
#[test]
fn exclusive_fields_transfer_through_raw_records_and_optional_payloads() {
    assert_eq!(messages(EXCLUSIVE), ["edited left", "edited right"]);
    rejects(
        &EXCLUSIVE.replace("edit(borrow mutable right)", "edit(borrow mutable left)"),
        "E_ALIAS",
    );
    rejects(
        &EXCLUSIVE.replace(
            "emit(value.left.text);",
            "left=\"too early\";emit(value.left.text);",
        ),
        "E_LOAN",
    );
}

#[test]
fn earlier_stored_and_direct_borrows_exclude_later_mutating_initializers() {
    let source = r#"
struct view['a] {ref string text <- 'a; constructor(ref string input)[input] {self.text=input;}}
struct mark {bool done;constructor(ref mutable string input) {deref input="changed";self.done=true;}}
struct held['a] {view['a] first;mark second;}
control main() exits done {
    mutable string owner="owner";
    mutable string other="other";
    auto value=held {view(borrow owner),mark(borrow mutable other)};
    emit(value.first.text);
    exit done;
}
"#;
    assert_eq!(messages(source), ["owner"]);
    rejects(
        &source.replace("mark(borrow mutable other)", "mark(borrow mutable owner)"),
        "E_ALIAS",
    );
    let direct = source
        .replace("view['a] first", "ref string first <- 'a")
        .replace("view(borrow owner)", "borrow owner")
        .replace("value.first.text", "value.first");
    assert_eq!(messages(&direct), ["owner"]);
    rejects(
        &direct.replace("mark(borrow mutable other)", "mark(borrow mutable owner)"),
        "E_ALIAS",
    );
    let nested = source
        .replace("view['a] first", "option<view>['a] first")
        .replace("view(borrow owner)", "some {view(borrow owner)}")
        .replace("emit(value.first.text);", "drop(value);");
    checked(&nested);
    rejects(
        &nested.replace("mark(borrow mutable other)", "mark(borrow mutable owner)"),
        "E_ALIAS",
    );
}

#[test]
fn inline_adapter_construction_only_evaluates_its_selected_arm() {
    let source = r#"
struct view['a] {
    ref string text <- 'a;
    constructor(ref string input)[input] {emit(input);self.text=input;}
}
struct packet['a] {view['a] value;}
control choose(ref string input, bool selected) exits ready {packet[input] value}, missing {
    exit delta selected {
        true => ready {packet {view(input)}},
        false => missing {}
    };
}
control main() exits done {
    string owner="chosen";
    auto pending=capture choose(borrow owner,false);
    delta pending {ready {auto value} => unexpected, missing => skipped}
    [unexpected]: emit(value.value.text);
    [skipped]: delta choose(borrow owner,true) {ready {auto result} => selected, missing => absent}
    [selected]: emit(result.value.text);
    [entry]: exit done;
}
"#;
    assert_eq!(messages(source), ["chosen", "chosen"]);
    rejects(
        &source.replace("packet {view(input)}", "packet {view(borrow \"local\")}"),
        "E_TEMPORARY",
    );
}

#[test]
fn raw_borrowed_composition_matches_native_and_expanded_execution() {
    use std::{fs, process::Command};
    let root = std::env::temp_dir().join(format!(
        "tellur-borrowed-composition-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (name, source) in [
        ("order", ORDER),
        ("records", RECORDS),
        ("sums", SUMS),
        ("exclusive", EXCLUSIVE),
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
fn constructor_fields_can_assemble_raw_borrowed_storage_without_extending_temporary_lifetimes() {
    let source = r#"
struct view['a] {ref string text <- 'a;constructor(ref string input)[input] {self.text=input;}}
struct packet['a] {view['a] value;}
struct wrapper['a] {
    packet['a] inner;
    constructor(ref string input,bool first)[input] {
        delta first {true=>first,false=>second}
        [first]: {auto local=view(input);self.inner=packet {local};}
        [second]: self.inner=packet {view(input)};
    }
}
control main() exits done {
    mutable string owner="retained";
    auto first=wrapper(borrow owner,true);
    auto second=wrapper(borrow owner,false);
    emit(first.inner.value.text);
    emit(second.inner.value.text);
    owner="released";
    exit done;
}
"#;
    assert_eq!(messages(source), ["retained", "retained"]);
    rejects(
        &source.replace(
            "emit(second.inner.value.text);",
            "owner=\"too early\";emit(second.inner.value.text);",
        ),
        "E_LOAN",
    );
    rejects(
        "struct text {string value;constructor(string input){self.value=move input;}}struct view {ref string text;}control main() exits done {auto value=view {borrow text(\"temporary\").value};emit(value.text);exit done;}",
        "E_LIFETIME",
    );
}
