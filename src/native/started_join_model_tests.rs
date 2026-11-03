use super::*;
use serde_json::Value as Json;
#[test]
fn compiled_started_join_phases_preserve_owned_and_borrowed_results() {
    let root = std::env::temp_dir().join(format!("tellur-started-join-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let original = crate::compile(
        include_str!("../../tests/fixtures/task_completion_model.tellur"),
        crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
            .unwrap(),
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let expected = serde_json::to_value(
        original
            .run(&crate::runtime::Scenario {
                entry: "main".into(),
                arguments: vec![],
                calls: vec![],
            })
            .unwrap(),
    )
    .unwrap();
    let expanded = original.expanded("start").unwrap();
    for (expanded, program) in [(false, original), (true, expanded)] {
        let entry = program.regions().position(|r| r.name == "main").unwrap();
        let normal = format!(
            "fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{drive(host,args,&MODEL_{entry})}}"
        );
        let replacement = format!(
            r#"fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{
            let quotas=std::env::var("QUOTAS").unwrap().split(',').map(|s|s.parse::<usize>().unwrap()).collect::<Vec<_>>();
            let mut quotas=quotas.into_iter().cycle();
            drive_started_probe(host,args,&MODEL_{entry},std::env::var("PHASE").unwrap().parse().unwrap(),||quotas.next().unwrap())
        }}"#
        );
        let generated = source(&program, "main").unwrap();
        assert_eq!(generated.matches(&normal).count(), 1);
        assert_eq!(generated.matches("state.queue_task(").count(), 1);
        let generated = generated
            .replacen(&normal, &replacement, 1)
            .replacen("state.queue_task(", "state.queue_started(", 1)
            .replacen(
                CONTEXTS,
                &format!("{CONTEXTS}\n{}", include_str!("started_join_model.rs")),
                1,
            );
        for opt in [0, 2] {
            let binary = root.join(format!("join-{expanded}-{opt}"));
            let project = program.native_project("main", &binary).unwrap();
            fs::write(&project.2, &generated).unwrap();
            program.finish_native_build(project, opt).unwrap();
            for phase in 0..4 {
                for quotas in ["99", "1", "0,3,1"] {
                    let out = Command::new(&binary)
                        .args(["--json", "--fuel", "1000"])
                        .env("QUOTAS", quotas)
                        .env("PHASE", phase.to_string())
                        .output()
                        .unwrap();
                    assert!(
                        out.status.success(),
                        "expanded={expanded} opt={opt} phase={phase} quotas={quotas}: {}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                    let observed: Json = serde_json::from_slice(&out.stdout).unwrap();
                    assert_eq!(observed, expected);
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
