mod support;
use tellur::{compile, manifest::Manifest, runtime::Scenario};
fn m() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
const SOURCE: &str = include_str!("fixtures/public_contracts.tellur");
fn reject(source: &str, code: &str) -> tellur::Diagnostic {
    let e = compile(source, m()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
    e
}
#[test]
fn public_shapes_require_explicit_labels_and_payloads() {
    for s in [
        "pub control f() exits auto {exit done;}",
        "pub control f() exits ready... {exit ready {1};}",
        "pub control f<t>(t value) exits auto {exit ready {value};}",
        "pub control f<t>(t value) exits ready... {exit ready {value};}",
    ] {
        reject(s, "E_PUBLIC_CONTRACT");
    }
}
#[test]
fn private_inference_and_explicit_public_forwarding_remain_compatible() {
    let p = compile(SOURCE, m()).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(
        p.run(&scenario).unwrap(),
        p.expanded("main").unwrap().run(&scenario).unwrap()
    );
    compile("control private() exits ready... {exit ready {1};}pub control public() exits ready {u32 value} {forward private();}",m()).unwrap();
}
#[test]
fn public_direct_borrows_require_explicit_origins() {
    let s =
        "pub control view(ref string input) exits ready {ref string value} {exit ready {input};}";
    reject(s, "E_PUBLIC_CONTRACT");
    compile(
        &s.replace("ref string value}", "ref string value from input}"),
        m(),
    )
    .unwrap();
    compile(&s.replace("pub control", "control"), m()).unwrap();
}
#[test]
fn public_aggregate_and_nested_results_require_all_origin_layers() {
    let s = "pub struct view['a] {ref string text <- 'a;}pub control make(ref string input) exits ready {view value} {exit ready {view {input}};}";
    reject(s, "E_PUBLIC_CONTRACT");
    compile(&s.replace("view value}", "view[input] value}"), m()).unwrap();
    let s = "pub struct view['a] {ref string text <- 'a;}pub control lend(ref view input) exits ready {ref view value from input} {exit ready {input};}";
    reject(s, "E_PUBLIC_CONTRACT");
    compile(
        &s.replace("ref view value from", "ref view[input['a]] value from"),
        m(),
    )
    .unwrap();
}
#[test]
fn named_completion_contracts_bind_public_origins() {
    compile("pub outcome reply {ready {string text},missing}pub control f() exits reply {exit ready {\"ok\"};}",m()).unwrap();
    let s = "pub outcome reply['a] {ready {ref string value <- 'a},missing}pub control f(ref string input) exits reply {exit ready {input};}";
    reject(s, "E_PUBLIC_CONTRACT");
    compile(
        &s.replace("exits reply {exit", "exits reply[input] {exit"),
        m(),
    )
    .unwrap();
}
#[test]
fn public_generic_borrow_origins_are_checked_without_a_caller() {
    let s = "pub control view<t>(ref t input) exits ready {ref t value} {exit ready {input};}";
    reject(s, "E_PUBLIC_CONTRACT");
    compile(&s.replace("ref t value}", "ref t value from input}"), m()).unwrap();
}
#[test]
fn growing_leaf_stops_at_public_boundary_and_reports_forwarding_chain() {
    let e = reject(
        &SOURCE.replacen("exit invalid;", "exit unexpected;", 1),
        "E_OUTCOME",
    );
    assert!(
        e.notes
            .iter()
            .any(|n| n.message.contains("`unexpected`") && n.message.contains("`leaf`")),
        "{e}"
    );
    assert!(
        e.notes
            .iter()
            .any(|n| n.message.contains("forwarded") && n.message.contains("`middle`")),
        "{e}"
    );
    assert!(
        e.notes.iter().all(|n| !n.message.contains("`ready`")),
        "{e}"
    );
    assert_eq!(e.notes[0].span.line, 5);
}
#[test]
fn missing_arms_show_origins_through_multiple_private_forwarders() {
    let source = "control leaf() exits auto {exit added;}\ncontrol middle() exits auto {forward leaf();}\ncontrol top() exits auto {forward middle();}\ncontrol main() exits done {delta top() {old=>old}[old]:exit done;}";
    // An unknown old label is reported before missing labels; use an empty arm set.
    let source = source
        .replace("{old=>old}", "{}")
        .replace("[old]:", "[entry]:");
    let e = reject(&source, "E_ARMS");
    assert_eq!(e.notes.len(), 3, "{e}");
    assert_eq!(
        e.notes.iter().map(|n| n.span.line).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(e.to_string().contains("forwarded from `middle` by `top`"));
}
#[test]
fn public_explicit_contract_does_not_hide_invalid_implementation_origins() {
    let e = reject(
        "pub control choose(ref string left,ref string right) exits ready {ref string value from left} {exit ready {right};}",
        "E_LIFETIME",
    );
    assert!(e.message.contains("declared input"), "{e}");
}
#[test]
fn contributor_notes_are_bounded_on_long_forwarding_chains() {
    let mut source = "control f0() exits auto {exit changed;}\n".to_string();
    for i in 1..45 {
        source.push_str(&format!(
            "control f{i}() exits auto {{forward f{}();}}\n",
            i - 1
        ));
    }
    source.push_str("control main() exits done {delta f44() {}[entry]:exit done;}");
    let e = reject(&source, "E_ARMS");
    assert_eq!(e.notes.len(), 33);
    assert!(e.notes.last().unwrap().message.contains("omitted"));
}
#[test]
fn native_public_boundary_preserves_execution_and_expansion() {
    let p = compile(SOURCE, m()).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    let expected = serde_json::to_value(p.run(&scenario).unwrap()).unwrap();
    let root = std::env::temp_dir().join(format!("tellur-public-contracts-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for expanded in [false, true] {
        let program = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let output = root.join(format!("program-{expanded}-{opt}"));
            support::scenario_executable(&program)
                .build_native("main", &output, opt)
                .unwrap();
            let result = std::process::Command::new(output)
                .arg("--json")
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap(),
                expected
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn declared_unproduced_outcomes_and_payload_changes_have_contributors() {
    let e = reject(
        "control leaf() exits ready,extra {exit ready;}control main() exits done {delta leaf() {ready=>ready}[ready]:exit done;}",
        "E_ARMS",
    );
    assert!(
        e.notes
            .iter()
            .any(|n| n.message.contains("`extra` is declared")),
        "{e}"
    );
    let e = reject(
        &SOURCE.replace("exit ready { 42 }", "exit ready { true }"),
        "E_TYPE",
    );
    assert!(
        e.notes
            .iter()
            .any(|n| n.message.contains("`ready` is produced")),
        "{e}"
    );
    assert!(
        e.notes.iter().all(|n| !n.message.contains("`invalid`")),
        "{e}"
    );
}
