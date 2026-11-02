use std::{collections::BTreeMap, fs, process::Command};
use tellur::{
    compile,
    manifest::Manifest,
    runtime::{ExecutionProgress, RunCompletion, Scenario},
};

#[test]
fn process_status_stays_entry_only_after_nested_fault_retirement() {
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let p = compile(include_str!("fixtures/process_nested.tellur"), manifest)
        .unwrap()
        .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
        .unwrap();
    let invalid = format!(
        "{}\ncontrol invalid() exits done {{ main(); exit done; }}",
        include_str!("fixtures/process_nested.tellur")
    );
    assert_eq!(
        compile(
            &invalid,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
        )
        .unwrap_err()
        .code,
        "E_ENTRY_ONLY"
    );
    let mut expanded = p.clone();
    for name in p.regions().map(|r| r.name.clone()).collect::<Vec<_>>() {
        expanded = expanded.expanded(&name).unwrap();
    }
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    let root = std::env::temp_dir().join(format!("tellur-process-nested-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&s).unwrap()).unwrap();
    for (index, p) in [p, expanded].into_iter().enumerate() {
        let mut expected = None;
        for quotas in [vec![1], vec![0, 2, 0, 7], vec![3, 1, 9]] {
            let mut execution = p.start_execution(&s, 1000);
            let result = quotas
                .iter()
                .copied()
                .cycle()
                .take(1000)
                .find_map(|q| match execution.advance(q) {
                    ExecutionProgress::Paused => None,
                    ExecutionProgress::Complete(result) => Some(result.unwrap()),
                    ExecutionProgress::Finished => panic!("early finished"),
                })
                .unwrap();
            let RunCompletion::Process(ref status) = result else {
                panic!("process termination expected")
            };
            assert_eq!(status.status, 23);
            let json = serde_json::to_value(result).unwrap();
            let messages: Vec<_> = json["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "emit")
                .map(|e| e["message"].as_str().unwrap())
                .collect();
            assert_eq!(messages, ["escalating", "stopping"]);
            assert!(matches!(execution.advance(1), ExecutionProgress::Finished));
            assert!(matches!(execution.advance(0), ExecutionProgress::Finished));
            assert!(execution.events().is_empty());
            if let Some(expected) = &expected {
                assert_eq!(&json, expected);
            } else {
                expected = Some(json);
            }
        }
        for opt in [0, 2] {
            let binary = root.join(format!("program-{index}-{opt}"));
            p.build_native("main", &binary, opt).unwrap();
            let output = Command::new(&binary)
                .arg("--scenario")
                .arg(&input)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(23),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
                *expected.as_ref().unwrap()
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}
