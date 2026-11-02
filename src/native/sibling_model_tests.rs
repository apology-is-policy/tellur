use super::*;
use serde_json::Value as Json;

#[test]
fn related_compiled_children_settle_borrowers_before_the_owning_contingency() {
    let root = std::env::temp_dir().join(format!("tellur-native-siblings-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let manifest = super::resource_tests::resource_manifest(&root);
    let host = super::resource_tests::HOST.replace(
        "rust_api::make_outcome::made(Counter { path: path.into(), id })",
        "let counter=Counter { path: path.into(), id }; counter.record(\"make\"); rust_api::make_outcome::made(counter)",
    );
    fs::write(root.join("host.rs"), host).unwrap();
    let original = crate::compile(
        include_str!("../../tests/fixtures/native_siblings.tellur"),
        manifest,
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let mut expanded = original.clone();
    for name in original
        .regions()
        .map(|region| region.name.clone())
        .collect::<Vec<_>>()
    {
        expanded = expanded.expanded(&name).unwrap();
    }
    let log = root.join("owners.log");
    for (is_expanded, program) in [(false, original), (true, expanded)] {
        let names: BTreeMap<_, _> = program
            .regions()
            .enumerate()
            .map(|(id, region)| (region.name.as_str(), id))
            .collect();
        let entry = names["main"];
        let normal = format!(
            "fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{drive(host,args,&MODEL_{entry})}}"
        );
        let controls = [
            "main",
            "sibling",
            "descendant",
            "failing",
            "unstarted",
            "independent",
            "recovering",
        ]
        .map(|name| format!("&MODEL_{}", names[name]))
        .join(",");
        let replacement = format!(
            r#"fn entry_{entry}(host:&mut Host,mut args:Vec<V>)->R<Completion>{{
            let V::String(path)=args.remove(0) else {{panic!("path")}};
            let fail=std::env::var("TELLUR_SIBLING_FAIL").unwrap()=="true";
            let quotas=std::env::var("TELLUR_NATIVE_MODEL_QUOTAS").unwrap().split(',').map(|s|s.parse::<usize>().unwrap()).collect::<Vec<_>>();
            drive_siblings(host,[{controls}],path,fail,&quotas)
        }}"#
        );
        let source = source(&program, "main").unwrap();
        assert_eq!(source.matches(&normal).count(), 1);
        let source = source.replacen(&normal, &replacement, 1);
        for opt in [0, 2] {
            let binary = root.join(format!("siblings-{is_expanded}-{opt}"));
            let project = program.native_project("main", &binary).unwrap();
            fs::write(&project.2, &source).unwrap();
            program.finish_native_build(project, opt).unwrap();
            for fail in [false, true] {
                for quotas in ["1", "0,2,0,7", "3,1,9"] {
                    if log.exists() {
                        fs::remove_file(&log).unwrap();
                    }
                    let output = Command::new(&binary)
                        .args(["--json", "--fuel", "10000", "--", log.to_str().unwrap()])
                        .env("TELLUR_SIBLING_FAIL", fail.to_string())
                        .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "expanded={is_expanded},opt={opt},fail={fail},quotas={quotas}: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    let output: Json = serde_json::from_slice(&output.stdout).unwrap();
                    let events = output["events"].as_array().unwrap();
                    assert_eq!(events.last().unwrap()["kind"], "siblings_settled");
                    assert_eq!(events.last().unwrap()["rounds"], 3);
                    let expected = if fail {
                        "make 1\nmake 3\nborrow 1\nmake 2\nmake 4\nborrow 1\nmake 5\nborrow 1\ndrop 5\ndrop 4\ndrop 2\nclose 1\ndrop 1\nborrow 3\nclose 3\ndrop 3\n"
                    } else {
                        "make 1\nmake 3\nborrow 1\nmake 2\nmake 4\nborrow 1\nmake 5\nborrow 4\nborrow 4\nclose 4\ndrop 4\nclose 2\ndrop 2\nclose 5\ndrop 5\nborrow 1\nclose 1\ndrop 1\nborrow 3\nclose 3\ndrop 3\n"
                    };
                    assert_eq!(fs::read_to_string(&log).unwrap(), expected.repeat(3));
                    let faults: Vec<_> = events
                        .iter()
                        .filter(|event| event["kind"] == "fault")
                        .collect();
                    assert_eq!(faults.len(), if fail { 6 } else { 3 });
                    assert_eq!(
                        faults
                            .iter()
                            .filter(|event| event["report"]["message"] == "E_FAULT: local child")
                            .count(),
                        3
                    );
                    if fail {
                        for (round, fault) in faults
                            .iter()
                            .filter(|event| event["report"]["message"] == "E_FAULT: child failed")
                            .enumerate()
                        {
                            let original = events
                                .iter()
                                .find(|event| {
                                    event["kind"] == "sibling_failure" && event["round"] == round
                                })
                                .unwrap();
                            assert_eq!(fault["report"]["cause"], original["report"]);
                            assert_eq!(fault["report"]["message"], "E_FAULT: child failed");
                            assert!(
                                original["report"]["activations"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .any(|activation| activation["control"] == "failing")
                            );
                        }
                    }
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
