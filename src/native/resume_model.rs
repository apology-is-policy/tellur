//! Bounded compiled-continuation experiment. Public native emission is unchanged.
use super::*;

fn source(program: &CheckedProgram, entry: &str) -> Result<String> {
    for region in program.regions() {
        if region.boundary.is_some()
            || region.contingency
            || region.code.iter().any(|i| {
                matches!(
                    i.action,
                    Action::Loop { .. }
                        | Action::Boundary { .. }
                        | Action::Supervise { .. }
                        | Action::Again { .. }
                        | Action::Recover { .. }
                        | Action::Terminate
                        | Action::TerminateProcess(_)
                        | Action::Escalate
                        | Action::Complete {
                            unwind: Some(_),
                            ..
                        }
                )
            })
        {
            return Err(error(
                "compiled continuation model does not yet support loops or boundaries",
            ));
        }
    }
    let names: BTreeMap<_, _> = program
        .regions()
        .enumerate()
        .map(|(i, r)| (r.name.as_str(), i))
        .collect();
    let mut g = Generator {
        constructors: program
            .regions()
            .filter(|r| r.is_constructor())
            .map(|r| r.name.as_str())
            .collect(),
        constructing: false,
        boundaries: BTreeMap::new(),
        display_names: program
            .regions()
            .map(|r| (r.name.as_str(), program.runtime_name(&r.name)))
            .collect(),
        names: names.clone(),
        code: String::new(),
        layout: FrameLayout::default(),
    };
    let mut source = program.native_source(entry)?;
    assert_eq!(source.matches(SUPPORT).count(), 1);
    source = source.replacen(
        SUPPORT,
        &format!(
            "{SUPPORT}\nmod compiled_model {{\n{}\n}}\npub use compiled_model::*;\n",
            include_str!("compiled_model.rs")
        ),
        1,
    );
    for region in program.regions() {
        let id = names[region.name.as_str()];
        g.layout = FrameLayout::new(region);
        g.constructing = g.constructors.contains(region.name.as_str());
        let _ = writeln!(
            g.code,
            "static MODEL_{id}:Control=Control{{name:{},slots:{},atoms:{},parameters:&{:?},constructor:{},resume:model_{id}}};",
            string(&program.runtime_name(&region.name)),
            g.layout.slots.len(),
            g.layout.atoms.len(),
            g.layout
                .slots(&region.parameters.iter().map(|p| p.id).collect::<Vec<_>>()),
            g.constructing
        );
        let mut deliveries = String::new();
        let _ = writeln!(
            g.code,
            "fn model_{id}(host:&mut Host, mut v:&mut Frames, state:&mut Activation)->R<Step>{{\nlet pc=state.pc;state.pc+=1;let base=state.base();let destination=state.destination.unwrap_or(0);let mut a=&mut state.facts;\nmatch pc {{"
        );
        for (pc, instruction) in region.code.iter().enumerate() {
            let _ = writeln!(g.code, "{pc}=>{{");
            let call = match &instruction.action {
                Action::Decide {
                    operation:
                        Operation::Region(name)
                        | Operation::Run(name)
                        | Operation::Await(name)
                        | Operation::AwaitTask(name),
                    args,
                    arms,
                } => Some((name, args, arms)),
                _ => None,
            };
            if let Some((name, args, arms)) = call {
                let p = &instruction.provenance;
                let _ = writeln!(
                    g.code,
                    "host.at({});host.tick({}, {})?;if {} {{",
                    instruction.id,
                    p.span.line,
                    p.span.column,
                    guard(&instruction.guard, &g.layout)
                );
                let mut arguments = expressions(args, &g.layout);
                if matches!(
                    &instruction.action,
                    Action::Decide {
                        operation: Operation::AwaitTask(_),
                        ..
                    }
                ) {
                    arguments = format!("await_arguments({arguments}, {})?", string(name));
                }
                let destination = if g.constructors.contains(name.as_str()) {
                    format!("Some(base+{})", g.layout.slots[&arms[0].slots[0]])
                } else {
                    "None".into()
                };
                let target = names[name.as_str()];
                let _ = writeln!(
                    g.code,
                    "let arguments={arguments};return Ok(Step::Call{{control:&MODEL_{target},arguments,destination:{destination},deliver:deliver_{id}_{pc}}});\n}}Ok(Step::Continue)\n}},"
                );
                let _ = writeln!(
                    deliveries,
                    "fn deliver_{id}_{pc}(state:&mut Activation,v:&mut Frames,result:Completion)->R<()>{{let base=state.base();let a=&mut state.facts;"
                );
                if g.constructors.contains(name.as_str()) {
                    let _ = writeln!(
                        deliveries,
                        "result.constructed()?;a[{}]=true;",
                        g.layout.atoms[&arms[0].atom]
                    );
                } else {
                    deliveries.push_str(
                        "let (outcome,fields)=result.outcome()?;match outcome.as_str(){\n",
                    );
                    for arm in arms {
                        let _ = writeln!(
                            deliveries,
                            "{}=>{{a[{}]=true;let mut fields=fields.into_iter();",
                            string(&arm.outcome),
                            g.layout.atoms[&arm.atom]
                        );
                        for slot in &arm.slots {
                            let _ = writeln!(
                                deliveries,
                                "v[base+{}]=fields.next();",
                                g.layout.slots[slot]
                            );
                        }
                        deliveries.push_str("},\n");
                    }
                    deliveries.push_str(
                        "_=>return Err(\"E_NATIVE: unmatched declared outcome\".into()),}\n",
                    );
                }
                deliveries.push_str("Ok(())}\n");
            } else {
                g.code.push_str("let result=(||->R<Completion>{\n");
                g.instructions(std::slice::from_ref(instruction));
                g.code.push_str("Ok(Completion::Fallthrough)})();match result?{Completion::Fallthrough=>Ok(Step::Continue),result=>Ok(Step::Complete(result))}\n},\n");
            }
        }
        g.code
            .push_str("_=>Err(\"E_NATIVE: checked region fell through\".into()),}}\n");
        g.code.push_str(&deliveries);
        if !region.is_constructor() && !region.parameters.iter().any(|p| p.ty.starts_with('&')) {
            let old = format!(
                "fn entry_{id}(host: &mut Host, args: Vec<V>)->R<Completion> {{ let mut v=Frames::default(); region_{id}(host, &mut v, args, &mut vec![]) }}"
            );
            assert_eq!(source.matches(&old).count(), 1);
            source=source.replacen(&old,&format!("fn entry_{id}(host:&mut Host,args:Vec<V>)->R<Completion>{{drive(host,args,&MODEL_{id})}}"),1);
        }
    }
    source.push_str(&g.code);
    Ok(source)
}

#[cfg(test)]
#[path = "resume_model_tests.rs"]
mod tests;
