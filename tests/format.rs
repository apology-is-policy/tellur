use std::{fs, path::Path, process::Command};
use tellur::{
    compile,
    format::{self, Options},
    ir::Value,
    manifest::Manifest,
    runtime::Scenario,
};

fn formatted(text: &str) -> String {
    let result = format::source(text, Options::default()).unwrap();
    assert_eq!(
        result,
        format::source(&result, Options::default()).unwrap(),
        "format twice: {text}"
    );
    result
}

#[test]
fn labels_groups_and_nested_adapters_have_distinct_layout() {
    let source = "control main(bool flag) exits done {delta flag{true=>yes,false=>exit done}[yes]:{emit(\"selected\");[entry]:emit(\"still selected\");}path resumed=entry;[resumed]:exit delta operation(){done=>[observed]:delta other(){done=>done{}}};}";
    assert_eq!(
        formatted(source),
        "control main(bool flag) exits done {\n    delta flag {\n        true => yes,\n        false => exit done\n    }\n    [yes]: {\n        emit(\"selected\");\n        [entry]: emit(\"still selected\");\n    }\n    path resumed = entry;\n    [resumed]: exit delta operation() {\n        done => [observed]: delta other() {\n            done => done {}\n        }\n    };\n}\n"
    );
}

#[test]
fn comments_literals_and_line_endings_survive_byte_for_byte() {
    let text = "// lead 😀  \r\ncontrol main() exits done { // body\r\nstring text=\"a\n//not a comment {\\\"😀\";// trailing  \r\n\r\n// paragraph\r\nemit(text);exit done;} // end";
    let result = formatted(text);
    assert!(result.contains("\"a\n//not a comment {\\\"😀\""));
    for comment in [
        "// lead 😀  \r\n",
        "// body\r\n",
        "// trailing  \r\n",
        "// paragraph\r\n",
        "// end\r\n",
    ] {
        assert!(
            result.contains(comment),
            "missing {comment:?} in {result:?}"
        );
    }
    assert!(result.contains("// trailing  \r\n\r\n    // paragraph"));
    assert_eq!(formatted(" // only comment  "), "// only comment  \n");
    assert_eq!(formatted(" \n\t"), "");
    assert_eq!(
        formatted("control main() exits done {exit done;//closing\n}"),
        "control main() exits done {\n    exit done; //closing\n}\n"
    );
    // Exercise comment boundaries in types, contracts, calls, masks and arms.
    let tokens: Vec<_> = "control main < t : clone > ( t input ) exits done { helper < t > ( input ) ; delta true { true => yes , false => no } [ yes | no ] : exit done ; }".split_whitespace().collect();
    for boundary in 0..=tokens.len() {
        let source = format!(
            "{} // boundary 😀  \n {}",
            tokens[..boundary].join(" "),
            tokens[boundary..].join(" ")
        );
        assert!(
            formatted(&source).contains("// boundary 😀  \n"),
            "boundary {boundary}"
        );
    }
}

#[test]
fn generics_comparisons_origins_annotations_and_legacy_spellings() {
    let source = "struct pair<t>['a]{ref t left<-'a;}outcome status{@exit 0 done,@exit 1 failed {string text}}trait reader {type item:clone;control peek(ref self input) exits found {ref item value from input};}implement reader for string {type item=string;control peek(ref self input) exits found {ref item value from input} {exit found{input};}}control main<u:clone>(u input) exits done{bool less=1<2;vector<vector<u32>> values=[[1]];exit done;}";
    let result = formatted(source);
    for part in [
        "pair<t>['a]",
        "ref t left <- 'a;",
        "1 < 2",
        "vector<vector<u32>> values",
        "@exit 0 done",
        "type item = string;",
    ] {
        assert!(result.contains(part), "missing {part}: {result}");
    }
    let legacy = "region main() exits { Done } { [Entry]: site = Δ (1 < 2) -> [ True => Yes, False => No ]; [Yes | No]: exit Done; }";
    assert!(formatted(legacy).contains("-> [\n        True => Yes,\n        False => No\n    ];"));
    // Failed type speculation must not turn comparison angles into generic angles.
    assert!(formatted("control main() exits done {bool test=a<b;exit done;}").contains("a < b"));
    assert!(formatted("control main() exits done {bool test=a>b;exit done;}").contains("a > b"));
    assert!(
        formatted("control main<t>(t input) exits done {helper<t>(input);exit done;}")
            .contains("helper<t>(input);")
    );
}

#[test]
fn tabs_blank_lines_and_comments_in_empty_groups_are_stable() {
    let source = "control main() exits done {[entry]:{\n// retained\n}\n\nexit done;}";
    let options = Options {
        tab_size: 2,
        insert_spaces: false,
        ..Options::default()
    };
    let text = format::source(source, options).unwrap();
    assert_eq!(
        text,
        "control main() exits done {\n\t[entry]: {\n\t\t// retained\n\t}\n\n\texit done;\n}\n"
    );
    assert_eq!(text, format::source(&text, options).unwrap());
    let spaces = format::source(
        source,
        Options {
            insert_spaces: true,
            ..options
        },
    )
    .unwrap();
    assert!(spaces.contains("\n    // retained\n  }"));
    assert!(
        format::source(
            source,
            Options {
                tab_size: 0,
                ..options
            }
        )
        .is_err()
    );
    assert!(format::source("control main( {", options).is_err());
}

#[test]
fn authored_wrapping_and_comments_inside_headers_and_payloads_are_retained() {
    let source = "control main(\nref string input,// source\nbool flag)\nexits found {\nref string value from input\n},missing {\nexit found { // payload\ninput\n};}";
    let result = formatted(source);
    assert_eq!(
        result,
        "control main(\n    ref string input, // source\n    bool flag\n)\n    exits found {\n        ref string value from input\n    },\n    missing\n{\n    exit found { // payload\n        input\n    };\n}\n"
    );
    assert!(
        formatted("control main() exits done {vector<\nu32\n> values=[\n1,\n2\n];exit done;}")
            .contains("vector<\n        u32\n    > values = [\n        1,\n        2\n    ];")
    );
}

#[test]
fn every_maintained_source_is_parseable_and_idempotent_after_formatting() {
    fn walk(path: &Path, count: &mut usize) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if !["build", "target", ".tellur-build"]
                    .contains(&path.file_name().unwrap().to_str().unwrap())
                {
                    walk(&path, count);
                }
            } else if path
                .extension()
                .is_some_and(|e| e == "tellur" || e == "path")
            {
                let source = fs::read_to_string(&path).unwrap();
                for max_width in [40, 240] {
                    let options = Options {
                        max_width,
                        ..Default::default()
                    };
                    let result = format::source(&source, options).unwrap();
                    assert_eq!(
                        result,
                        format::source(&result, options).unwrap(),
                        "{} at width {max_width}",
                        path.display()
                    );
                }
                let result = format::source(&source, Options::default())
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert_eq!(
                    result,
                    format::source(&result, Options::default()).unwrap(),
                    "{}",
                    path.display()
                );
                let manifest_path = path
                    .parent()
                    .unwrap()
                    .ancestors()
                    .map(|p| p.join("manifest.json"))
                    .find(|p| p.is_file())
                    .unwrap();
                let manifest = Manifest::from_json_at(
                    &fs::read_to_string(&manifest_path).unwrap(),
                    &manifest_path,
                )
                .unwrap();
                let before = tellur::compile_file(&path, manifest.clone());
                let after = tellur::modules::compile_with_loader(&path, manifest, |p| {
                    Ok(formatted(&fs::read_to_string(p)?))
                });
                match (before, after) {
                    (Ok(before), Ok(after)) => {
                        assert_eq!(before.regions().count(), after.regions().count());
                        assert_eq!(
                            before.generic_controls().len(),
                            after.generic_controls().len()
                        );
                    }
                    (Err(before), Err(after)) => {
                        assert_eq!(
                            before.diagnostic.code,
                            after.diagnostic.code,
                            "{}",
                            path.display()
                        );
                        assert_eq!(
                            before.diagnostic.message,
                            after.diagnostic.message,
                            "{}",
                            path.display()
                        );
                    }
                    (before, after) => panic!(
                        "formatting changed checking for {}: before={before:?}, after={after:?}",
                        path.display()
                    ),
                }
                *count += 1;
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut count = 0;
    for folder in ["playground", "examples", "applications"] {
        walk(&root.join(folder), &mut count);
    }
    assert!(count >= 61, "only {count} maintained sources");
    println!(
        "Formatted and reparsed {count} maintained sources twice without changing tokens or comments."
    );
}

#[test]
fn formatting_preserves_selected_execution_and_expansion() {
    let source = "control main(bool flag,bool brief) exits done {delta flag{true=>selected,false=>other}[other]:emit(\"other\");[selected]:{string text=\"selected\";delta brief{true=>short,false=>long}[short]:emit(\"short\");[long]:{emit(\"long\");}emit(\"continuation\");path inside=entry;}[inside]:emit(text);[other]:{}exit done;}";
    let manifest = || Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let before = compile(source, manifest()).unwrap();
    let after = compile(&formatted(source), manifest()).unwrap();
    for flag in [false, true] {
        for brief in [false, true] {
            let input = Scenario {
                entry: "main".into(),
                arguments: vec![Value::Bool(flag), Value::Bool(brief)],
                calls: vec![],
            };
            let expected = before.run(&input).unwrap();
            assert_eq!(expected, after.run(&input).unwrap());
            assert_eq!(
                expected,
                after.expanded("main").unwrap().run(&input).unwrap()
            );
        }
    }
}

#[test]
fn cli_stdout_check_write_and_syntax_errors_need_no_manifest() {
    let root = std::env::temp_dir().join(format!("tellur-format-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("file with spaces.tellur");
    let original = "control main() exits done {exit done;}";
    fs::write(&path, original).unwrap();
    let run = |flags: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_tellurc"))
            .arg("format")
            .arg(&path)
            .args(flags)
            .output()
            .unwrap()
    };
    let stdout = run(&[]);
    assert!(
        stdout.status.success(),
        "{}",
        String::from_utf8_lossy(&stdout.stderr)
    );
    assert_eq!(
        String::from_utf8(stdout.stdout).unwrap(),
        formatted(original)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    assert!(!run(&["--check"]).status.success());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    assert!(!run(&["--write", "--check"]).status.success());
    for flags in [
        vec!["--write", "--width", "39"],
        vec!["--width", "241", "--write"],
        vec!["--width"],
        vec!["--width", "60", "--width", "80"],
    ] {
        assert!(!run(&flags).status.success());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
    assert!(run(&["--width", "60"]).status.success());
    assert!(run(&["--write"]).status.success());
    assert_eq!(fs::read_to_string(&path).unwrap(), formatted(original));
    assert!(run(&["--check"]).status.success());
    fs::write(&path, "control broken(").unwrap();
    assert!(!run(&["--write"]).status.success());
    assert_eq!(fs::read_to_string(&path).unwrap(), "control broken(");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn width_wraps_signatures_conditions_calls_and_payloads_with_stable_label_layout() {
    let source = "control summarize(string first_name,string second_name,bool first_available,bool second_available) exits ready {string first,string second},missing {delta (first_available && second_available) {true=>present,false=>absent}[present]:exit ready {concat(first_name,\" first\"),concat(second_name,\" second\")};[absent]:exit missing;}";
    for max_width in [40, 60, 80, 100, 240] {
        let options = Options {
            max_width,
            ..Default::default()
        };
        let text = format::source(source, options).unwrap();
        assert_eq!(text, format::source(&text, options).unwrap());
        assert!(text.contains("[absent]: exit missing;"), "{text}");
        assert!(text.contains("[present]: exit ready {"), "{text}");
        assert!(
            text.lines().all(|line| line.chars().count() <= max_width),
            "width {max_width}: {text}"
        );
        if max_width < 100 {
            assert!(text.contains("\n    string first_name,"), "{text}");
        }
    }
    let source = "control main(bool first_condition,bool second_condition,bool third_condition) exits done {bool available=first_condition && second_condition || third_condition;exit done;}";
    let options = Options {
        max_width: 60,
        ..Default::default()
    };
    let text = format::source(source, options).unwrap();
    assert!(
        text.contains("\n        && second_condition\n        || third_condition;"),
        "{text}"
    );
    assert_eq!(text, format::source(&text, options).unwrap());
}

#[test]
fn user_server_style_keeps_labels_and_indents_nested_adapter_bodies() {
    let source = include_str!("../applications/settings/server.tellur");
    let result = formatted(source);
    assert!(result.contains("    [invalid]: exit invalid_configuration;"));
    assert!(result.contains("    [listening]:\n        emit("));
    assert!(result.contains("[failed]: emit(message);"));
    assert!(result.contains("[entry]: recover again accept_next;"));
    assert!(
        result.contains(
            "\n                sent => sent {},\n                failed { string message }"
        )
    );
    assert!(result.contains("\n        u32 status,\n        string body,"));
    assert!(result.lines().all(|line| line.chars().count() <= 100));
}

#[test]
fn width_is_soft_for_literals_comments_and_accounts_for_tabs_and_unicode() {
    let literal = "😀".repeat(90);
    let source = format!(
        "control main(string first_name,string second_name) exits done {{// {}\nemit(\"{}\");exit done;}}",
        "comment ".repeat(30),
        literal
    );
    let options = Options {
        max_width: 40,
        tab_size: 4,
        insert_spaces: false,
    };
    let result = format::source(&source, options).unwrap();
    assert!(result.contains(&format!("\"{literal}\"")));
    assert!(result.contains(&format!("// {}", "comment ".repeat(30))));
    assert!(result.contains("\n\tstring first_name,\n\tstring second_name\n)"));
    assert_eq!(result, format::source(&result, options).unwrap());
}

#[test]
fn lexical_contingency_and_supervised_loop_layout_is_idempotent() {
    let source = "control main() exits done {supervised loop requests(mutable bool first=true){bool broken=first;first=false;string context=\"request\";fault boundary{delta broken{true=>fault \"broken\",false=>exit done}}contingency[context]{emit(borrow @fault.message);recover again requests;}}}";
    let result = formatted(source);
    assert!(result.contains("} contingency [context] {"), "{result}");
    assert!(
        result.contains("supervised loop requests(mutable bool first = true) {"),
        "{result}"
    );
    compile(
        &result,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap();
}
