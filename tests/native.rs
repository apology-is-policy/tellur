mod support;
use serde_json::{Value as Json, json};
use std::{fs, path::Path, process::Command};
use tellur::{
    compile,
    ir::Value,
    manifest::Manifest,
    runtime::{Scenario, ScriptCall},
};
fn opaque(ty: &str, token: &str) -> Value {
    Value::Opaque {
        ty: ty.into(),
        token: token.into(),
    }
}
fn call(op: &str, out: &str, fields: Vec<Value>) -> ScriptCall {
    ScriptCall {
        operation: op.into(),
        outcome: out.into(),
        fields,
    }
}
fn scenario(entry: &str, arguments: Vec<Value>, calls: Vec<ScriptCall>) -> Scenario {
    Scenario {
        entry: entry.into(),
        arguments,
        calls,
    }
}
fn run(binary: &Path, scenario: &Scenario, file: &Path) -> Json {
    fs::write(file, serde_json::to_vec(scenario).unwrap()).unwrap();
    let output = Command::new(binary)
        .arg("--scenario")
        .arg(file)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn native_and_llvm_match_reference_before_and_after_expansion_and_optimization() {
    let root = std::env::temp_dir().join(format!("path-native-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let mut manifest = Manifest::from_json(include_str!("../examples/manifest.json")).unwrap();
    manifest.types.push(tellur::manifest::TypeDef::Record {
        name: "text_pair".into(),
        fields: ["left", "right"]
            .into_iter()
            .map(|name| tellur::manifest::Field {
                name: name.into(),
                ty: "String".into(),
            })
            .collect(),
    });
    let mut source = [
        include_str!("../examples/refresh.tellur"),
        include_str!("../examples/port.tellur"),
        include_str!("../examples/fetch_retry.tellur"),
        include_str!("../examples/partial_moves.tellur"),
        include_str!("../examples/optional_handle.tellur"),
        include_str!("../examples/owned_variant.tellur"),
    ]
    .join("\n");
    source.push_str("region forever() exits Done { loop L() { [Entry]: again L(); } }\n");
    source.push_str("region swap(p: HandlePair) exits Swapped(pair: HandlePair) { [Entry]: exit Swapped(HandlePair(p.right,p.left,p.label)); } region looping(p: HandlePair) exits Done(pair: HandlePair) { loop L(current: HandlePair = p,first: Bool = true) { [Entry]: delta first -> [true => First,false => Later]; [First]: delta swap(current) -> [Swapped(next) => Ready]; [Ready]: again L(next,false); [Later]: exit Done(current); } } region loop_caller(p: HandlePair) exits Done(pair: HandlePair) { [Entry]: delta looping(p) -> [Done(result) => Ready]; [Ready]: exit Done(result); }\n");
    let mut scenarios = vec![];
    for file in [
        include_str!("../examples/double-failure.json"),
        include_str!("../examples/refresh-double-failure.json"),
        include_str!("../examples/retry-success.json"),
        include_str!("../examples/partial-moves-double-failure.json"),
        include_str!("../examples/optional-close-failure.json"),
        include_str!("../examples/variant-missing.json"),
    ] {
        scenarios.push(serde_json::from_str(file).unwrap());
    }
    for entry in ["load", "refresh", "optional_file", "variant_file"] {
        for outcome in ["NotFound", "Denied"] {
            scenarios.push(scenario(
                entry,
                vec![Value::String("file".into())],
                vec![call("open", outcome, vec![])],
            ));
        }
        for read_bad in [false, true] {
            for close_bad in [false, true] {
                let mut calls = vec![call("open", "Opened", vec![opaque("Handle", "h")])];
                if entry == "load" || entry == "refresh" {
                    calls.push(call(
                        "read",
                        if read_bad { "Failed" } else { "Data" },
                        vec![if read_bad {
                            opaque("ReadError", "read")
                        } else {
                            Value::Bytes(vec![0, 255, 97])
                        }],
                    ));
                }
                calls.push(call(
                    "close",
                    if close_bad { "Failed" } else { "Closed" },
                    if close_bad {
                        vec![opaque("CloseError", "close")]
                    } else {
                        vec![]
                    },
                ));
                scenarios.push(scenario(entry, vec![Value::String("file".into())], calls));
            }
        }
    }
    for (text, out, fields) in [
        ("443", "Number", vec![Value::U32(443)]),
        ("0", "Number", vec![Value::U32(0)]),
        ("65536", "Number", vec![Value::U32(65536)]),
        ("bad", "Malformed", vec![]),
        ("4294967296", "Overflow", vec![]),
    ] {
        scenarios.push(scenario(
            "port",
            vec![Value::Construct {
                ty: "Option<String>".into(),
                name: "Some".into(),
                fields: vec![Value::String(text.into())],
            }],
            vec![call("parse_u32", out, fields)],
        ));
    }
    scenarios.push(scenario(
        "port",
        vec![Value::Construct {
            ty: "Option<String>".into(),
            name: "None".into(),
            fields: vec![],
        }],
        vec![],
    ));
    scenarios.push(scenario(
        "fetch_retry",
        vec![Value::String("url".into())],
        vec![call("fetch", "Cancelled", vec![])],
    ));
    scenarios.push(scenario(
        "fetch_retry",
        vec![Value::String("url".into())],
        vec![
            call("fetch", "Unavailable", vec![opaque("NetworkError", "net")]),
            call("checked_mul", "Product", vec![Value::U32(100)]),
            call("pause", "Cancelled", vec![]),
        ],
    ));
    let pair = Value::Construct {
        ty: "HandlePair".into(),
        name: "HandlePair".into(),
        fields: vec![
            opaque("Handle", "left"),
            opaque("Handle", "right"),
            Value::String("label".into()),
        ],
    };
    scenarios.push(scenario("loop_caller", vec![pair], vec![]));
    // Bounded generated programs mix scoped paths, immutable joins, calls and loops.
    for n in 0..16 {
        source.push_str(&format!("region gen{n}(flag: Bool) exits Done(n: U32,text: String) {{ [Entry]: delta flag -> [true => A,false => B]; [A] over &: {{ [Entry]: emit(\"branch {n}\"); }} [A]: value := {n}; [B]: value := {}; loop L(number: U32 = value,first: Bool = true) {{ [Entry]: delta first -> [true => Start,false => End]; [Start]: again L(number,false); [End]: exit Done(number,\"é😀\\n\"); }} }}\n",n+1));
        for b in [false, true] {
            scenarios.push(scenario(&format!("gen{n}"), vec![Value::Bool(b)], vec![]));
        }
    }
    source.push_str("region two_files(path: String) exits Done { [Entry]: first = delta load(path) -> [Loaded(a) => A,Missing => B,Failed(e) => C]; second = delta load(\"second\") -> [Loaded(b) => D,Missing => E,Failed(f) => F]; [C & F]: emit(\"both failed\"); [Entry]: exit Done; }\n");
    let loader_cases = scenarios
        .iter()
        .filter(|s| s.entry == "load")
        .cloned()
        .collect::<Vec<_>>();
    for left in &loader_cases {
        for right in &loader_cases {
            let mut calls = left.calls.clone();
            calls.extend(right.calls.clone());
            scenarios.push(scenario(
                "two_files",
                vec![Value::String("first".into())],
                calls,
            ));
        }
    }
    source.push_str("region expressions() exits Done(flag: Bool,bytes: Bytes,empty: Unit) { [Entry]: flag := !false && (3 != 4) && (2 >= 1) && (\"a\" == \"a\") && (unit == unit) || false; exit Done(flag,bytes\"00ff7a\",unit); }\n");
    scenarios.push(scenario("expressions", vec![], vec![]));
    source.push_str("region nested_option() exits Done(file: Option<Option<U32>>) { [Entry]: exit Done(Some(None)); }\n");
    scenarios.push(scenario("nested_option", vec![], vec![]));
    source.push_str(r#"
        control duplicate_text(&string input) exits auto {
            [entry]: string copy=clone(input); emit(input); exit copied {copy};
        }
        control owned_values(bool flag) exits auto {
            [entry]:
                mutable text_pair pair=text_pair {"left", "right"};
                auto left=&mutable pair.left;
                auto right=&pair.right;
                *left="changed";
                delta duplicate_text(right) {copied {string copied}=>ready}
            [ready]: drop(left); drop(right); string taken=pair.left; pair.left="restored";
                delta flag {true=>yes,false=>no}
            [yes]: pair.right="branch";
            [entry] over &: { [entry]: string temporary="scope storage"; emit(&temporary); }
            loop carry(text_pair current=pair, bool first=true) {
                [entry]: delta first {true=>start,false=>finish}
                [start]: auto view=&current.right; emit(view); drop(view); again carry(current,false);
                [finish]: exit done {current};
            }
        }
        control owned_caller(bool flag) exits auto {
            [entry]: delta owned_values(flag) {done {text_pair pair}=>ready}
            [ready]: exit done {pair};
        }
    "#);
    for flag in [true, false] {
        scenarios.push(scenario("owned_caller", vec![Value::Bool(flag)], vec![]));
    }
    let program = compile(&source, manifest).unwrap();
    let expected = scenarios
        .iter()
        .map(|s| serde_json::to_value(program.run(s).unwrap()).unwrap())
        .collect::<Vec<_>>();
    let mut expanded = program.clone();
    for name in program.regions().map(|r| r.name.clone()) {
        expanded = expanded.expanded(&name).unwrap();
    }
    for (expanded_flag, p) in [(false, &program), (true, &expanded)] {
        for opt in [0, 2] {
            let binary = root.join(format!("program-{expanded_flag}-{opt}"));
            let build = support::scenario_executable(p)
                .build_native("refresh", &binary, opt)
                .unwrap();
            let llvm = fs::read_to_string(build.llvm).unwrap();
            assert!(llvm.contains("target triple") && llvm.contains("define"));
            for (s, expected) in scenarios.iter().zip(&expected) {
                assert_eq!(
                    &run(&binary, s, &root.join("scenario.json")),
                    expected,
                    "entry {} expanded {expanded_flag} opt {opt}",
                    s.entry
                );
            }
            let output = Command::new(&binary)
                .args(["--entry", "forever", "--fuel", "100"])
                .output()
                .unwrap();
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("E_FUEL"));
            let malformed =
                json!({"entry":"load","arguments":[{"kind":"u32","value":1}],"calls":[]});
            fs::write(root.join("bad.json"), malformed.to_string()).unwrap();
            assert!(
                !Command::new(&binary)
                    .arg("--scenario")
                    .arg(root.join("bad.json"))
                    .status()
                    .unwrap()
                    .success()
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn live_adapters_execute_real_inputs_and_enforce_contracts() {
    let root = std::env::temp_dir().join(format!("path-live-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let source=[
        include_str!("../playground/count.tellur").replace("control main()","control count_main()").replace("process_result","count_process_result"),
        include_str!("../playground/port.tellur").replace("control main()","control port_main()").replace("process_result","port_process_result"),
        include_str!("../playground/read_file.tellur").replace("control main()","control file_main()").replace("process_result","file_process_result"),
        "region divide(a: U32,b: U32) exits done(n: U32), Zero { [Entry]: delta checked_div(a,b) -> [quotient(n) => Q,zero_divisor => Z]; [Q]: exit done(n); [Z]: exit Zero; }".into(),
        "region arithmetic(a: U32,b: U32) exits sum(n: U32), overflow { [Entry]: delta checked_add(a,b) -> [sum(n) => S,overflow => O]; [S]: exit sum(n); [O]: exit overflow; }".into(),
        "region text() exits done(text: String), Bad { [Entry]: delta concat(\"é\",\"😀\") -> [text(s) => T]; delta decode_utf8(bytes\"ff\") -> [text(unused) => V,invalid => I]; [T & I]: exit done(s); [V]: exit Bad; }".into(),
    ].join("\n");
    let p = compile(&source, manifest).unwrap();
    let executable = root.join("live");
    support::scenario_executable(&p)
        .build_native("count_main", &executable, 2)
        .unwrap();
    let run = |entry: &str, args: &[&str]| {
        let output = Command::new(&executable)
            .args(["--json", "--entry", entry, "--"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Json>(&output.stdout).unwrap()
    };
    assert_eq!(
        run("count_main", &[])["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "emit")
            .count(),
        5
    );
    for (input, expected) in [
        ("8080", "ready"),
        ("65535", "ready"),
        ("0", "invalid"),
        ("65536", "invalid"),
        ("abc", "invalid"),
        ("+1", "invalid"),
        ("4294967296", "invalid"),
    ] {
        assert_eq!(run("parse_port", &[input])["outcome"], expected);
    }
    assert_eq!(run("divide", &["8", "2"])["fields"][0]["value"], 4);
    assert_eq!(run("divide", &["8", "0"])["outcome"], "Zero");
    assert_eq!(
        run("arithmetic", &["4294967295", "1"])["outcome"],
        "overflow"
    );
    assert_eq!(run("text", &[])["fields"][0]["value"], "é😀");
    let filename = root.join("data.txt");
    fs::write(&filename, "real file é😀\n").unwrap();
    let result = run("show_file", &[filename.to_str().unwrap()]);
    assert_eq!(result["outcome"], "done");
    let operations = result["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["operation"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(operations, vec!["open", "read", "close", "decode_utf8"]);
    assert_eq!(
        run("show_file", &[root.join("absent.txt").to_str().unwrap()])["outcome"],
        "missing"
    );
    fs::write(&filename, [255, 254]).unwrap();
    assert_eq!(
        run("show_file", &[filename.to_str().unwrap()])["outcome"],
        "failed"
    );
    fs::remove_dir_all(root).unwrap();
}
