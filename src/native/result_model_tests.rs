use super::*;
use serde_json::Value as Json;

#[test]
fn compiled_results_retain_owners_and_borrows_until_join_or_emergency_retirement() {
    let root = std::env::temp_dir().join(format!("tellur-native-results-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let manifest = super::resource_tests::resource_manifest(&root);
    let host = super::resource_tests::HOST.replace(
        "rust_api::make_outcome::made(Counter { path: path.into(), id })",
        "let counter=Counter { path: path.into(), id }; counter.record(\"make\"); rust_api::make_outcome::made(counter)",
    );
    fs::write(root.join("host.rs"), host).unwrap();
    let original = crate::compile(
        include_str!("../../tests/fixtures/native_results.tellur"),
        manifest,
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let mut expanded = original.clone();
    for name in original
        .regions()
        .map(|r| r.name.clone())
        .collect::<Vec<_>>()
    {
        expanded = expanded.expanded(&name).unwrap();
    }
    let log = root.join("owners.log");
    for (is_expanded, program) in [(false, original), (true, expanded)] {
        let names: BTreeMap<_, _> = program
            .regions()
            .enumerate()
            .map(|(id, r)| (r.name.as_str(), id))
            .collect();
        let entry = names["main"];
        let normal = format!(
            "fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{drive(host,args,&MODEL_{entry})}}"
        );
        let controls = [
            "main",
            "produce",
            "consume",
            "failing",
            "process_entry",
            "root_result",
        ]
        .map(|name| format!("&MODEL_{}", names[name]))
        .join(",");
        let replacement = format!(
            r#"fn entry_{entry}(host:&mut Host,mut args:Vec<V>)->R<Completion>{{
            let V::String(path)=args.remove(0) else {{panic!("path")}};
            let mode=std::env::var("TELLUR_RESULT_MODE").unwrap();
            let quotas=std::env::var("TELLUR_NATIVE_MODEL_QUOTAS").unwrap().split(',').map(|s|s.parse::<usize>().unwrap()).collect::<Vec<_>>();
            drive_results(host,[{controls}],path,&mode,&quotas)
        }}"#
        );
        let source = source(&program, "main").unwrap();
        assert_eq!(source.matches(&normal).count(), 1);
        let source = source.replacen(&normal, &replacement, 1);
        for opt in [0, 2] {
            let binary = root.join(format!("results-{is_expanded}-{opt}"));
            let project = program.native_project("main", &binary).unwrap();
            fs::write(&project.2, &source).unwrap();
            program.finish_native_build(project, opt).unwrap();
            for mode in ["normal", "reverse", "fault", "joined-fault", "process"] {
                for quotas in ["1", "0,2,0,7", "3,1,9"] {
                    if log.exists() {
                        fs::remove_file(&log).unwrap();
                    }
                    let output = Command::new(&binary)
                        .args([
                            "--json",
                            "--fuel",
                            "10000",
                            "--",
                            log.to_str().unwrap(),
                            "1",
                        ])
                        .env("TELLUR_RESULT_MODE", mode)
                        .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                        .output()
                        .unwrap();
                    let context =
                        format!("expanded={is_expanded},opt={opt},mode={mode},quotas={quotas}");
                    assert_eq!(
                        output.status.code(),
                        Some(if mode == "process" { 23 } else { 0 }),
                        "{context}: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    let output: Json = serde_json::from_slice(&output.stdout).unwrap();
                    let events = output["events"].as_array().unwrap();
                    let rounds = if mode == "process" { 1 } else { 3 };
                    assert_eq!(
                        events.last().unwrap()["kind"],
                        "results_settled",
                        "{context}"
                    );
                    assert_eq!(events.last().unwrap()["rounds"], rounds, "{context}");
                    let mut expected="make 1\nmake 3\nborrow 1\nmake 2\nborrow 1\nmake 4\nborrow 3\nmake 5\nmake 6\n".to_owned();
                    expected.push_str(match mode {
                        "process"=>"drop 6\ndrop 5\ndrop 4\ndrop 2\ndrop 3\ndrop 1\n",
                        "fault"|"joined-fault"=>"borrow 1\ndrop 4\ndrop 2\nclose 1\ndrop 1\n",
                        "reverse"=>"borrow 1\nclose 4\ndrop 4\nborrow 1\nclose 2\ndrop 2\nborrow 1\nclose 1\ndrop 1\n",
                        _=>"borrow 1\nclose 2\ndrop 2\nborrow 1\nclose 4\ndrop 4\nborrow 1\nclose 1\ndrop 1\n",
                    });
                    if mode != "process" {
                        expected.push_str("borrow 3\nclose 5\ndrop 5\nborrow 3\nclose 3\ndrop 3\nclose 6\ndrop 6\n");
                    }
                    assert_eq!(
                        fs::read_to_string(&log).unwrap(),
                        expected.repeat(rounds as usize),
                        "{context}"
                    );
                    let handlers = events
                        .iter()
                        .filter(|e| e["kind"] == "emit" && e["message"] == "owner handler")
                        .count();
                    assert_eq!(
                        handlers,
                        if mode == "fault" || mode == "joined-fault" {
                            3
                        } else {
                            0
                        },
                        "{context}"
                    );
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
