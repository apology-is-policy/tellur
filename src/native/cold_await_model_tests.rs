use super::*;
use serde_json::Value as Json;
#[test]
fn compiled_cold_await_preparation_advances_parent_beside_retained_borrowers() {
    let root = std::env::temp_dir().join(format!("tellur-cold-progress-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let original = crate::compile(
        include_str!("../../playground/owner_await_progress.tellur"),
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
            drive_cold_progress(host,&MODEL_{entry},args,std::env::var("STATE").unwrap().parse().unwrap(),&quotas)
        }}"#
        );
        let generated = source(&program, "main").unwrap();
        assert_eq!(generated.matches(&normal).count(), 1);
        let generated = generated.replacen(&normal, &replacement, 1).replacen(
            CONTEXTS,
            &format!("{CONTEXTS}\n{}", include_str!("cold_await_model.rs")),
            1,
        );
        for opt in [0, 2] {
            let binary = root.join(format!("progress-{expanded}-{opt}"));
            let project = program.native_project("main", &binary).unwrap();
            fs::write(&project.2, &generated).unwrap();
            program.finish_native_build(project, opt).unwrap();
            for state in 0..3 {
                for quotas in ["99", "1", "0,3,1"] {
                    let out = Command::new(&binary)
                        .args(["--json", "--fuel", "1000"])
                        .env("STATE", state.to_string())
                        .env("QUOTAS", quotas)
                        .output()
                        .unwrap();
                    assert!(
                        out.status.success(),
                        "expanded={expanded} opt={opt} state={state} quotas={quotas}: {}",
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
