mod support;
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
const OWNED: &str = r#"
variant message { text {string value}, empty }
control inspect(ref message input) exits found {ref string value from input}, missing {
    exit delta input {text {auto value}=>found {value},empty=>missing {}};
}
control main() exits done {
    mutable message saved=text {"before"};
    delta inspect(borrow saved) {found {auto value}=>found,missing=>missing}
    [found]:emit(value);
    [entry]:delta borrow mutable saved {text {auto edit}=>text,empty=>empty}
    [text]:deref edit="after";
    [entry]:delta saved {text {auto owned}=>owned,empty=>none}
    [owned]:emit(owned);[entry]:exit done;
}
"#;
#[test]
fn shared_inspection_and_exclusive_payload_edit_leave_the_sum_owned() {
    same(OWNED);
    same(&OWNED.replace("text {\"before\"}", "empty {}"));
    rejects(
        &OWNED.replace("emit(value)", "drop(saved);emit(value)"),
        "E_LOAN",
    );
    rejects(
        &OWNED.replace("borrow mutable saved", "borrow saved"),
        "E_MUTABILITY",
    );
}
#[test]
fn borrowed_named_outcome_does_not_discharge_handling() {
    let source = OWNED.replace("variant message", "outcome message");
    same(&source);
    let bad = source.replace(
        "[entry]:delta saved {text {auto owned}=>owned,empty=>none}\n    [owned]:emit(owned);",
        "",
    );
    rejects(&bad, "E_OUTCOME");
}
#[test]
fn borrowed_optional_payload_is_not_moved_and_exclusive_siblings_are_disjoint() {
    same(
        r#"control main() exits done {mutable option<string> saved=some {"hello"};
    delta borrow saved {some {auto text}=>some,none=>none}
    [some]:emit(text);[entry]:delta saved {some {auto owned}=>owned,none=>empty}
    [owned]:emit(owned);[entry]:exit done;}"#,
    );
    same(
        r#"variant duo {both {string a,string b},empty}
    control main() exits done {mutable duo saved=duo.both {"a","b"};
    delta borrow mutable saved {both {auto a,auto b}=>both,empty=>empty}
    [both]:deref a="A";deref b="B";emit(a);emit(b);[entry]:drop(saved);exit done;}"#,
    );
}
const STORED: &str = r#"
control peek(ref option<ref mutable string> input) exits found {ref string text from input['source]}, missing {
    exit delta input {some {auto text}=>found {text},none=>missing {}};
}
control edit(ref mutable option<ref mutable string> input) exits found {ref mutable string text from input['source]}, missing {
    exit delta input {some {auto text}=>found {text},none=>missing {}};
}
control main() exits done {
    mutable string owner="before";
    mutable option<ref mutable string> saved=some {borrow mutable owner};
    delta peek(borrow saved) {found {auto shared}=>shared,missing=>missing}
    [shared]:emit(shared);
    [entry]:delta edit(borrow mutable saved) {found {auto child}=>child,missing=>absent}
    [child]:deref child="after";emit(child);
    [entry]:drop(saved);owner="released";exit done;
}
"#;
#[test]
fn direct_reference_payloads_reborrow_with_carrier_authority() {
    same(STORED);
    same(&STORED.replace("some {borrow mutable owner}", "none {}"));
    rejects(
        &STORED.replace("emit(shared)", "deref shared=\"bad\";emit(shared)"),
        "E_MUTABILITY",
    );
    rejects(
        &STORED.replace("emit(child)", "drop(saved);emit(child)"),
        "E_LOAN",
    );
    rejects(
        &STORED.replace("emit(child)", "owner=\"bad\";emit(child)"),
        "E_LOAN",
    );
    rejects(
        &STORED.replace(
            "emit(child)",
            "auto inspected=capture peek(borrow saved);emit(child)",
        ),
        "E_LOAN",
    );
}
#[test]
fn nested_borrowed_payloads_preserve_contained_permissions() {
    same(
        r#"struct views['a,'b] {ref mutable string a <- 'a;ref mutable string b <- 'b;}
    variant wrapped['a,'b] {found {views['a,'b] fields},empty}
    control edit(ref mutable wrapped input) exits done {
        delta input {found {auto fields}=>found,empty=>empty}
        [found]:deref fields.a="A";deref fields.b="B";[entry]:exit done;
    }
    control main() exits done {
        mutable string a="a";mutable string b="b";
        mutable wrapped saved=found {views {borrow mutable a,borrow mutable b}};
        edit(borrow mutable saved);drop(saved);emit(borrow a);emit(borrow b);exit done;
    }"#,
    );
    same(
        r#"control inspect(ref option<option<ref string>> input) exits found {ref string text from input['source]},missing {
        exit delta input {some {auto inner}=>delta inner {some {auto text}=>found {text},none=>missing {}},none=>missing {}};
    }
    control main() exits done {string owner="hello";option<option<ref string>> saved=some {some {borrow owner}};
        delta inspect(borrow saved) {found {auto text}=>found,missing=>missing}
        [found]:emit(text);[entry]:drop(saved);exit done;
    }"#,
    );
}
#[test]
fn shared_payloads_can_outlive_carriers_but_exclusive_permissions_cannot() {
    let shared = r#"control inspect_value(ref option<ref string> input) exits found {ref string text from input['source]},missing {
      exit delta input {some {auto text}=>found {text},none=>missing {}};
    }
    control main() exits done {mutable string owner="hello";option<ref string> saved=some {borrow owner};
      delta inspect_value(borrow saved) {found {auto text}=>found,missing=>missing}
      [found]:drop(saved);emit(text);[entry]:owner="released";exit done;
    }"#;
    same(shared);
    rejects(
        &shared.replace("emit(text)", "owner=\"bad\";emit(text)"),
        "E_LOAN",
    );
    let exclusive = shared
        .replace("option<ref string>", "option<ref mutable string>")
        .replace("some {borrow owner}", "some {borrow mutable owner}");
    rejects(&exclusive, "E_LOAN");
}
#[test]
fn borrowed_nested_records_enforce_shared_access_and_last_use() {
    let s = r#"struct views['a,'b] {ref mutable string a <- 'a;ref mutable string b <- 'b;}
    variant wrapped['a,'b] {found {views['a,'b] fields},empty}
    control edit(ref mutable wrapped input) exits done {
      delta input {found {auto fields}=>found,empty=>empty}
      [found]:deref fields.a="A";deref fields.b="B";[entry]:exit done;
    }
    control main() exits done {mutable string a="a";mutable string b="b";
      mutable wrapped saved=found {views {borrow mutable a,borrow mutable b}};
      edit(borrow mutable saved);drop(saved);exit done;
    }"#;
    rejects(
        &s.replace("ref mutable wrapped input", "ref wrapped input")
            .replace("edit(borrow mutable saved)", "edit(borrow saved)"),
        "E_MUTABILITY",
    );
    let ret=s.replace("exits done {\n      delta input", "exits found {ref mutable string text from input['a]},empty {\n      delta input")
      .replace("deref fields.a=\"A\";deref fields.b=\"B\";[entry]:exit done;", "exit found {borrow mutable deref fields.a};[empty]:exit empty;")
      .replace("edit(borrow mutable saved);drop(saved);exit done;", "delta edit(borrow mutable saved) {found {auto text}=>found,empty=>empty}[found]:deref text=\"changed\";[entry]:drop(saved);exit done;");
    same(&ret);
    rejects(
        &ret.replace(
            "deref text=\"changed\"",
            "drop(saved);deref text=\"changed\"",
        ),
        "E_LOAN",
    );
}
#[test]
fn generic_borrowed_sums_keep_ownership_and_expansion() {
    same(
        r#"variant maybe<t> {found {t value},empty}
    control inspect_value<t>(ref maybe<t> input) exits found {ref t value from input},empty {
      exit delta input {found {auto value}=>found {value},empty=>empty {}};
    }
    control main() exits done {maybe<string> saved=maybe<string>.found {"hello"};
      delta inspect_value<string>(borrow saved) {found {auto value}=>found,empty=>empty}
      [found]:emit(value);[entry]:drop(saved);exit done;
    }"#,
    );
}
#[test]
fn native_borrowed_sum_carriers_match_before_and_after_expansion() {
    let populated = include_str!("fixtures/sum_carriers.tellur");
    let absent = populated.replace(
        "text_result.found { borrow mutable owner }",
        "text_result.missing {}",
    );
    let nested = r#"control inspect_nested(ref option<option<ref mutable string>> input) exits found {ref string text from input['source]},missing {
      exit delta input {some {auto inner}=>delta inner {some {auto text}=>found {text},none=>missing {}},none=>missing {}};
    }
    control main() exits done {mutable string owner="nested";option<option<ref mutable string>> saved=some {some {borrow mutable owner}};
      delta inspect_nested(borrow saved) {found {auto text}=>found,missing=>missing}[found]:emit(text);[entry]:drop(saved);exit done;
    }"#;
    for (case, source) in [
        ("populated", populated),
        ("absent", absent.as_str()),
        ("owned", OWNED),
        ("optional", STORED),
        ("nested", nested),
    ] {
        let p = same(source);
        let expected = p
            .run(&Scenario {
                entry: "main".into(),
                arguments: vec![],
                calls: vec![],
            })
            .unwrap();
        let root = std::env::temp_dir().join(format!("tellur-sum-carriers-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for expanded in [false, true] {
            let program = if expanded {
                p.expanded("main").unwrap()
            } else {
                p.clone()
            };
            for opt in [0, 2] {
                let binary = root.join(format!("{case}-{expanded}-{opt}"));
                support::scenario_executable(&program)
                    .build_native("main", &binary, opt)
                    .unwrap();
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
    }
}

#[test]
fn grouped_carrier_results_work_and_ungrouped_sums_remain_gated() {
    compile(r#"control identity(ref option<ref string> input) exits ready {ref option<ref string> value from input} {exit ready {input};}"#, manifest()).unwrap();
    rejects(
        r#"variant ungrouped {found {ref string text},empty} control inspect_ungrouped(ref ungrouped input) exits done {exit done;}"#,
        "E_MANIFEST",
    );
}

#[test]
fn borrowed_decisions_reject_payload_moves_and_keep_tag_storage_live() {
    rejects(
        &OWNED.replace("emit(value)", "string moved=deref value;emit(borrow moved)"),
        "E_BORROW_MOVE",
    );
    rejects(
        &OWNED.replace("emit(value)", "saved=message.empty {};emit(value)"),
        "E_LOAN",
    );
    let local = r#"control main() exits done {mutable string owner="x";
      mutable option<ref mutable string> saved=some {borrow mutable owner};
      ref mutable option<ref mutable string> alias=borrow mutable saved;
      delta alias {some {auto text}=>some,none=>none}
      [some]:deref text="changed";[entry]:drop(saved);owner="released";exit done;
    }"#;
    same(local);
    rejects(
        &local.replace("delta alias", "owner=\"bad\";delta alias"),
        "E_LOAN",
    );
    rejects(
        &local.replace("deref text=", "drop(saved);deref text="),
        "E_LOAN",
    );
}

#[test]
fn inspecting_resource_sums_keeps_completion_with_the_owner() {
    let inspect = r#"control inspect_file(ref file_state input) exits done {
      delta input {missing=>missing,present {auto handle}=>present,blocked {auto reason}=>blocked}
      [present]:drop(handle);[blocked]:emit(reason);[entry]:exit done;
    }"#;
    compile(inspect, manifest()).unwrap();
    let leak = inspect
        .replace("ref file_state input", "file_state input")
        .replace("delta input", "delta borrow input");
    rejects(&leak, "E_RESOURCE");
}
