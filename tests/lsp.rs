use lsp_server::{Connection, Message, Notification, Request};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tellur::{
    Span,
    lsp::{Server, diagnostic_range},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    uri: String,
    server: Server,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tellur-lsp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("manifest.json"),
            include_str!("../examples/manifest.json"),
        )
        .unwrap();
        let uri = url::Url::from_file_path(root.join("a space.path"))
            .unwrap()
            .to_string();
        let mut server = Server::default();
        server.initialize(
            &json!({"rootUri":url::Url::from_directory_path(&root).unwrap().to_string()}),
        );
        Self { root, uri, server }
    }
    fn open(&mut self, text: &str) {
        self.server.notification(
            "textDocument/didOpen",
            &json!({"textDocument":{"uri":self.uri,"languageId":"path","version":1,"text":text}}),
        );
    }
    fn change(&mut self, text: &str, version: i64) {
        self.server.notification("textDocument/didChange",&json!({"textDocument":{"uri":self.uri,"version":version},"contentChanges":[{"text":text}]}));
    }
    fn diagnostics(&self) -> Value {
        self.server.diagnostics(&self.uri)["diagnostics"].clone()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
const VALID: &str = "region main() exits Done { [Entry]: exit Done; }";

fn hover_at(f: &Fixture, source: &str, needle: &str) -> Value {
    let offset = source.find(needle).unwrap();
    let prefix = &source[..offset];
    f.server.hover(
        &f.uri,
        prefix.bytes().filter(|b| *b == b'\n').count() as u64,
        prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u64,
    )
}

#[test]
fn explicit_exit_adapter_diagnostics_repair_independently_before_hover_returns() {
    let mut f = Fixture::new();
    let source = "control choose(bool flag) exits ready {u32 value} {\nexit delta flag {\ntrue=>ready {1},\nfalse=>ready {2}\n};}\ncontrol main() exits ready {u32 value} {forward choose(true);}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let broken = source
        .replace("ready {1}", "ready {true}")
        .replace("ready {2}", "ready {false}");
    f.change(&broken, 2);
    let errors = f.diagnostics();
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    for (error, line) in errors.as_array().unwrap().iter().zip([2, 3]) {
        assert_eq!(error["code"], "E_TYPE");
        assert_eq!(error["range"]["start"]["line"], line);
    }
    assert!(hover_at(&f, &broken, "choose(true)").is_null());
    let partial = broken.replace("ready {true}", "ready {1}");
    f.change(&partial, 3);
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 1);
    assert!(hover_at(&f, &partial, "choose(true)").is_null());
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "choose(true)")
            .to_string()
            .contains("exits ready { u32 value }")
    );
}

#[test]
fn explicit_exit_adapter_diagnostics_preserve_imported_nested_arm_locations() {
    let mut f = Fixture::new();
    let library = f.root.join("jobs.tellur");
    let uri = url::Url::from_file_path(&library).unwrap().to_string();
    let source = "pub control choose(bool flag) exits ready {u32 value} {\nexit delta flag {\ntrue=>delta flag {\ntrue=>ready {1},false=>ready {2}},\nfalse=>ready {3}\n};}";
    fs::write(&library, source).unwrap();
    let caller = "import \"jobs.tellur\" as jobs;control main() exits ready {u32 value} {forward jobs.choose(true);}";
    f.open(caller);
    assert_eq!(f.diagnostics(), json!([]));
    let broken = source
        .replace("ready {1}", "ready {true}")
        .replace("ready {3}", "ready {false}");
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"version":1,"text":broken}}),
    );
    let errors = f.diagnostics();
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    for (error, line) in errors.as_array().unwrap().iter().zip([3, 4]) {
        assert_eq!(error["code"], "E_TYPE");
        let location = &error["relatedInformation"][0]["location"];
        assert_eq!(location["uri"], uri);
        assert_eq!(location["range"]["start"]["line"], line);
    }
    assert!(hover_at(&f, caller, "jobs.choose(true)").is_null());
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, caller, "jobs.choose(true)")
            .to_string()
            .contains("control jobs.choose(bool flag)")
    );
}

#[test]
fn lexical_handler_diagnostics_repair_independently_and_refresh_inferred_types() {
    let mut f = Fixture::new();
    let source = "control main() exits done {\nfault boundary {} contingency [] {\nu32 first=1;\nstring second=\"ready\";\nauto message=borrow @fault.message;\nterminate process 1;\n}\nexit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "auto message")
            .to_string()
            .contains("ref string")
    );
    let broken = source
        .replace("first=1", "first=true")
        .replace("second=\"ready\"", "second=7");
    f.change(&broken, 2);
    let errors = f.diagnostics();
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    for (error, line) in errors.as_array().unwrap().iter().zip([2, 3]) {
        assert_eq!(error["code"], "E_TYPE");
        assert_eq!(error["range"]["start"]["line"], line);
    }
    assert!(hover_at(&f, &broken, "auto message").is_null());
    let partial = broken.replace("first=true", "first=1");
    f.change(&partial, 3);
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 1);
    assert!(hover_at(&f, &partial, "auto message").is_null());
    let repaired = source.replace("borrow @fault.message", "9");
    f.change(&repaired, 4);
    assert_eq!(f.diagnostics(), json!([]));
    let hover = hover_at(&f, &repaired, "auto message").to_string();
    assert!(
        hover.contains("u32") && !hover.contains("ref string"),
        "{hover}"
    );
}

#[test]
fn lexical_handler_diagnostics_keep_imported_disjoint_locations_and_dependency_suppression() {
    let mut f = Fixture::new();
    let library = f.root.join("jobs.tellur");
    let uri = url::Url::from_file_path(&library).unwrap().to_string();
    let source = "pub control process(bool flag) exits done faults {\nfault boundary {exit done;} contingency [flag] {\ndelta flag {true=>yes,false=>no}\n[yes]:u32 first=1;escalate;\n[no]:string second=\"ready\";escalate;\n}}";
    fs::write(&library, source).unwrap();
    let caller =
        "import \"jobs.tellur\" as jobs;control main() exits done {forward jobs.process(true);}";
    f.open(caller);
    assert_eq!(f.diagnostics(), json!([]));
    let broken = source
        .replace("first=1", "first=true")
        .replace("second=\"ready\"", "second=7");
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"version":1,"text":broken}}),
    );
    let errors = f.diagnostics();
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    for (error, line) in errors.as_array().unwrap().iter().zip([3, 4]) {
        assert_eq!(error["code"], "E_TYPE");
        let location = &error["relatedInformation"][0]["location"];
        assert_eq!(location["uri"], uri);
        assert_eq!(location["range"]["start"]["line"], line);
    }
    assert!(hover_at(&f, caller, "jobs.process(true)").is_null());
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, caller, "jobs.process(true)")
            .to_string()
            .contains("control jobs.process(bool flag) exits done faults")
    );
}

#[test]
fn lexical_work_diagnostics_repair_before_handler_errors_and_inferred_hover() {
    let mut f = Fixture::new();
    let source = include_str!("../playground/recovery.tellur")
        .replace(
            "fault boundary {",
            "fault boundary {\n u32 work_count=1;\n string work_text=\"ready\";",
        )
        .replace(
            "contingency [context] {",
            "contingency [context] {\n u32 report_count=1;\n string report_text=\"handled\";",
        );
    f.open(&source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, &source, "auto view")
            .to_string()
            .contains("ref string")
    );
    let broken = source
        .replace("work_count=1", "work_count=true")
        .replace("work_text=\"ready\"", "work_text=7")
        .replace("report_count=1", "report_count=false")
        .replace("report_text=\"handled\"", "report_text=8");
    f.change(&broken, 2);
    let errors = f.diagnostics();
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    for (error, needle) in errors
        .as_array()
        .unwrap()
        .iter()
        .zip(["work_count=true", "work_text=7"])
    {
        assert_eq!(error["code"], "E_TYPE");
        let line = broken[..broken.find(needle).unwrap()]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        assert_eq!(error["range"]["start"]["line"], line);
    }
    assert!(hover_at(&f, &broken, "auto view").is_null());
    let handler_broken = broken
        .replace("work_count=true", "work_count=1")
        .replace("work_text=7", "work_text=\"ready\"");
    f.change(&handler_broken, 3);
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 2);
    assert!(hover_at(&f, &handler_broken, "auto view").is_null());
    let partial = handler_broken.replace("report_count=false", "report_count=1");
    f.change(&partial, 4);
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 1);
    f.change(&source, 5);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, &source, "auto view")
            .to_string()
            .contains("ref string")
    );
}

#[test]
fn lexical_work_diagnostics_keep_imported_locations_and_fresh_call_contracts() {
    let mut f = Fixture::new();
    let library = f.root.join("jobs.tellur");
    let uri = url::Url::from_file_path(&library).unwrap().to_string();
    let source = "pub control process() exits done faults {\nfault boundary {\nu32 first=1;\nstring second=\"ready\";\nexit done;\n} contingency [] {escalate;}\n}";
    fs::write(&library, source).unwrap();
    let caller =
        "import \"jobs.tellur\" as jobs;control main() exits done {forward jobs.process();}";
    f.open(caller);
    assert_eq!(f.diagnostics(), json!([]));
    let broken = source
        .replace("first=1", "first=true")
        .replace("second=\"ready\"", "second=7");
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"version":1,"text":broken}}),
    );
    let errors = f.diagnostics();
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    for (error, line) in errors.as_array().unwrap().iter().zip([2, 3]) {
        assert_eq!(error["code"], "E_TYPE");
        let location = &error["relatedInformation"][0]["location"];
        assert_eq!(location["uri"], uri);
        assert_eq!(location["range"]["start"]["line"], line);
    }
    assert!(hover_at(&f, caller, "jobs.process()").is_null());
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, caller, "jobs.process()")
            .to_string()
            .contains("exits done faults")
    );
}

#[test]
fn named_boundary_diagnostics_repair_handler_then_work_before_hover_returns() {
    let mut f = Fixture::new();
    let source = "boundary request(string context) retains context {\nu32 work_count=1;\nstring work_text=\"ready\";\nterminate;\n} contingency(string reason) {\nu32 report_count=1;\nstring report_text=\"handled\";\nterminate;\n}\ncontrol main() exits done {supervise request(\"test\");exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "request(\"test\")")
            .to_string()
            .contains("Boundary contract")
    );
    let broken = source
        .replace("work_count=1", "work_count=true")
        .replace("work_text=\"ready\"", "work_text=7")
        .replace("report_count=1", "report_count=false")
        .replace("report_text=\"handled\"", "report_text=8");
    f.change(&broken, 2);
    let diagnostics = f.diagnostics();
    assert_eq!(diagnostics.as_array().unwrap().len(), 2, "{diagnostics}");
    for (error, line) in diagnostics.as_array().unwrap().iter().zip([5, 6]) {
        assert_eq!(error["code"], "E_TYPE");
        assert_eq!(error["range"]["start"]["line"], line);
    }
    assert!(hover_at(&f, &broken, "request(\"test\")").is_null());
    let work_broken = broken
        .replace("report_count=false", "report_count=1")
        .replace("report_text=8", "report_text=\"handled\"");
    f.change(&work_broken, 3);
    let diagnostics = f.diagnostics();
    assert_eq!(diagnostics.as_array().unwrap().len(), 2, "{diagnostics}");
    for (error, line) in diagnostics.as_array().unwrap().iter().zip([1, 2]) {
        assert_eq!(error["code"], "E_TYPE");
        assert_eq!(error["range"]["start"]["line"], line);
    }
    let partial = work_broken.replace("work_count=true", "work_count=1");
    f.change(&partial, 4);
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 1);
    assert!(hover_at(&f, &partial, "request(\"test\")").is_null());
    f.change(source, 5);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "request(\"test\")")
            .to_string()
            .contains("retains context")
    );
}

#[test]
fn named_boundary_diagnostics_follow_unsaved_imports_and_disjoint_paths() {
    let mut f = Fixture::new();
    let library = f.root.join("jobs.tellur");
    let uri = url::Url::from_file_path(&library).unwrap().to_string();
    let source = "pub boundary request(bool flag) {\ndelta flag {true=>yes,false=>no}\n[yes]:u32 first=1;terminate;\n[no]:string second=\"ready\";terminate;\n} contingency(string reason) {terminate;}";
    fs::write(&library, source).unwrap();
    let caller = "import \"jobs.tellur\" as jobs;control main() exits done {supervise jobs.request(true);exit done;}";
    f.open(caller);
    assert_eq!(f.diagnostics(), json!([]));
    let broken = source
        .replace("first=1", "first=true")
        .replace("second=\"ready\"", "second=7");
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"version":1,"text":broken}}),
    );
    let direct = f.server.diagnostics(&uri)["diagnostics"].clone();
    assert_eq!(direct.as_array().unwrap().len(), 2, "{direct}");
    for (error, line) in direct.as_array().unwrap().iter().zip([2, 3]) {
        assert_eq!(error["code"], "E_TYPE");
        assert_eq!(error["range"]["start"]["line"], line);
        assert!(!error.to_string().contains("_path_m_"));
    }
    let propagated = f.diagnostics();
    assert_eq!(propagated.as_array().unwrap().len(), 2, "{propagated}");
    assert!(hover_at(&f, caller, "jobs.request(true)").is_null());
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, caller, "jobs.request(true)")
            .to_string()
            .contains("pub boundary jobs.request(bool flag)")
    );
}

#[test]
fn boundary_call_hover_distinguishes_handled_faults_and_outcome_free_completion() {
    let mut f = Fixture::new();
    let source = include_str!("../playground/contingency.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = hover_at(&f, source, "request(true");
    let text = h["contents"]["value"].as_str().unwrap();
    assert!(text.contains("**Boundary contract**"), "{text}");
    assert!(
        text.contains("boundary request(bool broken, string context) retains context\n"),
        "{text}"
    );
    assert!(
        text.contains("no outcome, payload or fault value")
            && text.contains("cannot escape through the source-fault channel"),
        "{text}"
    );
    assert!(
        !text.contains(" exits ") && !text.contains("$contingency"),
        "{text}"
    );
    assert_eq!(
        h["range"]["end"]["character"].as_u64().unwrap()
            - h["range"]["start"]["character"].as_u64().unwrap(),
        "request".len() as u64
    );
}

#[test]
fn boundary_call_hover_respects_declared_and_inferred_contingency_escape() {
    let mut f = Fixture::new();
    f.uri = "untitled:boundary-call-hover".into();
    let source = "pub boundary allowed() faults {terminate;} contingency(string reason) {terminate;} boundary inferred() {fault \"broken\";} contingency(string reason) {escalate;} control main() exits done {supervise allowed();supervise inferred();exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    for (callee, signature) in [
        ("allowed();", "pub boundary allowed() faults"),
        ("inferred();", "boundary inferred() faults"),
    ] {
        let text = hover_at(&f, source, callee).to_string();
        assert!(
            text.contains(signature) && text.contains("escape abandons the supervisor"),
            "{text}"
        );
    }
    let repaired = source.replace("{escalate;}", "{terminate;}");
    f.change(&repaired, 2);
    assert_eq!(f.diagnostics(), json!([]));
    let text = hover_at(&f, &repaired, "inferred();").to_string();
    assert!(
        text.contains("boundary inferred()\\n") && !text.contains("boundary inferred() faults"),
        "{text}"
    );
    assert!(
        hover_at(&f, &repaired, "allowed();")
            .to_string()
            .contains("pub boundary allowed() faults")
    );
}

#[test]
fn boundary_call_hover_refreshes_imported_retention_without_private_layouts() {
    let mut f = Fixture::new();
    let library = f.root.join("jobs.tellur");
    let uri = url::Url::from_file_path(&library).unwrap().to_string();
    let valid = "pub struct ticket {string secret;} pub boundary request(ticket input, string context) retains context, input {fault \"broken\";} contingency(string reason) {drop(input);emit(context);terminate;}";
    fs::write(&library, valid).unwrap();
    let source = "import \"jobs.tellur\" as first;import \"jobs.tellur\" as jobs;control main(first.ticket value) exits done {supervise jobs.request(move value, \"context\");exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let text = hover_at(&f, source, "jobs.request(move").to_string();
    assert!(
        text.contains(
            "pub boundary jobs.request(jobs.ticket input, string context) retains context, input"
        ),
        "{text}"
    );
    assert!(
        !text.contains("secret") && !text.contains("_path_") && !text.contains("$contingency"),
        "{text}"
    );
    let changed = valid
        .replace("input", "fresh")
        .replace("retains context, fresh", "retains fresh, context");
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"version":1,"text":changed}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "jobs.request(move")
            .to_string()
            .contains("retains fresh, context")
    );
    let broken = changed.replace("string context", "ref string context");
    f.server.notification(
        "textDocument/didChange",
        &json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":broken}]}),
    );
    assert!(!f.diagnostics().as_array().unwrap().is_empty());
    assert!(hover_at(&f, source, "jobs.request(move").is_null());
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "jobs.request(move")
            .to_string()
            .contains("retains context, input")
    );
}

#[test]
fn boundary_call_hover_selects_nested_calls_and_rejects_ordinary_invocation() {
    let mut f = Fixture::new();
    let source = "struct message {string text;constructor(string input) {self.text=move input;}} boundary request(message input) retains input {terminate;} contingency(string reason) {terminate;}control main() exits done {supervise request(message(\"hello\"));exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "request(message(\"hello\"))")
            .to_string()
            .contains("**Boundary contract**")
    );
    assert!(
        hover_at(&f, source, "message(\"hello\")")
            .to_string()
            .contains("**Constructor contract**")
    );
    let broken = source.replace("supervise ", "");
    f.change(&broken, 2);
    assert!(f.diagnostics().to_string().contains("E_DISPOSITION"));
    assert!(hover_at(&f, &broken, "request(message(\"hello\"))").is_null());
    f.change(source, 3);
    assert!(
        hover_at(&f, source, "request(message(\"hello\"))")
            .to_string()
            .contains("**Boundary contract**")
    );
}

#[test]
fn lexical_call_hover_resolves_self_and_associated_aliases_before_nominal_names() {
    let mut f = Fixture::new();
    let source = "struct item {u32 shadow;}\n".to_owned()
        + include_str!("../playground/trait_aliases.tellur");
    f.open(&source);
    assert_eq!(f.diagnostics(), json!([]));
    let text = hover_at(&f, &source, "reader<self>.read(owner)").to_string();
    assert!(
        text.contains("control reader<self>.read(ref message input) exits read_result<string>"),
        "{text}"
    );
    assert!(
        text.contains("received { string value }") && text.contains("Selected histories"),
        "{text}"
    );
    assert!(!text.contains("owner.text"), "{text}");
    let h = hover_at(&f, &source, "box<item>(move text)");
    let text = h["contents"]["value"].as_str().unwrap();
    assert!(text.contains("constructor(string input)"), "{text}");
    assert!(
        !text.contains("shadow") && !text.contains("constructor(item input)"),
        "{text}"
    );
    assert_eq!(
        h["range"]["end"]["character"].as_u64().unwrap()
            - h["range"]["start"]["character"].as_u64().unwrap(),
        "box<item>".len() as u64
    );
}

#[test]
fn lexical_call_hover_uses_normalized_sibling_aliases_and_fresh_definitions() {
    let mut f = Fixture::new();
    f.uri = "untitled:lexical-call-hover".into();
    let source = "struct box<t> {t value;constructor(t input) {self.value=move input;}}trait maker {type item:drop;type other;control wrap(item input) exits done;}implement maker for bool {type item=other;type other=string;control wrap(item owner) exits done {auto saved=box<item>(move owner);drop(saved);exit done;}}control main() exits done {exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "box<item>(move owner)")
            .to_string()
            .contains("constructor(string input)")
    );
    let changed = source.replace("type other=string", "type other=bytes");
    f.change(&changed, 2);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, &changed, "box<item>(move owner)")
            .to_string()
            .contains("constructor(bytes input)")
    );
    let broken = changed.replace("self.value=move input;", "");
    f.change(&broken, 3);
    assert!(!f.diagnostics().as_array().unwrap().is_empty());
    assert!(hover_at(&f, &broken, "box<item>(move owner)").is_null());
    f.change(source, 4);
    assert!(
        hover_at(&f, source, "box<item>(move owner)")
            .to_string()
            .contains("constructor(string input)")
    );
}

#[test]
fn lexical_call_hover_resolves_imported_traits_and_constructors_in_local_implementations() {
    let mut f = Fixture::new();
    fs::write(f.root.join("library.tellur"), "pub struct box<t> {t secret;pub constructor(t input) {self.secret=move input;}}pub trait maker {type item:drop;control wrap(item input) exits done;control dispatch(item input) exits done;}").unwrap();
    let source = "import \"library.tellur\" as one;import \"library.tellur\" as lib;struct target {u32 tag;}struct item {bool shadow;}implement lib.maker for target {type item=string;control wrap(item owner) exits done {auto saved=lib.box<item>(move owner);drop(saved);exit done;}control dispatch(item owner) exits done {forward lib.maker<self>.wrap(move owner);}}control main() exits done {lib.maker<target>.dispatch(\"hello\");exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let text = hover_at(&f, source, "lib.box<item>(move owner)").to_string();
    assert!(
        text.contains("pub constructor(string input)")
            && text.contains("Constructs `lib.box<item>`"),
        "{text}"
    );
    assert!(
        !text.contains("secret") && !text.contains("_path_m_"),
        "{text}"
    );
    let text = hover_at(&f, source, "lib.maker<self>.wrap(move owner)").to_string();
    assert!(
        text.contains("control lib.maker<self>.wrap(string input) exits done"),
        "{text}"
    );
}

#[test]
fn lexical_call_hover_keeps_associated_named_contracts_and_nested_origin_bindings() {
    let mut f = Fixture::new();
    let source = include_str!("../playground/aggregate_traits.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let text = hover_at(&f, source, "prepare<text_editor>(borrow").to_string();
    assert!(
        text.contains("exits preparation<string>[input, input['text]]"),
        "{text}"
    );
    assert!(
        text.contains("ready { edit_job<string>[input, input['text]] job }"),
        "{text}"
    );
    assert!(
        !text.contains("String") && !text.contains("_path_"),
        "{text}"
    );
}

#[test]
fn lexical_call_hover_normalizes_named_helpers_in_their_defining_module() {
    let mut f = Fixture::new();
    fs::write(f.root.join("types.tellur"), "pub outcome view<t>['a] {found {ref t value <- 'a}}pub trait source {type item;}implement source for bool {type item=string;}").unwrap();
    let library = f.root.join("api.tellur");
    let uri = url::Url::from_file_path(&library).unwrap().to_string();
    let valid = "import \"types.tellur\" as types;pub control peek<t:types.source>(ref types.source<t>.item input) exits types.view<types.source<t>.item>[input] {exit found {input};}";
    fs::write(&library, valid).unwrap();
    let source = "import \"types.tellur\" as kinds;import \"api.tellur\" as api;control main() exits done {string owner=\"hello\";delta api.peek<bool>(borrow owner) {found {auto text}=>found} [found]:emit(text);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let text = hover_at(&f, source, "api.peek<bool>(borrow").to_string();
    assert!(
        text.contains("control api.peek<bool>(ref string input) exits kinds.view<string>[input]"),
        "{text}"
    );
    assert!(
        text.contains("found { ref string value from input }"),
        "{text}"
    );
    assert!(!text.contains("_path_m_"), "{text}");
    let changed = valid.replace("input", "fresh");
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"version":1,"text":changed}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    let text = hover_at(&f, source, "api.peek<bool>(borrow").to_string();
    assert!(
        text.contains("kinds.view<string>[fresh]") && text.contains("from fresh"),
        "{text}"
    );
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert!(
        hover_at(&f, source, "api.peek<bool>(borrow")
            .to_string()
            .contains("from input")
    );
}

#[test]
fn lexical_call_hover_preserves_private_inference_without_inventing_group_arguments() {
    let mut f = Fixture::new();
    let source = "outcome view['a] {found {ref string value <- 'a}}control peek(ref string input) exits view {exit found {input};}control main() exits done {string owner=\"hi\";delta peek(borrow owner) {found {auto text}=>found} [found]:emit(text);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let text = hover_at(&f, source, "peek(borrow owner)").to_string();
    assert!(
        text.contains("exits view")
            && text.contains("found { ref string value }")
            && text.contains("Checked return lifetimes"),
        "{text}"
    );
    assert!(
        !text.contains("['a]") && !text.contains("view[input]"),
        "{text}"
    );
}

#[test]
fn trait_call_hover_preserves_declared_names_order_and_borrow_origins() {
    let mut f = Fixture::new();
    let source = "struct message {string secret;}\ntrait reader {type item: clone;control peek(ref self input) exits found {ref item value from input},missing;}\nimplement reader for message {type item=string;control peek(ref self owner) exits missing,found {ref item payload from owner} {exit found {borrow owner.secret};}}\ncontrol main() exits done {message source=message {\"hello\"};delta reader<message>.peek(borrow source) {found {auto text}=>shown,missing=>absent} [shown]:emit(text);[entry]:exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = hover_at(&f, source, "reader<message>.peek(borrow");
    let text = h["contents"]["value"].as_str().unwrap();
    assert!(text.contains("Trait control contract"), "{text}");
    assert!(text.contains("control reader<message>.peek(ref message input) exits found { ref string value from input }, missing"), "{text}");
    assert!(text.contains("Selected histories"), "{text}");
    assert!(
        !text.contains("owner") && !text.contains("payload") && !text.contains("secret"),
        "{text}"
    );
    assert_eq!(
        h["range"]["end"]["character"].as_u64().unwrap()
            - h["range"]["start"]["character"].as_u64().unwrap(),
        "reader<message>.peek".len() as u64
    );
    assert!(
        !hover_at(&f, source, "borrow source")
            .to_string()
            .contains("Trait control contract")
    );
}

#[test]
fn trait_call_hover_keeps_named_nested_contracts_and_declared_group_selectors() {
    let mut f = Fixture::new();
    let source = include_str!("../playground/aggregate_traits.tellur").replace(
        "capture prepare<text_editor>",
        "capture editor<text_editor>.prepare",
    );
    f.open(&source);
    assert_eq!(f.diagnostics(), json!([]));
    let text = hover_at(&f, &source, "editor<text_editor>.prepare(borrow").to_string();
    for expected in [
        "Trait control contract",
        "ref mutable text_view<string> input",
        "exits preparation<string>[input, input['text]]",
        "ready { edit_job<string>[input, input['text]] job }",
        "unavailable",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(
        !text.contains("owner") && !text.contains("_path_"),
        "{text}"
    );
    assert!(
        !hover_at(&f, &source, "editor<e>.prepare(input)")
            .to_string()
            .contains("Trait control contract")
    );
}

#[test]
fn trait_call_hover_normalizes_concrete_associated_type_selections() {
    let mut f = Fixture::new();
    let source = "trait source {type item;}implement source for bool {type item=string;}trait reader {control read(self input) exits done;}implement reader for string {control read(self owned) exits done {emit(owned);exit done;}}struct box<t> {t value;constructor(t input) {self.value=move input;}}\ncontrol echo<t:drop>(t input) exits done {drop(input);exit done;}\ncontrol main() exits done {reader<source<bool>.item>.read(\"hello\");auto value=box<source<bool>.item>(\"hello\");drop(value);echo<source<bool>.item>(\"hello\");exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let text = hover_at(&f, source, "reader<source<bool>.item>.read(").to_string();
    assert!(
        text.contains("control reader<source<bool>.item>.read(string input) exits done"),
        "{text}"
    );
    let text = hover_at(&f, source, "box<source<bool>.item>(").to_string();
    assert!(text.contains("constructor(string input)"), "{text}");
    let text = hover_at(&f, source, "echo<source<bool>.item>(").to_string();
    assert!(
        text.contains("control echo<source<bool>.item>(string input) exits done"),
        "{text}"
    );
}

#[test]
fn trait_call_hover_never_confuses_lexical_parameters_with_concrete_types() {
    let mut f = Fixture::new();
    let source = "struct value {u32 number;}trait reader {control read(ref self input) exits ready {u32 number};}implement reader for value {control read(ref self owner) exits ready {u32 output} {exit ready {owner.number};}}\ncontrol wrap<value:reader>(ref value input) exits ready {u32 number} {forward reader<value>.read(input);}\ncontrol main() exits done {value owner=value {7};delta reader<value>.read(borrow owner) {ready {auto number}=>read} [read]:drop(number);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        !hover_at(&f, source, "reader<value>.read(input)")
            .to_string()
            .contains("Trait control contract")
    );
    assert!(
        hover_at(&f, source, "reader<value>.read(borrow")
            .to_string()
            .contains("Trait control contract")
    );
    let constructor = "struct value {u32 number;}struct box<t> {t value;constructor(t input) {self.value=move input;}}control make<value>(value input) exits ready {box<value> result} {exit ready {box<value>(move input)};}control main() exits done {auto result=box<value>(value {7});drop(result);exit done;}";
    f.change(constructor, 2);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        !hover_at(&f, constructor, "box<value>(move input)")
            .to_string()
            .contains("Constructor contract")
    );
    assert!(
        hover_at(&f, constructor, "box<value>(value {7})")
            .to_string()
            .contains("Constructor contract")
    );
}

#[test]
fn trait_call_hover_resolves_manifest_named_outcome_alternatives() {
    let mut f = Fixture::new();
    let path = f.root.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["types"].as_array_mut().unwrap().push(json!({
        "kind":"outcome", "name":"answer", "constructors":[
            {"name":"ready","fields":[{"name":"declared","type":"u32"}]},
            {"name":"missing","fields":[]}
        ]
    }));
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let source = "trait reader {control read(self input) exits answer;}implement reader for bool {control read(self flag) exits ready {u32 actual},missing {exit ready {7};}}control main() exits done {delta reader<bool>.read(true) {ready {auto value}=>shown,missing=>absent} [shown]:drop(value);[entry]:exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let text = hover_at(&f, source, "reader<bool>.read(").to_string();
    assert!(
        text.contains("read(bool input) exits answer")
            && text.contains("ready { u32 declared }, missing"),
        "{text}"
    );
    assert!(!text.contains("actual") && !text.contains("flag"), "{text}");
}

#[test]
fn trait_call_hover_refreshes_imported_declarations_without_implementation_names() {
    let mut f = Fixture::new();
    let dependency = f.root.join("reader.tellur");
    let uri = url::Url::from_file_path(&dependency).unwrap().to_string();
    let valid = "pub struct message {string secret;}\npub trait reader {type item:clone;control peek(ref self input) exits found {ref item value from input};}\nimplement reader for message {type item=string;control peek(ref self owner) exits found {ref item payload from owner} {exit found {borrow owner.secret};}}\npub control make() exits ready {message value} {exit ready {message {\"hello\"}};}";
    fs::write(&dependency, valid).unwrap();
    let source = "import \"reader.tellur\" as one;import \"reader.tellur\" as lib;control main() exits done {delta lib.make() {ready {auto source}=>ready} [ready]:delta lib.reader<lib.message>.peek(borrow source) {found {auto text}=>shown} [shown]:emit(text);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = hover_at(&f, source, "lib.reader<lib.message>.peek").to_string();
    assert!(h.contains("control lib.reader<lib.message>.peek(ref lib.message input) exits found { ref string value from input }"), "{h}");
    assert!(
        !h.contains("secret") && !h.contains("owner") && !h.contains("_path_m_"),
        "{h}"
    );
    let changed = valid
        .replace("input", "source")
        .replace("item value from source", "item fresh from source");
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"version":1,"text":changed}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    let h = hover_at(&f, source, "lib.reader<lib.message>.peek").to_string();
    assert!(
        h.contains("ref lib.message source") && h.contains("ref string fresh from source"),
        "{h}"
    );
    let broken = changed.replace("from source", "from missing");
    f.server.notification(
        "textDocument/didChange",
        &json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":broken}]}),
    );
    assert!(!f.diagnostics().as_array().unwrap().is_empty());
    assert!(hover_at(&f, source, "lib.reader<lib.message>.peek").is_null());
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert!(
        hover_at(&f, source, "lib.reader<lib.message>.peek")
            .to_string()
            .contains("ref string value from input")
    );
}

#[test]
fn trait_call_hover_uses_declared_fault_and_contingency_permissions() {
    let mut f = Fixture::new();
    let source = "trait task {@contingent control ping(self input) exits done faults;control quiet(self input) exits done;control stop(self input) faults;}\nimplement task for string {control ping(self owned) exits done {exit done;}control quiet(self owned) exits done {exit done;}control stop(self reason) faults {fault reason;}}\ncontrol main() exits done {task<string>.ping(\"hi\");task<string>.quiet(\"hi\");exit done;}\ncontrol stop() faults {task<string>.stop(\"broken\");}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let ping = hover_at(&f, source, "task<string>.ping(").to_string();
    assert!(
        ping.contains("@contingent") && ping.contains("ping(string input) exits done faults"),
        "{ping}"
    );
    let quiet = hover_at(&f, source, "task<string>.quiet(").to_string();
    assert!(quiet.contains("quiet(string input) exits done"), "{quiet}");
    assert!(
        !quiet.contains("@contingent") && !quiet.contains(" faults"),
        "{quiet}"
    );
    let stop = hover_at(&f, source, "task<string>.stop(").to_string();
    assert!(stop.contains("stop(string input) faults"), "{stop}");
    assert!(!stop.contains(" exits "), "{stop}");
}

#[test]
fn trait_call_hover_handles_untitled_and_keeps_abstract_calls_independent() {
    let mut f = Fixture::new();
    f.uri = "untitled:trait-call-hover".into();
    let source = "trait sink {control put(self input) exits done;}implement sink for string {control put(self value) exits done {emit(value);exit done;}}control generic<t:sink>(t input) exits done {forward sink<t>.put(input);}control main() exits done {sink<string>.put(\"hello\");exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "sink<string>.put(")
            .to_string()
            .contains("control sink<string>.put(string input) exits done")
    );
    assert!(
        !hover_at(&f, source, "sink<t>.put(")
            .to_string()
            .contains("Trait control contract")
    );
}

#[test]
fn constructor_call_hover_has_checked_inputs_origins_and_exact_callee_range() {
    let mut f = Fixture::new();
    let source = "struct view['a] {ref mutable string hidden <- 'a;constructor(ref mutable string input)[input] {self.hidden=input;}}\ncontrol main() exits done {mutable string owner=\"hello\";auto result=(view(borrow mutable owner));emit(result.hidden);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = hover_at(&f, source, "view(borrow");
    let text = h["contents"]["value"].as_str().unwrap();
    assert!(
        text.contains("Constructor contract") && text.contains("Constructs `view`"),
        "{text}"
    );
    assert!(
        text.contains("constructor(ref mutable string input)[input]"),
        "{text}"
    );
    assert!(
        text.contains("['a]") && text.contains("Selected histories"),
        "{text}"
    );
    assert!(
        !text.contains("hidden")
            && !text.contains("$constructor")
            && !text.contains("$constructed"),
        "{text}"
    );
    assert_eq!(
        h["range"]["end"]["character"].as_u64().unwrap()
            - h["range"]["start"]["character"].as_u64().unwrap(),
        4
    );
    assert!(
        !hover_at(&f, source, "borrow mutable owner")
            .to_string()
            .contains("Constructor contract")
    );
    let broken = source.replace("[input]", "[missing]");
    f.change(&broken, 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_ORIGIN");
    assert!(hover_at(&f, &broken, "view(borrow").is_null());
    f.change(source, 3);
    assert!(
        hover_at(&f, source, "view(borrow")
            .to_string()
            .contains("[input]")
    );
}

#[test]
fn constructor_call_hover_specializes_imports_without_private_layouts() {
    let mut f = Fixture::new();
    let dependency = f.root.join("types.tellur");
    let uri = url::Url::from_file_path(&dependency).unwrap().to_string();
    let valid = "pub struct box<t> {t secret;pub constructor(t input) {self.secret=move input;}}";
    fs::write(&dependency, valid).unwrap();
    let source = "import \"types.tellur\" as one;import \"types.tellur\" as lib;control main() exits done {auto value=lib.box<string>(\"hello\");drop(value);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = hover_at(&f, source, "lib.box<string>");
    let text = h["contents"]["value"].as_str().unwrap();
    assert!(
        text.contains("Constructs `lib.box<string>`")
            && text.contains("pub constructor(string input)"),
        "{text}"
    );
    assert!(
        !text.contains("secret") && !text.contains("_path_m_") && !text.contains("$constructor"),
        "{text}"
    );
    assert_eq!(
        h["range"]["end"]["character"].as_u64().unwrap()
            - h["range"]["start"]["character"].as_u64().unwrap(),
        15
    );
    let changed = valid.replace("input", "fresh");
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"version":1,"text":changed}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "lib.box<string>")
            .to_string()
            .contains("constructor(string fresh)")
    );
    let broken = changed.replace("self.secret=move fresh;", "");
    f.server.notification(
        "textDocument/didChange",
        &json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":broken}]}),
    );
    assert!(!f.diagnostics().as_array().unwrap().is_empty());
    assert!(hover_at(&f, source, "lib.box<string>").is_null());
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert!(
        hover_at(&f, source, "lib.box<string>")
            .to_string()
            .contains("constructor(string input)")
    );
}

#[test]
fn constructor_call_hover_tracks_inferred_faults_and_nested_call_names() {
    let mut f = Fixture::new();
    let source = "struct message {string text;constructor(string input) {self.text=move input;}}\nstruct envelope {message inner;constructor(message input) {self.inner=move input;}}\ncontrol main() exits done {auto value=envelope(message(\"hello\"));emit(move value.inner.text);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "envelope(message")
            .to_string()
            .contains("constructor(message input)")
    );
    assert!(
        hover_at(&f, source, "message(\"hello")
            .to_string()
            .contains("constructor(string input)")
    );
    let faulting = source.replace("self.text=move input;", "fault \"unavailable\";");
    f.change(&faulting, 2);
    assert_eq!(f.diagnostics(), json!([]));
    let text = hover_at(&f, &faulting, "message(\"hello").to_string();
    assert!(
        text.contains("constructor(string input) faults") && text.contains("execution boundary"),
        "{text}"
    );
    assert!(
        hover_at(&f, &faulting, "envelope(message")
            .to_string()
            .contains("constructor(message input)")
    );
    f.change(source, 3);
    assert!(
        !hover_at(&f, source, "message(\"hello")
            .to_string()
            .contains(" faults")
    );
}

#[test]
fn constructor_call_hover_handles_untitled_and_leaves_abstract_calls_uninstantiated() {
    let mut f = Fixture::new();
    f.uri = "untitled:constructor-hover".into();
    let source = "struct box<t> {t value;constructor(t input) {self.value=move input;}}\ncontrol make<t>(t input) exits ready {box<t> value} {exit ready {box<t>(move input)};}\ncontrol main() exits done {auto value=box<u32>(7);drop(value);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "box<u32>(7)")
            .to_string()
            .contains("constructor(u32 input)")
    );
    assert!(
        !hover_at(&f, source, "box<t>(move")
            .to_string()
            .contains("Constructor contract")
    );
}

#[test]
fn named_completion_bindings_repair_independently_without_stale_hover() {
    let mut f = Fixture::new();
    let source = "outcome view['a] {found {ref string value <- 'a}}\ncontrol first(ref string input) exits view[input] {exit found {input};}\ncontrol second(ref string input) exits view[input] {exit found {input};}\ncontrol main() exits done {string owner=\"hello\";delta first(borrow owner) {found {auto text}=>shown} [shown]: emit(text);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "first(borrow")
            .to_string()
            .contains("Control contract")
    );
    assert!(
        hover_at(&f, source, "auto text")
            .to_string()
            .contains("ref string")
    );
    let broken = source.replace("view[input]", "view[input,input]");
    f.change(&broken, 2);
    let errors = f.diagnostics();
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    for (index, error) in errors.as_array().unwrap().iter().enumerate() {
        assert_eq!(error["code"], "E_ORIGIN");
        assert_eq!(error["range"]["start"]["line"], index + 1);
    }
    assert!(hover_at(&f, &broken, "first(borrow").is_null());
    assert!(hover_at(&f, &broken, "auto text").is_null());
    let partial = broken.replacen("view[input,input]", "view[input]", 1);
    f.change(&partial, 3);
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 1);
    assert!(hover_at(&f, &partial, "first(borrow").is_null());
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "first(borrow")
            .to_string()
            .contains("Control contract")
    );
    assert!(
        hover_at(&f, source, "auto text")
            .to_string()
            .contains("ref string")
    );
}

#[test]
fn unsaved_named_schema_edits_recheck_every_imported_completion_binding() {
    let mut f = Fixture::new();
    let dependency = f.root.join("views.tellur");
    let uri = url::Url::from_file_path(&dependency).unwrap().to_string();
    let valid = "pub outcome view['a] {found {ref string value <- 'a}}\npub control first(ref string input) exits view[input] {exit found {input};}\npub control second(ref string input) exits view[input] {exit found {input};}";
    fs::write(&dependency, valid).unwrap();
    let source = "import \"views.tellur\" as lib;control main() exits done {string owner=\"hi\";delta lib.first(borrow owner) {found {auto text}=>shown} [shown]: emit(text);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let broken = valid
        .replace("view['a]", "view['a,'b]")
        .replace("value <- 'a", "value <- 'a,ref string other <- 'b")
        .replace("found {input}", "found {input,input}");
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"version":1,"text":broken}}),
    );
    let errors = f.diagnostics();
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    for (index, error) in errors.as_array().unwrap().iter().enumerate() {
        assert_eq!(error["code"], "E_ORIGIN");
        let location = &error["relatedInformation"][0]["location"];
        assert_eq!(location["uri"], uri);
        assert_eq!(location["range"]["start"]["line"], index + 1);
        assert!(!error["message"].as_str().unwrap().contains("_path_m_"));
    }
    assert!(hover_at(&f, source, "lib.first(borrow").is_null());
    let partial = broken.replacen("view[input]", "view[input,input]", 1);
    f.server.notification(
        "textDocument/didChange",
        &json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":partial}]}),
    );
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 1);
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    let hover = hover_at(&f, source, "lib.first(borrow").to_string();
    assert!(hover.contains("exits lib.view[input]"), "{hover}");
    assert!(!hover.contains("_path_m_"));
}

#[test]
fn declaration_errors_repair_independently_before_call_hover_returns() {
    let mut f = Fixture::new();
    let source = "control one(u32 input,u32 other) exits done {exit done;}\ncontrol two(u32 value,u32 other) exits done {exit done;}\ncontrol main() exits done {one(1,2);two(3,4);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "one(1")
            .to_string()
            .contains("Control contract")
    );
    let broken = source
        .replace("u32 input,u32 other", "u32 input,u32 input")
        .replace("u32 value,u32 other", "u32 value,u32 value");
    f.change(&broken, 2);
    let errors = f.diagnostics();
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    for (line, error) in errors.as_array().unwrap().iter().enumerate() {
        assert_eq!(error["code"], "E_NAME");
        assert_eq!(error["range"]["start"]["line"], line);
    }
    assert!(hover_at(&f, &broken, "one(1").is_null());
    let partial = broken.replace("u32 input,u32 input", "u32 input,u32 other");
    f.change(&partial, 3);
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 1);
    assert!(hover_at(&f, &partial, "one(1").is_null());
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "one(1")
            .to_string()
            .contains("u32 other")
    );
}

#[test]
fn unsaved_imported_public_declarations_batch_and_refresh_after_repairs() {
    let mut f = Fixture::new();
    let dependency = f.root.join("library.tellur");
    let uri = url::Url::from_file_path(&dependency).unwrap().to_string();
    let valid =
        "pub control first() exits done {exit done;}\npub control second() exits done {exit done;}";
    fs::write(&dependency, valid).unwrap();
    let source =
        "import \"library.tellur\" as lib;control main() exits done {forward lib.first();}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let broken = valid.replace("exits done", "exits auto");
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"version":1,"text":broken}}),
    );
    let errors = f.diagnostics();
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    for (line, error) in errors.as_array().unwrap().iter().enumerate() {
        assert_eq!(error["code"], "E_PUBLIC_CONTRACT");
        let location = &error["relatedInformation"][0]["location"];
        assert_eq!(location["uri"], uri);
        assert_eq!(location["range"]["start"]["line"], line);
    }
    assert!(hover_at(&f, source, "lib.first()").is_null());
    let partial = broken.replacen("exits auto", "exits done", 1);
    f.server.notification(
        "textDocument/didChange",
        &json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":partial}]}),
    );
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 1);
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "lib.first()")
            .to_string()
            .contains("control lib.first() exits done")
    );
}

#[test]
fn call_hover_resolves_imports_aliases_and_fresh_unsaved_contracts() {
    let mut f = Fixture::new();
    let library = f.root.join("library.tellur");
    let uri = url::Url::from_file_path(&library).unwrap().to_string();
    let definition = "pub outcome reply {ready {u32 value}} pub control read(u32 number) exits reply {exit ready {number};}";
    fs::write(&library, definition).unwrap();
    let source = "import \"library.tellur\" as one; import \"library.tellur\" as lib;\ncontrol main() exits auto {forward lib.read(7);}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = hover_at(&f, source, "lib.read(7)");
    let text = h["contents"]["value"].as_str().unwrap();
    assert!(
        text.contains("control lib.read(u32 number) exits lib.reply"),
        "{text}"
    );
    assert!(
        text.contains("ready { u32 value }") && text.contains("Selected histories"),
        "{text}"
    );
    assert_eq!(
        h["range"]["end"]["character"].as_u64().unwrap()
            - h["range"]["start"]["character"].as_u64().unwrap(),
        8
    );
    assert!(!text.contains("_path_m_"));
    let changed = definition
        .replace("u32 value", "string value")
        .replace("ready {number}", "ready {\"fresh\"}");
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"version":1,"text":changed}}),
    );
    let h = hover_at(&f, source, "read(7)").to_string();
    assert!(h.contains("string value"), "{h}");
    f.server.notification("textDocument/didChange", &json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"pub control broken("}]}));
    assert!(hover_at(&f, source, "lib.read(7)").is_null());
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert!(
        hover_at(&f, source, "lib.read(7)")
            .to_string()
            .contains("u32 value")
    );
}

#[test]
fn call_hover_has_precise_names_and_leaves_arguments_comments_and_strings_alone() {
    let mut f = Fixture::new();
    let source = "control leaf(u32 input) exits auto {exit ready {input};}\ncontrol main() exits auto {emit(\"🙂 leaf(7)\"); exit delta (leaf // gap leaf(9)\n (7)) {ready {u32 value} => done {}}; }";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = hover_at(&f, source, "leaf //");
    assert!(
        h.to_string()
            .contains("control leaf(u32 input) exits ready"),
        "{h}"
    );
    assert_eq!(
        h["range"]["end"]["character"].as_u64().unwrap()
            - h["range"]["start"]["character"].as_u64().unwrap(),
        4
    );
    assert!(
        !hover_at(&f, source, "leaf(7)")
            .to_string()
            .contains("Control contract")
    );
    assert!(hover_at(&f, source, "leaf(9)").is_null());
    assert!(hover_at(&f, source, "gap").is_null());
    assert!(
        !hover_at(&f, source, "7))")
            .to_string()
            .contains("Control contract")
    );
}

#[test]
fn call_hover_shows_checked_inference_origins_and_fault_permission() {
    let mut f = Fixture::new();
    let source = "control peek(ref string input) exits auto {exit found {input};}\ncontrol fail(string reason) faults {fault reason;}\ncontrol main() exits done {string text=\"a\";delta peek(borrow text) {found {ref string item}=>found} [found]: fail(\"reason\");}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let peek = hover_at(&f, source, "peek(borrow").to_string();
    assert!(
        peek.contains("ref string input") && peek.contains("inferred"),
        "{peek}"
    );
    assert!(
        peek.contains("Checked return lifetimes") && peek.contains("borrows `input`"),
        "{peek}"
    );
    let fail = hover_at(&f, source, "fail(\"").to_string();
    assert!(
        fail.contains("control fail(string reason) faults"),
        "{fail}"
    );
    f.change(&source.replace("string text=\"a\"", "u32 text=\"a\""), 2);
    assert!(hover_at(&f, source, "peek(borrow").is_null());
}

#[test]
fn call_hover_specializes_concrete_generic_calls_without_name_prefix_collisions() {
    let mut f = Fixture::new();
    let library = f.root.join("library.tellur");
    fs::write(&library, "pub struct item {pub u32 value;} pub struct item_extra {pub string text;} pub control echo<t>(t value) exits ready {t value} {exit ready {value};}").unwrap();
    let source = "import \"library.tellur\" as lib; control main() exits auto {forward lib.echo<lib.item_extra>(lib.item_extra {text: \"hi\"});}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = hover_at(&f, source, "lib.echo<").to_string();
    assert!(h.contains("control lib.echo<lib.item_extra>(lib.item_extra value) exits ready { lib.item_extra value }"), "{h}");
    assert!(!h.contains("_path_m_"));
}

#[test]
fn call_hover_preserves_public_origins_without_exposing_private_layouts() {
    let mut f = Fixture::new();
    fs::write(f.root.join("library.tellur"), "pub struct message {string secret;} pub control make() exits made {message value} {exit made {message {secret: \"hi\"}};} pub control peek(ref message input) exits found {ref string value from input} {exit found {borrow input.secret};}").unwrap();
    let source = "import \"library.tellur\" as lib;control main() exits auto {delta lib.make() {made {lib.message value}=>made} [made]:delta lib.peek(borrow value) {found {ref string text}=>found} [found]: emit(text); exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = hover_at(&f, source, "lib.peek(").to_string();
    assert!(
        h.contains("ref lib.message input") && h.contains("from input"),
        "{h}"
    );
    assert!(!h.contains("secret"), "{h}");
}

#[test]
fn call_hover_keeps_public_grouped_result_mappings() {
    let mut f = Fixture::new();
    fs::write(
        f.root.join("library.tellur"),
        r#"
        pub struct pair['left, 'right] {
            pub ref string left <- 'left;
            pub ref string right <- 'right;
        }
        pub control join(ref string first, ref string second)
            exits ready {pair[first, second] value}
        { exit ready {pair {left: first, right: second}}; }
    "#,
    )
    .unwrap();
    let source = "import \"library.tellur\" as lib;control main() exits done {string a=\"a\";string b=\"b\";delta lib.join(borrow a,borrow b) {ready {auto value}=>got} [got]:emit(value.left);emit(value.right);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = hover_at(&f, source, "lib.join(").to_string();
    assert!(h.contains("lib.pair[first, second] value"), "{h}");
    assert!(h.contains("ref string first, ref string second"), "{h}");
}

#[test]
fn primitive_call_hover_includes_modes_effects_and_host_execution() {
    let mut f = Fixture::new();
    fs::write(
        f.root.join("manifest.json"),
        include_str!("../examples/json_file/manifest.json"),
    )
    .unwrap();
    let source = "control main() exits auto {forward file_read(borrow \"unused\", 17);}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = hover_at(&f, source, "file_read(").to_string();
    assert!(
        h.contains("Manifest primitive") && h.contains("file_read(ref string path, u32 limit)"),
        "{h}"
    );
    assert!(
        h.contains("file.read")
            && h.contains("Host execution: blocking")
            && h.contains("Selected histories"),
        "{h}"
    );
    assert!(!f.root.join("host.rs").exists());
    let decoded = "control main() exits auto {forward json_decode(\"{}\");}";
    f.change(decoded, 2);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, decoded, "json_decode(")
            .to_string()
            .contains("Consume-mode inputs: `text`")
    );
}

#[test]
fn call_hover_tracks_captures_inline_adapters_statement_arms_and_pure_arguments() {
    let mut f = Fixture::new();
    fs::write(
        f.root.join("manifest.json"),
        include_str!("../playground/manifest.json"),
    )
    .unwrap();
    let source = r#"
        control one(u32 input) exits ready {u32 value} {exit ready {input};}
        control noop() exits done {exit done;}
        control main() exits auto {
            auto saved = capture one(1);
            delta saved {ready {u32 first} => got}
            [got]: {
                noop();
                emit(concat("a", u32_text(first)));
                auto adapted = delta one(2) {ready {u32 second} => done {}};
                delta adapted {done => {
                    exit delta one(3) {ready {u32 third} => done {}};
                }}
            }
        }
    "#;
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    for needle in ["one(1)", "one(2)", "one(3)"] {
        let h = hover_at(&f, source, needle).to_string();
        assert!(
            h.contains("control one(u32 input) exits ready { u32 value }")
                && h.contains("Selected histories"),
            "{needle}: {h}"
        );
    }
    assert!(
        hover_at(&f, source, "noop();")
            .to_string()
            .contains("control noop() exits done")
    );
    for needle in ["concat(\"", "u32_text(first)"] {
        let h = hover_at(&f, source, needle).to_string();
        assert!(
            h.contains("Manifest primitive") && h.contains("Pure builtin"),
            "{h}"
        );
    }
}

#[test]
fn call_hover_uses_distinct_display_identities_for_unimported_transitive_types() {
    let mut f = Fixture::new();
    fs::write(
        f.root.join("types.tellur"),
        "pub struct token {pub u32 value;}",
    )
    .unwrap();
    fs::write(f.root.join("library.tellur"), "import \"types.tellur\" as data;pub control make() exits made {data.token value} {exit made {data.token {value: 3}};}").unwrap();
    let source = "import \"library.tellur\" as lib;struct token {string local;}control main() exits auto {forward lib.make();}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = hover_at(&f, source, "lib.make()").to_string();
    assert!(
        h.contains("types.tellur::token value") && h.contains("display identities"),
        "{h}"
    );
    assert!(!h.contains("_path_m_"), "{h}");
}

#[test]
fn call_hover_handles_untitled_sources_and_does_not_guess_abstract_instantiations() {
    let mut f = Fixture::new();
    f.uri = "untitled:call-hover".into();
    let source =
        "control leaf() exits auto {exit ready {7};}control main() exits auto {forward leaf();}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        hover_at(&f, source, "leaf();")
            .to_string()
            .contains("control leaf() exits ready")
    );
    let generic = "control echo<t>(t value) exits auto {exit ready {value};}control wrap<t>(t value) exits auto {forward echo<t>(value);}control main() exits auto {forward wrap<u32>(7);}";
    f.change(generic, 2);
    assert_eq!(f.diagnostics(), json!([]));
    let abstract_call = hover_at(&f, generic, "echo<t>(value)").to_string();
    assert!(
        !abstract_call.contains("Control contract"),
        "{abstract_call}"
    );
    let concrete = hover_at(&f, generic, "wrap<u32>(7)").to_string();
    assert!(
        concrete.contains("control wrap<u32>(u32 value) exits ready"),
        "{concrete}"
    );
}
#[test]
fn absorption_snapshots_show_unsaved_internal_changes_across_imports() {
    let mut f = Fixture::new();
    let library = f.root.join("library.tellur");
    let library_uri = url::Url::from_file_path(&library).unwrap().to_string();
    let before = "control leaf() exits auto {exit missing;}pub control lookup() exits failed {exit delta leaf() {missing=>failed {}};}";
    fs::write(&library, before).unwrap();
    f.open("import \"library.tellur\" as lib;pub control lookup() exits failed {forward lib.lookup();}");
    let snapshot = |f: &Fixture| {
        tellur::absorption::Snapshot::from_json(
            &f.server.absorption_snapshot(&f.uri).unwrap().to_string(),
        )
        .unwrap()
    };
    let first = snapshot(&f);
    f.server.notification("textDocument/didOpen",&json!({"textDocument":{"uri":library_uri,"version":1,"text":before.replace("missing","malformed")}}));
    let second = snapshot(&f);
    let diff = first.compare(&second).unwrap();
    assert!(diff.public_contracts_equal && !diff.recorded_evidence_equal);
    let evidence = &second.controls["@entry::lookup"].outcomes["failed"].contributors;
    assert!(
        evidence
            .iter()
            .any(|c| c.control == "file:library.tellur::leaf"
                && c.outcome == "malformed"
                && c.location.module == "file:library.tellur")
    );
    f.server.notification("textDocument/didChange",&json!({"textDocument":{"uri":library_uri,"version":2},"contentChanges":[{"text":"control broken("}]}));
    assert!(f.server.absorption_snapshot(&f.uri).is_err());
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":library_uri}}),
    );
    assert_eq!(snapshot(&f), first);
    f.change("pub control bad() exits auto {exit done;}", 2);
    assert!(f.server.absorption_snapshot(&f.uri).is_err());
    f.change("pub control lookup() exits done {exit done;}", 1);
    assert!(f.server.absorption_snapshot(&f.uri).is_err());
}
#[test]
fn absorption_snapshots_require_open_file_and_manifest() {
    let mut f = Fixture::new();
    assert!(f.server.absorption_snapshot(&f.uri).is_err());
    f.open("pub control lookup() exits done {exit done;}");
    fs::remove_file(f.root.join("manifest.json")).unwrap();
    assert!(f.server.absorption_snapshot(&f.uri).is_err());
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":"untitled:absorption","version":1,"text":VALID}}),
    );
    assert!(
        f.server
            .absorption_snapshot("untitled:absorption")
            .unwrap_err()
            .contains("save the source file")
    );
}
#[test]
fn contract_snapshots_use_unsaved_graphs_and_refuse_partial_or_stale_results() {
    let mut f = Fixture::new();
    let library = f.root.join("library.tellur");
    let library_uri = url::Url::from_file_path(&library).unwrap().to_string();
    fs::write(&library, "pub struct record {pub u32 value;}").unwrap();
    f.open("import \"library.tellur\" as lib;pub control echo(lib.record input) exits ready {lib.record value} {exit ready {input};}");
    let first = f.server.contract_snapshot(&f.uri).unwrap();
    assert_eq!(
        first["modules"]["file:library.tellur"]["declarations"]["record"]["fields"][0]["type"],
        "u32"
    );
    f.server.notification("textDocument/didOpen", &json!({"textDocument":{"uri":library_uri,"version":1,"text":"pub struct record {pub string value;}"}}));
    let second = f.server.contract_snapshot(&f.uri).unwrap();
    assert_eq!(
        second["modules"]["file:library.tellur"]["declarations"]["record"]["fields"][0]["type"],
        "string"
    );
    f.server.notification("textDocument/didChange", &json!({"textDocument":{"uri":library_uri,"version":2},"contentChanges":[{"text":"pub struct record {pub missing value;}"}]}));
    assert!(f.server.contract_snapshot(&f.uri).is_err());
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":library_uri}}),
    );
    assert_eq!(f.server.contract_snapshot(&f.uri).unwrap(), first);
    f.change("pub control echo() exits auto {exit done;}", 2);
    assert!(
        f.server
            .contract_snapshot(&f.uri)
            .unwrap_err()
            .contains("E_PUBLIC_CONTRACT")
    );
    f.change("pub control echo() exits done {exit done;}", 1);
    assert!(f.server.contract_snapshot(&f.uri).is_err());
}

#[test]
fn contract_snapshots_require_a_file_identity_manifest_and_open_buffer() {
    let mut f = Fixture::new();
    assert!(f.server.contract_snapshot(&f.uri).is_err());
    f.open("pub control f() exits done {exit done;}");
    fs::remove_file(f.root.join("manifest.json")).unwrap();
    assert!(f.server.contract_snapshot(&f.uri).is_err());
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":"untitled:scratch","version":1,"text":VALID}}),
    );
    assert!(
        f.server
            .contract_snapshot("untitled:scratch")
            .unwrap_err()
            .contains("save the source file")
    );
}
#[test]
fn formatting_uses_current_buffer_options_and_utf16_document_extent() {
    let mut f = Fixture::new();
    let original = "control main() exits done {emit(\"😀\");exit done;}//😀";
    f.open(original);
    let options = tellur::format::Options {
        tab_size: 2,
        insert_spaces: true,
        ..Default::default()
    };
    let edits = f.server.formatting(&f.uri, options);
    assert_eq!(
        edits[0]["range"],
        json!({"start":{"line":0,"character":0},"end":{"line":0,"character":original.encode_utf16().count()}})
    );
    let formatted = edits[0]["newText"].as_str().unwrap();
    assert!(formatted.contains("\n  emit(\"😀\");"));
    f.change(formatted, 2);
    assert_eq!(f.server.formatting(&f.uri, options), json!([]));
    f.change("control main() exits done {\r\nexit done;} //😀", 3);
    let edits = f.server.formatting(
        &f.uri,
        tellur::format::Options {
            insert_spaces: false,
            ..options
        },
    );
    assert_eq!(
        edits[0]["range"]["end"],
        json!({"line":1,"character":"exit done;} //😀".encode_utf16().count()})
    );
    assert!(
        edits[0]["newText"]
            .as_str()
            .unwrap()
            .contains("\r\n\texit done;")
    );
}

#[test]
fn formatting_works_without_semantics_or_manifest_and_refuses_broken_syntax() {
    let mut server = Server::default();
    let uri = "untitled:formatting";
    let options = tellur::format::Options::default();
    assert_eq!(server.formatting(uri, options), json!([]));
    let text = "import \"missing.tellur\" as missing;control main() exits done {u32 value=\"wrong type\";exit done;}";
    server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"version":1,"text":text}}),
    );
    assert_eq!(
        server.formatting(uri, options)[0]["newText"],
        tellur::format::source(text, options).unwrap()
    );
    server.notification("textDocument/didChange", &json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"control broken("}]}));
    assert_eq!(server.formatting(uri, options), json!([]));
    // Older notifications must not restore an edit for the replaced document.
    server.notification(
        "textDocument/didChange",
        &json!({"textDocument":{"uri":uri,"version":1},"contentChanges":[{"text":text}]}),
    );
    assert_eq!(server.formatting(uri, options), json!([]));
    server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(server.formatting(uri, options), json!([]));
}
#[test]
fn independent_errors_update_together_without_stale_inferred_hover() {
    let mut f = Fixture::new();
    let source = "control main() exits done {auto value=\"current\";emit(value);exit done;}\ncontrol first() exits done {u32 value=1;exit done;}\ncontrol second() exits done {string value=\"ok\";exit done;}\ncontrol caller() exits done {forward first();}";
    let column = source.lines().next().unwrap().find("auto").unwrap() as u64;
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        f.server
            .hover(&f.uri, 0, column)
            .to_string()
            .contains("string")
    );
    let broken = source
        .replace("u32 value=1", "u32 value=true")
        .replace("string value=\"ok\"", "string value=7");
    f.change(&broken, 2);
    let errors = f.diagnostics();
    assert_eq!(errors.as_array().unwrap().len(), 2);
    assert_eq!(errors[0]["range"]["start"]["line"], 1);
    assert_eq!(errors[1]["range"]["start"]["line"], 2);
    assert!(f.server.hover(&f.uri, 0, column).is_null());
    f.change(&broken.replace("u32 value=true", "u32 value=1"), 3);
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 1);
    assert!(f.server.hover(&f.uri, 0, column).is_null());
    f.change(source, 2);
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 1);
    f.change(
        &source.replace("auto value=\"current\";emit(value);", "auto value=7;"),
        4,
    );
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        f.server
            .hover(&f.uri, 0, column)
            .to_string()
            .contains("u32")
    );
}

#[test]
fn statement_errors_repair_independently_and_hover_uses_only_fully_checked_source() {
    let mut f = Fixture::new();
    let source = "control main() exits done {\n    auto value=\"current\";\n    u32 first=1;\n    u32 dependent=first;\n    string second=\"ok\";\n    exit done;\n}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(f.server.hover(&f.uri, 1, 4).to_string().contains("string"));
    let broken = source
        .replace("first=1", "first=true")
        .replace("second=\"ok\"", "second=7");
    f.change(&broken, 2);
    let diagnostics = f.diagnostics();
    assert_eq!(diagnostics.as_array().unwrap().len(), 2);
    assert_eq!(diagnostics[0]["range"]["start"]["line"], 2);
    assert_eq!(diagnostics[1]["range"]["start"]["line"], 4);
    assert!(f.server.hover(&f.uri, 1, 4).is_null());
    f.change(&broken.replace("first=true", "first=1"), 3);
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 1);
    assert!(f.server.hover(&f.uri, 1, 4).is_null());
    f.change(&source.replace("value=\"current\"", "value=7"), 4);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(f.server.hover(&f.uri, 1, 4).to_string().contains("u32"));
}

#[test]
fn nested_branch_errors_clear_independently_without_stale_hover() {
    let mut f = Fixture::new();
    let source = "control main(bool flag) exits done {\n auto title=\"current\";\n delta flag {true=>yes,false=>no}\n [yes]: {u32 first=1;exit done;}\n [no] over &: {string second=\"ok\";exit done;}\n}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(f.server.hover(&f.uri, 1, 1).to_string().contains("string"));
    let broken = source
        .replace("first=1", "first=true")
        .replace("second=\"ok\"", "second=7");
    f.change(&broken, 2);
    let diagnostics = f.diagnostics();
    assert_eq!(diagnostics.as_array().unwrap().len(), 2);
    assert_eq!(diagnostics[0]["range"]["start"]["line"], 3);
    assert_eq!(diagnostics[1]["range"]["start"]["line"], 4);
    assert!(f.server.hover(&f.uri, 1, 1).is_null());
    f.change(&broken.replace("first=true", "first=1"), 3);
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 1);
    f.change(&source.replace("title=\"current\"", "title=7"), 4);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(f.server.hover(&f.uri, 1, 1).to_string().contains("u32"));
}

#[test]
fn independent_imported_errors_follow_unsaved_repairs_and_closing_buffers() {
    let mut f = Fixture::new();
    let path = f.root.join("library.tellur");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let source = "pub control first() exits done {u32 value=1;exit done;}\npub control second() exits done {string value=\"ok\";exit done;}";
    fs::write(&path, source).unwrap();
    f.open("import \"library.tellur\" as lib;\ncontrol main() exits done {forward lib.first();}");
    assert_eq!(f.diagnostics(), json!([]));
    let broken = source
        .replace("u32 value=1", "u32 value=true")
        .replace("string value=\"ok\"", "string value=7");
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"languageId":"tellur","version":1,"text":broken}}),
    );
    let errors = f.diagnostics();
    assert_eq!(errors.as_array().unwrap().len(), 2);
    for (index, error) in errors.as_array().unwrap().iter().enumerate() {
        assert_eq!(error["code"], "E_TYPE");
        let related = &error["relatedInformation"][0]["location"];
        assert_eq!(related["uri"], uri);
        assert_eq!(related["range"]["start"]["line"], index);
    }
    f.server.notification("textDocument/didChange",&json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":broken.replace("u32 value=true","u32 value=1") }]}));
    assert_eq!(f.diagnostics().as_array().unwrap().len(), 1);
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn diagnostic_budget_is_visible_in_the_editor_and_clears_on_repair() {
    let mut f = Fixture::new();
    let source = (0..40)
        .map(|i| format!("control broken_{i}() exits done {{u32 value=true;exit done;}}\n"))
        .collect::<String>();
    f.open(&source);
    let errors = f.diagnostics();
    let errors = errors.as_array().unwrap();
    assert_eq!(errors.len(), 32);
    assert!(
        errors.last().unwrap()["message"]
            .as_str()
            .unwrap()
            .contains("diagnostic limit reached")
    );
    f.change(VALID, 2);
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn expression_ranges_use_utf16_and_follow_unsaved_edits() {
    let mut f = Fixture::new();
    let valid = "control main() exits done {\r\n emit(\"😀\"); auto value = \"a 😀\r\nb\";\r\n exit done;\r\n}";
    f.open(valid);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(f.server.hover(&f.uri, 1, 14).to_string().contains("string"));
    let invalid = valid.replace("auto value", "u32 value");
    f.change(&invalid, 2);
    let errors = f.diagnostics();
    assert_eq!(errors[0]["code"], "E_TYPE");
    let line = invalid.split('\n').nth(1).unwrap();
    let start = line[..line.find("\"a").unwrap()].encode_utf16().count();
    assert_eq!(
        errors[0]["range"],
        json!({"start":{"line":1,"character":start},"end":{"line":2,"character":2}})
    );
    f.change(valid, 3);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(f.server.hover(&f.uri, 1, 14).to_string().contains("string"));
}

#[test]
fn imported_expression_ranges_use_unsaved_dependency_text() {
    let mut f = Fixture::new();
    let path = f.root.join("library.tellur");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let valid = "pub control read() exits done { exit done; }";
    fs::write(&path, valid).unwrap();
    f.open("import \"library.tellur\" as lib;\ncontrol main() exits done {forward lib.read();}");
    assert_eq!(f.diagnostics(), json!([]));
    f.server.notification("textDocument/didOpen", &json!({"textDocument":{"uri":uri,"languageId":"tellur","version":1,"text":"pub control read() exits done {\n u32 value = \"😀\";\n exit done;\n}"}}));
    let errors = f.diagnostics();
    assert_eq!(errors[0]["code"], "E_TYPE");
    assert_eq!(
        errors[0]["range"],
        json!({"start":{"line":0,"character":0},"end":{"line":0,"character":6}})
    );
    let location = &errors[0]["relatedInformation"][0]["location"];
    assert_eq!(location["uri"], uri);
    assert_eq!(
        location["range"],
        json!({"start":{"line":1,"character":13},"end":{"line":1,"character":17}})
    );
    f.server.notification(
        "textDocument/didChange",
        &json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":valid}]}),
    );
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn retained_resource_diagnostics_do_not_need_the_rust_source() {
    let mut f = Fixture::new();
    fs::write(
        f.root.join("manifest.json"),
        include_str!("../examples/retained_file/manifest.json"),
    )
    .unwrap();
    let source = include_str!("../examples/retained_file/main.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    f.change(&source.replace("file_release(file);", ""), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_RESOURCE");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(!f.root.join("host.rs").exists());
}
#[test]
fn explicit_rust_modules_do_not_need_source_files_during_editor_checks() {
    let mut f = Fixture::new();
    let mut manifest: serde_json::Value =
        serde_json::from_str(include_str!("../examples/retained_file/manifest.json")).unwrap();
    manifest["rust"]["modules"] = json!({"files":"absent/file.rs"});
    fs::write(f.root.join("manifest.json"), manifest.to_string()).unwrap();
    let source = include_str!("../examples/retained_file/main.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    f.change(&source.replace("file_release(file);", ""), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_RESOURCE");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(!f.root.join("absent/file.rs").exists());
}
#[test]
fn outcome_status_hover_and_unsaved_annotation_errors() {
    let mut f = Fixture::new();
    let source = "outcome result {\n @exit 0 done,\n @exit 23 missing\n}\ncontrol main() exits result {exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let hover = f.server.hover(&f.uri, 0, 9).to_string();
    assert!(hover.contains("Process exit defaults"), "{hover}");
    assert!(hover.contains("23"), "{hover}");
    f.change(&source.replace("23", "256"), 2);
    let errors = f.diagnostics();
    assert_eq!(errors[0]["code"], "E_EXIT_CODE");
    assert_eq!(errors[0]["range"]["start"]["line"], 2);
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn ellipsis_payload_hover_and_unsaved_diagnostics_follow_the_checked_body() {
    let mut f = Fixture::new();
    let source = include_str!("../playground/inferred_payloads.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let hover = f.server.hover(&f.uri, 0, 10).to_string();
    assert!(hover.contains("ready { string"), "{hover}");
    assert!(hover.contains("Inferred from"), "{hover}");
    f.change(
        &source.replace(
            "ready { \"Hello through an inferred payload\" }",
            "ready {}",
        ),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_INFER");
    assert!(
        f.server
            .symbols(&f.uri)
            .to_string()
            .contains("ready... (unresolved)")
    );
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn stored_reference_records_diagnose_unsaved_owner_conflicts_and_moves() {
    let mut f = Fixture::new();
    let source = include_str!("../playground/stored_references.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    f.change(
        &source.replace(
            "message_view second = first;",
            "original.text=\"bad\";message_view second = first;",
        ),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_LOAN");
    f.change(
        &source.replace("emit(second.label);", "emit(first.label);"),
        3,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_MOVED");
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn owned_variant_diagnostics_follow_unsaved_edits() {
    let mut f = Fixture::new();
    let source = include_str!("../examples/optional_handle.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    for (version, insert, code, name) in [
        (2, "[Available]: duplicate := file;", "E_MOVED", "file"),
        (
            3,
            "[Absent]: duplicate := handle;",
            "E_UNAVAILABLE",
            "handle",
        ),
    ] {
        f.change(
            &source.replace(
                "    // Try `[Available]",
                &format!("    {insert}\n    // Try `[Available]"),
            ),
            version,
        );
        let diagnostics = f.diagnostics();
        assert_eq!(diagnostics[0]["code"], code);
        assert!(diagnostics[0]["message"].as_str().unwrap().contains(name));
    }
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn partial_move_diagnostics_follow_unsaved_edits() {
    let mut f = Fixture::new();
    let source = include_str!("../examples/partial_moves.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    for (version, expression, code) in [(2, "pair", "E_PARTIAL_MOVE"), (3, "pair.left", "E_MOVED")]
    {
        f.change(
            &source.replace(
                "first := pair.left;",
                &format!("first := pair.left; duplicate := {expression};"),
            ),
            version,
        );
        let diagnostics = f.diagnostics();
        assert_eq!(diagnostics[0]["code"], code);
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .unwrap()
                .contains("pair.left")
        );
    }
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn diagnostics_follow_unsaved_buffers_and_clear() {
    let mut f = Fixture::new();
    f.open("region main() exits Done { [Entry]: emit(\"hello\"); }");
    assert_eq!(f.diagnostics()[0]["code"], "E_UNCOVERED");
    f.change(VALID, 2);
    assert_eq!(f.diagnostics(), json!([]));
    assert_eq!(f.server.diagnostics(&f.uri)["version"], 2);
}
#[test]
fn stale_versions_do_not_replace_newer_text() {
    let mut f = Fixture::new();
    f.open(VALID);
    f.change("broken", 3);
    f.change(VALID, 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_PARSE");
    assert_eq!(f.server.diagnostics(&f.uri)["version"], 3);
}
#[test]
fn closed_documents_clear_diagnostics() {
    let mut f = Fixture::new();
    f.open("broken");
    let result = f
        .server
        .notification(
            "textDocument/didClose",
            &json!({"textDocument":{"uri":f.uri}}),
        )
        .unwrap();
    assert_eq!(result["diagnostics"], json!([]));
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn utf16_positions_and_crlf() {
    let text = "// header\r\n😀 Δ emit";
    let range = diagnostic_range(text, Span { line: 2, column: 5 });
    assert_eq!(
        range,
        json!({"start":{"line":1,"character":5},"end":{"line":1,"character":9}})
    );
    let mut f = Fixture::new();
    let text = "region main() exits Done { [Entry]: emit(\"😀\"); # }";
    f.open(text);
    let position = text[..text.find('#').unwrap()].encode_utf16().count();
    assert_eq!(f.diagnostics()[0]["range"]["start"]["character"], position);
    assert_eq!(f.diagnostics()[0]["code"], "E_LEX");
}
#[test]
fn compiler_diagnostics_are_reused() {
    let mut f = Fixture::new();
    let source = include_str!("../examples/load.tellur")
        .replace("[ReadBad & CloseBad]: exit Failed(Both(re, ce));", "");
    f.open(&source);
    let d = f.diagnostics();
    assert_eq!(d[0]["code"], "E_UNCOVERED");
    assert!(
        d[0]["message"]
            .as_str()
            .unwrap()
            .contains("ReadBad & CloseBad")
    );
}
#[test]
fn missing_manifest_reports_warning_but_syntax_still_works() {
    let mut f = Fixture::new();
    fs::remove_file(f.root.join("manifest.json")).unwrap();
    f.open(VALID);
    assert_eq!(f.diagnostics()[0]["code"], "E_MANIFEST");
    assert_eq!(f.diagnostics()[0]["severity"], 2);
    f.change("broken", 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_PARSE");
}
#[test]
fn manifest_configuration_and_disk_edits() {
    let mut f = Fixture::new();
    fs::rename(f.root.join("manifest.json"), f.root.join("primitives.json")).unwrap();
    f.open(VALID);
    assert_eq!(f.diagnostics()[0]["code"], "E_MANIFEST");
    f.server.notification(
        "workspace/didChangeConfiguration",
        &json!({"settings":{"tellurLanguage":{"manifestPath":"primitives.json"}}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    fs::write(f.root.join("primitives.json"), "invalid").unwrap();
    f.server
        .notification("workspace/didChangeWatchedFiles", &json!({"changes":[]}));
    assert_eq!(f.diagnostics()[0]["code"], "E_MANIFEST");
}
#[test]
fn outline_has_exact_name_ranges() {
    let mut f = Fixture::new();
    f.open("// 😀\nregion\n  first() exits Done { [Entry]: exit Done; }\nregion second() exits Done { [Entry]: exit Done; }");
    let symbols = f.server.symbols(&f.uri);
    assert_eq!(symbols.as_array().unwrap().len(), 2);
    assert_eq!(symbols[0]["name"], "first");
    assert_eq!(
        symbols[0]["selectionRange"]["start"],
        json!({"line":2,"character":2})
    );
    assert_eq!(symbols[0]["range"]["start"]["line"], 1);
}
#[test]
fn untitled_document_uses_workspace_manifest() {
    let mut f = Fixture::new();
    f.uri = "untitled:Untitled-1".into();
    f.open(VALID);
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn ranged_changes_are_not_misinterpreted_as_full_documents() {
    let mut f = Fixture::new();
    f.open(VALID);
    f.server.notification("textDocument/didChange",&json!({"textDocument":{"uri":f.uri,"version":2},"contentChanges":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"text":"broken"}]}));
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn protocol_initialize_diagnostics_unknown_method_and_shutdown() {
    let (server, client) = Connection::memory();
    let handle = std::thread::spawn(move || tellur::lsp::serve(server).map_err(|e| e.to_string()));
    client
        .sender
        .send(Request::new(1.into(), "initialize".into(), json!({"capabilities":{}})).into())
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
    else {
        panic!("expected initialize response")
    };
    let capabilities = response.response_result.unwrap()["capabilities"].clone();
    assert_eq!(capabilities["textDocumentSync"]["change"], 1);
    assert_eq!(capabilities["documentFormattingProvider"], true);
    assert_eq!(capabilities["experimental"]["tellurContractSnapshot"], 1);
    assert_eq!(capabilities["experimental"]["tellurAbsorptionSnapshot"], 1);
    assert_eq!(capabilities["experimental"]["tellurExplain"], 1);
    client
        .sender
        .send(Notification::new("initialized".into(), json!({})).into())
        .unwrap();
    client
        .sender
        .send(
            Notification::new(
                "textDocument/didOpen".into(),
                json!({"textDocument":{"uri":"untitled:test","version":1,"text":"broken"}}),
            )
            .into(),
        )
        .unwrap();
    let Message::Notification(n) = client
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
    else {
        panic!("expected diagnostic")
    };
    assert_eq!(n.params["diagnostics"][0]["code"], "E_PARSE");
    client
        .sender
        .send(
            Request::new(
                20.into(),
                "tellur/contractSnapshot".into(),
                json!({"textDocument":{"uri":"untitled:test"}}),
            )
            .into(),
        )
        .unwrap();
    let Message::Response(response) = client
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
    else {
        panic!("expected snapshot response")
    };
    assert_eq!(response.response_result.unwrap_err().code, -32803);
    for (id, method, params, code) in [
        (30, "tellur/explain", json!({}), -32602),
        (
            31,
            "tellur/explain",
            json!({"textDocument":{"uri":"untitled:test"}}),
            -32602,
        ),
        (
            32,
            "tellur/explain",
            json!({"textDocument":{"uri":"untitled:test"},"query":{"kind":"outcome","control":"main","outcome":"done"}}),
            -32803,
        ),
        (
            33,
            "tellur/explainTargets",
            json!({"textDocument":{"uri":"untitled:test"}}),
            -32803,
        ),
    ] {
        client
            .sender
            .send(Request::new(id.into(), method.into(), params).into())
            .unwrap();
        let Message::Response(response) = client
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
        else {
            panic!("expected explanation response")
        };
        assert_eq!(response.response_result.unwrap_err().code, code);
    }
    for (id, options, error) in [
        (10, json!({"tabSize": 4, "insertSpaces": true}), false),
        (11, json!({"tabSize": 0, "insertSpaces": true}), true),
        (12, json!({"tabSize": 4}), true),
        (
            40,
            json!({"tabSize": 4, "insertSpaces": true, "maxLineLength": 60}),
            false,
        ),
        (
            41,
            json!({"tabSize": 4, "insertSpaces": true, "maxLineLength": 39}),
            true,
        ),
        (
            42,
            json!({"tabSize": 4, "insertSpaces": true, "maxLineLength": 241}),
            true,
        ),
        (
            43,
            json!({"tabSize": 4, "insertSpaces": true, "maxLineLength": "60"}),
            true,
        ),
    ] {
        client
            .sender
            .send(
                Request::new(
                    id.into(),
                    "textDocument/formatting".into(),
                    json!({"textDocument":{"uri":"untitled:test"}, "options":options}),
                )
                .into(),
            )
            .unwrap();
        let Message::Response(response) = client
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
        else {
            panic!("expected formatting response")
        };
        if error {
            assert_eq!(response.response_result.unwrap_err().code, -32602);
        } else {
            assert_eq!(response.response_result.unwrap(), json!([]));
        }
    }
    client
        .sender
        .send(Request::new(2.into(), "unsupported".into(), json!({})).into())
        .unwrap();
    let Message::Response(r) = client
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
    else {
        panic!("expected response")
    };
    assert_eq!(r.response_result.unwrap_err().code, -32601);
    client
        .sender
        .send(Request::new(3.into(), "shutdown".into(), json!(null)).into())
        .unwrap();
    assert!(matches!(
        client
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap(),
        Message::Response(_)
    ));
    client
        .sender
        .send(Notification::new("exit".into(), json!(null)).into())
        .unwrap();
    handle.join().unwrap().unwrap();
}

#[test]
fn inferred_contracts_are_visible_in_outline_and_declaration_hover() {
    let mut f = Fixture::new();
    f.open("// 😀\ncontrol answer() exits auto { [entry]: u32 value = 42; exit ready { value }; }");
    assert_eq!(f.diagnostics(), json!([]));
    let symbols = f.server.symbols(&f.uri);
    assert!(
        symbols[0]["detail"]
            .as_str()
            .unwrap()
            .contains("ready { u32 value }")
    );
    let hover = f.server.hover(&f.uri, 1, 9);
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("Inferred")
    );
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("ready { u32 value }")
    );
    assert_eq!(f.server.hover(&f.uri, 0, 3), Value::Null);
    f.change(
        "control answer() exits auto { [entry]: string value = 42; exit ready { value }; }",
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_TYPE");
    assert_eq!(f.server.hover(&f.uri, 0, 9), Value::Null);
}

#[test]
fn ownership_and_reference_diagnostics_follow_unsaved_edits() {
    let mut f = Fixture::new();
    let source = r#"control main() exits auto { [entry]: mutable string s="before"; auto ref=&mutable s; *ref="after"; drop(ref); exit done {s}; }"#;
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    f.change(&source.replace("drop(ref);", "auto moved=s; emit(ref);"), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_LOAN");
    f.change(
        &source.replace("*ref=\"after\";", "auto moved=ref; *ref=\"after\";"),
        3,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_MOVED");
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
    let borrowed =
        "control duplicate(&string text) exits auto { [entry]: exit done {clone(text)}; }";
    f.change(borrowed, 5);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        f.server.hover(&f.uri, 0, 10)["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("ref string text")
    );
}
#[test]
fn returned_borrow_lifetimes_are_visible_and_local_escape_is_diagnosed() {
    let mut f = Fixture::new();
    let source = "control view(&string text) exits auto { [entry]: exit found {text}; }";
    f.open(source);
    let hover = f.server.hover(&f.uri, 0, 9);
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("payload 1 borrows `text`")
    );
    f.change(
        "control view() exits auto { [entry]: string text=\"local\"; exit found {&text}; }",
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_LIFETIME");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn collection_borrow_hover_and_unsaved_diagnostics_use_element_origins() {
    let mut f = Fixture::new();
    let source = "control first(&vector<string> words) exits auto { [entry]: delta vector_get(words,0) {found {auto word}=>yes,out_of_bounds=>no} [yes]: exit found {word}; [no]: exit empty; }";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let hover = f.server.hover(&f.uri, 0, 9);
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("payload 1 borrows `words[*]`")
    );
    let main = "control main() exits done { [entry]: mutable vector<string> words=[\"hello\"]; delta first(&words) {found {auto word}=>yes,empty=>no} [yes]: delta vector_push(&mutable words,\"new\") {pushed=>pushed} [pushed]: emit(word); exit done; [no]: exit done; }";
    f.change(&format!("{source}\n{main}"), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_LOAN");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn inline_adapters_and_capture_obligations_follow_unsaved_edits() {
    let mut f = Fixture::new();
    let source = "control main(bool flag) exits auto {auto saved=capture choose(flag);exit delta saved {ready {auto n}=>done {n},empty=>empty {}};}\ncontrol choose(bool flag) exits auto {exit delta flag {true=>ready {1},false=>empty {}};}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        f.server.hover(&f.uri, 0, 9)["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("done { u32")
    );
    f.change(
        &source.replace(
            "exit delta saved {ready {auto n}=>done {n},empty=>empty {}};",
            "exit done {};",
        ),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_OUTCOME");
    f.change(&source.replace(",empty=>empty {}", ""), 3);
    assert_eq!(f.diagnostics()[0]["code"], "E_ARMS");
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn stored_adapter_contracts_and_ownership_diagnostics_follow_unsaved_edits() {
    let mut f = Fixture::new();
    let source = "control main(bool flag) exits auto {auto result=delta flag {true=>ready {\"hello\"},false=>empty {}};exit result;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        f.server.hover(&f.uri, 0, 9)["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("ready { string")
    );
    f.change(&source.replace("exit result;", "exit discarded;"), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_OUTCOME");
    f.change(&source.replace("empty {}", "ready {3}"), 3);
    assert_eq!(f.diagnostics()[0]["code"], "E_TYPE");
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn named_contracts_have_outline_hover_and_unsaved_diagnostics() {
    let mut f = Fixture::new();
    let source = "outcome message {ready {string text},empty}\ncontrol main() exits message {message saved=ready {\"hello\"};exit saved;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let symbols = f.server.symbols(&f.uri);
    assert_eq!(symbols[0]["name"], "message");
    assert_eq!(symbols[0]["kind"], 10);
    assert_eq!(symbols[1]["name"], "main");
    assert!(
        f.server.hover(&f.uri, 0, 9)["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("outcome message { ready { string text }, empty }")
    );
    assert!(
        f.server.hover(&f.uri, 1, 9)["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("exits message")
    );
    f.change(&source.replace("exit saved;", "exit empty;"), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_OUTCOME");
    f.change(&source.replace("string text", "u32 text"), 3);
    assert_eq!(f.diagnostics()[0]["code"], "E_TYPE");
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn forward_contracts_and_single_completion_diagnostics_follow_edits() {
    let mut f = Fixture::new();
    let source = "control main() exits done {display();forward display();} control display() exits done {exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    f.change(
        &source.replace("display() exits done", "display() exits done,failed"),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_DISPOSITION");
    f.change(
        &source
            .replace("display();forward", "forward")
            .replace("display() exits done", "display() exits done,failed"),
        3,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_OUTCOME");
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
    let borrow = "control relay(&string text) exits auto {forward view(text);} control view(&string text) exits auto {exit found {text};}";
    f.change(borrow, 5);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        f.server.hover(&f.uri, 0, 9)["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("payload 1 borrows `text`")
    );
}

#[test]
fn source_data_declarations_have_outline_hover_and_unsaved_layout_diagnostics() {
    let mut f = Fixture::new();
    let source = "struct id {u32 value;}\nvariant choice {yes {id value},no}\ncontrol main() exits done {choice saved=choice.yes {id {7}};exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let symbols = f.server.symbols(&f.uri);
    assert_eq!(symbols[0]["name"], "id");
    assert_eq!(symbols[0]["kind"], 23);
    assert_eq!(symbols[1]["name"], "choice");
    assert_eq!(symbols[1]["kind"], 10);
    let hover = f.server.hover(&f.uri, 0, 8);
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("struct id { u32 value; }")
    );
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("Copy: true")
    );
    f.change(&source.replace("yes {id value}", "yes {missing value}"), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_MANIFEST");
    assert_eq!(f.diagnostics()[0]["range"]["start"]["line"], 1);
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
    f.change(&source.replace("variant choice", "outcome choice"), 4);
    assert_eq!(f.diagnostics()[0]["code"], "E_OUTCOME");
}

#[test]
fn imported_unsaved_documents_and_disk_edits_refresh_root_diagnostics() {
    let mut f = Fixture::new();
    let dependency = f.root.join("library.path");
    let valid = "pub control go() exits done {exit done;}";
    fs::write(&dependency, valid).unwrap();
    let dependency_uri = url::Url::from_file_path(&dependency).unwrap().to_string();
    f.open(
        "import \"library.path\" as library;\ncontrol main() exits done {forward library.go();}",
    );
    assert_eq!(f.diagnostics(), json!([]));
    f.server.notification("textDocument/didOpen",&json!({"textDocument":{"uri":dependency_uri,"languageId":"path","version":1,"text":"// Unsaved header\n\npub control go() exits done {\n string text=7;\n exit done;\n}"}}));
    let ds = f.diagnostics();
    assert_eq!(ds[0]["code"], "E_TYPE");
    assert_eq!(ds[0]["range"]["start"]["line"], 0);
    assert_eq!(
        ds[0]["relatedInformation"][0]["location"]["range"]["start"]["line"],
        3
    );
    assert_eq!(
        f.server.diagnostics(&dependency_uri)["diagnostics"][0]["range"]["start"]["line"],
        3
    );
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":dependency_uri}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    fs::write(&dependency, "control go() exits done {exit done;}").unwrap();
    f.server.notification(
        "workspace/didChangeWatchedFiles",
        &json!({"changes":[{"uri":dependency_uri,"type":2}]}),
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_VISIBILITY");
    fs::write(&dependency, valid).unwrap();
    assert_eq!(f.diagnostics(), json!([]));
}

#[cfg(unix)]
#[test]
fn conflicting_unsaved_alias_buffers_do_not_silently_choose_a_module_version() {
    let mut f = Fixture::new();
    let path = f.root.join("library.path");
    let link = f.root.join("alias.path");
    fs::write(&path, "pub control go() exits done {exit done;}").unwrap();
    std::os::unix::fs::symlink(&path, &link).unwrap();
    f.open("import \"library.path\" as library;control main() exits done {forward library.go();}");
    for (file, text) in [
        (path, "pub control go() exits done {exit done;}"),
        (
            link,
            "pub control go() exits done {emit(\"different\");exit done;}",
        ),
    ] {
        let uri = url::Url::from_file_path(file).unwrap().to_string();
        f.server.notification(
            "textDocument/didOpen",
            &json!({"textDocument":{"uri":uri,"languageId":"path","version":1,"text":text}}),
        );
    }
    assert_eq!(f.diagnostics()[0]["code"], "E_IMPORT");
    assert!(
        f.diagnostics()[0]["message"]
            .as_str()
            .unwrap()
            .contains("conflicting open buffers")
    );
}

#[test]
fn named_field_diagnostics_follow_unsaved_layout_and_ownership_edits() {
    let mut f = Fixture::new();
    let source = "struct pair {string first;string second;} control main() exits done {string owner=\"Ada\";auto value=pair {second:clone(&owner),first:owner};exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    for (version, old, new, code) in [
        (
            2,
            "second:clone(&owner),first:owner",
            "first:owner,second:clone(&owner)",
            "E_MOVED",
        ),
        (
            3,
            "second:clone(&owner),first:owner",
            "first:owner",
            "E_FIELD",
        ),
        (
            4,
            "second:clone(&owner)",
            "missing:clone(&owner)",
            "E_FIELD",
        ),
    ] {
        f.change(&source.replace(old, new), version);
        assert_eq!(f.diagnostics()[0]["code"], code);
    }
    f.change(source, 5);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn generic_declaration_hover_and_unsaved_argument_errors_work_without_instantiations() {
    let mut f = Fixture::new();
    let source = "struct box<t>{t value;}\noutcome answer<t>{ready {t value},empty}\ncontrol main() exits answer<string> {exit ready {\"Ada\"};}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    for (line, name) in [(0, "box<t>"), (1, "answer<t>")] {
        let hover = f.server.hover(&f.uri, line, 9);
        assert!(hover.to_string().contains(name), "{hover}");
    }
    f.change(&source.replace("answer<string>", "answer<string,u32>"), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_GENERIC");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn generic_control_hover_exposes_checked_parametric_exits_and_unsaved_errors() {
    let mut f = Fixture::new();
    let source = "control id<t>(t value) exits auto {exit ready {value};}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let hover = f.server.hover(&f.uri, 0, 9);
    assert!(hover.to_string().contains("id<t>"), "{hover}");
    assert!(hover.to_string().contains("ready { t"), "{hover}");
    assert!(f.server.symbols(&f.uri).to_string().contains("ready { t"));
    f.change(&source.replace("exit ready {value};", "exit ready;"), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_RESOURCE");
    assert!(
        f.diagnostics()[0]["message"]
            .as_str()
            .unwrap()
            .contains("generic definition check")
    );
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn bounded_control_hover_and_caller_diagnostics_survive_unsaved_repairs() {
    let mut f = Fixture::new();
    let source = "control twice<t: copy>(t value) exits auto {exit ready {value,value};}\ncontrol main() exits auto {forward twice<u32>(1);}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        f.server
            .hover(&f.uri, 0, 10)
            .to_string()
            .contains("t: copy")
    );
    assert!(f.server.symbols(&f.uri).to_string().contains("t: copy"));
    f.change(
        &source.replace("twice<u32>(1)", "twice<string>(\"Ada\")"),
        2,
    );
    let d = f.diagnostics();
    assert_eq!(d[0]["code"], "E_BOUND");
    assert_eq!(d[0]["range"]["start"]["line"], 1);
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn bounded_playground_declarations_have_hover_with_leading_comments_and_generic_data() {
    let mut f = Fixture::new();
    f.open(include_str!("../playground/capabilities.tellur"));
    assert_eq!(f.diagnostics(), json!([]));
    let hover = f.server.hover(&f.uri, 3, 10);
    assert!(hover.to_string().contains("item: clone"), "{hover}");
}

#[test]
fn trait_outline_hover_and_unsaved_conformance_errors_are_checked() {
    let mut f = Fixture::new();
    let source = include_str!("../playground/traits.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let symbols = f.server.symbols(&f.uri);
    assert!(
        symbols
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "reader" && s["children"].as_array().unwrap().len() == 2)
    );
    let h = f.server.hover(&f.uri, 7, 8);
    assert!(h.to_string().contains("read_result<item>"), "{h}");
    f.change(
        &source.replace("type item = string;", "type item = u32;"),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_TYPE");
    f.change(
        &source.replace(
            "control read(ref self input) exits read_result<item> {",
            "control read(self input) exits read_result<item> {",
        ),
        3,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_TRAIT_SIGNATURE");
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn trait_bound_hover_reports_source_names_and_unsaved_missing_requirements() {
    let mut f = Fixture::new();
    fs::write(
        f.root.join("reader.path"),
        "pub trait reader {type item:clone;control read(&self input) exits ready {item value};}",
    )
    .unwrap();
    let source = "import \"reader.path\" as io;\ncontrol read_one<r:io.reader>(&r input) exits auto {forward io.reader<r>.read(input);}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = f.server.hover(&f.uri, 1, 10);
    assert!(h.to_string().contains("r: io.reader"), "{h}");
    assert!(h.to_string().contains("io.reader<r>.item"), "{h}");
    assert!(!h.to_string().contains("_path_"), "{h}");
    f.change(&source.replace("r:io.reader", "r"), 2);
    let d = f.diagnostics();
    assert_eq!(d[0]["code"], "E_BOUND");
    assert!(!d.to_string().contains("_path_poly_"));
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn declared_origins_appear_in_trait_and_generic_hover_and_diagnose_unsaved_errors() {
    let mut f = Fixture::new();
    let source = include_str!("../playground/borrowed_traits.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    for (line, column) in [(6, 12), (16, 10)] {
        let h = f.server.hover(&f.uri, line, column);
        assert!(h.to_string().contains("from input"), "{h}");
        assert!(h.to_string().contains("ref "), "{h}");
    }
    f.change(&source.replace("from input", "from missing"), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_LIFETIME");
    f.change(
        &source.replace(
            "exit found { borrow input.text };",
            "string local=\"bad\"; exit found { &local };",
        ),
        3,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_LIFETIME");
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn grouped_struct_origins_have_hover_outline_and_unsaved_diagnostics() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/origin_groups.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = f.server.hover(&f.uri, 0, 10);
    assert!(h.to_string().contains("text_pair['a, 'b]"), "{h}");
    assert!(h.to_string().contains("<- 'a"), "{h}");
    let h = f.server.hover(&f.uri, 5, 10);
    assert!(h.to_string().contains("text_pair[a, b]"), "{h}");
    assert!(h.to_string().contains("payload 1 ['a]"), "{h}");
    assert!(f.server.symbols(&f.uri).to_string().contains("<- 'a"));
    f.change(
        &source.replace("text_pair[a, b] value", "text_pair[b, a] value"),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_LIFETIME");
    f.change(&source.replace("left <- 'a", "left <- 'unknown"), 3);
    assert_eq!(f.diagnostics()[0]["code"], "E_ORIGIN");
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn generic_grouped_result_hover_retains_explicit_origin_arguments() {
    let mut f = Fixture::new();
    f.open("struct view<t>['a] {ref t value <- 'a;}\ncontrol view_of<t>(ref t input) exits ready {view<t>[input] value} {exit ready {view<t> {input}};}");
    assert_eq!(f.diagnostics(), json!([]));
    let h = f.server.hover(&f.uri, 1, 10);
    assert!(h.to_string().contains("view<t>[input]"), "{h}");
    assert!(
        f.server
            .symbols(&f.uri)
            .to_string()
            .contains("view<t>[input]")
    );
}

#[test]
fn reference_auto_is_inferred_in_unsaved_declarations_and_patterns() {
    let mut f = Fixture::new();
    let source = "control main() exits done {mutable string first=\"before\";ref mutable auto first_b=borrow mutable first;deref first_b=\"after\";emit(first);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    f.change(&source.replace("ref mutable auto", "ref auto"), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_TYPE");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn record_input_selectors_hover_and_unsaved_contract_errors() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/record_inputs.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let line = source
        .lines()
        .position(|l| l.starts_with("control heading_of"))
        .unwrap();
    let hover = f.server.hover(&f.uri, line as u64, 10).to_string();
    assert!(hover.contains("input['heading]"), "{hover}");
    f.change(
        &source.replace("from input['heading]", "from input['content]"),
        2,
    );
    assert!(f.diagnostics().to_string().contains("E_LIFETIME"));
    f.change(
        &source.replace("from input['heading]", "from input['caption]"),
        3,
    );
    assert!(
        f.diagnostics()
            .to_string()
            .contains("unknown public origin group")
    );
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn exclusive_carrier_hover_reports_authorization_and_unsaved_conflicts() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/carrier_reborrows.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let line = source
        .lines()
        .position(|l| l.starts_with("control reborrow"))
        .unwrap();
    let hover = f.server.hover(&f.uri, line as u64, 10).to_string();
    assert!(
        hover.contains("input['first]") && hover.contains("authorizing carrier borrow"),
        "{hover}"
    );
    f.change(
        &source.replace("ref mutable text_editors input", "ref text_editors input"),
        2,
    );
    assert!(
        f.diagnostics()
            .to_string()
            .contains("exclusive field cannot borrow a shared input origin")
    );
    f.change(
        &source.replace(
            "emit(children.first);",
            "drop(parents);emit(children.first);",
        ),
        3,
    );
    assert!(f.diagnostics().to_string().contains("E_LOAN"));
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn opaque_split_hover_and_unsaved_alias_errors() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/opaque_splits.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let line = source
        .lines()
        .position(|l| l.starts_with("control edit"))
        .unwrap();
    let hover = f.server.hover(&f.uri, line as u64, 10).to_string();
    assert!(
        hover.contains("disjoint access") && hover.contains("from input"),
        "{hover}"
    );
    f.change(
        &source.replace("borrow mutable input.body", "borrow mutable input.title"),
        2,
    );
    assert!(f.diagnostics().to_string().contains("E_ALIAS"));
    f.change(
        &source.replace("emit(title);", "drop(draft);emit(title);"),
        3,
    );
    assert!(f.diagnostics().to_string().contains("E_LOAN"));
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}
#[test]
fn borrowed_outcomes_report_unsaved_owner_and_handling_errors() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/borrowed_outcomes.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let line = source
        .lines()
        .position(|l| l.starts_with("outcome first_word"))
        .unwrap();
    let hover = f.server.hover(&f.uri, line as u64, 10).to_string();
    assert!(hover.contains("ref string"), "{hover}");
    f.change(
        &source.replace("first_word message =", "drop(words);first_word message ="),
        2,
    );
    assert!(f.diagnostics().to_string().contains("E_LOAN"));
    f.change("control peek(ref string input) exits found {ref string text from input} {exit found {input};} control main() exits done {string word=\"x\";auto saved=capture peek(borrow word);exit done;}",3);
    assert!(f.diagnostics().to_string().contains("E_OUTCOME"));
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}

fn auto_hover(f: &Fixture, source: &str, marker: &str, expected: &str) {
    let byte = source.find(marker).unwrap();
    let prefix = &source[..byte];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() as u64;
    let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u64;
    let hover = f.server.hover(&f.uri, line, column + 1);
    let value = hover["contents"]["value"].as_str().unwrap_or("");
    assert!(
        value.contains(expected),
        "{marker}: expected {expected:?}, got {hover}"
    );
    assert!(
        !value.contains("$adapter_") && !value.contains("_path_m_") && !value.contains("$abstract"),
        "{hover}"
    );
    assert_eq!(
        hover["range"],
        json!({"start":{"line":line,"character":column},"end":{"line":line,"character":column+4}})
    );
}

#[test]
fn auto_hover_uses_checked_types_and_updates_after_unsaved_edits() {
    let mut f = Fixture::new();
    let source = "control main() exits done { emit(\"😀 auto fake\"); auto value = 42; exit done; } // auto fake";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto value", "u32 value");
    for (i, (value, ty)) in [("true", "bool value"), ("\"hello\"", "string value")]
        .iter()
        .enumerate()
    {
        let edited = source.replace("42", value);
        f.change(&edited, i as i64 + 2);
        assert_eq!(f.diagnostics(), json!([]));
        auto_hover(&f, &edited, "auto value", ty);
    }
    let invalid = source.replace("42", "unknown");
    f.change(&invalid, 4);
    assert!(!f.diagnostics().as_array().unwrap().is_empty());
    let col = invalid[..invalid.find("auto value").unwrap()]
        .encode_utf16()
        .count();
    assert_eq!(f.server.hover(&f.uri, 0, col as u64), Value::Null);
    f.change(source, 5);
    auto_hover(&f, source, "auto value", "u32 value");
    for byte in source.match_indices("auto fake").map(|(i, _)| i) {
        let col = source[..byte].encode_utf16().count() as u64;
        let hover = f.server.hover(&f.uri, 0, col);
        // A string can describe its containing statement's selection, never a fake inferred binding.
        assert!(!hover.to_string().contains("Inferred binding"));
        if byte > source.find("//").unwrap() {
            assert_eq!(hover, Value::Null);
        }
    }
}

#[test]
fn auto_hover_preserves_reference_modes_even_when_aliases_are_erased() {
    let mut f = Fixture::new();
    let source = r#"control main() exits done {
        mutable string owner = "hello";
        ref mutable auto edit = borrow mutable owner;
        deref edit = "changed";
        ref // qualifier comment
        auto view = borrow owner;
        auto copied = view;
        emit(copied);
        exit done;
    }"#;
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto edit", "ref mutable string edit");
    auto_hover(&f, source, "auto view", "ref string view");
    auto_hover(&f, source, "auto copied", "ref string copied");
}

#[test]
fn auto_hover_covers_collections_and_separate_lexical_bindings() {
    let mut f = Fixture::new();
    let source = r#"control main(vector<string> input_words, array<u32, 2> input_numbers, map<string, u32> input_map) exits done {
        auto words = input_words;
        auto numbers = input_numbers;
        auto lookup = input_map;
        auto count = vector_length(borrow words);
        exit delta vector_get(borrow words, 0) {
            found { ref auto item } => delta array_get(borrow numbers, 0) {
                found { ref auto number } => done {}, out_of_bounds => done {}
            },
            out_of_bounds => done {}
        };
    }
    control other() exits done { auto words = 7; exit done; }"#;
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    for (marker, expected) in [
        ("auto words = input", "vector<string> words"),
        ("auto numbers", "array<u32, 2> numbers"),
        ("auto lookup", "map<string, u32> lookup"),
        ("auto count", "u32 count"),
        ("auto item", "ref string item"),
        ("auto number }", "ref u32 number"),
        ("auto words = 7", "u32 words"),
    ] {
        auto_hover(&f, source, marker, expected);
    }
}

#[test]
fn auto_hover_covers_captures_inline_adapters_and_regular_payload_bindings() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/borrowed_outcomes.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto pending", "outcome<first> pending");
    auto_hover(&f, source, "auto text", "ref string text");
    // Exercise inference independently of the runnable example's process contract.
    let inferred = include_str!("../playground/stored_adapters.tellur")
        .replace("exits process_result", "exits auto");
    let source = inferred.as_str();
    f.change(source, 2);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto saved", "outcome { ready { string");
    auto_hover(&f, source, "auto {", "exits done { string");
    let source = "control text() exits text {string value} {exit text {\"hi\"};} control main() exits done { delta text() {text {auto value} => converted} [converted]: emit(value);exit done;}";
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto value", "string value");
}

#[test]
fn auto_hover_in_generic_definitions_is_independent_of_concrete_calls() {
    let mut f = Fixture::new();
    let source = include_str!("../playground/generic_controls.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto received", "item received");
    auto_hover(&f, source, "auto boxed", "box<string> boxed");
    auto_hover(&f, source, "auto text", "ref string text");
    let source =
        "control identity<t>(t input) exits ready {t value} {auto local=input; exit ready{local};}";
    f.change(source, 2);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto local", "t local");
    let source = format!(
        "{source} control wrapper<u>(u input) exits ready {{u value}} {{forward identity<u>(input);}} control main() exits auto {{forward identity<string>(\"hello\");}}"
    );
    f.change(&source, 3);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, &source, "auto local", "t local");
    let source = "control wrap<t>(bool flag, t input) exits ready {t value} {auto saved = delta flag {true=>ready{input},false=>ready{input}};exit saved;}";
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto saved", "outcome { ready { t");
}

#[test]
fn auto_hover_uses_import_aliases_and_unsaved_dependency_types() {
    let mut f = Fixture::new();
    let library = "pub struct record {pub string text;} pub control make() exits ready {record value} {exit ready {record{\"hello\"}};}";
    let path = f.root.join("library.path");
    fs::write(&path, library).unwrap();
    let source = "import \"library.path\" as lib; control main() exits done {delta lib.make() {ready{auto value}=>available} [available]:drop(value);exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto value", "lib.record value");
    let uri = url::Url::from_file_path(path).unwrap().to_string();
    let library = "pub control make() exits ready {u32 value} {auto result=9;exit ready {result};}";
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"languageId":"path","version":1,"text":library}}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto value", "u32 value");
}

#[test]
fn auto_hover_distinguishes_sibling_arm_bindings_and_trait_method_locals() {
    let mut f = Fixture::new();
    let source = r#"control choose(bool flag) exits number {u32 value}, text {string value} {
        exit delta flag { true => number {1}, false => text {"hi"} };
    }
    control main() exits done {
        exit delta choose(true) {
            number {auto item} => [numeric]: done {},
            text {auto item} => [textual]: done {}
        };
    }"#;
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto item} => [numeric]", "u32 item");
    auto_hover(&f, source, "auto item} => [textual]", "string item");
    let source = include_str!("../playground/borrowed_traits.tellur").replace(
        "exit found { borrow input.text };",
        "auto local = borrow input.text; exit found { local };",
    );
    f.change(&source, 2);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, &source, "auto local", "ref string local");
}

#[test]
fn auto_hover_on_mutable_reference_payloads_keeps_exclusivity() {
    let mut f = Fixture::new();
    let source = r#"control lend(ref mutable string owner)
        exits ready {ref mutable string value from owner} { exit ready {owner}; }
    control main() exits done {
        mutable string owner = "before";
        delta lend(borrow mutable owner) {ready {ref mutable auto edit} => ready}
        [ready]: deref edit = "after"; emit(owner); exit done;
    }"#;
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto edit", "ref mutable string edit");
}

#[test]
fn nested_borrowed_storage_has_inferred_hovers_and_unsaved_loan_diagnostics() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/nested_borrows.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto message", "envelope message");
    auto_hover(&f, source, "auto values", "editors values");
    f.change(
        &source.replace("delta pending", "title=\"too soon\";delta pending"),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_LOAN");
    f.change(
        &source.replace("title = \"published title\"", "body = \"too soon\""),
        3,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_LOAN");
    f.change(
        &source.replace("delta pending", "drop(pending);delta pending"),
        4,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_RESOURCE");
    f.change(source, 5);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto values", "editors values");
}

#[test]
fn nested_anonymous_outcome_hover_keeps_symbolic_payloads_readable() {
    let mut f = Fixture::new();
    let source = "control wrap<t>(bool flag, ref t input) exits done {auto first=delta flag {true=>found{input},false=>empty{}};auto second=delta true {true=>wrapped{first},false=>wrapped{first}};exit delta second {wrapped{auto inner}=>delta inner{found{auto value}=>done{},empty=>done{}}};}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(
        &f,
        source,
        "auto second",
        "outcome { wrapped { outcome { found { ref t",
    );
    auto_hover(&f, source, "auto inner", "outcome { found { ref t");
}

#[test]
fn grouped_variant_hover_and_unsaved_owner_conflicts_follow_public_boundaries() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/public_variants.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let line = source
        .lines()
        .position(|l| l.starts_with("variant lookup"))
        .unwrap();
    let hover = f.server.hover(&f.uri, line as u64, 9).to_string();
    assert!(hover.contains("lookup['title, 'body]"), "{hover}");
    assert!(hover.contains("text_views['title, 'body]"), "{hover}");
    let line = source
        .lines()
        .position(|l| l.starts_with("control lookup_text"))
        .unwrap();
    let hover = f.server.hover(&f.uri, line as u64, 10).to_string();
    assert!(hover.contains("guarded by their alternatives"), "{hover}");
    let line = source
        .lines()
        .position(|l| l.contains("auto second"))
        .unwrap();
    let column = source.lines().nth(line).unwrap().find("auto").unwrap();
    assert!(
        f.server
            .hover(&f.uri, line as u64, column as u64 + 1)
            .to_string()
            .contains("envelope second")
    );
    f.change(
        &source.replace(
            "emit(fields.title);",
            "title=\"too early\";emit(fields.title);",
        ),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_LOAN");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn named_origin_schema_and_completion_binding_hovers_preserve_schematic_spelling() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/outcome_origins.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let line = source
        .lines()
        .position(|l| l.starts_with("outcome text_result"))
        .unwrap();
    let hover = f.server.hover(&f.uri, line as u64, 10).to_string();
    assert!(hover.contains("text_result['title, 'body]"), "{hover}");
    assert!(hover.contains("text_views['title, 'body]"), "{hover}");
    let line = source
        .lines()
        .position(|l| l.starts_with("control lookup_text"))
        .unwrap();
    let hover = f.server.hover(&f.uri, line as u64, 10).to_string();
    assert!(hover.contains("exits text_result[title, body]"), "{hover}");
    auto_hover(&f, source, "auto pending", "option<text_result> pending");
    f.change(
        &source.replace(
            "emit(fields.title);",
            "title=\"too early\";emit(fields.title);",
        ),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_LOAN");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn borrowed_sum_payload_hover_reports_reborrowed_mode_and_unsaved_errors() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/sum_carriers.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto preview", "ref string preview");
    auto_hover(&f, source, "auto editable", "ref mutable string editable");
    let shared = source.find("auto text").unwrap();
    let line = source[..shared].bytes().filter(|b| *b == b'\n').count();
    let column = source[..shared].rsplit('\n').next().unwrap().len();
    let hover = f
        .server
        .hover(&f.uri, line as u64, column as u64 + 1)
        .to_string();
    assert!(hover.contains("ref string text"), "{hover}");
    f.change(
        &source.replace(
            "deref editable =",
            "owner = \"too early\"; deref editable =",
        ),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_LOAN");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn carrier_result_hover_reports_storage_contents_and_inferred_reference_types() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/carrier_results.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto view", "ref mutable text_view view");
    auto_hover(
        &f,
        source,
        "auto returned",
        "ref mutable option<ref mutable string> returned",
    );
    let line = source
        .lines()
        .position(|l| l.starts_with("control lend("))
        .unwrap();
    let hover = f.server.hover(&f.uri, line as u64, 9).to_string();
    assert!(
        hover.contains("borrows `input`.") && hover.contains("input['source]"),
        "{hover}"
    );
    f.change(
        &source.replace("deref view.text =", "drop(saved); deref view.text ="),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_LOAN");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn stored_carrier_hovers_and_unsaved_release_errors_survive_repair() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/stored_carriers.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto view", "ref mutable text_view view");
    auto_hover(&f, source, "auto job", "ref mutable edit_job job");
    auto_hover(&f, source, "auto transferred", "outcome<lend> transferred");
    f.change(
        &source.replace("delta transferred", "drop(saved); delta transferred"),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_LOAN");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn public_carrier_hovers_show_storage_and_nested_group_maps() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/public_carriers.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto pending", "outcome<prepare> pending");
    auto_hover(&f, source, "auto job", "edit_job job");
    let line = source
        .lines()
        .position(|l| l.starts_with("struct edit_job"))
        .unwrap();
    let hover = f.server.hover(&f.uri, line as u64, 8).to_string();
    assert!(
        hover.contains("ref mutable text_view['text] target <- 'storage"),
        "{hover}"
    );
    let line = source
        .lines()
        .position(|l| l.starts_with("control relay"))
        .unwrap();
    let hover = f.server.hover(&f.uri, line as u64, 9).to_string();
    assert!(
        hover.contains("input['storage]") && hover.contains("input['text]"),
        "{hover}"
    );
    f.change(
        &source.replace(
            "deref job.target.text =",
            "drop(saved);deref job.target.text =",
        ),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_LOAN");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn aggregate_trait_hovers_and_unsaved_contract_diagnostics() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/aggregate_traits.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto job", "edit_job<string> job");
    auto_hover(&f, source, "auto pending", "preparation<string> pending");
    f.change(
        &source.replace(
            "preparation<item>[owner, owner['text]]",
            "preparation<item>[owner['text], owner]",
        ),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_ORIGIN");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn borrowed_collection_hovers_and_unsaved_owner_conflicts() {
    let mut f = Fixture::new();
    let source = include_str!("fixtures/borrowed_collections.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto word", "ref string word");
    f.change(
        &source.replace("emit(word)", "first = \"early\"; emit(word)"),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_LOAN");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn public_contract_diagnostics_and_forwarding_locations_follow_unsaved_edits() {
    let mut f = Fixture::new();
    fs::write(
        f.root.join("manifest.json"),
        include_str!("../playground/manifest.json"),
    )
    .unwrap();
    let source = include_str!("fixtures/public_contracts.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    f.change(
        &source.replace("exits ready { u32 value }, invalid", "exits auto"),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_PUBLIC_CONTRACT");
    f.change(&source.replacen("exit invalid;", "exit unexpected;", 1), 3);
    let d = f.diagnostics();
    assert_eq!(d[0]["code"], "E_OUTCOME");
    let notes = d[0]["relatedInformation"].as_array().unwrap();
    assert_eq!(notes.len(), 2, "{d}");
    assert_eq!(notes[0]["location"]["range"]["start"]["line"], 4);
    assert_eq!(notes[1]["location"]["range"]["start"]["line"], 8);
    assert!(notes.iter().all(|n| n["location"]["uri"] == f.uri));
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn rust_adapter_project_checks_unsaved_sources_without_loading_host_code() {
    let mut f = Fixture::new();
    fs::write(
        f.root.join("manifest.json"),
        include_str!("../applications/settings/manifest.json"),
    )
    .unwrap();
    fs::write(
        f.root.join("settings.tellur"),
        include_str!("../applications/settings/settings.tellur")
            .replace("../../library/json_file.tellur", "json_file.tellur"),
    )
    .unwrap();
    fs::write(
        f.root.join("json_file.tellur"),
        include_str!("../library/json_file.tellur"),
    )
    .unwrap();
    fs::write(
        f.root.join("server.tellur"),
        include_str!("../applications/settings/server.tellur")
            .replace("../../library/loopback_http.tellur", "loopback_http.tellur"),
    )
    .unwrap();
    fs::write(
        f.root.join("http.tellur"),
        include_str!("../applications/settings/http.tellur"),
    )
    .unwrap();
    fs::write(
        f.root.join("loopback_http.tellur"),
        include_str!("../library/loopback_http.tellur"),
    )
    .unwrap();
    // No host.rs or Cargo.lock exists: checking Tellur must not build or run Rust.
    let source = include_str!("../applications/settings/main.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    f.change(
        &source.replace("found { string value }", "found { u32 value }"),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_TYPE");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
    let uri = url::Url::from_file_path(f.root.join("http.tellur"))
        .unwrap()
        .to_string();
    let original = include_str!("../applications/settings/http.tellur");
    let invalid = original.replacen("400,", "\"bad status\",", 1);
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"languageId":"tellur","version":1,"text":invalid}}),
    );
    let errors = f.server.diagnostics(&uri);
    let error = &errors["diagnostics"][0];
    assert_eq!(error["code"], "E_TYPE");
    let (line, text) = invalid
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("bad status"))
        .unwrap();
    let start = text.find('"').unwrap();
    assert_eq!(
        error["range"],
        json!({"start":{"line":line,"character":start},"end":{"line":line,"character":start+12}})
    );
    f.server.notification(
        "textDocument/didChange",
        &json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":original}]}),
    );
    assert_eq!(f.diagnostics(), json!([]));
    let independent = original
        .replace(
            "    delta map_get(headers, borrow \"content-type\")",
            "    u32 invalid_media = true;\n    delta map_get(headers, borrow \"content-type\")",
        )
        .replace(
            "control encode_value(string value) exits auto {",
            "control encode_value(string value) exits auto {\n    string invalid_value = 7;",
        );
    f.server.notification(
        "textDocument/didChange",
        &json!({"textDocument":{"uri":uri,"version":3},"contentChanges":[{"text":independent}]}),
    );
    let errors = f.diagnostics();
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    assert!(
        errors
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["code"] == "E_TYPE")
    );
    f.server.notification(
        "textDocument/didChange",
        &json!({"textDocument":{"uri":uri,"version":4},"contentChanges":[{"text":original}]}),
    );
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn stored_outcome_contributor_locations_follow_unsaved_adapters() {
    let mut f = Fixture::new();
    fs::write(
        f.root.join("manifest.json"),
        include_str!("../playground/manifest.json"),
    )
    .unwrap();
    let source = "control leaf() exits auto {exit missing;}\ncontrol main() exits done {\n auto saved=capture leaf();\n auto mapped=delta saved {missing=>unavailable {}};\n exit delta mapped {unavailable=>done {}};\n}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    f.change(&source.replace("{unavailable=>done {}}", "{}"), 2);
    let diagnostics = f.diagnostics();
    assert_eq!(diagnostics[0]["code"], "E_ARMS");
    let notes = diagnostics[0]["relatedInformation"].as_array().unwrap();
    for line in [0, 2, 3] {
        assert!(
            notes
                .iter()
                .any(|n| n["location"]["range"]["start"]["line"] == line),
            "{diagnostics}"
        );
    }
    assert!(notes.iter().all(|n| n["location"]["uri"] == f.uri));
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn structural_witness_data_is_current_and_distinct_from_message_text() {
    let mut f = Fixture::new();
    fs::write(
        f.root.join("manifest.json"),
        include_str!("../playground/manifest.json"),
    )
    .unwrap();
    let source =
        "control main(bool flag) exits done {delta flag {true=>yes,false=>no}[yes]:exit done;}";
    f.open(source);
    let diagnostics = f.diagnostics();
    assert_eq!(diagnostics[0]["code"], "E_UNCOVERED");
    assert_eq!(diagnostics[0]["data"]["schema"], "tellur.diagnostic.v1");
    assert_eq!(
        diagnostics[0]["data"]["witness"]["kind"],
        "structural_may_reach"
    );
    assert_eq!(
        diagnostics[0]["data"]["witness"]["selected_paths"],
        json!(["no"])
    );
    assert!(
        diagnostics[0]["data"]["witness"]["control"]
            .as_str()
            .unwrap()
            .ends_with("main")
    );
    f.change(&source.replace("[yes]:", "[yes | no]:"), 2);
    assert_eq!(f.diagnostics(), json!([]));
    f.change("control main() exits done {u32 n=true;exit done;}", 3);
    let diagnostics = f.diagnostics();
    assert_eq!(diagnostics[0]["code"], "E_TYPE");
    assert!(diagnostics[0].get("data").is_none());
}

#[test]
fn grouped_paths_keep_inferred_hover_and_report_unsaved_selection_errors() {
    let mut f = Fixture::new();
    let source = include_str!("../playground/path_groups.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto heading", "string heading");
    f.change(
        &source.replace(
            "[entry]:\n            emit(heading);",
            "[absent]:\n            emit(heading);",
        ),
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_EMPTY");
    assert!(f.server.hover(&f.uri, 4, 8).is_null());
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto heading", "string heading");
}

#[test]
fn direct_arm_exits_hover_payloads_and_report_unsaved_exit_contract_errors() {
    let mut f = Fixture::new();
    fs::write(
        f.root.join("manifest.json"),
        include_str!("../playground/manifest.json"),
    )
    .unwrap();
    let source = "control main(option<string> input) exits ready {string value},missing {\n delta input {some {auto value}=>exit ready {value},none=>exit missing}\n}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto value", "string value");
    f.change(&source.replace("ready {value}", "ready {false}"), 2);
    let diagnostic = &f.diagnostics()[0];
    assert_eq!(diagnostic["code"], "E_TYPE");
    assert_eq!(diagnostic["range"]["start"]["line"], 1);
    assert_eq!(
        diagnostic["range"]["end"]["character"].as_u64().unwrap()
            - diagnostic["range"]["start"]["character"].as_u64().unwrap(),
        5
    );
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto value", "string value");
}

fn history_hover(f: &Fixture, source: &str, marker: &str) -> Value {
    let byte = source.find(marker).unwrap();
    let line = source[..byte].bytes().filter(|b| *b == b'\n').count();
    let start = source[..byte].rfind('\n').map_or(0, |i| i + 1);
    let column = source[start..byte].encode_utf16().count();
    f.server.hover(&f.uri, line as u64, column as u64)
}
fn history_text(hover: &Value) -> &str {
    hover["contents"]["value"]
        .as_str()
        .unwrap_or_else(|| panic!("missing hover: {hover}"))
}

#[test]
fn identity_routes_grouped_bindings_and_qualified_paths_follow_editor_changes() {
    let mut f = Fixture::new();
    fs::write(
        f.root.join("manifest.json"),
        include_str!("../playground/manifest.json"),
    )
    .unwrap();
    let source = r#"control choose(bool flag) exits ready {u32 number}, absent {
    exit delta flag {true=>ready {7},false=>absent {}};
}
control main() exits done {
    first = delta choose(true) {
        ready {auto number} => { auto text = u32_text(number); }
    }
    [first.ready]: emit(text); exit done;
    [first.absent]: exit done;
}"#;
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    auto_hover(&f, source, "auto number", "u32 number");
    auto_hover(&f, source, "auto text", "string text");
    assert!(history_text(&history_hover(&f, source, "[first.ready]")).contains("first.ready"));
    f.change(&source.replace("[first.absent]: exit done;", ""), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_UNCOVERED");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn selected_history_hovers_resolve_aliases_groups_and_entry_restoration() {
    let mut f = Fixture::new();
    let source = r#"control main(bool a,bool b) exits done {
 delta a {true=>yes,false=>no}
 delta b {true=>short,false=>long}
 path selected=yes & (short | long);
 [selected]: {
   [short]:emit("short work");
   [long]: {emit("long work");}
   emit("group continuation");
 }
 emit("all work");
 exit done;
}"#;
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    for marker in ["path selected", "[selected]", "group continuation"] {
        let h = history_hover(&f, source, marker);
        let text = history_text(&h);
        assert!(text.contains("yes & (short | long)"), "{text}");
        assert!(text.contains("not a proven feasible input"));
        assert!(text.contains("line 2, column 11"), "{text}");
    }
    let h = history_hover(&f, source, "short work");
    assert!(
        history_text(&h).contains("yes & (short | long) & short"),
        "{h}"
    );
    let h = history_hover(&f, source, "all work");
    assert!(history_text(&h).contains("\nentry\n"), "{h}");
    assert!(!history_text(&h).contains("Decision arms:"));
    assert_eq!(h["range"]["start"]["line"], 9);
    assert_eq!(h["range"]["end"]["character"], 18);
    let alias = history_hover(&f, source, "path selected");
    assert!(history_text(&alias).contains("declaring the alias executes no work"));
}

#[test]
fn flat_selection_extents_and_empty_groups_have_checked_hovers() {
    let mut f = Fixture::new();
    let source = "control main(bool flag) exits done {delta flag {true=>yes,false=>no}[yes]:emit(\"one\");emit(\"two\");path impossible=yes & no;[impossible]: {} [entry]:exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    for marker in ["[yes]", "\"one\"", "\"two\""] {
        assert!(history_text(&history_hover(&f, source, marker)).contains("\nyes\n"));
    }
    for marker in ["path impossible", "[impossible]"] {
        assert!(history_text(&history_hover(&f, source, marker)).contains("structurally empty"));
    }
    assert!(history_text(&history_hover(&f, source, "[entry]")).contains("\nentry\n"));
}

#[test]
fn nested_adapter_and_direct_exit_hovers_identify_local_outcomes() {
    let mut f = Fixture::new();
    let source = "control main(bool a,bool b) exits done {\n delta a {true=>go,false=>exit done}\n [go]:exit delta b {true=>delta a {true=>done {},false=>done {}},false=>done {}};\n}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let direct = history_hover(&f, source, "exit done");
    assert!(history_text(&direct).contains("\nfalse\n"), "{direct}");
    assert!(history_text(&direct).contains("local outcome fact"));
    let nested = history_hover(&f, source, "done {}");
    let text = history_text(&nested);
    assert!(text.contains("go & true@3:"), "{text}");
    assert_eq!(text.matches("true@3:").count(), 2, "{text}");
    assert!(text.contains("line 3, column"));
    let outer = history_hover(&f, source, "[go]");
    assert!(history_text(&outer).contains("\ngo\n"), "{outer}");
}

#[test]
fn loop_hovers_describe_live_entry_and_one_iteration() {
    let mut f = Fixture::new();
    let source = "control main(bool flag) exits done {\n delta flag {true=>exit done,false=>continue}\n loop retry(bool state=true) {\n   delta state {true=>again_path,false=>exit done}\n   [again_path]:again retry(false);\n }\n}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = history_hover(&f, source, "loop retry");
    assert!(
        history_text(&h).contains("Only histories still live"),
        "{h}"
    );
    let h = history_hover(&f, source, "again retry");
    let text = history_text(&h);
    assert!(
        text.contains("\nagain_path\n") && text.contains("one iteration of `retry`"),
        "{text}"
    );
    assert!(text.contains("not the whole control entry"));
}

#[test]
fn history_hovers_follow_utf16_comments_ranges_and_unsaved_repairs() {
    let mut f = Fixture::new();
    let source = "control main(bool flag) exits done {\r\n emit(\"😀\"); delta flag {true=>yes,false=>no}\r\n [yes]: // not executable\r\n emit(\"first 😀\r\nsecond\");\r\n [entry]:exit done;\r\n}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = history_hover(&f, source, "delta flag");
    assert_eq!(h["range"]["start"]["character"], 13);
    assert!(history_hover(&f, source, "not executable").is_null());
    let h = history_hover(&f, source, "second");
    assert!(history_text(&h).contains("\nyes\n"), "{h}");
    assert_eq!(h["range"]["start"]["line"], 3);
    assert_eq!(h["range"]["end"]["line"], 4);
    let changed = source.replace("[yes]:", "[no]:");
    f.change(&changed, 2);
    assert!(history_text(&history_hover(&f, &changed, "second")).contains("\nno\n"));
    let invalid = changed.replace("emit(\"first", "emit(unknown + \"first");
    f.change(&invalid, 3);
    assert!(!f.diagnostics().as_array().unwrap().is_empty());
    assert!(history_hover(&f, &invalid, "second").is_null());
    f.change(source, 4);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(history_text(&history_hover(&f, source, "second")).contains("\nyes\n"));
}

#[test]
fn generic_and_trait_bodies_show_universal_selections_without_instantiations() {
    let mut f = Fixture::new();
    let source = "control choose<t>(t input,bool flag) exits ready {t value} {delta flag {true=>left,false=>right} [left|right]:exit ready {input};}\ntrait worker {control run(&self input,bool flag) exits done;}\nstruct record {u32 value;}\nimplement worker for record {control run(&self input,bool flag) exits done {delta flag {true=>go,false=>exit done} [go]:exit done;}}\ncontrol main() exits done {exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = history_hover(&f, source, "exit ready {input}");
    assert!(history_text(&h).contains("left | right"), "{h}");
    assert!(!history_text(&h).contains("_path_poly"));
    let h = history_hover(&f, source, "[go]");
    assert!(history_text(&h).contains("\ngo\n"), "{h}");
}

#[test]
fn history_display_is_bounded_without_suggesting_unconditional_execution() {
    let mut f = Fixture::new();
    let mut source = "control main(bool flag) exits done {\n".to_string();
    for i in 0..60 {
        source.push_str(&format!("delta flag {{true=>yes_{i},false=>no_{i}}}\n"));
    }
    let mask = (0..60)
        .map(|i| format!("yes_{i}"))
        .collect::<Vec<_>>()
        .join(" | ");
    source.push_str(&format!(
        "[{mask}]:emit(\"selected\");\n[entry]:exit done;}}"
    ));
    f.open(&source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = history_hover(&f, &source, "selected");
    let text = history_text(&h);
    assert!(text.contains("Display limited"), "{text}");
    assert!(text.contains("…"));
    assert!(text.len() < 6000, "hover must stay bounded: {}", text.len());
    assert!(!text.contains("\nentry\n"));
}

#[test]
fn imported_history_positions_and_unsaved_dependency_errors_are_current() {
    let mut f = Fixture::new();
    let library =
        "pub control choose(bool flag) exits yes,no {delta flag {true=>exit yes,false=>exit no}}";
    fs::write(f.root.join("library.tellur"), library).unwrap();
    let source = "import \"library.tellur\" as lib;\ncontrol main(bool flag) exits done {\n delta lib.choose(flag) {yes=>selected,no=>exit done}\n [selected]:exit done;\n}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let h = history_hover(&f, source, "[selected]");
    assert!(history_text(&h).contains("line 3, column 26"), "{h}");
    let imported = url::Url::from_file_path(f.root.join("library.tellur"))
        .unwrap()
        .to_string();
    f.server.notification("textDocument/didOpen",&json!({"textDocument":{"uri":imported,"languageId":"tellur","version":1,"text":library.replace("exit yes","exit unknown")}}));
    assert!(history_hover(&f, source, "[selected]").is_null());
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":imported}}),
    );
    assert!(!history_hover(&f, source, "[selected]").is_null());
}

fn explain_at(f: &Fixture, source: &str, marker: &str) -> Result<Value, String> {
    let before = &source[..source.find(marker).unwrap()];
    f.server.explain(&f.uri,&json!({"kind":"selection","position":{"line":before.bytes().filter(|&c|c==b'\n').count(),"character":before.rsplit('\n').next().unwrap().encode_utf16().count()}}))
}
#[test]
fn explanation_queries_use_current_utf16_positions_and_refuse_stale_sources() {
    let mut f = Fixture::new();
    let source = "control main(bool flag) exits done {emit(\"😀\");delta flag {true=>yes,false=>no}[yes]:emit(\"chosen\");[entry]:exit done;}";
    f.open(source);
    let report = explain_at(&f, source, "chosen").unwrap();
    assert_eq!(report["result"]["selection"]["condition"], "yes");
    let arm = source.find("true=>").unwrap();
    assert_eq!(
        report["result"]["selection"]["decision_arms"][0]["location"]["column"],
        source[..arm].chars().count() + 1
    );
    let emoji = source[..source.find('😀').unwrap()].encode_utf16().count();
    assert!(
        f.server
            .explain(
                &f.uri,
                &json!({"kind":"selection","position":{"line":0,"character":emoji+1}})
            )
            .unwrap_err()
            .contains("UTF-16")
    );
    let changed = source.replace("[yes]:", "[no]:");
    f.change(&changed, 2);
    assert_eq!(
        explain_at(&f, &changed, "chosen").unwrap()["result"]["selection"]["condition"],
        "no"
    );
    f.change(&changed.replace("exit done;", "exit wrong;"), 3);
    assert!(explain_at(&f, &changed, "chosen").is_err());
    f.change(source, 2); // rejected stale version must not repair the invalid graph
    assert!(explain_at(&f, source, "chosen").is_err());
    f.change(source, 4);
    assert!(explain_at(&f, source, "chosen").is_ok());
    for query in [
        json!({"kind":"selection","position":{"line":0,"character":0},"extra":true}),
        json!({"kind":"selection","line":1,"column":1}),
        json!({"kind":"outcome","control":"main"}),
        json!({"kind":"outcome","control":"main","outcome":"done","extra":true}),
    ] {
        assert!(f.server.explain(&f.uri, &query).is_err());
    }
}
#[test]
fn outcome_queries_and_target_lists_follow_unsaved_imports_and_close() {
    let mut f = Fixture::new();
    let library = f.root.join("library.tellur");
    let uri = url::Url::from_file_path(&library).unwrap().to_string();
    let text = "control leaf() exits auto {exit missing;}pub control lookup() exits failed {exit delta leaf() {missing=>failed {}};}";
    fs::write(&library, text).unwrap();
    f.open(
        "import \"library.tellur\" as lib;control lookup() exits failed {forward lib.lookup();}",
    );
    let query = json!({"kind":"outcome","control":"lookup","outcome":"failed"});
    assert_eq!(
        f.server.explain_targets(&f.uri).unwrap(),
        json!([{"control":"lookup","outcome":"failed"}])
    );
    let first = f.server.explain(&f.uri, &query).unwrap();
    f.server.notification(
        "textDocument/didOpen",
        &json!({"textDocument":{"uri":uri,"version":1,"text":text.replace("missing","malformed")}}),
    );
    let second = f.server.explain(&f.uri, &query).unwrap();
    assert!(
        second["result"]["evidence"]["contributors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["control"] == "file:library.tellur::leaf" && c["outcome"] == "malformed")
    );
    f.server.notification(
        "textDocument/didChange",
        &json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"broken"}]}),
    );
    assert!(f.server.explain(&f.uri, &query).is_err());
    assert!(f.server.explain_targets(&f.uri).is_err());
    f.server.notification(
        "textDocument/didClose",
        &json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(f.server.explain(&f.uri, &query).unwrap(), first);
}
#[test]
fn explanation_requests_require_open_file_and_valid_manifest() {
    let mut f = Fixture::new();
    let query = json!({"kind":"outcome","control":"main","outcome":"done"});
    assert!(f.server.explain(&f.uri, &query).is_err());
    assert!(f.server.explain_targets(&f.uri).is_err());
    f.open("control main() exits done {exit done;}");
    fs::remove_file(f.root.join("manifest.json")).unwrap();
    assert!(f.server.explain(&f.uri, &query).is_err());
    f.server.notification("textDocument/didOpen",&json!({"textDocument":{"uri":"untitled:explain","version":1,"text":"control main() exits done {exit done;}"}}));
    assert!(
        f.server
            .explain("untitled:explain", &query)
            .unwrap_err()
            .contains("save the source file")
    );
}

#[cfg(unix)]
#[test]
fn explanation_serialization_errors_do_not_crash_the_server() {
    use std::os::unix::ffi::OsStringExt;
    let mut f = Fixture::new();
    let path = f.root.join(std::ffi::OsString::from_vec(
        b"invalid-\xff.tellur".to_vec(),
    ));
    f.uri = url::Url::from_file_path(path).unwrap().to_string();
    f.open("control main() exits done {exit done;}");
    assert_eq!(
        f.server.explain_targets(&f.uri).unwrap(),
        json!([{"control":"main","outcome":"done"}])
    );
    assert!(
        f.server
            .explain(
                &f.uri,
                &json!({"kind":"outcome","control":"main","outcome":"done"})
            )
            .is_err()
    );
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn source_fault_hover_outline_and_unsaved_permissions() {
    let mut f = Fixture::new();
    let source = "control stop() faults {fault \"broken\";}\ncontrol main() exits auto {stop();}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    for line in [0, 1] {
        let hover = f.server.hover(&f.uri, line, 9);
        let text = hover["contents"]["value"].as_str().unwrap();
        assert!(text.contains("faults"), "{text}");
        assert!(!text.contains("exits  "), "{text}");
    }
    let auto_hover = f.server.hover(&f.uri, 1, 22);
    assert!(
        auto_hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("control main() faults")
    );
    assert!(
        f.server.symbols(&f.uri)[1]["detail"]
            .as_str()
            .unwrap()
            .contains("faults")
    );
    f.change(
        "control stop() faults {fault \"broken\";}\npub control main() exits done {stop();}",
        2,
    );
    assert_eq!(f.diagnostics()[0]["code"], "E_FAULT_CONTRACT");
    f.change(
        "control stop() faults {fault \"broken\";}\npub control main() faults {stop();}",
        3,
    );
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn boundary_and_contingent_hovers_follow_unsaved_contracts() {
    let mut f = Fixture::new();
    let source = "@contingent\npub control report(string reason) exits done {emit(reason);exit done;}\nboundary request() {fault \"broken\";}\ncontingency(string reason) {report(reason);terminate;}\ncontrol main() exits done {supervise request();exit done;}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let hover = f.server.hover(&f.uri, 2, 10);
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("supervise")
    );
    let hover = f.server.hover(&f.uri, 1, 13);
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("@contingent")
    );
    assert!(
        f.server
            .symbols(&f.uri)
            .as_array()
            .unwrap()
            .iter()
            .all(|s| !s["name"].as_str().unwrap().contains('$'))
    );
    f.change(&source.replace("@contingent\n", ""), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_CONTINGENT");
}

#[test]
fn explicit_moves_and_temporary_arguments_keep_inference_and_unsaved_diagnostics() {
    let mut f = Fixture::new();
    let source = "control show(ref string text) exits done {emit(text);exit done;}\ncontrol main() exits auto {string text=\"hello\";auto owned=move text;show(borrow \"temporary\");exit done {move owned};}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let column = source.lines().nth(1).unwrap().find("auto owned").unwrap();
    assert!(
        f.server.hover(&f.uri, 1, column as u64)["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("string owned")
    );
    f.change(&source.replace("move owned", "move text"), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_MOVED");
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
}

#[test]
fn lexical_recovery_and_fault_report_hovers_and_diagnostics() {
    let mut f = Fixture::new();
    let source = "control main() exits done {\n supervised loop requests {\n  fault boundary {exit done;} contingency [] {\n   emit(borrow @fault.message);\n   recover again requests;\n  }\n }\n}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    for (line, character, part) in [
        (1, 3, "supervised tail loop"),
        (2, 34, "Lexical contingency"),
        (3, 17, "ref fault_report"),
        (4, 4, "preserved at boundary entry"),
    ] {
        assert!(
            f.server.hover(&f.uri, line, character)["contents"]["value"]
                .as_str()
                .unwrap_or("")
                .contains(part),
            "{line}: {}",
            f.server.hover(&f.uri, line, character)
        );
    }
    assert!(
        f.server.hover(&f.uri, 3, 4)["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("contingency entry")
    );
    f.change(&source.replace("recover again requests;", "exit done;"), 2);
    assert_eq!(f.diagnostics()[0]["code"], "E_CONTINGENCY_EXIT");
    f.change("control main() exits done {fault boundary {fault \"broken\";} contingency [] {terminate process 2;}}",3);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        f.server.hover(&f.uri, 0, 9)["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("Executable entry only")
    );
}

#[test]
fn constructor_outline_hover_and_inferred_locals_follow_unsaved_edits() {
    let mut f = Fixture::new();
    let source = "struct message {\n string text;\n constructor(string input) {\n  auto value=move input;\n  self.text=move value;\n }\n}\ncontrol main() exits done {\n auto text=message(\"hello\");\n emit(move text.text);exit done;\n}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let symbols = f.server.symbols(&f.uri);
    let record = symbols
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "message")
        .unwrap();
    assert_eq!(record["children"][0]["name"], "constructor");
    assert_eq!(record["children"][0]["kind"], 9);
    let hover = f.server.hover(&f.uri, 2, 3).to_string();
    assert!(hover.contains("constructor(string input)"), "{hover}");
    assert!(hover.contains("Raw brace construction"), "{hover}");
    let hover = f.server.hover(&f.uri, 3, 3).to_string();
    assert!(hover.contains("string value"), "{hover}");
    let hover = f.server.hover(&f.uri, 8, 2).to_string();
    assert!(hover.contains("message text"), "{hover}");
    f.change(
        &source.replace("  self.text=move value;", "  emit(borrow self.text);"),
        2,
    );
    assert!(
        f.diagnostics()
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "E_UNAVAILABLE")
    );
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
    let formatted = tellur::format::source(source, Default::default()).unwrap();
    assert!(
        formatted.contains("constructor(string input) {"),
        "{formatted}"
    );
    assert_eq!(
        formatted,
        tellur::format::source(&formatted, Default::default()).unwrap()
    );
}

#[test]
fn borrowed_constructor_origin_hover_and_unsaved_diagnostics() {
    let mut f = Fixture::new();
    let source = "struct view['a] {\n ref string text <- 'a;\n constructor(ref string input)[input] {self.text=input;}\n}\ncontrol main() exits done {\n string owner=\"hello\";\n auto value=view(borrow owner);\n emit(value.text);exit done;\n}";
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    assert!(
        f.server
            .hover(&f.uri, 2, 3)
            .to_string()
            .contains("constructor(ref string input)[input]")
    );
    assert!(
        f.server
            .hover(&f.uri, 6, 2)
            .to_string()
            .contains("view value")
    );
    let symbols = f.server.symbols(&f.uri).to_string();
    assert!(symbols.contains("constructor(ref string input)[input]"));
    f.change(&source.replace("[input]", "[missing]"), 2);
    assert!(
        f.diagnostics()
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "E_ORIGIN")
    );
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
    let formatted = tellur::format::source(source, Default::default()).unwrap();
    assert!(
        formatted.contains("constructor(ref string input)[input]"),
        "{formatted}"
    );
    assert_eq!(
        formatted,
        tellur::format::source(&formatted, Default::default()).unwrap()
    );
}

#[test]
fn composed_constructor_hover_and_unsaved_loan_diagnostics() {
    let mut f = Fixture::new();
    let source = include_str!("../playground/composed_construction.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let line = source
        .lines()
        .position(|l| l.contains("auto value ="))
        .unwrap();
    assert!(
        f.server
            .hover(&f.uri, line as u64, 4)
            .to_string()
            .contains("message value")
    );
    f.change(
        &source.replace(
            "emit(body.text);",
            "owner = \"too early\"; emit(body.text);",
        ),
        2,
    );
    assert!(
        f.diagnostics()
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "E_LOAN")
    );
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
    let formatted = tellur::format::source(source, Default::default()).unwrap();
    assert_eq!(
        formatted,
        tellur::format::source(&formatted, Default::default()).unwrap()
    );
}

#[test]
fn projected_constructor_auto_hovers_and_unsaved_loan_diagnostics() {
    let mut f = Fixture::new();
    let source = include_str!("../playground/projected_construction.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    for (needle, inferred) in [
        ("auto title", "string title"),
        ("ref auto text", "ref string text"),
        ("ref mutable auto edit", "ref mutable string edit"),
    ] {
        let (line, text) = source
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains(needle))
            .unwrap();
        let column = text.find("auto").unwrap();
        assert!(
            f.server
                .hover(&f.uri, line as u64, column as u64)
                .to_string()
                .contains(inferred),
            "{inferred}"
        );
    }
    f.change(
        &source.replace("emit(text);", "owner = \"too early\"; emit(text);"),
        2,
    );
    assert!(
        f.diagnostics()
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "E_LOAN")
    );
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
    let formatted = tellur::format::source(source, Default::default()).unwrap();
    assert_eq!(
        formatted,
        tellur::format::source(&formatted, Default::default()).unwrap()
    );
}

#[test]
fn sequential_initializer_hover_and_retained_borrow_diagnostics() {
    let mut f = Fixture::new();
    let source = include_str!("../playground/sequential_initializers.tellur");
    f.open(source);
    assert_eq!(f.diagnostics(), json!([]));
    let (line, text) = source
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("auto changes"))
        .unwrap();
    assert!(
        f.server
            .hover(&f.uri, line as u64, text.find("auto").unwrap() as u64)
            .to_string()
            .contains("change_log changes")
    );
    f.change(
        &source.replace(
            "emit(changes.current);",
            "owner = \"too early\"; emit(changes.current);",
        ),
        2,
    );
    assert!(
        f.diagnostics()
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "E_LOAN")
    );
    f.change(source, 3);
    assert_eq!(f.diagnostics(), json!([]));
    let formatted = tellur::format::source(source, Default::default()).unwrap();
    assert_eq!(
        formatted,
        tellur::format::source(&formatted, Default::default()).unwrap()
    );
}
