use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use tellur::{contracts::Snapshot, manifest::Manifest, modules};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tellur-contracts-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("manifest.json"),
            include_str!("../playground/manifest.json"),
        )
        .unwrap();
        Self { root }
    }
    fn manifest(&self) -> Manifest {
        let path = self.root.join("manifest.json");
        Manifest::from_json_at(&fs::read_to_string(&path).unwrap(), &path).unwrap()
    }
    fn write(&self, name: &str, source: &str) {
        fs::write(self.root.join(name), source).unwrap();
    }
    fn snapshot(&self, source: &str) -> Snapshot {
        self.write("entry.tellur", source);
        modules::contract_snapshot(self.root.join("entry.tellur"), self.manifest())
            .unwrap_or_else(|e| panic!("{e}\n{source}"))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn value(snapshot: &Snapshot) -> Value {
    serde_json::to_value(snapshot).unwrap()
}
fn entry(snapshot: &Snapshot) -> Value {
    value(snapshot)["modules"]["@entry"]["declarations"].clone()
}

#[test]
fn public_snapshot_ignores_bodies_comments_formatting_and_private_declarations() {
    let f = Fixture::new();
    let source = include_str!("fixtures/public_contracts.tellur");
    let before = f.snapshot(source);
    let changed = source.replace("exit ready { 42 }", "exit ready { 17 }")
        + "\ncontrol unrelated() exits auto {exit elsewhere;}";
    assert_eq!(before, f.snapshot(&changed));
    assert_eq!(
        before,
        f.snapshot(&tellur::format::source(source, Default::default()).unwrap())
    );
    assert_eq!(
        entry(&before)
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        [&"read_setting"]
    );
    assert_eq!(
        entry(&before)["read_setting"]["exits"]["alternatives"]["ready"][0]["type"],
        "u32"
    );
    let text = serde_json::to_string_pretty(&before).unwrap();
    assert!(
        !text.contains("leaf")
            && !text.contains("span")
            && !text.contains("_path_")
            && !text.contains(f.root.to_str().unwrap())
    );
    assert_eq!(before, Snapshot::from_json(&text).unwrap());
    let modern = f.snapshot(
        "pub control echo(string input) exits ready {string value} {exit ready {input};}",
    );
    let legacy = f.snapshot(
        "pub region echo(string input) exits ready(string value) {[entry]:exit ready(input);}",
    );
    assert_eq!(
        modern.compare(&legacy).unwrap().changes[0].path,
        "/modules/@entry/declarations/echo/profile"
    );
}

#[test]
fn module_identities_survive_relocation_alias_changes_and_import_order() {
    let a = Fixture::new();
    let b = Fixture::new();
    for f in [&a, &b] {
        f.write("types.tellur", "pub struct record {pub u32 value;}");
        f.write("helper.tellur", "control private() exits done {exit done;}");
    }
    let source = "import \"types.tellur\" as types; import \"helper.tellur\" as helper; pub control echo(types.record input) exits ready {types.record value} {exit ready {input};}";
    let first = a.snapshot(source);
    let second = b.snapshot(
        &source
            .replace(
                "import \"types.tellur\" as types; import \"helper.tellur\" as helper;",
                "import \"helper.tellur\" as helper; import \"types.tellur\" as renamed;",
            )
            .replace("types.record", "renamed.record"),
    );
    assert_eq!(first, second);
    assert_eq!(
        entry(&first)["echo"]["parameters"][0]["type"],
        "file:types.tellur::record"
    );
    assert_eq!(
        first.modules.len(),
        2,
        "private-only imports should not add empty public modules"
    );
    b.write("types.tellur", "pub struct record {pub string value;}");
    let difference = first.compare(&b.snapshot(source)).unwrap();
    assert!(difference.changes.iter().any(|c| {
        c.path
            .contains("file:types.tellur/declarations/record/fields")
    }));
}

#[test]
fn private_representation_changes_are_filtered_but_public_permissions_are_not() {
    let f = Fixture::new();
    let before = f.snapshot("pub struct record {string hidden; pub u32 id;}");
    let renamed = f.snapshot("pub struct record {pub u32 id; string renamed;}");
    assert_eq!(before, renamed);
    let record = entry(&before)["record"].clone();
    assert_eq!(record["constructible"], false);
    assert_eq!(record["fields"].as_array().unwrap().len(), 1);
    assert_eq!(
        record["capabilities"],
        json!({"copy":null,"clone":[],"drop":[]})
    );
    let after = f.snapshot("pub struct record {handle hidden; pub u32 id;}");
    let changes = before.compare(&after).unwrap().changes;
    assert_eq!(changes.len(), 2);
    assert!(changes.iter().all(|c| c.path.contains("capabilities")));
    assert!(changes.iter().any(|c| c.path.ends_with("/drop")));
    let public = f.snapshot("pub struct record {pub string hidden; pub u32 id;}");
    assert_eq!(entry(&public)["record"]["constructible"], true);
    let reordered = f.snapshot("pub struct record {pub u32 id; pub string hidden;}");
    assert_ne!(
        public, reordered,
        "positional construction makes public field order significant"
    );
}

#[test]
fn symbolic_capabilities_include_private_generic_and_nested_storage() {
    let f = Fixture::new();
    let source = "struct inner<t> {t value;}pub struct wrapper<t> {inner<t> value;}pub struct bag<t> {vector<t> values;}pub struct unused<t> {u32 value;}pub variant maybe<t> {present {t value},empty}pub outcome completion<t> {ready {t value}}";
    let snapshot = f.snapshot(source);
    let declarations = entry(&snapshot);
    assert_eq!(
        declarations["wrapper"]["capabilities"],
        json!({"copy":["t"],"clone":["t"],"drop":["t"]})
    );
    assert_eq!(
        declarations["bag"]["capabilities"],
        json!({"copy":null,"clone":["t"],"drop":["t"]})
    );
    assert_eq!(
        declarations["unused"]["capabilities"],
        json!({"copy":[],"clone":[],"drop":[]})
    );
    assert_eq!(
        declarations["maybe"]["capabilities"],
        declarations["wrapper"]["capabilities"]
    );
    assert_eq!(
        declarations["completion"]["capabilities"],
        json!({"copy":null,"clone":null,"drop":null})
    );
    // Compare the symbolic rules with capabilities on concrete checked layouts.
    for ty in ["u32", "string", "handle"] {
        let concrete = format!(
            "{source}control carry(wrapper<{ty}> input) exits ready {{wrapper<{ty}> value}} {{exit ready {{input}};}}"
        );
        f.write("entry.tellur", &concrete);
        let program = modules::compile_file(f.root.join("entry.tellur"), f.manifest()).unwrap();
        let canonical = tellur::manifest::canonical_type(ty);
        let application = format!("wrapper<{canonical}>");
        assert_eq!(
            program.manifest().is_copy(&application),
            program.manifest().is_copy(&canonical)
        );
        assert_eq!(
            program.manifest().is_cloneable(&application),
            program.manifest().is_cloneable(&canonical)
        );
        assert_eq!(
            program.manifest().linear(&application),
            program.manifest().linear(&canonical)
        );
        assert_eq!(
            snapshot,
            f.snapshot(&concrete),
            "private specializations must not leak into the API"
        );
    }
    let mut repeated = "pub struct level0<t> {t value;}".to_string();
    for i in 1..24 {
        repeated.push_str(&format!(
            "pub struct level{i}<t> {{level{}<t> left;level{}<t> right;}}",
            i - 1,
            i - 1
        ));
    }
    assert_eq!(
        entry(&f.snapshot(&repeated))["level23"]["capabilities"],
        json!({"copy":["t"],"clone":["t"],"drop":["t"]})
    );
}

#[test]
fn borrowed_results_keep_storage_contained_and_referent_origin_contracts() {
    let f = Fixture::new();
    let source = "pub struct pair['a,'b] {pub ref string left <- 'a; pub ref string right <- 'b;}pub control pair_up(ref string left,ref string right) exits ready {pair[left,right] value} {exit ready {pair {left,right}};}pub control first(ref pair input) exits found {ref string value from input['a]} {exit found {input.left};}";
    let snapshot = f.snapshot(source);
    let declarations = entry(&snapshot);
    assert_eq!(declarations["pair"]["origin_parameters"], json!(["a", "b"]));
    assert_eq!(
        declarations["pair_up"]["exits"]["alternatives"]["ready"][0]["origins"],
        json!(["left", "right"])
    );
    assert_eq!(
        declarations["first"]["exits"]["alternatives"]["found"][0]["from"],
        "input['a]"
    );
    let after = f.snapshot(
        &source
            .replace("from input['a]", "from input['b]")
            .replace("exit found {input.left}", "exit found {input.right}"),
    );
    assert!(
        snapshot
            .compare(&after)
            .unwrap()
            .changes
            .iter()
            .any(|c| c.path.contains("/first/exits/alternatives/found"))
    );
    let nested = f.snapshot("pub struct view['a] {pub ref string text <- 'a;} pub struct carrier['s,'a] {pub ref view['a] value <- 's;}");
    assert_eq!(
        entry(&nested)["carrier"]["fields"][0]["origins"],
        json!(["'s"])
    );
    assert_eq!(
        entry(&nested)["carrier"]["fields"][0]["referent_origins"],
        json!(["'a"])
    );
}

#[test]
fn traits_associated_bindings_and_generic_requirements_are_public_contracts() {
    let f = Fixture::new();
    let source = "pub trait reader {type item:clone;control peek(ref self input) exits found {ref item value from input};}pub struct message {string text;}implement reader for message {type item=string;control peek(ref self input) exits found {ref item value from input} {exit found {borrow input.text};}}pub control peek<r:reader>(ref r input) exits found {ref reader<r>.item value from input} {forward reader<r>.peek(input);}";
    let snapshot = f.snapshot(source);
    let declarations = entry(&snapshot);
    assert_eq!(
        declarations["reader"]["associated"]["item"],
        json!(["clone"])
    );
    assert_eq!(
        declarations["peek"]["requirements"]["r"],
        json!(["@entry::reader"])
    );
    assert_eq!(
        declarations["peek"]["exits"]["alternatives"]["found"][0]["type"],
        "ref @entry::reader<r>.item"
    );
    assert_eq!(
        value(&snapshot)["modules"]["@entry"]["implementations"]["@entry::reader for @entry::message"]
            ["associated"]["item"],
        "string"
    );
    let renamed = source.replace("control peek(ref self input) exits found {ref item value from input} {exit found {borrow input.text};}", "control peek(ref self record) exits found {ref item value from record} {exit found {borrow record.text};}");
    assert_eq!(
        snapshot,
        f.snapshot(&renamed),
        "implementation parameter names are private"
    );
    let private_target = source.replace("pub struct message", "struct message");
    assert!(
        value(&f.snapshot(&private_target))["modules"]["@entry"]["implementations"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn named_outcomes_keep_status_policy_and_diff_alternatives_by_name() {
    let f = Fixture::new();
    let source = "pub outcome result {@exit 0 done,@exit 2 failed {string message}}pub control run() exits result {exit done;}";
    let before = f.snapshot(source);
    assert_eq!(
        entry(&before)["run"]["exits"],
        json!({"kind":"named","type":"@entry::result","origins":[]})
    );
    assert_eq!(
        before,
        f.snapshot(&source.replace(
            "@exit 0 done,@exit 2 failed {string message}",
            "@exit 2 failed {string message},@exit 0 done"
        ))
    );
    let changed = f.snapshot(&source.replace("@exit 2", "@exit 3"));
    let diff = before.compare(&changed).unwrap();
    assert_eq!(diff.changes.len(), 1);
    assert_eq!(
        diff.changes[0].path,
        "/modules/@entry/declarations/result/exit_codes/failed"
    );
    let after = f.snapshot(&source.replace("@exit 0 done,", "@exit 0 done, @exit 4 unavailable,"));
    let diff = before.compare(&after).unwrap();
    assert!(
        diff.changes
            .iter()
            .any(|c| c.kind == "added" && c.path.ends_with("/alternatives/unavailable"))
    );
    assert!(
        after
            .compare(&before)
            .unwrap()
            .changes
            .iter()
            .any(|c| c.kind == "removed")
    );
}

#[test]
fn only_referenced_manifest_types_are_included_recursively() {
    let f = Fixture::new();
    let before = f.snapshot(
        "pub control pass(handle input) exits ready {handle value} {exit ready {input};}",
    );
    assert_eq!(
        before.manifest_types.keys().collect::<Vec<_>>(),
        [&"handle"]
    );
    assert_eq!(
        entry(&before)["pass"]["parameters"][0]["type"],
        "@manifest::handle"
    );
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(f.root.join("manifest.json")).unwrap()).unwrap();
    manifest["types"].as_array_mut().unwrap().push(
        json!({"kind":"record","name":"envelope","fields":[{"name":"file","type":"handle"}]}),
    );
    f.write("manifest.json", &manifest.to_string());
    assert_eq!(
        before,
        f.snapshot(
            "pub control pass(handle input) exits ready {handle value} {exit ready {input};}"
        )
    );
    let snapshot = f.snapshot(
        "pub control pass(envelope input) exits ready {envelope value} {exit ready {input};}",
    );
    assert_eq!(snapshot.manifest_types.len(), 2);
    let mut direct = f.manifest();
    let tellur::manifest::TypeDef::Record { fields, .. } = direct
        .types
        .iter_mut()
        .find(|t| t.name() == "envelope")
        .unwrap()
    else {
        panic!("record")
    };
    fields[0].ty = "string".into();
    let normalized = modules::contract_snapshot(f.root.join("entry.tellur"), direct).unwrap();
    assert_eq!(
        value(&normalized)["manifest_types"]["envelope"]["capabilities"],
        json!({"copy":null,"clone":[],"drop":[]})
    );
}

#[test]
fn failed_checking_never_returns_a_partial_snapshot() {
    let f = Fixture::new();
    for source in [
        "pub control f() exits auto {exit done;}",
        "pub control f<t>() exits done {exit wrong;}",
        "pub control f() exits done {u32 n=true;exit done;}",
        "import \"missing.tellur\" as missing;pub control f() exits done {exit done;}",
    ] {
        f.write("entry.tellur", source);
        assert!(
            modules::contract_snapshot(f.root.join("entry.tellur"), f.manifest()).is_err(),
            "{source}"
        );
    }
}

#[test]
fn snapshot_reader_rejects_wrong_versions_unknown_fields_and_duplicate_names() {
    let f = Fixture::new();
    let snapshot = f.snapshot("pub control f() exits done {exit done;}");
    let mut input = value(&snapshot);
    input["version"] = json!(999);
    assert!(Snapshot::from_json(&input.to_string()).is_err());
    let mut input = value(&snapshot);
    input["unexpected"] = json!(true);
    assert!(Snapshot::from_json(&input.to_string()).is_err());
    let mut input = value(&snapshot);
    input["modules"]["@entry"]["declarations"]["f"]["future_contract"] = json!(true);
    assert!(Snapshot::from_json(&input.to_string()).is_err());
    let raw = serde_json::to_string(&snapshot)
        .unwrap()
        .replace("\"declarations\":", "\"declarations\":{},\"declarations\":");
    assert!(Snapshot::from_json(&raw).is_err());
    let raw = r#"{"format":"tellur.public-contracts","version":1,"modules":{"@entry":{"declarations":{},"implementations":{}},"@entry":{"declarations":{},"implementations":{}}},"manifest_types":{}}"#;
    assert!(Snapshot::from_json(raw).is_err());
}

#[test]
fn settings_application_supplies_a_reviewable_unchanged_baseline() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("applications/settings");
    let manifest_path = root.join("manifest.json");
    let manifest =
        Manifest::from_json_at(&fs::read_to_string(&manifest_path).unwrap(), &manifest_path)
            .unwrap();
    let snapshot =
        modules::contract_snapshot(root.join("settings.tellur"), manifest.clone()).unwrap();
    assert_eq!(
        snapshot,
        Snapshot::from_json(&fs::read_to_string(root.join("settings.contract.json")).unwrap())
            .unwrap()
    );
    assert_eq!(entry(&snapshot).as_object().unwrap().len(), 5);
    let source = fs::read_to_string(root.join("settings.tellur")).unwrap();
    let changed = source.replace("1048576", "2097152");
    let after =
        modules::contract_snapshot_with_loader(root.join("settings.tellur"), manifest, |_| {
            Ok(changed.clone())
        })
        .unwrap();
    assert!(
        snapshot.compare(&after).unwrap().equal,
        "behavior can change without changing the public contract"
    );
}

#[test]
fn all_maintained_tellur_sources_export_checked_roundtrippable_contracts() {
    fn walk(path: &Path, count: &mut usize) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if !["build", "target", ".tellur-build"]
                    .contains(&path.file_name().unwrap().to_str().unwrap())
                {
                    walk(&path, count);
                }
            } else if path.extension().is_some_and(|e| e == "tellur") {
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
                let snapshot = modules::contract_snapshot(&path, manifest)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert_eq!(
                    snapshot,
                    Snapshot::from_json(&serde_json::to_string(&snapshot).unwrap()).unwrap()
                );
                *count += 1;
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut count = 0;
    for folder in ["playground", "examples", "applications"] {
        walk(&root.join(folder), &mut count);
    }
    assert!(count >= 61);
    println!("Exported and roundtripped checked contracts for {count} maintained sources.");
}

#[test]
fn cli_exports_compares_checks_and_rejects_bad_inputs_without_writing_sources() {
    let f = Fixture::new();
    f.write("entry.tellur", "pub control f() exits done {exit done;}");
    let compiler = env!("CARGO_BIN_EXE_tellurc");
    let snapshot = Command::new(compiler)
        .arg("contract-snapshot")
        .arg(f.root.join("entry.tellur"))
        .output()
        .unwrap();
    assert!(
        snapshot.status.success(),
        "{}",
        String::from_utf8_lossy(&snapshot.stderr)
    );
    let before = String::from_utf8(snapshot.stdout).unwrap();
    Snapshot::from_json(&before).unwrap();
    f.write("before.json", &before);
    f.write(
        "after.json",
        &serde_json::to_string(&f.snapshot("pub control f() exits changed {exit changed;}"))
            .unwrap(),
    );
    let run = |after: &str, check: bool| {
        let mut command = Command::new(compiler);
        command
            .arg("contract-diff")
            .arg(f.root.join("before.json"))
            .arg(f.root.join(after));
        if check {
            command.arg("--check");
        }
        command.output().unwrap()
    };
    assert!(run("before.json", true).status.success());
    assert!(run("after.json", false).status.success());
    let changed = run("after.json", true);
    assert!(!changed.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&changed.stdout).unwrap()["equal"],
        false
    );
    f.write("bad.json", "{}");
    let bad = run("bad.json", false);
    assert!(!bad.status.success());
    assert!(bad.stdout.is_empty());
    f.write("entry.tellur", "pub control bad() exits auto {exit done;}");
    let invalid = Command::new(compiler)
        .arg("contract-snapshot")
        .arg(f.root.join("entry.tellur"))
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
}

#[test]
fn process_termination_changes_public_call_authority() {
    let f = Fixture::new();
    let before = f.snapshot("pub control main() exits done {exit done;}");
    let after=f.snapshot("pub control main() exits done {fault boundary {fault \"startup\";} contingency [] {terminate process 2;}}");
    assert_eq!(entry(&after)["main"]["entry_only"], true);
    assert_ne!(before, after);
}
