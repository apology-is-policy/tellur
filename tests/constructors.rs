mod support;
use tellur::{
    compile,
    manifest::Manifest,
    runtime::{Event, Scenario},
};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn checked(s: &str) -> tellur::CheckedProgram {
    compile(s, manifest()).unwrap()
}
fn rejects(s: &str, code: &str) {
    let e = match compile(s, manifest()) {
        Err(e) => e,
        Ok(_) => panic!("expected {code} for {s}"),
    };
    assert_eq!(e.code, code, "{e}");
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
fn messages(p: &tellur::CheckedProgram) -> Vec<String> {
    p.run(&scenario())
        .unwrap()
        .events
        .into_iter()
        .filter_map(|e| match e {
            Event::Emit { message } => Some(message),
            _ => None,
        })
        .collect()
}
const BASIC: &str = r#"
struct message {
 string text;
 constructor(string text) { self.text = move text; }
}
struct pair {
 message left;
 message right;
 constructor(message a, message b) {self.right = move b; self.left = move a;}
}
control main() exits done {
 auto p = pair(message("left"), message("right"));
 emit(move p.left.text);
 emit(move p.right.text);
 exit done;
}
"#;
#[test]
fn owning_nested_construction_and_expansion() {
    let p = checked(BASIC);
    assert_eq!(messages(&p), ["left", "right"]);
    assert_eq!(messages(&p.expanded("main").unwrap()), ["left", "right"]);
}
#[test]
fn native_constructors_deliver_to_caller_storage() {
    let p = checked(BASIC);
    let native = support::scenario_executable(&p)
        .native_source("main")
        .unwrap();
    let mut constructors = 0;
    for function in native.split("fn region_").skip(1) {
        let function = function.split("fn entry_").next().unwrap();
        if !function
            .lines()
            .next()
            .unwrap()
            .contains("destination:usize")
        {
            continue;
        }
        constructors += 1;
        assert!(function.contains("v[destination] = Some(constructed)"));
        assert!(function.contains("return Ok(Completion::Constructed)"));
        assert!(!function.contains("return Ok(Completion::Outcome"));
    }
    assert_eq!(constructors, 2);
    assert!(native.contains("?.constructed()?"));
}
#[test]
fn initialization_is_path_sensitive() {
    let good = r#"
struct message { string text; constructor(bool flag) {
 delta flag {true=>yes,false=>no}
 [yes]:self.text="yes";
 [no]:self.text="no";
}}
control main() exits done {auto a=message(true); auto b=message(false);emit(move a.text);emit(move b.text);exit done;}
"#;
    assert_eq!(messages(&checked(good)), ["yes", "no"]);
    rejects(&good.replace("[no]:self.text=\"no\";", ""), "E_UNAVAILABLE");
    rejects(
        &BASIC.replace(
            "self.text = move text;",
            "emit(borrow self.text); self.text = move text;",
        ),
        "E_UNAVAILABLE",
    );
    rejects(
        &BASIC.replace(
            "self.text = move text;",
            "self.text = move text; self.text = \"again\";",
        ),
        "E_REDEFINE",
    );
    rejects(
        &BASIC.replace(
            "self.text = move text;",
            "drop(self); self.text = move text;",
        ),
        "E_CONSTRUCTOR",
    );
    rejects(
        &BASIC.replace(
            "self.text = move text;",
            "self.text = move text; drop(move self.text);",
        ),
        "E_MOVED",
    );
}
#[test]
fn field_initialization_inside_selected_scope() {
    let s = r#"struct message {string text; constructor(bool flag) {
 delta flag {true=>yes,false=>no}
 [yes] over &: {self.text="yes";}
 [no]:self.text="no";
}} control main() exits done {auto a=message(true);emit(move a.text);exit done;}"#;
    assert_eq!(messages(&checked(s)), ["yes"]);
}
#[test]
fn generic_forwarding_constructor() {
    let s = r#"struct box<t> { t value; constructor(t value) {self.value=move value;} }
 control main() exits done {auto a=box<string>("generic");emit(move a.value);exit done;}"#;
    assert_eq!(messages(&checked(s)), ["generic"]);
}
#[test]
fn constructors_are_values_not_outcomes() {
    rejects(
        &BASIC.replace(
            "auto p = pair(message(\"left\"), message(\"right\"));",
            "delta pair(message(\"left\"), message(\"right\"));",
        ),
        "E_CONSTRUCTOR",
    );
    rejects(
        &BASIC.replace(
            "self.text = move text;",
            "self.text = move text; exit done;",
        ),
        "E_CONSTRUCTOR",
    );
}

const ORDER: &str = r#"
struct mark {string text; u32 number; bool yes;
 constructor(ref mutable u32 counter, string text) {
  emit(borrow text); deref counter=2;
  self.text=move text; self.number=2; self.yes=true;
 }
}
struct pair {u32 before; mark after;}
control take(u32 before, mark after) exits done {emit(u32_text(before));emit(move after.text);exit done;}
control main() exits done {
 mutable u32 n=1;
 take(n, mark(borrow mutable n, "call"));
 n=1;
 auto p=pair {n, mark(borrow mutable n,"record")};
 emit(u32_text(p.before));emit(move p.after.text);
 n=1;
 bool same=n==(mark(borrow mutable n,"binary")).number;
 delta same {true=>fault "left operand was evaluated too late",false=>compared}
 [compared]:
 bool skipped=false && (mark(borrow mutable n,"WRONG and")).yes;
 bool skipped_or=true || (mark(borrow mutable n,"WRONG or")).yes;
 bool evaluated=true && (mark(borrow mutable n,"and")).yes;
 bool evaluated_or=false || (mark(borrow mutable n,"or")).yes;
 delta skipped {true=>fault "bad and",false=>and_checked}
 [and_checked]: delta skipped_or {true=>or_checked,false=>fault "bad or"}
 [or_checked]:exit done;
}
"#;
#[test]
fn effects_preserve_written_order_and_short_circuiting() {
    let p = checked(ORDER);
    let expected = [
        "call", "1", "call", "record", "1", "record", "binary", "and", "or",
    ];
    assert_eq!(messages(&p), expected);
    assert_eq!(messages(&p.expanded("main").unwrap()), expected);
}
#[test]
fn constructors_in_exits_captures_lists_and_selected_paths() {
    let s = r#"
struct mark {string text; constructor(string text) {emit(borrow text);self.text=move text;}}
control make() exits ready {mark first,mark second} {exit ready {mark("first"),mark("second")};}
control main() exits done {
 auto result=capture make();
 delta result {ready {auto first,auto second}=>ready}
 [ready]:emit(move first.text);emit(move second.text);
 vector<mark> values=[mark("list one"),mark("list two")];drop(values);
 delta false {true=>yes,false=>no}
 [yes]:auto unused=mark("WRONG inactive");drop(unused);exit done;
 [no]:exit done;
}
"#;
    assert_eq!(
        messages(&checked(s)),
        ["first", "second", "first", "second", "list one", "list two"]
    );
}
#[test]
fn fault_permissions_contingency_and_pending_resources() {
    let prefix = r#"struct broken {string text; constructor() {fault "construction failed";}}"#;
    rejects(
        &format!(
            "{prefix} control main(handle pending) exits ready {{handle file,broken value}} {{exit ready {{pending,broken()}};}}"
        ),
        "E_FAULT_RESOURCE",
    );
    rejects(
        &format!(
            "{prefix} struct pair {{handle file;broken value;}} control main(handle pending) exits ready {{pair value}} {{exit ready {{pair {{pending,broken()}}}};}}"
        ),
        "E_FAULT_RESOURCE",
    );
    rejects(
        "struct box {handle file;constructor(handle file){self.file=move file;fault \"partial\";}}",
        "E_FAULT_RESOURCE",
    );
    rejects(
        "pub struct box {string text;pub constructor(){fault \"partial\";}}",
        "E_FAULT_CONTRACT",
    );
    checked("pub struct box {string text;pub constructor() faults {fault \"partial\";}}");
    let contingent = r#"struct message {string text;@contingent pub constructor(string value) {self.text=move value;}}
 control main() exits done {fault boundary {fault "request";} contingency [] {auto m=message("handled");emit(move m.text);terminate process 23;}}"#;
    let p = checked(contingent);
    let tellur::runtime::RunCompletion::Process(result) =
        p.run_completion(&scenario(), 10000).unwrap()
    else {
        panic!("normal outcome")
    };
    assert_eq!(result.status, 23);
    rejects(
        &contingent.replace("@contingent pub constructor", "pub constructor"),
        "E_CONTINGENT",
    );
}
#[test]
fn temporary_borrow_and_fresh_loop_initializers() {
    let s = r#"struct message {string text;constructor(string text){self.text=move text;}}
 control show(ref message value) exits done {emit(borrow value.text);exit done;}
 control main() exits done {show(borrow message("temporary"));loop work(message value=message("loop")){emit(move value.text);exit done;}}"#;
    assert_eq!(messages(&checked(s)), ["temporary", "loop"]);
}
#[test]
fn native_construction_agrees_with_reference_at_both_optimization_levels() {
    use std::{fs, process::Command};
    let p = checked(ORDER);
    let root =
        std::env::temp_dir().join(format!("tellur-constructor-native-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let case = root.join("scenario.json");
    fs::write(&case, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (opt, p) in [(0, p.clone()), (2, p.expanded("main").unwrap())] {
        let binary = root.join(format!("construction-{opt}"));
        support::scenario_executable(&p)
            .build_native("main", &binary, opt)
            .unwrap();
        let output = Command::new(binary)
            .args(["--scenario", case.to_str().unwrap()])
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
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn constructor_fault_reports_use_source_names_and_never_deliver_partial_values() {
    let p = checked(
        "struct message {string text;constructor(){self.text=\"initialized\";fault \"broken invariant\";}}control main() exits done {auto value=message();emit(move value.text);exit done;}",
    );
    let failure = p.run_detailed(&scenario(), 10000).unwrap_err();
    assert_eq!(
        failure
            .report
            .activations
            .iter()
            .map(|a| a.control.as_str())
            .collect::<Vec<_>>(),
        ["main", "message.constructor"]
    );
    assert!(
        !serde_json::to_string(&failure.report)
            .unwrap()
            .contains("$constructor")
    );
    assert!(failure.events.is_empty());
    let ctor = p.regions().find(|r| r.is_constructor()).unwrap();
    assert!(p.entry_exit_codes(&ctor.name).is_err());
    assert!(
        p.run(&Scenario {
            entry: ctor.name.clone(),
            ..scenario()
        })
        .is_err()
    );
}
#[test]
fn borrowed_constructors_require_origins_and_raw_inline_assembly_stays_gated() {
    rejects(
        "struct view['a] {ref string text <- 'a;constructor(ref string text){self.text=text;}}",
        "E_ORIGIN",
    );
    let s = r#"struct text {string value;constructor(string value){self.value=move value;}}struct view {ref string reference;text owned;}control main() exits done {string owner="owner";auto mixed=view {borrow owner,text("owned")};emit(borrow mixed.owned.value);drop(mixed);exit done;}"#;
    rejects(s, "E_CONSTRUCTOR");
    checked(&s.replace(
        "auto mixed=view {borrow owner,text(\"owned\")};",
        "auto value=text(\"owned\");auto mixed=view {borrow owner,move value};",
    ));
}

#[test]
fn legacy_borrow_modes_evaluate_temporary_arguments_in_written_order() {
    let mut config: serde_json::Value =
        serde_json::from_str(include_str!("../playground/manifest.json")).unwrap();
    config["primitives"].as_array_mut().unwrap().push(serde_json::json!({
  "name":"inspect_pair", "parameters":[{"name":"left","type":"string","mode":"borrow"},{"name":"right","type":"string","mode":"value"}],
  "outcomes":[{"name":"done","fields":[]}],"effects":[],"returns":true
 }));
    let source = r#"struct message {string text;constructor(string text){emit(borrow text);self.text=move text;}}
 control main() exits done {inspect_pair(borrow (message("left")).text,(message("right")).text);exit done;}"#;
    let p = compile(source, Manifest::from_json(&config.to_string()).unwrap()).unwrap();
    let scenario:Scenario=serde_json::from_value(serde_json::json!({"entry":"main","arguments":[],"calls":[{"operation":"inspect_pair","outcome":"done","fields":[]}]})).unwrap();
    let events = p.run(&scenario).unwrap().events;
    let emitted = events
        .iter()
        .filter_map(|e| match e {
            Event::Emit { message } => Some(message.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(emitted, ["left", "right"]);
    assert_eq!(
        p.run(&scenario).unwrap(),
        p.expanded("main").unwrap().run(&scenario).unwrap()
    );
}

#[test]
fn staging_preserves_payload_alias_checks_and_boolean_operand_types() {
    let prefix = "struct mark {bool yes;constructor(ref mutable string input){deref input=\"changed\";self.yes=true;}}";
    rejects(
        &format!(
            "{prefix} control make(ref mutable string input) exits ready {{ref string text from input,mark value}} {{exit ready {{borrow deref input,mark(input)}};}}"
        ),
        "E_ALIAS",
    );
    rejects(
        &format!(
            "{prefix} control make(ref mutable string input) exits auto {{exit ready {{borrow deref input,mark(input)}};}}"
        ),
        "E_ALIAS",
    );
    rejects(
        &format!(
            "{prefix} control make(ref mutable string input) exits auto {{auto saved=delta true {{true=>ready {{borrow deref input,mark(input)}},false=>missing {{}}}};exit saved;}}"
        ),
        "E_ALIAS",
    );
    rejects(
        "variant fake {true,false} struct flag {bool yes;constructor(){self.yes=true;}}control main() exits done {bool value=(fake.true {}) && (flag()).yes;exit done;}",
        "E_TYPE",
    );
}
