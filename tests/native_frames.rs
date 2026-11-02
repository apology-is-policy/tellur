mod support;
use std::{fs, process::Command};
use tellur::{CheckedProgram, compile, fault::Report, manifest::Manifest, runtime::Scenario};

const SOURCE: &str = r#"
struct message {
 string text;
 constructor(string text) {self.text=move text;}
}
control lend(ref mutable string input) exits lent {ref mutable string value from input} {
 exit lent {input};
}
control revise(ref mutable string input, string text) exits done {
 deref input=move text;
 exit done;
}
control main() exits done {
 auto owned=message("before");
 mutable string parent=move owned.text;
 delta lend(borrow mutable parent) {lent {auto child}=>lent}
 [lent]:revise(child,"after");
 emit(child);
 parent="released";
 emit(borrow parent);
 exit done;
}
"#;

fn checked(padding: usize) -> CheckedProgram {
    let mut source = String::new();
    for i in 0..padding {
        source.push_str(&format!(
            "control unused_{i}(string text) exits done {{emit(borrow text);exit done;}}\n"
        ));
    }
    source.push_str(SOURCE);
    compile(
        &source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap()
}

#[test]
fn frame_capacity_depends_on_local_storage_not_global_id_ranges() {
    let mut capacities = vec![];
    for padding in [0, 128] {
        let p = checked(padding);
        let source = support::scenario_executable(&p)
            .native_source("main")
            .unwrap();
        let mut selected = vec![];
        for (id, region) in p.regions().enumerate() {
            let prefix = format!("static MODEL_{id}:Control=");
            let descriptor = source
                .split(&prefix)
                .nth(1)
                .unwrap()
                .split(';')
                .next()
                .unwrap();
            let slots = region.slots.len();
            let atoms = region.atoms.len();
            assert!(
                descriptor.contains(&format!(",slots:{slots},atoms:{atoms},")),
                "{}",
                region.name
            );
            if !region.name.starts_with("unused_") {
                selected.push((region.name.clone(), slots, atoms));
            }
        }
        if padding > 0 {
            let main = p.region("main").unwrap();
            assert!(*main.slots.keys().max().unwrap() > main.slots.len() * 10);
        }
        capacities.push(selected);
    }
    assert_eq!(capacities[0], capacities[1]);
}

#[test]
fn dense_frames_preserve_cross_call_borrows_and_every_fuel_boundary() {
    let root = std::env::temp_dir().join(format!("tellur-dense-frames-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario).unwrap()).unwrap();
    let original = checked(16);
    for (expanded, p) in [
        (false, original.clone()),
        (true, original.expanded("main").unwrap()),
    ] {
        for opt in [0, 2] {
            let binary = root.join(format!("frames-{expanded}-{opt}"));
            support::scenario_executable(&p)
                .build_native("main", &binary, opt)
                .unwrap();
            let mut completed = false;
            for fuel in 0..128 {
                let output = Command::new(&binary)
                    .args([
                        "--scenario",
                        input.to_str().unwrap(),
                        "--fuel",
                        &fuel.to_string(),
                    ])
                    .output()
                    .unwrap();
                match p.run_detailed(&scenario, fuel) {
                    Ok(result) => {
                        assert!(
                            output.status.success(),
                            "{}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                        assert_eq!(
                            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
                            serde_json::to_value(result).unwrap()
                        );
                        completed = true;
                        break;
                    }
                    Err(failure) => {
                        assert_eq!(output.status.code(), Some(1));
                        assert!(output.stdout.is_empty());
                        let actual: Report = serde_json::from_slice(&output.stderr).unwrap();
                        assert!(actual.message.contains("E_FUEL"));
                        assert_eq!(actual.phase, failure.report.phase);
                        assert_eq!(actual.site, failure.report.site);
                        assert_eq!(actual.activations, failure.report.activations);
                    }
                }
            }
            assert!(
                completed,
                "test must cover both exhaustion and normal completion"
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}
