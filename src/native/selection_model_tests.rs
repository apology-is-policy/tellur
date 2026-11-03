use super::*;
use serde_json::Value as Json;

#[test]
fn compiled_selection_rejects_before_callback_moves_effects_and_pending_delivery() {
    let root = std::env::temp_dir().join(format!("tellur-selection-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let original = crate::compile(
        include_str!("../../tests/fixtures/native_resume_loops.tellur"),
        crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
            .unwrap(),
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let expanded = original
        .expanded("startup")
        .unwrap()
        .expanded("main")
        .unwrap();
    for (is_expanded, program) in [(false, original), (true, expanded)] {
        let entry = program.regions().position(|r| r.name == "main").unwrap();
        let normal = format!(
            "fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{drive(host,args,&MODEL_{entry})}}"
        );
        let replacement = format!(
            r#"fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{
            let quotas=std::env::var("TELLUR_NATIVE_MODEL_QUOTAS").unwrap().split(',').map(|s|s.parse::<usize>().unwrap()).collect::<Vec<_>>();
            drive_selection(host,&MODEL_{entry},args,std::env::var("TELLUR_SELECTION_PROBES").unwrap()=="true",&quotas)
        }}"#
        );
        let generated = source(&program, "main").unwrap();
        assert_eq!(generated.matches(&normal).count(), 1);
        let generated = generated.replacen(&normal, &replacement, 1).replacen(
            CONTEXTS,
            &format!("{CONTEXTS}\n{}", include_str!("selection_model.rs")),
            1,
        );
        let mut baseline = None;
        for opt in [0, 2] {
            let binary = root.join(format!("selection-{is_expanded}-{opt}"));
            let project = program.native_project("main", &binary).unwrap();
            fs::write(&project.2, &generated).unwrap();
            program.finish_native_build(project, opt).unwrap();
            for probes in [false, true] {
                for quotas in ["1", "0,3,1"] {
                    let output = Command::new(&binary)
                        .args(["--json", "--fuel", "1000", "--", "false"])
                        .env("TELLUR_SELECTION_PROBES", probes.to_string())
                        .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "expanded={is_expanded} opt={opt} probes={probes} quotas={quotas}: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    let observed: Json = serde_json::from_slice(&output.stdout).unwrap();
                    if let Some(baseline) = &baseline {
                        assert_eq!(&observed, baseline);
                    } else {
                        baseline = Some(observed);
                    }
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
