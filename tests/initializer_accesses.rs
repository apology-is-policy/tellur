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
    let error = compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err();
    assert_eq!(error.code, code, "{error}");
}
const STAMP: &str = r#"
struct stamp {
    string text;
    constructor(ref mutable string state,string label) {
        deref state=clone(borrow label);
        emit(borrow label);
        self.text=move label;
    }
}
"#;
const ORDER: &str = r#"
struct bundle['a] {ref string text <- 'a;stamp second;string before;stamp first;}
control main() exits done {
    mutable string owner="before";
    auto value=bundle {
        before:clone(borrow owner),
        first:stamp(borrow mutable owner,"first"),
        second:stamp(borrow mutable owner,"second"),
        text:borrow owner
    };
    emit(borrow value.before);
    emit(borrow value.first.text);
    emit(borrow value.second.text);
    emit(value.text);
    owner="released";
    exit done;
}
"#;
#[test]
fn completed_calls_release_inputs_before_later_initializers_but_values_keep_written_order() {
    let source = format!("{STAMP}{ORDER}");
    assert_eq!(
        messages(&source),
        ["first", "second", "before", "first", "second", "second"]
    );
    rejects(
        &source.replace("emit(value.text);", "owner=\"early\";emit(value.text);"),
        "E_LOAN",
    );
    rejects(
        &source
            .replace(
                "before:clone(borrow owner),",
                "before:clone(borrow owner),text:borrow owner,",
            )
            .replace(
                "second:stamp(borrow mutable owner,\"second\"),\n        text:borrow owner",
                "second:stamp(borrow mutable owner,\"second\")",
            ),
        "E_ALIAS",
    );
}
const NESTED: &str = r#"
struct pair {stamp first;stamp second;constructor(stamp first,stamp second){self.first=move first;self.second=move second;}}
control show(ref stamp first,ref string text) exits done {emit(borrow first.text);emit(text);exit done;}
control main() exits done {
    mutable string owner="before";
    auto value=pair(stamp(borrow mutable owner,"first"),stamp(borrow mutable owner,"second"));
    emit(borrow value.first.text);emit(move value.second.text);
    vector<stamp> values=[stamp(borrow mutable owner,"third"),stamp(borrow mutable owner,"fourth")];
    drop(values);
    emit(concat(stamp(borrow mutable owner,"a").text,stamp(borrow mutable owner,"b").text));
    show(borrow stamp(borrow mutable owner,"temporary"),borrow owner);
    exit done;
}
"#;
#[test]
fn nested_calls_collections_pure_arguments_and_call_duration_temporaries_are_sequential() {
    assert_eq!(
        messages(&format!("{STAMP}{NESTED}")),
        [
            "first",
            "second",
            "first",
            "second",
            "third",
            "fourth",
            "a",
            "b",
            "ab",
            "temporary",
            "temporary",
            "temporary"
        ]
    );
}
const RETAINED: &str = r#"
struct view<t>['a] {
    ref t text <- 'a;
    constructor(ref t input,ref mutable string scratch)[input] {deref scratch="scratch";self.text=input;}
}
struct bundle['a] {view<string>['a] first;stamp second;}
control main() exits done {
    mutable string owner="retained";
    mutable string scratch="before";
    auto value=bundle {view<string>(borrow owner,borrow mutable scratch),stamp(borrow mutable scratch,"after")};
    emit(value.first.text);emit(move value.second.text);
    owner="released";
    exit done;
}
"#;
#[test]
fn only_returned_origins_survive_and_nested_results_keep_them_protected() {
    let source = format!("{STAMP}{RETAINED}");
    assert_eq!(messages(&source), ["after", "retained", "after"]);
    rejects(
        &source.replace(
            "stamp(borrow mutable scratch,\"after\")",
            "stamp(borrow mutable owner,\"after\")",
        ),
        "E_ALIAS",
    );
    let sum=source.replace("view<string>['a] first", "option<view<string>>['a] first")
        .replace("bundle {view<string>(borrow owner,borrow mutable scratch),", "bundle {some {view<string>(borrow owner,borrow mutable scratch)},")
        .replace("emit(value.first.text);", "delta value.first {some {auto first}=>present,none=>absent} [present]:emit(first.text);[entry]:");
    assert_eq!(messages(&sum), ["after", "retained", "after"]);
    rejects(
        &sum.replace(
            "stamp(borrow mutable scratch,\"after\")",
            "stamp(borrow mutable owner,\"after\")",
        ),
        "E_ALIAS",
    );
}
const PROJECTIONS: &str = r#"
struct view['a] {
    ref string text <- 'a;
    string owned;
    constructor(ref string input)[input] {self.text=input;self.owned=clone(input);}
}
struct bundle {string before;stamp after;}
struct held['a] {ref string text <- 'a;stamp after;}
control main() exits done {
    mutable string owner="before";
    auto value=bundle {view(borrow owner).owned,stamp(borrow mutable owner,"after")};
    emit(move value.before);emit(move value.after.text);
    exit done;
}
"#;
#[test]
fn projected_owned_values_release_unselected_loans_but_projected_references_stay_live() {
    let source = format!("{STAMP}{PROJECTIONS}");
    assert_eq!(messages(&source), ["after", "before", "after"]);
    rejects(
        &source
            .replace(
                "bundle {view(borrow owner).owned",
                "held {view(borrow owner).text",
            )
            .replace("value.before", "value.text"),
        "E_ALIAS",
    );
}
#[test]
fn earlier_direct_and_exclusive_borrows_exclude_later_calls_and_reads() {
    let prefix = format!(
        "{STAMP} control use_pair(ref string text,stamp value) exits done {{emit(text);drop(value);exit done;}} control main() exits done {{mutable string owner=\"before\";"
    );
    rejects(
        &format!(
            "{prefix} use_pair(borrow owner,stamp(borrow mutable owner,\"after\"));exit done;}}"
        ),
        "E_ALIAS",
    );
    rejects(
        &format!(
            "{STAMP} control use_pair(ref mutable string text,string copy) exits done {{emit(text);emit(move copy);exit done;}} control main() exits done {{mutable string owner=\"before\";use_pair(borrow mutable owner,stamp(borrow mutable owner,\"after\").text);exit done;}}"
        ),
        "E_ALIAS",
    );
    rejects(
        &format!(
            "{STAMP} struct pair['a] {{ref mutable string text <- 'a;string copy;stamp value;}} control main() exits done {{mutable string owner=\"before\";mutable string other=\"other\";auto value=pair {{borrow mutable owner,clone(borrow owner),stamp(borrow mutable other,\"after\")}};drop(value);exit done;}}"
        ),
        "E_ALIAS",
    );
    rejects(
        &format!(
            "{STAMP} struct pair {{string first;stamp second;}} control main() exits done {{mutable string owner=\"before\";auto value=pair {{move owner,stamp(borrow mutable owner,\"after\")}};drop(value);exit done;}}"
        ),
        "E_MOVED",
    );
}
const ADAPTERS: &str = r#"
struct bundle['a] {stamp value;ref string text <- 'a;}
outcome result['a] {ready {stamp value,ref string text <- 'a},missing}
control make(ref mutable string input,bool selected) exits ready {stamp value,ref string text from input},missing {
    exit delta selected {true=>ready {stamp(input,"made"),borrow deref input},false=>missing {}};
}
control show(result input) exits done {delta input {ready {auto value,auto text}=>ready,missing=>missing} [ready]:emit(move value.text);emit(text);[entry]:exit done;}
control main() exits done {
    mutable string owner="before";
    result first=ready {stamp(borrow mutable owner,"stored"),borrow owner};
    show(move first);
    auto missing=capture make(borrow mutable owner,false);
    delta missing {ready {auto unexpected,auto unused}=>unexpected,missing=>skipped}
    [unexpected]:drop(unexpected);emit(unused);
    [skipped]:auto pending=capture make(borrow mutable owner,true);
    delta pending {ready {auto value,auto text}=>ready,missing=>missing}
    [ready]:emit(move value.text);emit(text);
    [entry]:exit done;
}
"#;
#[test]
fn exits_captures_tagged_payloads_and_selected_adapters_use_the_same_order() {
    assert_eq!(
        messages(&format!("{STAMP}{ADAPTERS}")),
        ["stored", "stored", "stored", "made", "made", "made"]
    );
}
#[test]
fn conflicts_inside_one_nested_initializer_are_still_checked() {
    let source = format!(
        "{STAMP} struct bad {{bool yes;constructor(ref mutable string first,ref string second){{emit(second);deref first=\"bad\";self.yes=true;}}}} struct pair {{bad first;stamp second;}} control main() exits done {{mutable string owner=\"before\";mutable string other=\"other\";auto value=pair {{bad(borrow mutable owner,borrow owner),stamp(borrow mutable other,\"after\")}};drop(value);exit done;}}"
    );
    rejects(&source, "E_ALIAS");
    let raw = format!(
        "{STAMP} struct bad['a,'b] {{ref mutable string first <- 'a;ref string second <- 'b;}} struct pair['a,'b] {{bad['a,'b] first;stamp second;}} control main() exits done {{mutable string owner=\"before\";mutable string other=\"other\";auto value=pair {{bad {{borrow mutable owner,borrow owner}},stamp(borrow mutable other,\"after\")}};drop(value);exit done;}}"
    );
    rejects(&raw, "E_LOAN");
}
#[test]
fn initializer_access_precision_matches_native_and_expanded_execution() {
    use std::{fs, process::Command};
    let root = std::env::temp_dir().join(format!(
        "tellur-initializer-accesses-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (name, source) in [
        ("shared-collection", SHARED_COLLECTION),
        ("order", ORDER),
        ("nested", NESTED),
        ("retained", RETAINED),
        ("projections", PROJECTIONS),
        ("adapters", ADAPTERS),
    ] {
        let original = checked(&format!("{STAMP}{source}"));
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
fn initializer_retirement_does_not_expire_caller_or_outer_pending_loans() {
    let source = format!("{STAMP}{PROJECTIONS}")
        .replace(
            "auto value=bundle",
            "ref auto existing=borrow owner;auto value=bundle",
        )
        .replace(
            "emit(move value.before);",
            "emit(existing);emit(move value.before);",
        );
    rejects(&source, "E_LOAN");
    assert_eq!(
        messages(
            &source
                .replace(
                    "ref auto existing=borrow owner;",
                    "ref auto existing=borrow owner;emit(existing);drop(existing);"
                )
                .replace(
                    "emit(existing);emit(move value.before);",
                    "emit(move value.before);"
                )
        ),
        ["before", "after", "before", "after"]
    );
    let nested = format!("{STAMP}{RETAINED}")
        .replace("stamp second;}", "stamp second;stamp third;}")
        .replace(
            "stamp(borrow mutable scratch,\"after\")};",
            "stamp(borrow mutable scratch,\"after\"),stamp(borrow mutable owner,\"bad\")};",
        );
    rejects(&nested, "E_ALIAS");
}

const SHARED_COLLECTION: &str = r#"
struct choose['a, 'b] {
    ref string selected <- 'a;
    ref string unused <- 'b;
    constructor(ref string left, ref string right)[left, right] {
        self.selected = left;
        self.unused = right;
    }
}
struct view['a] {
    ref string text <- 'a;
    constructor(ref string input, ref mutable string scratch)[input] {
        deref scratch = "changed";
        self.text = input;
    }
}
control main() exits done {
    string owner = "owner";
    mutable string scratch = "before";
    vector<ref string> values = [
        choose(borrow owner, borrow scratch).selected,
        view(borrow owner, borrow mutable scratch).text
    ];
    emit(borrow scratch);
    delta vector_get(borrow values, 0) {found {auto first}=>found,out_of_bounds=>absent}
    [found]:emit(first);
    [entry]:exit done;
}
"#;

#[test]
fn shared_reference_collection_initializers_retire_only_unselected_anonymous_loans() {
    assert_eq!(messages(SHARED_COLLECTION), ["changed", "owner"]);
    let array = SHARED_COLLECTION
        .replace("vector<ref string>", "array<ref string,2>")
        .replace("vector_get(", "array_get(");
    assert_eq!(messages(&array), ["changed", "owner"]);
    rejects(
        &SHARED_COLLECTION
            .replace("string owner =", "mutable string owner =")
            .replace(
                "view(borrow owner, borrow mutable scratch)",
                "view(borrow scratch, borrow mutable owner)",
            ),
        "E_ALIAS",
    );
    rejects(
        &SHARED_COLLECTION
            .replace("string owner =", "mutable string owner =")
            .replace(
                "emit(borrow scratch);",
                "owner=\"too early\";emit(borrow scratch);",
            ),
        "E_LOAN",
    );
}
