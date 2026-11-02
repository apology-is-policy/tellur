//! Bounded compiled-continuation experiment. Public native emission is unchanged.
use super::*;

enum PointKind<'a> {
    Instruction(&'a Instruction),
    InvalidEnd(&'static str),
    BoundaryEnd(usize),
}
struct Point<'a> {
    kind: PointKind<'a>,
    next: usize,
    body: Option<usize>,
    handler: Option<usize>,
}
fn points<'a>(code: &'a [Instruction], result: &mut Vec<Point<'a>>) {
    for instruction in code {
        let pc = result.len();
        result.push(Point {
            kind: PointKind::Instruction(instruction),
            next: pc + 1,
            body: None,
            handler: None,
        });
        match &instruction.action {
            Action::Loop { body, .. } => {
                result[pc].body = Some(result.len());
                points(body, result);
                result.push(Point {
                    kind: PointKind::InvalidEnd("checked loop fell through"),
                    next: 0,
                    body: None,
                    handler: None,
                });
                result[pc].next = result.len();
            }
            Action::Boundary { body, handler, .. } => {
                result[pc].body = Some(result.len());
                points(body, result);
                result.push(Point {
                    kind: PointKind::BoundaryEnd(pc),
                    next: 0,
                    body: None,
                    handler: None,
                });
                result[pc].handler = Some(result.len());
                points(handler, result);
                result.push(Point {
                    kind: PointKind::InvalidEnd("contingency fell through"),
                    next: 0,
                    body: None,
                    handler: None,
                });
                result[pc].next = result.len();
            }
            _ => {}
        }
    }
}

fn source(program: &CheckedProgram, entry: &str) -> Result<String> {
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
    // Exercise the exact shared implementation embedded by normal generation.
    // Only compiled fixture entry points and explicit test schedules are injected.
    assert_eq!(source.matches(EXECUTION).count(), 1);
    assert_eq!(source.matches(CONTEXTS).count(), 1);
    source = source.replacen(
        CONTEXTS,
        &format!(
            "{CONTEXTS}\n{}\n{}\nmod sibling_model {{\n{}\n}}\nmod result_model {{\n{}\n}}\npub use result_model::*;\npub use sibling_model::*;\n",
            include_str!("compiled_model.rs"),
            include_str!("context_model.rs"),
            include_str!("sibling_model.rs"),
            include_str!("result_model.rs")
        ),
        1,
    );
    source = source.replacen(
        "pub use contexts::{ContextKey, ContextPoll, Contexts, Readiness};",
        "pub use contexts::*;",
        1,
    );
    source = source.replacen(
        "use support::*;",
        "use support::*; use support::execution::*;",
        1,
    );
    for region in program.regions() {
        let id = names[region.name.as_str()];
        g.layout = FrameLayout::new(region);
        g.constructing = g.constructors.contains(region.name.as_str());
        let _ = writeln!(
            g.code,
            "static MODEL_{id}:Control=Control{{name:{},slots:{},atoms:{},parameters:&{:?},retained:&{:?},constructor:{},resume:model_{id}}};",
            string(&program.runtime_name(&region.name)),
            g.layout.slots.len(),
            g.layout.atoms.len(),
            g.layout
                .slots(&region.parameters.iter().map(|p| p.id).collect::<Vec<_>>()),
            g.layout.slots(
                region
                    .boundary
                    .as_ref()
                    .map(|b| b.retained.as_slice())
                    .unwrap_or(&[])
            ),
            g.constructing
        );
        let mut deliveries = String::new();
        let _ = writeln!(
            g.code,
            "fn model_{id}(host:&mut Host, mut v:&mut Frames, state:&mut Activation)->R<Step>{{\nlet pc=state.pc;let base=state.base();let destination=state.destination.unwrap_or(0);let mut a=&mut state.facts;\nmatch pc {{"
        );
        let mut plan = vec![];
        points(&region.code, &mut plan);
        for (pc, point) in plan.iter().enumerate() {
            let instruction = match &point.kind {
                PointKind::Instruction(instruction) => *instruction,
                PointKind::InvalidEnd(message) => {
                    let _ = writeln!(
                        g.code,
                        "{pc}=>Err({}.into()),",
                        string(&format!("E_NATIVE: {message}"))
                    );
                    continue;
                }
                PointKind::BoundaryEnd(owner) => {
                    let _ = writeln!(
                        g.code,
                        "{pc}=>Ok(Step::BoundaryEnd(&BOUNDARY_{id}_{owner})),"
                    );
                    continue;
                }
            };
            let _ = writeln!(g.code, "{pc}=>{{state.pc={};", point.next);
            if let Action::Supervise { boundary, args } = &instruction.action {
                let work = program.region(boundary).unwrap();
                let handler = &work.boundary.as_ref().unwrap().handler;
                let p = &instruction.provenance;
                let _ = writeln!(
                    g.code,
                    "host.at({});host.tick({}, {})?;if {} {{let arguments={};return Ok(Step::Supervise{{work:&MODEL_{},handler:&MODEL_{},arguments}});}}Ok(Step::Continue)}},",
                    instruction.id,
                    p.span.line,
                    p.span.column,
                    guard(&instruction.guard, &g.layout),
                    expressions(args, &g.layout),
                    names[boundary.as_str()],
                    names[handler.as_str()]
                );
                continue;
            }
            if let Action::Boundary {
                retained,
                retire,
                work_slots,
                handler_slots,
                fault_slot,
                ..
            } = &instruction.action
            {
                let p = &instruction.provenance;
                let _ = writeln!(
                    g.code,
                    "host.at({});host.tick({}, {})?;if {} {{return Ok(Step::Boundary(&BOUNDARY_{id}_{pc}));}}Ok(Step::Continue)}},",
                    instruction.id,
                    p.span.line,
                    p.span.column,
                    guard(&instruction.guard, &g.layout)
                );
                let name = format!("{}$boundary@{}", p.region, p.span.line);
                let _ = writeln!(
                    deliveries,
                    "static BOUNDARY_{id}_{pc}:BoundaryControl=BoundaryControl{{name:{},body:{},handler:{},after:{},retained:&{:?},retire:&{:?},work:&{:?},handling:&{:?},fault_slot:{}}};",
                    string(&name),
                    point.body.unwrap(),
                    point.handler.unwrap(),
                    point.next,
                    g.layout.slots(retained),
                    g.layout.slots(retire),
                    g.layout.slots(work_slots),
                    g.layout.slots(handler_slots),
                    g.layout.slots[fault_slot]
                );
                continue;
            }
            if let Action::Loop {
                id: target,
                parameters,
                local_slots,
                local_atoms,
                ..
            } = &instruction.action
            {
                let p = &instruction.provenance;
                let _ = writeln!(
                    g.code,
                    "host.at({});host.tick({}, {})?;if {} {{",
                    instruction.id,
                    p.span.line,
                    p.span.column,
                    guard(&instruction.guard, &g.layout)
                );
                let arguments = expressions(
                    &parameters
                        .iter()
                        .map(|(_, e)| e.clone())
                        .collect::<Vec<_>>(),
                    &g.layout,
                );
                let _ = writeln!(
                    g.code,
                    "let arguments={arguments};return Ok(Step::Loop{{control:&LOOP_{id}_{pc},arguments}});}}Ok(Step::Continue)}},"
                );
                let _ = writeln!(
                    deliveries,
                    "static LOOP_{id}_{pc}:LoopControl=LoopControl{{target:{target},body:{},after:{},locals:&{:?},reset:reset_{id}_{pc}}};",
                    point.body.unwrap(),
                    point.next,
                    g.layout.slots(local_slots)
                );
                let _ = writeln!(
                    deliveries,
                    "fn reset_{id}_{pc}(state:&mut Activation,v:&mut Frames,args:Vec<V>)->R<()>{{if args.len()!={}{{return Err(\"E_NATIVE: invalid compiled loop arguments\".into());}}let base=state.base();let a=&mut state.facts;",
                    parameters.len()
                );
                for slot in local_slots {
                    let _ = writeln!(deliveries, "v[base+{}]=None;", g.layout.slots[slot]);
                }
                for atom in local_atoms {
                    let _ = writeln!(deliveries, "a[{}]=false;", g.layout.atoms[atom]);
                }
                deliveries.push_str("let mut args=args.into_iter();\n");
                for (slot, _) in parameters {
                    let _ = writeln!(deliveries, "v[base+{}]=args.next();", g.layout.slots[slot]);
                }
                deliveries.push_str("Ok(())}\n");
                continue;
            }
            if let Action::Decide {
                operation: Operation::Defer(name),
                args,
                arms,
            } = &instruction.action
            {
                debug_assert_eq!(arms.len(), 1);
                debug_assert_eq!(arms[0].slots.len(), 1);
                let p = &instruction.provenance;
                let target = names[name.as_str()];
                let _ = writeln!(
                    g.code,
                    "host.at({});host.tick({}, {})?;if {} {{let mut arguments={};let task=state.defer_task(host,&MODEL_{target},{},&mut arguments)?;v[base+{}]=Some(task);state.facts[{}]=true;}}Ok(Step::Continue)}},",
                    instruction.id,
                    p.span.line,
                    p.span.column,
                    guard(&instruction.guard, &g.layout),
                    expressions(args, &g.layout),
                    string(&crate::manifest::task_type(name)),
                    g.layout.slots[&arms[0].slots[0]],
                    g.layout.atoms[&arms[0].atom]
                );
                continue;
            }
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
                let arguments = expressions(args, &g.layout);
                let awaiting = matches!(
                    &instruction.action,
                    Action::Decide {
                        operation: Operation::AwaitTask(_),
                        ..
                    }
                );
                let destination = if g.constructors.contains(name.as_str()) {
                    format!("Some(base+{})", g.layout.slots[&arms[0].slots[0]])
                } else {
                    "None".into()
                };
                let target = names[name.as_str()];
                if awaiting {
                    debug_assert_eq!(args.len(), 1);
                    let _ = writeln!(
                        g.code,
                        "let mut arguments={arguments};let mut value=arguments.pop();let task=state.queue_task(host,&MODEL_{target},&mut value)?;return Ok(Step::Await{{task,deliver:deliver_{id}_{pc}}});\n}}Ok(Step::Continue)\n}},"
                    );
                } else {
                    let _ = writeln!(
                        g.code,
                        "let arguments={arguments};return Ok(Step::Call{{control:&MODEL_{target},arguments,destination:{destination},deliver:deliver_{id}_{pc}}});\n}}Ok(Step::Continue)\n}},"
                    );
                }
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
        if !region.is_constructor()
            && region.boundary.is_none()
            && !region.contingency
            && !region.parameters.iter().any(|p| p.ty.starts_with('&'))
        {
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

#[cfg(test)]
#[path = "resume_model_resource_tests.rs"]
mod resource_tests;

#[cfg(test)]
#[path = "context_model_tests.rs"]
mod context_tests;

#[cfg(test)]
#[path = "sibling_model_tests.rs"]
mod sibling_tests;

#[cfg(test)]
#[path = "result_model_tests.rs"]
mod result_tests;
