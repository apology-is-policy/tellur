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

pub(crate) fn wrapper_name(operation: &str) -> String {
    format!("{operation}{SUFFIX}")
}
/// The host operation performed by a generated wrapper control.
pub(crate) fn operation(name: &str) -> Option<&str> {
    name.strip_suffix(SUFFIX)
}

fn wrapper_source(p: &Primitive) -> String {
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
    for p in operations.iter().filter(|p| used.contains(&p.name)) {
        let mut regions = crate::syntax::parse(&wrapper_source(p)).map_err(|e| {
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
        region.name = wrapper_name(&p.name);
        unit.regions.push(region);
    }
    Ok(())
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
