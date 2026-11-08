//! Async host operations. Every source invocation of `op(args)` (`await`, `run`,
//! `defer`, `spawn` and `spawn … into`) enters a compiler-generated async control
//! whose body is the single suspending host leaf, and `tasks<op>` names a set of
//! that control. The generated name cannot be written in source; a plain call is
//! rejected by async admission before lowering.
use crate::{
    Diagnostic,
    manifest::{Manifest, Mode, Primitive, display_type},
    syntax::{ExprKind, Unit},
};
use std::collections::BTreeSet;

const SUFFIX: &str = "$host";
/// The variant entered where an await or run decision lists `cancelled`. It has
/// the extra exit `cancelled`, selected when its task is cancelled.
const CANCELLABLE_SUFFIX: &str = "$host$cancel";

pub(crate) fn wrapper_name(operation: &str) -> String {
    format!("{operation}{SUFFIX}")
}
pub(crate) fn cancellable_name(operation: &str) -> String {
    format!("{operation}{CANCELLABLE_SUFFIX}")
}
/// The host operation performed by a generated wrapper control.
pub(crate) fn operation(name: &str) -> Option<&str> {
    name.strip_suffix(CANCELLABLE_SUFFIX)
        .or_else(|| name.strip_suffix(SUFFIX))
}
/// Whether a generated control observes cancellation of its task.
pub(crate) fn is_cancellable(name: &str) -> bool {
    name.ends_with(CANCELLABLE_SUFFIX)
}
/// The compiler-recognized alternative of a cancelled host operation.
pub(crate) const CANCELLED: &str = "cancelled";

fn wrapper_source(p: &Primitive, cancellable: bool) -> String {
    let parameters = p
        .parameters
        .iter()
        .map(|param| match param.mode {
            Mode::Borrow => format!("ref {} {}", display_type(&param.ty), param.name),
            _ => format!("{} {}", display_type(&param.ty), param.name),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let exits = p
        .outcomes
        .iter()
        .map(|o| {
            let fields = o
                .fields
                .iter()
                .map(|f| format!("{} {}", display_type(&f.ty), f.name))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} {{{fields}}}", o.name)
        })
        .chain(cancellable.then(|| format!("{CANCELLED} {{}}")))
        .collect::<Vec<_>>()
        .join(", ");
    let arguments = p
        .parameters
        .iter()
        .map(|param| match param.mode {
            Mode::Consume => format!("move {}", param.name),
            _ => param.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "async control {name}({parameters}) exits {{{exits}}} {{\n    forward {name}({arguments});\n}}\n",
        name = p.name
    )
}

/// Route each invocation of an async host operation to its generated control and
/// add those controls. Invalid manifests are left for the ordinary diagnostics.
pub(crate) fn prepare(unit: &mut Unit, manifest: &Manifest) -> crate::CheckResult<()> {
    let mut checked = manifest.clone();
    checked.canonicalize_types();
    let operations = checked
        .primitives
        .iter()
        .filter(|p| p.asynchronous)
        .cloned()
        .collect::<Vec<_>>();
    if operations.is_empty() || checked.validate().is_err() {
        return Ok(());
    }
    let names = operations
        .iter()
        .map(|p| p.name.clone())
        .collect::<BTreeSet<_>>();
    // Generate only the controls a program invokes; an unused declaration keeps
    // every other backend's program unchanged.
    let mut used = BTreeSet::new();
    let mut cancellable = BTreeSet::new();
    for region in &mut unit.regions {
        route_cancellable(&mut region.body, &names, &mut cancellable);
    }
    // `tasks<op>` is a set of the generated control's children.
    let route = |ty: &mut String, used: &mut BTreeSet<String>| {
        if let Some(producer) = crate::manifest::task_set_inner(ty)
            && names.contains(producer)
        {
            used.insert(producer.to_owned());
            *ty = crate::manifest::task_set_type(&wrapper_name(producer));
        }
    };
    for region in &mut unit.regions {
        crate::constructors::items(&mut region.body, &mut |_, _| Ok(()), &mut |e, _| {
            if let ExprKind::Call(name, _) = &mut e.kind
                && names.contains(name.as_str())
            {
                used.insert(name.clone());
                *name = wrapper_name(name);
            }
            Ok(())
        })?;
        retype(&mut region.body, &mut |ty| route(ty, &mut used));
    }
    let wrappers = operations
        .iter()
        .filter(|p| used.contains(&p.name))
        .map(|p| (p, false))
        .chain(
            operations
                .iter()
                .filter(|p| cancellable.contains(&p.name))
                .map(|p| (p, true)),
        );
    for (p, observes) in wrappers {
        let mut regions = crate::syntax::parse(&wrapper_source(p, observes)).map_err(|e| {
            Diagnostic::new(
                "E_MANIFEST",
                crate::Span::default(),
                format!(
                    "async host operation `{}` has no generated wrapper: {}",
                    p.name, e.message
                ),
            )
        })?;
        let mut region = regions.pop().expect("one generated wrapper");
        region.name = if observes {
            cancellable_name(&p.name)
        } else {
            wrapper_name(&p.name)
        };
        unit.regions.push(region);
    }
    Ok(())
}

/// Route an `await` or `run` of a host operation whose decision lists the
/// `cancelled` alternative to the operation's cancellable control.
fn route_cancellable(
    body: &mut [crate::syntax::Item],
    names: &BTreeSet<String>,
    used: &mut BTreeSet<String>,
) {
    use crate::syntax::{CallMode, Expr, Item, OutcomeExpr, Statement};
    fn lists(outcomes: impl IntoIterator<Item = String>) -> bool {
        outcomes
            .into_iter()
            .any(|o| o.rsplit('.').next() == Some(CANCELLED))
    }
    fn route(e: &mut Expr, names: &BTreeSet<String>, used: &mut BTreeSet<String>) {
        if matches!(e.call_mode, CallMode::Await | CallMode::Run)
            && let ExprKind::Call(name, _) = &mut e.kind
            && names.contains(name.as_str())
        {
            used.insert(name.clone());
            *name = cancellable_name(name);
        }
    }
    fn outcome(o: &mut OutcomeExpr, names: &BTreeSet<String>, used: &mut BTreeSet<String>) {
        if let OutcomeExpr::Delta(_, e, arms) = o {
            if lists(arms.iter().map(|a| a.pattern.outcome.clone())) {
                route(e, names, used);
            }
            for arm in arms {
                outcome(&mut arm.result, names, used);
            }
        }
    }
    fn statement(s: &mut Statement, names: &BTreeSet<String>, used: &mut BTreeSet<String>) {
        match s {
            Statement::Delta(_, e, arms) => {
                if lists(arms.iter().map(|a| a.outcome.clone())) {
                    route(e, names, used);
                }
                for arm in arms {
                    route_cancellable(&mut arm.body, names, used);
                    if let Some(termination) = &mut arm.termination {
                        statement(&mut termination.1, names, used);
                    }
                }
            }
            Statement::BindOutcome(_, _, o, _) | Statement::ExitOutcome(o) => {
                outcome(o, names, used)
            }
            Statement::Boundary { body, handler, .. } => {
                route_cancellable(body, names, used);
                route_cancellable(handler, names, used);
            }
            _ => {}
        }
    }
    for item in body {
        match item {
            Item::Guard(_, statements) => {
                for (_, s) in statements {
                    statement(s, names, used);
                }
            }
            Item::Group(_, _, b) | Item::Over(_, _, b) | Item::Loop(_, _, _, b, _) => {
                route_cancellable(b, names, used)
            }
            Item::Alias(..) => {}
        }
    }
}

/// Rewrite the declared types of local bindings and loop parameters.
fn retype(body: &mut [crate::syntax::Item], f: &mut impl FnMut(&mut String)) {
    use crate::syntax::{Item, Statement};
    for item in body {
        match item {
            Item::Guard(_, statements) => {
                for (_, s) in statements {
                    match s {
                        Statement::Bind(_, Some(annotation), _, _) => f(&mut annotation.ty),
                        Statement::Delta(_, _, arms) => {
                            for arm in arms {
                                retype(&mut arm.body, f);
                            }
                        }
                        Statement::Boundary { body, handler, .. } => {
                            retype(body, f);
                            retype(handler, f);
                        }
                        _ => {}
                    }
                }
            }
            Item::Group(_, _, b) | Item::Over(_, _, b) => retype(b, f),
            Item::Loop(_, _, parameters, b, _) => {
                for p in parameters {
                    f(&mut p.field.ty);
                }
                retype(b, f);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ir::{Action, Operation};

    fn program() -> crate::CheckedProgram {
        let mut config: serde_json::Value =
            serde_json::from_str(include_str!("../playground/manifest.json")).unwrap();
        config["primitives"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "name": "tick", "async": true,
                "parameters": [{"name": "label", "type": "string", "mode": "consume"}],
                "outcomes": [{"name": "ticked", "fields": [{"name": "label", "type": "string"}]}],
                "effects": [], "returns": true
            }));
        let manifest = crate::manifest::Manifest::from_json(&config.to_string()).unwrap();
        crate::compile(
            "async control work() exits auto {\n    forward await tick(\"x\");\n}\ncontrol main() exits auto {\n    forward run work();\n}\n",
            manifest,
        )
        .unwrap()
    }
    fn awaited(p: &mut crate::CheckedProgram) -> &mut Operation {
        let work = p.regions.get_mut("work").unwrap();
        work.code
            .iter_mut()
            .find_map(|i| match &mut i.action {
                Action::Decide { operation, .. } if matches!(operation, Operation::Await(_)) => {
                    Some(operation)
                }
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn verifier_keeps_host_leaves_in_generated_controls_reached_by_await() {
        let p = program();
        p.verify().unwrap();
        assert_eq!(
            *awaited(&mut p.clone()),
            Operation::Await("tick$host".into())
        );
        // A host leaf in any other async control is rejected.
        let mut moved = p.clone();
        let mut wrapper = moved.regions.remove("tick$host").unwrap();
        wrapper.name = "tick_shim".into();
        moved.regions.insert(wrapper.name.clone(), wrapper);
        for ty in &mut moved.manifest.types {
            if let crate::manifest::TypeDef::Variant { name, .. } = ty
                && name == "outcome<tick$host>"
            {
                *name = "outcome<tick_shim>".into();
            }
        }
        *awaited(&mut moved) = Operation::Await("tick_shim".into());
        let error = moved.verify().unwrap_err();
        assert!(
            error.message.contains("outside its generated control"),
            "{error}"
        );
        // Since 0.227 generated controls follow the ordinary async invocation
        // rules; swapping only the operation leaves await-shaped arms on a defer.
        let mut deferred = p.clone();
        *awaited(&mut deferred) = Operation::Defer("tick$host".into());
        assert!(deferred.verify().is_err());
        // A generated control must name a declared async host operation.
        let mut orphan = p.clone();
        orphan
            .manifest
            .primitives
            .iter_mut()
            .find(|p| p.name == "tick")
            .unwrap()
            .asynchronous = false;
        assert!(orphan.verify().is_err());
    }
}
