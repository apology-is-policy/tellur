//! Dynamic task sets: `tasks<producer>`, `spawn producer(..) into set;`,
//! `await next set` with its `empty` alternative, and `count(borrow set)`.
use tellur::{
    compile,
    ir::Value,
    manifest::Manifest,
    runtime::{Event, ExecutionProgress, RunCompletion, Scenario, ScriptCall},
};

fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn scenario(calls: Vec<ScriptCall>) -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls,
    }
}
fn messages(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Emit { message } => Some(message.clone()),
            _ => None,
        })
        .collect()
}
/// Run original and expanded programs under several host quotas; every run must
/// agree exactly and complete normally.
fn schedules(source: &str, calls: Vec<ScriptCall>) -> Vec<String> {
    let p = compile(source, manifest()).unwrap();
    let mut expected = None;
    for p in [p.clone(), p.expanded("main").unwrap()] {
        for quota in [1, 2, 7, usize::MAX] {
            let scenario = scenario(calls.clone());
            let mut session = p.start_execution(&scenario, 10_000);
            let mut polls = 0;
            let result = loop {
                polls += 1;
                assert!(polls < 100_000);
                match session.advance(quota) {
                    ExecutionProgress::Complete(r) => break r.unwrap(),
                    ExecutionProgress::Paused => {}
                    _ => panic!("unexpected progress"),
                }
            };
            let RunCompletion::Outcome(result) = result else {
                panic!("expected a normal outcome")
            };
            let observed = messages(&result.events);
            if let Some(expected) = &expected {
                assert_eq!(&observed, expected, "quota={quota}");
            } else {
                expected = Some(observed);
            }
        }
    }
    expected.unwrap()
}
fn rejects(source: &str, code: &str, fragment: &str) {
    let error = compile(source, manifest()).unwrap_err();
    assert_eq!(error.code, code, "{error}");
    assert!(error.message.contains(fragment), "{error}");
}
const WORK: &str = "async control work(string name, u32 rounds) exits done { string name } {
    delta (rounds >= 1) { true => first, false => none }
    [none]: exit done { name };
    [first]: yield; delta (rounds >= 2) { true => second, false => one }
    [one]: exit done { name };
    [second]: yield; delta (rounds >= 3) { true => third, false => two }
    [two]: exit done { name };
    [third]: yield; exit done { name };
}
control main() exits done { forward run startup(); }
";
fn program(startup: &str) -> String {
    format!("{WORK}{startup}")
}
const DRAIN: &str = "loop reap(tasks<work> pending = move active) {
        delta await next pending { done { string name } => finished, empty => drained }
        [finished]: emit(name); again reap(pending);
        [drained]: exit done;
    }";

#[test]
fn members_are_joined_in_completion_order_across_quotas_and_expansion() {
    let source = program(&format!(
        "async control startup() exits done {{
    tasks<work> active = tasks();
    spawn work(\"slow\", 3) into active;
    spawn work(\"fast\", 1) into active;
    spawn work(\"middle\", 2) into active;
    spawn work(\"instant\", 0) into active;
    emit(u32_text(count(borrow active)));
    {DRAIN}
}}"
    ));
    assert_eq!(
        schedules(&source, vec![]),
        ["4", "instant", "fast", "middle", "slow"]
    );
    // An already completed member is joined before a running one, and members
    // that complete while the parent is busy are reaped earliest first.
    let busy = source.replace(
        "    loop reap(",
        "    yield;\n    yield;\n    yield;\n    yield;\n    loop reap(",
    );
    assert_eq!(
        schedules(&busy, vec![]),
        ["4", "instant", "fast", "middle", "slow"]
    );
}

#[test]
fn empty_sets_drain_immediately_and_sets_are_loop_carried() {
    let source = program(&format!(
        "async control startup() exits done {{
    tasks<work> active = tasks();
    {DRAIN}
}}"
    ));
    assert!(schedules(&source, vec![]).is_empty());
    // Bounded width: keep at most two members in flight while spawning six.
    let source = program(
        "async control startup() exits done {
    loop feed(u32 index = 0, tasks<work> active = tasks()) {
        delta (index == 6) { true => done_spawning, false => spawning }
        [done_spawning]:
            delta await next active { done { string name } => drained_one, empty => drained }
            [drained_one]: emit(concat(\"joined \", name)); again feed(index, active);
            [drained]: exit done;
        [spawning]:
            delta (count(borrow active) < 2) { true => room, false => full }
            [full]:
                delta await next active { done { string name } => reaped, empty => impossible }
                [reaped]: emit(concat(\"reaped \", name)); again feed(index, active);
                [impossible]: exit done;
            [room]:
                emit(concat(\"spawn \", u32_text(index)));
                spawn work(u32_text(index), index) into active;
                delta checked_add(index, 1) { sum { u32 next } => advanced, overflow => overflowed }
                [advanced]: again feed(next, active);
                [overflowed]: again feed(6, active);
    }
}",
    );
    let calls = (1..=6)
        .map(|n| ScriptCall {
            operation: "checked_add".into(),
            outcome: "sum".into(),
            fields: vec![Value::U32(n)],
            waits: 0,
        })
        .collect::<Vec<_>>();
    let observed = schedules(&source, calls);
    let mut live = 0usize;
    for message in &observed {
        if message.starts_with("spawn ") {
            live += 1;
            assert!(live <= 2, "{observed:?}");
        } else {
            live -= 1;
        }
    }
    assert_eq!(live, 0);
    assert_eq!(
        observed.iter().filter(|m| m.starts_with("spawn ")).count(),
        6
    );
}

#[test]
fn faulting_member_settles_its_siblings_before_the_owning_contingency() {
    let source = include_str!("fixtures/source_task_set_fault.tellur");
    assert_eq!(
        schedules(source, vec![]),
        ["slow", "broken", "member failed"]
    );
    // A healthy member that completes before the parent awaits is settled too:
    // its result is never joined and nothing else runs after the fault.
    let busy = source
        .replace(
            "spawn work(\"slow\", false) into active;",
            "spawn work(\"slow\", false) into active;\n    spawn work(\"fast\", false) into active;",
        )
        .replace(
            "    loop reap(",
            "    yield;\n    yield;\n    yield;\n    yield;\n    yield;\n    loop reap(",
        );
    let observed = schedules(&busy, vec![]);
    assert_eq!(observed.last().unwrap(), "member failed");
    assert!(
        !observed.iter().any(|m| m.starts_with("joined")),
        "{observed:?}"
    );
}

#[test]
fn members_may_own_resources_completed_before_their_handoffs() {
    let source = "async control use(string path) exits done {
    delta open(path) { opened { handle h } => opened, not_found => missing, denied => refused }
    [missing | refused]: yield; exit done;
    [opened]:
        delta close(h) { closed => closed, failed { close_error e } => failed }
        [closed | failed]: yield; exit done;
}
async control startup() exits done {
    tasks<use> active = tasks();
    spawn use(\"a\") into active;
    spawn use(\"b\") into active;
    loop reap(tasks<use> pending = move active) {
        delta await next pending { done => joined, empty => drained }
        [joined]: emit(\"joined\"); again reap(pending);
        [drained]: exit done;
    }
}
control main() exits done { forward run startup(); }
";
    let call = |operation: &str, outcome: &str, fields| ScriptCall {
        operation: operation.into(),
        outcome: outcome.into(),
        fields,
        waits: 0,
    };
    let handle = |token: &str| Value::Opaque {
        ty: "handle".into(),
        token: token.into(),
    };
    let calls = vec![
        call("open", "opened", vec![handle("a")]),
        call("close", "closed", vec![]),
        call("open", "opened", vec![handle("b")]),
        call("close", "closed", vec![]),
    ];
    assert_eq!(schedules(source, calls), ["joined", "joined"]);
    // A member that retains the handle across a handoff needs a fault disposition.
    rejects(
        &source.replace(
            "        delta close(h) { closed => closed, failed { close_error e } => failed }\n        [closed | failed]: yield; exit done;",
            "        yield;\n        delta close(h) { closed => closed, failed { close_error e } => failed }\n        [closed | failed]: exit done;",
        ),
        "E_ASYNC",
        "spawn requires fault dispositions",
    );
}

#[test]
fn task_sets_reject_escape_aliasing_and_unhandled_members() {
    let start = |body: &str| program(&format!("async control startup() exits done {{ {body} }}"));
    rejects(
        &start("tasks<work> active = tasks(); spawn work(\"a\", 1) into active; exit done;"),
        "E_RESOURCE",
        "remains owned",
    );
    rejects(
        &start(
            "tasks<work> active = tasks(); delta await next active { done { string name } => d, empty => e } [d]: exit done; [e]: exit done;",
        ),
        "E_RESOURCE",
        "remains owned",
    );
    rejects(
        &start(&format!(
            "tasks<work> active = tasks(); spawn work(\"a\", 1) into active; fault boundary {{ emit(\"x\"); }} contingency [] {{ emit(\"y\"); }} {DRAIN}"
        )),
        "E_ASYNC",
        "before a direct inline fault boundary",
    );
    rejects(
        &start("tasks<work> active = tasks(); drop(active); exit done;"),
        "E_RESOURCE",
        "drop cannot discard",
    );
    rejects(
        &start(
            "tasks<work> active = tasks(); loop l(u32 i = 0) { spawn work(\"a\", 1) into active; again l(i); }",
        ),
        "E_RESOURCE",
        "remains owned",
    );
    rejects(
        &start("auto active = tasks(); exit done;"),
        "E_ASYNC",
        "tasks() creates an empty task set",
    );
    rejects(
        &start("tasks<work> active = tasks(); forward await next active;"),
        "E_ASYNC",
        "await next needs a delta",
    );
    rejects(
        &program(
            "async control other() exits done { exit done; }
async control startup() exits done { tasks<work> active = tasks(); spawn other() into active; exit done; }",
        ),
        "E_ASYNC",
        "spawn into it only with that producer",
    );
    rejects(
        &program(
            "control helper() exits done { tasks<work> active = tasks(); exit done; }
async control startup() exits done { exit done; }",
        ),
        "E_ASYNC",
        "belongs to an async control",
    );
    rejects(
        &program(
            "async control helper(tasks<work> active) exits done { exit done; }
async control startup() exits done { exit done; }",
        ),
        "E_ASYNC",
        "pass them only as loop parameters",
    );
    rejects(
        &program(
            "async control give() exits ready { tasks<work> active } { exit ready {}; }
async control startup() exits done { exit done; }",
        ),
        "E_ASYNC",
        "drain them with await next before exiting",
    );
    rejects(
        &program(
            "async control lend(ref string name) exits done { yield; exit done; }
async control startup() exits done { tasks<lend> active = tasks(); exit done; }",
        ),
        "E_ASYNC",
        "borrowed captures and results remain gated",
    );
    rejects(
        &program(
            "async control odd() exits empty, full { exit full; }
async control startup() exits done { tasks<odd> active = tasks(); exit done; }",
        ),
        "E_ASYNC",
        "reserved `empty` alternative",
    );
    rejects(
        &start("spawn work(\"a\", 1);"),
        "E_UNSUPPORTED",
        "spawn producer(arguments) into set",
    );
}

#[test]
fn native_task_sets_match_reference() {
    use std::{collections::BTreeMap, fs, process::Command};
    let root = std::env::temp_dir().join(format!("tellur-task-sets-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    let run = scenario(vec![]);
    fs::write(&input, serde_json::to_vec(&run).unwrap()).unwrap();
    for (label, source) in [
        ("sets", include_str!("../playground/task_sets.tellur")),
        (
            "fault",
            include_str!("fixtures/source_task_set_fault.tellur"),
        ),
        (
            "parent_fault",
            include_str!("fixtures/source_task_set_parent_fault.tellur"),
        ),
    ] {
        let p = compile(source, manifest())
            .unwrap()
            .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
            .unwrap();
        let binary = root.join(label);
        p.build_native("main", &binary, 0).unwrap();
        let out = Command::new(&binary)
            .arg("--scenario")
            .arg(&input)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap(),
            serde_json::to_value(p.run(&run).unwrap()).unwrap(),
            "{label}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}
