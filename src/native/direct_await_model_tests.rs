use super::*;
use serde_json::Value as Json;
#[test]
fn compiled_direct_and_cold_awaits_preserve_explicit_handoffs_and_borrowed_results() {
    let root = std::env::temp_dir().join(format!("tellur-direct-await-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let original = crate::compile(
        include_str!("../../playground/direct_await.tellur"),
        crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
            .unwrap(),
    )
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
    let expanded = original
        .expanded("startup")
        .unwrap()
        .expanded("main")
        .unwrap();
    for (expanded, program) in [(false, original), (true, expanded)] {
        let entry = program.regions().position(|r| r.name == "main").unwrap();
        let normal = format!(
            "fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{drive(host,args,&MODEL_{entry})}}"
        );
        let replacement = format!(
            r#"fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{
            let quotas=std::env::var("QUOTAS").unwrap().split(',').map(|s|s.parse::<usize>().unwrap()).collect::<Vec<_>>();
            let mut quotas=quotas.into_iter().cycle();drive_direct_probe(host,args,&MODEL_{entry},||quotas.next().unwrap())
        }}"#
        );
        let generated = source(&program, "main").unwrap();
        assert_eq!(generated.matches(&normal).count(), 1);
        assert_eq!(generated.matches("state.queue_invocation(").count(), 3);
        assert_eq!(generated.matches("state.queue_task(").count(), 2);
        let generated = generated.replacen(&normal, &replacement, 1).replacen(
            CONTEXTS,
            &format!("{CONTEXTS}\n{}", include_str!("direct_await_model.rs")),
            1,
        );
        for opt in [0, 2] {
            let binary = root.join(format!("progress-{expanded}-{opt}"));
            let project = program.native_project("main", &binary).unwrap();
            fs::write(&project.2, &generated).unwrap();
            program.finish_native_build(project, opt).unwrap();
            for quotas in ["99", "1", "0,3,1"] {
                let out = Command::new(&binary)
                    .args(["--json", "--fuel", "1000"])
                    .env("QUOTAS", quotas)
                    .output()
                    .unwrap();
                assert!(
                    out.status.success(),
                    "expanded={expanded} opt={opt} quotas={quotas}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
                let observed: Json = serde_json::from_slice(&out.stdout).unwrap();
                assert_eq!(observed, expected);
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
