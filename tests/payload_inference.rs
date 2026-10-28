mod support;
use tellur::{compile, ir::Value, manifest::Manifest, runtime::Scenario};
fn m() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
fn rejects(source: &str, code: &str) {
    let e = compile(source, m()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}
const SOURCE: &str = r#"
control pick(bool present) exits ready..., missing {
    exit delta present {true=>ready {"hello",42},false=>missing {}};
}
control relay(bool present) exits ready..., missing {forward pick(present);}
control main() exits done... {
    auto saved=capture relay(true);
    exit delta saved {ready {string text,u32 number}=>done {text,number},missing=>done {"missing",0}};
}
"#;
#[test]
fn closed_labels_infer_fixed_payloads_through_adapters_capture_and_forwarding() {
    let p = compile(SOURCE, m()).unwrap();
    let r = p.run(&scenario()).unwrap();
    assert_eq!(
        r.fields,
        vec![Value::String("hello".into()), Value::U32(42)]
    );
    assert_eq!(r, p.expanded("main").unwrap().run(&scenario()).unwrap());
    assert_eq!(
        p.region("pick")
            .unwrap()
            .exits
            .iter()
            .map(|o| o.name.as_str())
            .collect::<Vec<_>>(),
        vec!["ready", "missing"]
    );
}
#[test]
fn marked_exits_are_nonempty_and_must_have_inference_evidence() {
    rejects("control main() exits ready... {exit ready;}", "E_INFER");
    rejects(
        "control main() exits ready...,missing {exit missing;}",
        "E_INFER",
    );
    rejects(
        "control main() exits ready... {exit different {1};}",
        "E_EXIT",
    );
    rejects("control main() exits ready {exit ready {1};}", "E_ARITY");
    rejects(
        "control main() exits ready... {exit ready {[]};}",
        "E_INFER",
    );
    rejects(
        "control main() exits ready...,ready {exit ready {1};}",
        "E_NAME",
    );
}
#[test]
fn every_path_must_agree_and_explicit_payloads_keep_their_constraints() {
    let s = "control main(bool flag) exits ready...,missing {exit delta flag {true=>ready {1},false=>ready {2}};}";
    compile(s, m()).unwrap();
    rejects(&s.replace("ready {2}", "ready {2,3}"), "E_INFER");
    rejects(&s.replace("ready {2}", "ready {\"bad\"}"), "E_TYPE");
    rejects(&s.replace("ready {2}", "missing {2}"), "E_ARITY");
    compile("control main(bool flag) exits ready...,error {string message},missing {exit delta flag {true=>ready {1},false=>error {\"bad\"}};}",m()).unwrap();
}
#[test]
fn inference_preserves_borrowed_origins_and_rejects_local_escape() {
    let s = r#"control peek(ref string input) exits found... {exit found {input};}
    control main() exits done... {string owner="hello";exit delta peek(borrow owner) {found {auto text}=>done {clone(text)}};}"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    assert_eq!(p.region("peek").unwrap().borrow_returns.len(), 1);
    rejects(
        &s.replace(
            "exit found {input};",
            "string local=\"bad\";exit found {borrow local};",
        ),
        "E_LIFETIME",
    );
}
#[test]
fn generic_definitions_infer_without_relying_on_a_concrete_caller() {
    let s = r#"control identity<t>(t input) exits ready... {exit ready {input};}
    control main() exits ready... {forward identity<string>("hello");}"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("hello".into())]
    );
    assert_eq!(p.generic_controls()[0].exits[0].fields[0].ty, "t");
    rejects(
        "control invalid<t>(t input) exits ready... {exit ready {input,input};}",
        "E_MOVED",
    );
}
#[test]
fn markers_cannot_replace_named_contracts_or_appear_without_a_body() {
    compile(
        "outcome ready {different {u32 value}} control main() exits ready... {exit ready {1};}",
        m(),
    )
    .unwrap();
    compile(
        "outcome answer {ready {u32 value},missing} control main() exits answer {exit ready {1};}",
        m(),
    )
    .unwrap();
    rejects("trait maker {control make() exits ready...;}", "E_PARSE");
    rejects("outcome answer {ready...}", "E_PARSE");
    rejects(
        "control main() exits ready.... {exit ready {1};}",
        "E_PARSE",
    );
    rejects("control main() exits ready... {exit ready...;}", "E_PARSE");
}
#[test]
fn native_payload_inference_matches_interpreter_before_and_after_expansion() {
    let p = compile(SOURCE, m()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    let root = std::env::temp_dir().join(format!("path-payload-inference-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let binary = root.join(format!("payload-{expanded}-{opt}"));
            support::scenario_executable(&p)
                .build_native("main", &binary, opt)
                .unwrap();
            let actual = std::process::Command::new(binary)
                .arg("--json")
                .output()
                .unwrap();
            assert!(
                actual.status.success(),
                "{}",
                String::from_utf8_lossy(&actual.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
