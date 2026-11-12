//! Linked C-table proof for private launch registration, independent of source admission.
use super::*;

#[test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn linked_adapter_tables_preserve_live_scripted_and_legacy_launch_behavior() {
    use serde_json::json;
    let directory =
        std::env::temp_dir().join(format!("tellur-adapter-launch-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let manifest = json!({"version":"0.1","types":[],"rust":{"source":"not-read.rs","lockfile":"Cargo.lock"},
        "primitives":[{"name":"inspect","parameters":[{"name":"input","type":"String","mode":"borrow"}],
        "outcomes":[{"name":"ready","fields":[{"name":"output","type":"String"}]}],
        "effects":[],"returns":true,"rust":{"execution":"immediate"}}]});
    let info = json!({"abi":1,"manifest":manifest.to_string(),"entry":"main","sites":"{}",
        "entries":[{"name":"main","display":"main","arguments":["String"],"exits":[["done",0]]}]})
    .to_string();
    let constant = info
        .as_bytes()
        .iter()
        .map(|b| format!("\\{b:02X}"))
        .collect::<String>();
    let header = format!(
        "@info = private constant [{} x i8] c\"{}\"\n",
        info.len(),
        constant
    );
    let source = r#"
%operation = type { ptr, i64, { i32, i32, ptr } }
%table = type { i32, i32, ptr, i64 }
@operation = private constant %operation { ptr @name, i64 7, { i32, i32, ptr } { i32 1, i32 0, ptr @adapter } }
@table = private constant %table { i32 TABLE_VERSION, i32 0, ptr @operation, i64 1 }
@entries = private constant [1 x ptr] [ptr @entry]
@name = private constant [7 x i8] c"inspect"
@ready = private constant [5 x i8] c"ready"
@done = private constant [4 x i8] c"done"
@foreign = private constant [7 x i8] c"foreign"
declare i32 @tellur_rt_v1_launch(ptr, i64, ptr, i64)
declare i32 @tellur_rt_v2_launch(ptr, i64, ptr, i64, ptr)
declare i64 @tellur_rt_v1_primitive(ptr, ptr, i64, ptr, i64)
declare i32 @tellur_rt_v1_route(ptr, i64, ptr, i64, ptr, i64)
declare void @tellur_rt_v1_complete(ptr, ptr, i64, ptr, i64)
declare i64 @tellur_adapter_v1_value(ptr, i32, i64, ptr, i64)
declare void @tellur_adapter_v1_complete(ptr, ptr, i64, ptr, i64)
define void @adapter(ptr %ctx, ptr %args, i64 %count) {
  %slot = alloca i64
  %value = call i64 @tellur_adapter_v1_value(ptr %ctx, i32 3, i64 0, ptr @foreign, i64 7)
  store i64 %value, ptr %slot
  call void @tellur_adapter_v1_complete(ptr %ctx, ptr @ready, i64 5, ptr %slot, i64 1)
  ret void
}
define void @entry(ptr %ctx, ptr %args, i64 %count) {
  %slot = alloca i64
  %outcome = call i64 @tellur_rt_v1_primitive(ptr %ctx, ptr @name, i64 7, ptr %args, i64 %count)
  %matched = call i32 @tellur_rt_v1_route(ptr %ctx, i64 %outcome, ptr @ready, i64 5, ptr %slot, i64 1)
  call void @tellur_rt_v1_complete(ptr %ctx, ptr @done, i64 4, ptr %slot, i64 1)
  ret void
}
define i32 @main() {
  %status = call i32 @tellur_rt_v2_launch(ptr @info, i64 INFO_LENGTH, ptr @entries, i64 1, ptr @table)
  ret i32 %status
}
"#;
    let dummy = crate::compile(
        "outcome result {@exit 0 done} control main() exits result {exit done;}",
        crate::manifest::Manifest::from_json(r#"{"version":"0.1","types":[],"primitives":[]}"#)
            .unwrap(),
    )
    .unwrap();
    let scenario = directory.join("scenario.json");
    fs::write(&scenario,json!({"entry":"main","arguments":[{"kind":"string","value":"input"}],"calls":[{"operation":"inspect","outcome":"ready","fields":[{"kind":"string","value":"script"}]}]}).to_string()).unwrap();
    let mut reports = vec![];
    for opt in [0, 2] {
        for mode in ["valid", "invalid", "legacy"] {
            let mut source = format!("{header}{source}")
                .replace("INFO_LENGTH", &info.len().to_string())
                .replace("TABLE_VERSION", if mode == "invalid" { "2" } else { "1" });
            if mode == "legacy" {
                source = source
                    .replace(
                        "@tellur_rt_v2_launch(ptr @info",
                        "@tellur_rt_v1_launch(ptr @info",
                    )
                    .replace("ptr @entries, i64 1, ptr @table)", "ptr @entries, i64 1)");
            }
            let executable = dummy
                .build_llvm_source(source, &directory.join(format!("{mode}-{opt}")), opt)
                .unwrap()
                .executable;
            for scripted in [false, true] {
                let mut command = Command::new(&executable);
                if scripted {
                    command.arg("--scenario").arg(&scenario);
                } else {
                    command.args(["--json", "input"]);
                }
                let result = command.output().unwrap();
                if mode != "valid" {
                    assert_eq!(result.status.code(), Some(1), "{mode}/{opt}: {result:?}");
                    let err = String::from_utf8_lossy(&result.stderr);
                    assert!(
                        err.contains(if mode == "invalid" {
                            "incompatible operation table"
                        } else {
                            "unsupported ABI invocation mode"
                        }),
                        "{err}"
                    );
                } else {
                    assert!(result.status.success(), "{result:?}");
                    assert!(result.stderr.is_empty());
                    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
                    assert_eq!(report["outcome"], "done");
                    assert_eq!(
                        report["fields"],
                        json!([{"kind":"string","value":if scripted {"script"} else {"foreign"}}])
                    );
                    let calls = report["events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|e| e["kind"] == "primitive")
                        .collect::<Vec<_>>();
                    assert_eq!(calls.len(), 1);
                    assert_eq!(calls[0]["operation"], "inspect");
                    assert_eq!(
                        calls[0]["arguments"],
                        json!([{"kind":"string","value":"input"}])
                    );
                    assert_eq!(calls[0]["fields"], report["fields"]);
                    reports.push((scripted, result.stdout));
                }
            }
        }
    }
    assert_eq!(reports[0], reports[2]);
    assert_eq!(reports[1], reports[3]);
    fs::remove_dir_all(directory).unwrap();
}
