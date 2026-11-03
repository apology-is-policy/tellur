//! Test-only pending-operation continuations over checked source and real runtime
//! readiness/settlement. No manifest mode or public source handoff is introduced.
use super::*;
use std::{cell::RefCell, rc::Rc};

const WORK: &str = r#"
async control leaf(ref mutable string text, string request)
    exits ready {ref mutable string value from text}
{
    delta sample(text, move request) {ready {string next} => received}
    [received]:
        deref text = move next;
        emit(text);
        exit ready {text};
}
async control worker(ref mutable string text, string request) exits done {
    auto child = defer leaf(text, move request);
    emit("captured");
    delta await child {ready {auto value} => received}
    [received]: emit(value); exit done;
}
control start(string initial, string request) exits done {
    mutable string text = move initial;
    run worker(borrow mutable text, move request);
    exit done;
}
control nested(string initial, string request) exits done {
    fault boundary {
        mutable string text = move initial;
        run worker(borrow mutable text, move request);
    }
    contingency [] {emit("nested contingency must not run"); escalate;}
    exit done;
}
"#;

type Trace = Rc<RefCell<Vec<(String, String)>>>;

pub(super) struct Pending<'a> {
    operation: &'a str,
    arms: &'a [Arm],
    arguments: Vec<Value>,
    supplier: Place,
    partial: String,
    trace: Trace,
    request: String,
}

impl Pending<'_> {
    fn observe(&self, kind: &str) {
        self.trace
            .borrow_mut()
            .push((self.request.clone(), kind.into()));
    }

    fn borrowed_text(&self, m: &Machine<'_>) -> String {
        let Value::String(text) = m.shared.values.get(&self.supplier.root).unwrap() else {
            panic!("borrowed supplier retired before the pending operation")
        };
        text.clone()
    }

    pub(super) fn retire(self, m: &mut Machine<'_>) -> Result<()> {
        // Settlement must visit the operation before unwinding its supplier.
        assert!(!self.borrowed_text(m).is_empty());
        self.observe("retired");
        for value in self.arguments {
            retire_value(&mut m.shared.tasks, value, true)?;
        }
        Ok(())
    }
}

fn program(entry: &str) -> CheckedProgram {
    let mut config: serde_json::Value =
        serde_json::from_str(include_str!("../../playground/manifest.json")).unwrap();
    config["primitives"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "name":"sample",
            "parameters":[
                {"name":"text","type":"string","mode":"borrow"},
                {"name":"request","type":"string","mode":"consume"}
            ],
            "outcomes":[{"name":"ready","fields":[{"name":"text","type":"string"}]}],
            "effects":["sample"],"returns":true
        }));
    crate::compile(
        &format!("{WORK}{entry}"),
        Manifest::from_json(&config.to_string()).unwrap(),
    )
    .unwrap()
}

fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}

fn pending<'m, 'a>(m: &'m Machine<'a>) -> Result<&'m Pending<'a>> {
    match m.context.stack.last() {
        Some(Continuation::AdapterOperation(pending)) => Ok(pending),
        _ => Err(error("no selected pending operation")),
    }
}

fn at_sample(m: &Machine<'_>) -> bool {
    matches!(m.context.stack.last(), Some(Continuation::Block {code,next})
        if code.get(*next).is_some_and(|i| matches!(&i.action,
            Action::Decide {operation: Operation::Primitive(name), ..} if name == "sample")))
}

/// Intercept only the checked sample instruction in this private harness. This
/// consumes the same instruction fuel and moves operands once, then retains the
/// decision separately from its not-yet-produced normal result.
fn prepare<'a>(m: &mut Machine<'a>, trace: &Trace) {
    for _ in 0..200 {
        if at_sample(m) {
            break;
        }
        assert!(!m.drive(1), "sample was not reached");
    }
    let Continuation::Block { code, next } = m.context.stack.pop().unwrap() else {
        panic!("sample block missing")
    };
    let instruction = &code[next];
    let Action::Decide {
        operation: Operation::Primitive(name),
        args,
        arms,
    } = &instruction.action
    else {
        panic!("sample instruction missing")
    };
    assert_eq!(name, "sample");
    assert!(instruction.guard.evaluate(&m.context.frame.facts));
    assert!(m.context.flow.is_none() && m.shared.steps_left > 0);
    m.context.activations.at(instruction.id);
    m.shared.steps_left -= 1;
    // Existing primitive events carry value snapshots. Retain the resolved
    // supplier separately; a copied String alone would not prove a live borrow.
    let Expr::Place(place) = &args[0] else {
        panic!("sample needs a place: {:?}", args[0])
    };
    let supplier = address(&shift(place, m.context.frame.base), &m.shared.values).unwrap();
    assert!(supplier.fields.is_empty());
    let arguments = m.arguments(args).unwrap();
    let Value::String(request) = &arguments[1] else {
        panic!()
    };
    let operation = Pending {
        operation: name,
        arms,
        request: request.clone(),
        arguments,
        supplier,
        partial: String::new(),
        trace: trace.clone(),
    };
    operation.observe("prepared");
    m.context.stack.push(Continuation::Block {
        code,
        next: next + 1,
    });
    m.context
        .stack
        .push(Continuation::AdapterOperation(operation));
}

fn poll(m: &mut Machine<'_>, chunk: &str) -> Result<()> {
    pending(m)?.borrowed_text(m);
    let Some(Continuation::AdapterOperation(operation)) = m.context.stack.last_mut() else {
        unreachable!()
    };
    operation.partial.push_str(chunk);
    operation.observe("polled");
    Ok(())
}

fn deliver(m: &mut Machine<'_>, outcome: &str, fields: Vec<Value>) -> Result<()> {
    let operation = pending(m)?;
    let signature = m
        .shared
        .program
        .manifest
        .primitives
        .iter()
        .find(|p| p.name == operation.operation)
        .unwrap();
    let shape = signature
        .outcomes
        .iter()
        .find(|o| o.name == outcome)
        .ok_or_else(|| error("undeclared pending operation outcome"))?;
    if shape.fields.len() != fields.len() {
        return Err(error("pending operation payload arity"));
    }
    for (field, value) in shape.fields.iter().zip(&fields) {
        validate(value, &field.ty, &m.shared.program.manifest)?;
    }
    assert!(m.context.flow.is_none());
    let Continuation::AdapterOperation(operation) = m.context.stack.pop().unwrap() else {
        unreachable!()
    };
    operation.borrowed_text(m);
    m.bind(operation.arms, outcome.into(), fields.clone())?;
    operation.observe("completed");
    m.shared.events.push(Event::Primitive {
        operation: operation.operation.into(),
        arguments: operation.arguments,
        outcome: outcome.into(),
        fields,
        effects: signature.effects.clone(),
    });
    Ok(())
}

fn complete(m: &mut Machine<'_>) -> Result<()> {
    let operation = pending(m)?;
    let result = format!("{}:{}", operation.borrowed_text(m), operation.partial);
    deliver(m, "ready", vec![Value::String(result)])
}

fn normal_program() -> CheckedProgram {
    program("control main() exits done {start(\"unused\",\"unused\");exit done;}")
}

fn begin(m: &mut Machine<'_>, initial: &str, request: &str, trace: &Trace) -> Readiness {
    m.call(
        "start",
        vec![Value::String(initial.into()), Value::String(request.into())],
    )
    .unwrap();
    prepare(m, trace);
    poll(m, "A").unwrap();
    m.park_for_readiness().unwrap()
}

fn drain(m: &mut Machine<'_>, quota: usize) {
    assert!((0..200).any(|_| m.drive(quota)));
}

#[test]
fn partial_operations_preserve_arguments_borrows_and_completion_order() {
    for reverse in [false, true] {
        for quota in [1, 3, 99] {
            let p = normal_program();
            let s = scenario();
            let mut session = p.start_execution(&s, 400);
            let m = session.machine.as_mut().unwrap();
            let trace = Trace::default();
            let first = begin(m, "left", "one", &trace);
            m.context = Context::new(m.context.activations.child());
            let second = begin(m, "right", "two", &trace);
            let (first, second) = if reverse {
                (second, first)
            } else {
                (first, second)
            };
            let values = m.shared.values.clone();
            let fuel = m.shared.steps_left;
            let events = m.shared.events.clone();
            m.signal_readiness(&first).unwrap();
            assert!(m.signal_readiness(&first).is_err());
            assert_eq!(m.shared.values, values);
            assert_eq!(m.shared.events, events);
            assert_eq!(m.shared.steps_left, fuel);
            m.dispatch_ready().unwrap();
            assert!(!m.drive(0));
            assert!(
                !m.drive(quota),
                "pending operation cannot run its continuation"
            );
            poll(m, "").unwrap(); // spurious readiness is still not completion.
            assert_eq!(pending(m).unwrap().partial, "A");
            let again = m.park_for_readiness().unwrap();
            assert!(m.signal_readiness(&first).is_err());
            assert_eq!(m.shared.steps_left, fuel);
            assert_eq!(m.shared.suspended.calls.len(), 2);
            m.signal_readiness(&second).unwrap();
            m.dispatch_ready().unwrap();
            poll(m, "B").unwrap();
            complete(m).unwrap();
            assert!(complete(m).is_err(), "normal result is delivered once");
            m.signal_readiness(&again).unwrap();
            assert!(
                m.dispatch_ready().is_err(),
                "wake cannot interrupt selected work"
            );
            drain(m, quota);
            assert_eq!(m.shared.suspended.calls.len(), 1);
            m.dispatch_ready().unwrap();
            poll(m, "C").unwrap();
            complete(m).unwrap();
            drain(m, quota);
            let expected = if reverse {
                ["left:AB", "left:AB", "right:AC", "right:AC"]
            } else {
                ["right:AB", "right:AB", "left:AC", "left:AC"]
            };
            assert_eq!(&settlement_tests::messages(m)[2..], expected);
            for request in ["one", "two"] {
                for kind in ["prepared", "completed"] {
                    assert_eq!(
                        trace
                            .borrow()
                            .iter()
                            .filter(|(r, k)| r == request && k == kind)
                            .count(),
                        1
                    );
                }
            }
            assert!(!trace.borrow().iter().any(|(_, k)| k == "retired"));
            assert_eq!(
                m.shared.cursor, 0,
                "model does not consume/replay an immediate script"
            );
            settlement_tests::retired(m);
        }
    }
}

#[test]
fn invalid_completion_keeps_the_pending_operation_and_its_borrow() {
    let p = normal_program();
    let s = scenario();
    let mut session = p.start_execution(&s, 400);
    let m = session.machine.as_mut().unwrap();
    let trace = Trace::default();
    let token = begin(m, "owner", "request", &trace);
    m.signal_readiness(&token).unwrap();
    m.dispatch_ready().unwrap();
    let values = m.shared.values.clone();
    let fuel = m.shared.steps_left;
    let events = m.shared.events.clone();
    assert!(deliver(m, "unknown", vec![]).is_err());
    assert!(deliver(m, "ready", vec![]).is_err());
    assert!(deliver(m, "ready", vec![Value::U32(7)]).is_err());
    assert_eq!(pending(m).unwrap().borrowed_text(m), "owner");
    assert_eq!(pending(m).unwrap().partial, "A");
    assert_eq!(m.shared.values, values);
    assert_eq!(m.shared.steps_left, fuel);
    assert_eq!(m.shared.events, events);
    complete(m).unwrap();
    drain(m, 1);
    settlement_tests::retired(m);
}

#[test]
fn readiness_admission_failure_retains_prepared_operands_without_replay() {
    let p = normal_program();
    let s = scenario();
    let mut session = p.start_execution(&s, 400);
    let m = session.machine.as_mut().unwrap();
    let trace = Trace::default();
    m.call(
        "start",
        vec![
            Value::String("owner".into()),
            Value::String("request".into()),
        ],
    )
    .unwrap();
    prepare(m, &trace);
    poll(m, "A").unwrap();
    m.shared.tasks.exhaust_readiness_space();
    let values = m.shared.values.clone();
    let registry = m.shared.tasks.clone();
    let fuel = m.shared.steps_left;
    assert!(m.park_for_readiness().is_err());
    assert_eq!(m.shared.values, values);
    assert_eq!(m.shared.tasks, registry);
    assert_eq!(m.shared.steps_left, fuel);
    assert!(m.shared.waiting.is_empty());
    assert_eq!(pending(m).unwrap().partial, "A");
    complete(m).unwrap();
    drain(m, 99);
    settlement_tests::retired(m);
    assert_eq!(
        trace
            .borrow()
            .iter()
            .filter(|(_, k)| k == "prepared")
            .count(),
        1
    );
}

#[test]
fn pending_delivery_preserves_original_and_expanded_fuel_and_reports() {
    let original = normal_program();
    for p in [original.clone(), original.expanded("main").unwrap()] {
        let s = Scenario {
            calls: vec![ScriptCall {
                operation: "sample".into(),
                outcome: "ready".into(),
                fields: vec![Value::String("unused:AB".into())],
            }],
            ..scenario()
        };
        for fuel in 0..100 {
            let expected = p.run_completion(&s, fuel);
            let succeeds = expected.is_ok();
            for quota in [1, 3, 99] {
                let mut session = p.start_execution(&s, fuel);
                let m = session.machine.as_mut().unwrap();
                m.enter(&s).unwrap();
                let trace = Trace::default();
                let mut intercepted = false;
                let mut finished = false;
                for _ in 0..400 {
                    if !intercepted && at_sample(m) && m.shared.steps_left > 0 {
                        prepare(m, &trace);
                        poll(m, "A").unwrap();
                        let fuel = m.shared.steps_left;
                        let first = m.park_for_readiness().unwrap();
                        m.signal_readiness(&first).unwrap();
                        m.dispatch_ready().unwrap();
                        poll(m, "").unwrap();
                        let second = m.park_for_readiness().unwrap();
                        assert!(m.signal_readiness(&first).is_err());
                        m.signal_readiness(&second).unwrap();
                        m.dispatch_ready().unwrap();
                        poll(m, "B").unwrap();
                        complete(m).unwrap();
                        // This harness supplies exactly the declared scripted response.
                        // Pending polls and readiness never consume/replay that response.
                        assert_eq!(m.shared.cursor, 0);
                        m.shared.cursor = 1;
                        assert_eq!(m.shared.steps_left, fuel);
                        intercepted = true;
                    }
                    if m.drive(if intercepted { quota } else { 1 }) {
                        finished = true;
                        break;
                    }
                }
                assert!(finished, "fuel={fuel}, quota={quota}");
                settlement_tests::retired(m);
                let actual = session.machine.take().unwrap().finish("main");
                match (&expected, &actual) {
                    (Ok(expected), Ok(actual)) => assert_eq!(
                        serde_json::to_value(expected).unwrap(),
                        serde_json::to_value(actual).unwrap()
                    ),
                    (Err(expected), Err(actual)) => {
                        assert_eq!(expected.diagnostic, actual.diagnostic);
                        assert_eq!(
                            serde_json::to_value(&expected.report).unwrap(),
                            serde_json::to_value(&actual.report).unwrap()
                        );
                        assert_eq!(expected.events, actual.events);
                    }
                    _ => {
                        panic!("pending delivery changed completion at fuel={fuel}, quota={quota}")
                    }
                }
            }
            if succeeds {
                break;
            }
            assert!(fuel < 99, "normal completion not reached");
        }
    }
}

#[test]
fn owning_fault_retires_partial_operations_before_suppliers_and_skips_nested_handlers() {
    let p = program(
        r#"
control main() exits done {
    mutable string left="left";
    mutable string right="right";
    fault boundary {emit("owner ready");fault "owning failure";}
    contingency [left,right] {
        emit(borrow left);emit(borrow right);terminate process 0;
    }
}
"#,
    );
    for ready in [false, true] {
        for nested in [false, true] {
            for quota in [1, 99] {
                let s = scenario();
                let mut session = p.start_execution(&s, 400);
                let m = session.machine.as_mut().unwrap();
                let trace = Trace::default();
                m.enter(&s).unwrap();
                settlement_tests::until_message(m, "owner ready");
                let domain = m.context.domain.unwrap();
                let args: Vec<_> = ["left", "right"]
                    .into_iter()
                    .map(|name| {
                        let slot = p
                            .region("main")
                            .unwrap()
                            .slots
                            .values()
                            .find(|s| s.name == name)
                            .unwrap();
                        Value::Reference {
                            ty: "&mutable String".into(),
                            place: Place {
                                root: m.context.frame.base + slot.id,
                                fields: vec![],
                                indirect: false,
                            },
                        }
                    })
                    .collect();
                let owner = std::mem::replace(&mut m.context, Context::new(Default::default()));
                let mut tokens = vec![];
                for (i, arg) in args.into_iter().enumerate() {
                    let arg = if nested {
                        Value::String(if i == 0 { "left" } else { "right" }.into())
                    } else {
                        arg
                    };
                    let mut child = Context::new(owner.activations.child());
                    child.domain = Some(domain);
                    m.context = child;
                    m.call(
                        if nested { "nested" } else { "worker" },
                        vec![arg, Value::String(format!("request-{i}"))],
                    )
                    .unwrap();
                    prepare(m, &trace);
                    poll(m, "partial").unwrap();
                    let token = m.park_for_readiness().unwrap();
                    tokens.push(token);
                }
                if ready {
                    for token in &tokens {
                        m.signal_readiness(token).unwrap();
                    }
                }
                let fuel = m.shared.steps_left;
                m.context = owner;
                drain(m, quota);
                assert!(matches!(m.context.flow, Some(Ok(Flow::Process(0)))));
                assert_eq!(
                    settlement_tests::messages(m),
                    ["owner ready", "captured", "captured", "left", "right"]
                );
                assert!(m.shared.steps_left < fuel);
                assert_eq!(
                    trace
                        .borrow()
                        .iter()
                        .filter(|(_, k)| k == "prepared")
                        .count(),
                    2
                );
                assert_eq!(
                    trace
                        .borrow()
                        .iter()
                        .filter(|(_, k)| k == "retired")
                        .count(),
                    2
                );
                assert!(!trace.borrow().iter().any(|(_, k)| k == "completed"));
                assert!(
                    !m.shared
                        .events
                        .iter()
                        .any(|e| matches!(e,Event::Primitive{operation,..} if operation=="sample"))
                );
                for token in tokens {
                    assert!(m.signal_readiness(&token).is_err());
                }
                settlement_tests::retired(m);
            }
        }
    }
}
