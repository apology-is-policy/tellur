//! Contracts at authored call names. Never infer callable identity from a word
//! alone, and never publish a signature from a failed check or a stale dependency.
use super::*;
use crate::{CheckedProgram, SourceRange, collection_support::generic, ir, syntax::*};

fn expressions<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    match &e.kind {
        ExprKind::Call(_, args) => {
            out.push(e);
            for e in args {
                expressions(e, out);
            }
        }
        ExprKind::Construct(_, args) | ExprKind::List(args) => {
            for e in args {
                expressions(e, out);
            }
        }
        ExprKind::NamedRecord(_, fields) => {
            for (_, e) in fields {
                expressions(e, out);
            }
        }
        ExprKind::Borrow(e)
        | ExprKind::BorrowMutable(e)
        | ExprKind::Move(e)
        | ExprKind::Deref(e)
        | ExprKind::Project(e, _)
        | ExprKind::Unary(e) => expressions(e, out),
        ExprKind::Binary(_, a, b) => {
            expressions(a, out);
            expressions(b, out);
        }
        ExprKind::Name(_)
        | ExprKind::String(_)
        | ExprKind::U32(_)
        | ExprKind::Bool(_)
        | ExprKind::Unit
        | ExprKind::Bytes(_) => (),
    }
}
fn outcome<'a>(e: &'a OutcomeExpr, out: &mut Vec<&'a Expr>) {
    match e {
        OutcomeExpr::Forward(_) => (),
        OutcomeExpr::Construct(_, _, args) => {
            for e in args {
                expressions(e, out);
            }
        }
        OutcomeExpr::Delta(_, e, arms) => {
            expressions(e, out);
            for a in arms {
                outcome(&a.result, out);
            }
        }
    }
}
fn statement<'a>(s: &'a Statement, out: &mut Vec<&'a Expr>) {
    match s {
        Statement::Boundary { body, handler, .. } => {
            items(body, out);
            items(handler, out);
        }
        Statement::Forward(e)
        | Statement::Invoke(e)
        | Statement::Emit(e)
        | Statement::Drop(e)
        | Statement::Fault(e)
        | Statement::Supervise(e)
        | Statement::Bind(_, _, e, _)
        | Statement::Capture(_, _, e, _) => expressions(e, out),
        Statement::Assign(a, b) => {
            expressions(a, out);
            expressions(b, out);
        }
        Statement::Exit(_, args) | Statement::Again(_, args) => {
            for e in args {
                expressions(e, out);
            }
        }
        Statement::ExitOutcome(e) | Statement::BindOutcome(_, _, e, _) => outcome(e, out),
        Statement::Delta(_, e, arms) => {
            expressions(e, out);
            for a in arms {
                items(&a.body, out);
                if let Some((_, s)) = a.termination.as_deref() {
                    statement(s, out);
                }
            }
        }
        Statement::Recover(_)
        | Statement::Terminate
        | Statement::TerminateProcess(_)
        | Statement::Escalate
        | Statement::ExitName(_) => (),
    }
}
fn items<'a>(body: &'a [Item], out: &mut Vec<&'a Expr>) {
    for item in body {
        match item {
            Item::Alias(..) => (),
            Item::Guard(_, ss) => {
                for (_, s) in ss {
                    statement(s, out);
                }
            }
            Item::Group(_, _, body) | Item::Over(_, _, body) => items(body, out),
            Item::Loop(_, _, parameters, body, _) => {
                for p in parameters {
                    expressions(&p.initial, out);
                }
                items(body, out);
            }
        }
    }
}
fn call_at<'a>(
    unit: &'a Unit,
    text: &str,
    at: Span,
) -> Option<(&'a str, SourceRange, &'a [String])> {
    let mut calls = vec![];
    for r in unit
        .regions
        .iter()
        .chain(unit.data.iter().filter_map(|d| d.constructor.as_ref()))
        .chain(unit.implementations.iter().flat_map(|i| &i.methods))
    {
        let mut expressions = vec![];
        items(&r.body, &mut expressions);
        calls.extend(
            expressions
                .into_iter()
                .map(|e| (e, r.type_parameters.as_slice())),
        );
    }
    let tokens = lex(text).ok()?;
    let key = |s: Span| (s.line, s.column);
    let hit = tokens
        .iter()
        .find(|t| key(t.span) <= key(at) && key(at) < key(t.end))?;
    for (e, parameters) in calls {
        let ExprKind::Call(name, _) = &e.kind else {
            unreachable!()
        };
        let mut prefix = tokens
            .iter()
            .skip_while(|t| key(t.span) < key(e.range.start))
            .take_while(|t| key(t.span) < key(e.range.end))
            // Parentheses around a call are folded into its expression range.
            .skip_while(|t| t.is("("))
            .take_while(|t| !t.is("("));
        let Some(first) = prefix.next() else {
            continue;
        };
        let last = prefix.last().unwrap_or(first);
        if key(first.span) <= key(hit.span) && key(hit.end) <= key(last.end) {
            return Some((
                name,
                SourceRange {
                    start: first.span,
                    end: last.end,
                },
                parameters,
            ));
        }
    }
    None
}

/// Replace whole qualified identifiers, never prefixes of another type/name.
fn renamed(text: &str, names: &BTreeMap<String, String>) -> String {
    let mut result = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        let c = rest.chars().next().unwrap();
        if c.is_ascii_alphabetic() || c == '_' {
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
                .unwrap_or(rest.len());
            let word = &rest[..end];
            result.push_str(names.get(word).map(String::as_str).unwrap_or(word));
            rest = &rest[end..];
        } else {
            result.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    result
}
fn scope_names(path: &Path, program: &CheckedProgram) -> BTreeMap<String, String> {
    let mut names = BTreeMap::new();
    if let Some(current) = program.source_files().iter().find(|file| file.path == path) {
        for (global, local) in &current.names {
            names.insert(local.clone(), global.clone());
        }
        for (alias, target) in &current.imports {
            if let Some(file) = program
                .source_files()
                .iter()
                .find(|file| &file.path == target)
            {
                for (global, local) in &file.names {
                    names.insert(format!("{alias}.{local}"), global.clone());
                }
            }
        }
    }
    names
}

fn normalized(name: &str, names: &BTreeMap<String, String>, program: &CheckedProgram) -> String {
    crate::generics::substitute(
        &crate::manifest::canonical_type(&renamed(name, names)),
        &program.associated_types,
    )
}

fn implementation_types(
    unit: &Unit,
    at: Span,
    names: &BTreeMap<String, String>,
    program: &CheckedProgram,
) -> Option<BTreeMap<String, String>> {
    let key = |s: Span| (s.line, s.column);
    let Some(implementation) = unit.implementations.iter().find(|i| {
        i.methods
            .iter()
            .any(|m| key(m.span) <= key(at) && key(at) < key(m.end))
    }) else {
        return Some(BTreeMap::new());
    };
    let target = normalized(&implementation.target, names, program);
    let trait_name = normalized(&implementation.trait_name, names, program);
    let mut bindings = BTreeMap::from([("self".into(), target.clone())]);
    for associated in &implementation.associated {
        let projection = format!("{trait_name}<{target}>.{}", associated.name);
        bindings.insert(
            associated.name.clone(),
            program.associated_types.get(&projection)?.clone(),
        );
    }
    Some(bindings)
}

fn checked_sources(
    server: &Server,
    uri: &str,
) -> Option<(CheckedProgram, BTreeMap<PathBuf, String>)> {
    let manifest = server.manifest(uri).ok()?;
    let mut sources = BTreeMap::new();
    let program = if let Some(path) = file_path(uri) {
        crate::modules::check_with_loader(&path, manifest, |path| {
            let source = server.source_text(path)?;
            sources.insert(path.to_owned(), source.clone());
            Ok(source)
        })
        .ok()?
    } else {
        crate::check_source(&server.documents.get(uri)?.text, manifest).ok()?
    };
    Some((program, sources))
}

pub(super) fn hover(
    server: &Server,
    uri: &str,
    unit: &Unit,
    line: u64,
    character: u64,
) -> Option<Value> {
    let text = &server.documents.get(uri)?.text;
    let at = scalar_span(text, line, character)?;
    let (written, range, parameters) = call_at(unit, text, at)?;
    let abstract_names = parameters
        .iter()
        .map(|name| (name.clone(), "$abstract".into()))
        .collect();
    if crate::generics::substitute(written, &abstract_names) != written {
        // A lexical type parameter may shadow a real type with a concrete witness.
        // Never select that witness for an abstract call in a generic body.
        return None;
    }
    // A call name must not fall back to a stale signature after any source error.
    let Some((program, sources)) = checked_sources(server, uri) else {
        return Some(Value::Null);
    };
    let path = program
        .source_files()
        .first()
        .map(|f| f.path.clone())
        .unwrap_or_default();
    let names = scope_names(&path, &program);
    let lexical_types = implementation_types(unit, at, &names, &program)?;
    // Lexical aliases take precedence over same-named module declarations.
    let resolved = normalized(
        &crate::generics::substitute(written, &lexical_types),
        &names,
        &program,
    );
    let mut visible = BTreeMap::new();
    for (local, global) in &names {
        visible.entry(global.clone()).or_insert(local.clone());
    }
    // Keep the spelling of the namespace actually used at this call where aliases coexist.
    if let Some((alias, _)) = written.split_once('.') {
        for (local, global) in &names {
            if local.starts_with(&format!("{alias}.")) {
                visible.insert(global.clone(), local.clone());
            }
        }
    }
    let mut type_sources = BTreeMap::new();
    for file in program.source_files() {
        for (global, local) in &file.names {
            if !visible.contains_key(global) {
                // Transitive module types may not have an import alias in this file.
                // File-qualified display identities are explanatory, not source syntax.
                let label = format!(
                    "{}::{local}",
                    file.path
                        .strip_prefix(path.parent().unwrap_or(Path::new("")))
                        .unwrap_or(&file.path)
                        .display()
                );
                type_sources.insert(label.clone(), file.path.clone());
                visible.insert(global.clone(), label);
            }
        }
    }
    let mut description = if let Some(source) = program.trait_contracts.get(&resolved) {
        // This is the conformance-checked trait contract, not the implementing
        // body's narrower evidence or its independently chosen binding names.
        let contract = source
            .exit_contract
            .as_ref()
            .filter(|name| program.manifest().is_outcome(name));
        let outcomes = if let Some(name) = contract {
            program.manifest().outcomes(name)?.to_vec()
        } else {
            source
                .exits
                .iter()
                .map(|o| crate::manifest::Outcome {
                    name: o.name.clone(),
                    fields: o
                        .fields
                        .iter()
                        .map(|f| crate::manifest::Field {
                            name: f.name.clone(),
                            ty: f.ty.clone(),
                        })
                        .collect(),
                })
                .collect()
        };
        let alternatives = checked_exit_description(&outcomes, source);
        let exits = match contract {
            Some(name) if source.exit_origins.is_empty() => display_type(name),
            Some(name) => format!(
                "{}[{}]",
                display_type(name),
                source
                    .exit_origins
                    .iter()
                    .map(OriginArgument::display)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None => alternatives.clone(),
        };
        format!(
            "**Trait control contract**\n\n```tellur\n{}control {}({}){}\n```{}\n\nStatic dispatch through the declared trait contract. Associated types are resolved for this implementing type.",
            if source.contingent_contract == Some(true) {
                "@contingent\n"
            } else {
                ""
            },
            display_type(written),
            source
                .parameters
                .iter()
                .map(|f| source_field(f, false))
                .collect::<Vec<_>>()
                .join(", "),
            completion_description(&exits, source.fault_contract == Some(true)),
            if contract.is_some() {
                format!("\n\nContract alternatives: `{alternatives}`.")
            } else {
                String::new()
            }
        )
    } else if let Some(checked) = program.region(&crate::constructors::control_name(&resolved)) {
        let base = generic(&resolved).map_or(resolved.as_str(), |(base, _)| base);
        let (definition_path, local) = program
            .source_files()
            .iter()
            .find_map(|f| f.names.get(base).map(|local| (&f.path, local.as_str())))
            .or_else(|| {
                unit.data
                    .iter()
                    .any(|d| d.name == base)
                    .then_some((&path, base))
            })?;
        let definition = match sources.get(definition_path) {
            Some(text) => parse_unit(text).ok()?,
            None if definition_path == &path => unit.clone(),
            None => return None,
        };
        let data = definition.data.iter().find(|d| d.name == local)?;
        let mut source = data.constructor.as_ref()?.clone();
        // Use the checked specialization, but keep authored parameter and group
        // names. Constructor implementation fields are never part of this hover.
        for (parameter, checked) in source.parameters.iter_mut().zip(&checked.parameters) {
            parameter.ty = checked.ty.clone();
        }
        source.fault_contract = Some(checked.faults);
        source.contingent_contract = Some(checked.contingent);
        let origins = if data.origin_parameters.is_empty() {
            String::new()
        } else {
            format!(
                "\n\nResult origin groups: `[{}]`; the constructor's bracket arguments map to these groups in order.",
                data.origin_parameters
                    .iter()
                    .map(|name| format!("'{name}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        format!(
            "**Constructor contract**\n\nConstructs `{}`.\n\n```tellur\n{}\n```{origins}\n\nNormal completion yields the initialized value.{}",
            display_type(written),
            constructor_description(&source),
            if checked.faults {
                " Faults propagate to the caller's execution boundary."
            } else {
                ""
            }
        )
    } else if let Some(checked) = program.region(&resolved) {
        if checked.boundary.is_some() || checked.contingency || checked.is_constructor() {
            return None;
        }
        let base = generic(&resolved).map_or(resolved.as_str(), |(base, _)| base);
        let (definition_path, local) = program
            .source_files()
            .iter()
            .find_map(|f| f.names.get(base).map(|local| (&f.path, local.as_str())))
            .or_else(|| {
                unit.regions
                    .iter()
                    .any(|r| r.name == base)
                    .then_some((&path, base))
            })?;
        let definition = match sources.get(definition_path) {
            Some(text) => parse_unit(text).ok()?,
            None if definition_path == &path => unit.clone(),
            None => return None,
        };
        let mut source = definition.regions.iter().find(|r| r.name == local)?.clone();
        source.body.clear();
        if let Some((_, arguments)) = generic(&resolved) {
            let bindings = source
                .type_parameters
                .iter()
                .cloned()
                .zip(arguments.iter().map(|a| (*a).to_owned()))
                .collect();
            crate::generic_controls::rewrite_region(&mut source, &bindings);
        }
        let defining_names = scope_names(definition_path, &program);
        let params = source
            .parameters
            .iter()
            .zip(&checked.parameters)
            .map(|(source, checked)| {
                let mut source = source.clone();
                source.ty = checked.ty.clone();
                format!("{} {}", field_type(&source), source.name)
            })
            .collect::<Vec<_>>()
            .join(", ");
        let contract = source
            .exit_contract
            .as_ref()
            .map(|n| normalized(n, &defining_names, &program))
            .filter(|name| {
                matches!(
                    program.manifest().type_def(name),
                    Some(crate::manifest::TypeDef::Outcome { .. })
                )
            });
        if let Some(contract) = contract
            .as_ref()
            .and_then(|name| program.outcome_contracts.get(name))
        {
            // Reuse the validated source schema so named results keep each public
            // origin mapping after concrete associated-type substitution.
            crate::origins::bind_contract(&mut source, contract).ok()?;
        }
        let alternatives = checked_exit_description(&checked.exits, &source);
        let exits = match &contract {
            Some(name) if source.exit_origins.is_empty() => display_type(name),
            Some(name) => format!(
                "{}[{}]",
                display_type(name),
                source
                    .exit_origins
                    .iter()
                    .map(OriginArgument::display)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None => alternatives.clone(),
        };
        let annotation = if checked.contingent {
            "@contingent\n"
        } else {
            ""
        };
        let inferred = if source.infer_exits || !source.infer_payloads.is_empty() {
            "\n\nPayloads/outcomes inferred from the checked body."
        } else {
            ""
        };
        let details = if contract.is_some() {
            format!("\n\nContract alternatives: `{alternatives}`.")
        } else {
            String::new()
        };
        format!(
            "**Control contract**\n\n```tellur\n{annotation}control {}({params}){}\n```{inferred}{details}{}",
            display_type(written),
            completion_description(&exits, checked.faults),
            if source.public {
                String::new()
            } else {
                return_lifetimes(&program, checked)
            }
        )
    } else if let Some(p) = program
        .manifest()
        .primitives
        .iter()
        .find(|p| p.name == resolved)
    {
        let params = p
            .parameters
            .iter()
            .map(|p| {
                format!(
                    "{}{} {}",
                    if p.mode == crate::manifest::Mode::Borrow {
                        "ref "
                    } else {
                        ""
                    },
                    display_type(&p.ty),
                    p.name
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let contingent = if p.contingent {
            "\n\nAdmitted in contingency code."
        } else {
            ""
        };
        let effects = if p.effects.is_empty() {
            "none declared".into()
        } else {
            p.effects.join(", ")
        };
        let execution = p
            .rust
            .as_ref()
            .map(|b| match b.execution {
                crate::rust_host::Execution::Immediate => "\n\nHost execution: immediate.",
                crate::rust_host::Execution::Blocking => "\n\nHost execution: blocking.",
            })
            .unwrap_or("");
        let pure = if p.pure {
            "\n\nPure builtin: expression use returns the sole payload; delta use retains its declared outcome."
        } else {
            ""
        };
        let consumed = p
            .parameters
            .iter()
            .filter(|p| p.mode == crate::manifest::Mode::Consume)
            .map(|p| format!("`{}`", p.name))
            .collect::<Vec<_>>();
        let ownership = if consumed.is_empty() {
            String::new()
        } else {
            format!("\n\nConsume-mode inputs: {}.", consumed.join(", "))
        };
        let returns = if p.returns {
            ""
        } else {
            "\n\nThe manifest declares no normal return."
        };
        format!(
            "**Manifest primitive**\n\n```tellur\n{}({params}){}\n```\n\nEffects: {effects}.{execution}{contingent}{pure}{ownership}{returns}",
            display_type(written),
            completion_description(&exit_description(&p.outcomes), false)
        )
    } else {
        return None;
    };
    description = renamed(&description, &visible);
    if type_sources.keys().any(|label| description.contains(label)) {
        description.push_str("\n\nFile-qualified type names identify defining sources not imported here; they are display identities, not Tellur import syntax.");
    }
    let selection = syntax::selection_site_at(unit, text, at)
        .map(|site| server.selection_for_site(uri, &program, &site))
        .unwrap_or(Value::Null);
    if let Some(history) = selection["contents"]["value"].as_str() {
        description.push_str("\n\n---\n\n");
        description.push_str(history);
    }
    Some(
        json!({"contents":{"kind":"markdown","value":description},"range":{"start":position(text,range.start),"end":position(text,range.end)}}),
    )
}

pub(super) fn return_lifetimes(program: &CheckedProgram, checked: &ir::Region) -> String {
    let borrows = checked
        .borrow_returns
        .iter()
        .map(|((outcome, index), sources)| {
            let owners = sources
                .iter()
                .filter(|s| !s.guard)
                .map(|source| {
                    let parameter = &checked.parameters[source.parameter];
                    let mut name = parameter.name.clone();
                    if let Some(c) = &source.contained {
                        return format!("`{}['{}]`", name, c.group);
                    }
                    let mut ty = crate::manifest::reference_inner(&parameter.ty)
                        .unwrap()
                        .1
                        .to_owned();
                    for index in &source.fields {
                        let (suffix, field_ty) =
                            program.manifest().borrow_field(&ty, *index).unwrap();
                        name.push_str(&suffix);
                        ty = field_ty;
                    }
                    format!("`{name}`")
                })
                .collect::<Vec<_>>()
                .join(" or ");
            let guard = if sources.iter().any(|s| s.guard) {
                " The authorizing carrier borrow also stays live."
            } else {
                ""
            };
            format!(
                "\n- `{outcome}` payload {} borrows {owners}.{guard}",
                index + 1
            )
        })
        .collect::<String>();
    let mut borrows = borrows;
    let mut shown_groups = std::collections::BTreeSet::new();
    for result in &checked.stored_returns {
        if !shown_groups.insert((&result.outcome, result.payload, &result.group)) {
            continue;
        }
        let path = format!(
            "{} payload {} ['{}]",
            result.outcome,
            result.payload + 1,
            result.group
        );
        let owners = result
            .sources
            .iter()
            .filter(|s| !s.guard)
            .map(|s| match &s.contained {
                Some(c) => {
                    format!("`{}['{}]`", checked.parameters[s.parameter].name, c.group)
                }
                None => format!("`{}`", checked.parameters[s.parameter].name),
            })
            .collect::<Vec<_>>()
            .join(" or ");
        let guard = if result.sources.iter().any(|s| s.guard) {
            " The authorizing carrier borrow also stays live."
        } else {
            ""
        };
        borrows.push_str(&format!("\n- `{path}` borrows {owners}.{guard}"));
    }
    if checked.stored_returns.iter().any(|r| r.sum_leaf.is_some()) {
        borrows.push_str("\n\nVariant dependencies are guarded by their alternatives; unpacking retains only the selected payload's dependencies.");
    }
    let has_split = checked.exits.iter().any(|outcome| {
        let mut modes = outcome
            .fields
            .iter()
            .filter_map(|f| crate::manifest::reference_inner(&f.ty).map(|(m, _)| m))
            .collect::<Vec<_>>();
        for result in checked
            .stored_returns
            .iter()
            .filter(|r| r.outcome == outcome.name && r.through.is_empty())
        {
            let field_type = outcome.fields[result.payload].ty.as_str();
            // The carrier and its contents are not distinct result
            // permissions. Count the outer reference once for this note.
            if crate::manifest::reference_inner(field_type).is_some() {
                continue;
            }
            let mut ty = field_type;
            for index in &result.fields {
                ty = &program.manifest().field_at(ty, *index).unwrap().ty;
            }
            let tagged = result
                .sum_leaf
                .as_ref()
                .and_then(|leaf| leaf.reference_type(ty, program.manifest()));
            let ty = tagged.as_deref().unwrap_or(ty);
            modes.push(crate::manifest::reference_inner(ty).unwrap().0);
        }
        modes.len() > 1 && modes.contains(&true)
    });
    if has_split {
        borrows.push_str("\n\nExclusive result references are checked for disjoint access, even when owner lifetimes coincide.");
    }
    if borrows.is_empty() {
        borrows
    } else {
        format!("\n\nChecked return lifetimes:{borrows}")
    }
}
