//! Conservative source tail extraction. The original and generated compilation
//! units are checked, and public contracts are compared. This is a reviewable
//! source refactoring, not an expansion certificate or semantic equivalence proof.
use crate::{
    Diagnostic, Result, Span, compile,
    manifest::{self, Manifest},
    syntax::{self, Expr, Item, OutcomeExpr, Statement},
};
use std::collections::BTreeSet;
#[derive(Clone, Debug, serde::Serialize)]
pub struct Input {
    pub name: String,
    pub ty: String,
    pub mutable_local: bool,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Extraction {
    pub source: String,
    pub control: String,
    pub helper: String,
    pub start_line: usize,
    pub helper_line: usize,
    pub inputs: Vec<Input>,
    pub claim: &'static str,
}
fn error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_EXTRACT", span, message)
}
fn expression(e: &Expr, bound: &BTreeSet<String>, free: &mut BTreeSet<String>) {
    let mut names = BTreeSet::new();
    crate::liveness::expression(e, &mut names);
    let names = names
        .iter()
        .map(|n| n.split('.').next().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    free.extend(names.difference(bound).cloned());
}
fn outcome(e: &OutcomeExpr, bound: &BTreeSet<String>, free: &mut BTreeSet<String>) {
    match e {
        OutcomeExpr::Forward(_) => (),
        OutcomeExpr::Construct(_, _, args) => {
            for e in args {
                expression(e, bound, free);
            }
        }
        OutcomeExpr::Delta(_, e, arms) => {
            expression(e, bound, free);
            for arm in arms {
                let mut inner = bound.clone();
                inner.extend(arm.pattern.bindings.iter().map(|f| f.name.clone()));
                outcome(&arm.result, &inner, free);
            }
        }
    }
}
fn statement(s: &Statement, bound: &mut BTreeSet<String>, free: &mut BTreeSet<String>) {
    match s {
        Statement::Bind(n, _, e, _) | Statement::Capture(n, _, e, _) => {
            expression(e, bound, free);
            bound.insert(n.clone());
        }
        Statement::BindOutcome(n, _, e, _) => {
            outcome(e, bound, free);
            bound.insert(n.clone());
        }
        Statement::ExitOutcome(e) => outcome(e, bound, free),
        Statement::Delta(_, e, arms) => {
            expression(e, bound, free);
            for arm in arms {
                if let Some(exit) = &arm.termination {
                    let mut inner = bound.clone();
                    inner.extend(arm.bindings.iter().map(|f| f.name.clone()));
                    statement(&exit.1, &mut inner, free);
                } else {
                    bound.extend(arm.bindings.iter().map(|f| f.name.clone()));
                    items(&arm.body, bound, free);
                }
            }
        }
        Statement::ExitName(n) => {
            if !bound.contains(n) {
                free.insert(n.clone());
            }
        }
        other => {
            let mut names = BTreeSet::new();
            crate::liveness::statement(other, &mut names);
            let names = names
                .iter()
                .map(|n| n.split('.').next().unwrap().to_owned())
                .collect::<BTreeSet<_>>();
            free.extend(names.difference(bound).cloned());
        }
    }
}
fn items(body: &[Item], bound: &mut BTreeSet<String>, free: &mut BTreeSet<String>) {
    for item in body {
        match item {
            Item::Guard(_, ss) => {
                for (_, s) in ss {
                    statement(s, bound, free);
                }
            }
            Item::Loop(_, _, params, body, _) => {
                for p in params {
                    expression(&p.initial, bound, free);
                }
                let mut inner = bound.clone();
                inner.extend(params.iter().map(|p| p.field.name.clone()));
                items(body, &mut inner, free);
            }
            Item::Group(_, _, body) => items(body, bound, free),
            Item::Over(_, _, body) => items(body, &mut bound.clone(), free),
            Item::Alias(..) => (),
        }
    }
}
// Candidate names distinguish bare exit labels and nullary constructors from
// value reads. The final recheck resolves the actual boundary visibility.
fn declarations(
    body: &[Item],
    names: &mut BTreeSet<String>,
    mutable: &mut BTreeSet<String>,
    cut: usize,
) {
    fn patterns(e: &OutcomeExpr, names: &mut BTreeSet<String>) {
        if let OutcomeExpr::Delta(_, _, arms) = e {
            for a in arms {
                names.extend(a.pattern.bindings.iter().map(|f| f.name.clone()));
                patterns(&a.result, names);
            }
        }
    }
    for item in body {
        match item {
            Item::Guard(_, ss) => {
                for (span, s) in ss {
                    match s {
                        Statement::Bind(n, _, _, m)
                        | Statement::Capture(n, _, _, m)
                        | Statement::BindOutcome(n, _, _, m) => {
                            names.insert(n.clone());
                            if *m && span.line < cut {
                                mutable.insert(n.clone());
                            }
                        }
                        Statement::Delta(_, _, arms) => {
                            for arm in arms.iter().filter(|a| a.termination.is_none()) {
                                names.extend(arm.bindings.iter().map(|f| f.name.clone()));
                                declarations(&arm.body, names, mutable, cut);
                            }
                        }
                        _ => (),
                    }
                    if let Statement::ExitOutcome(e) | Statement::BindOutcome(_, _, e, _) = s {
                        patterns(e, names);
                    }
                }
            }
            Item::Loop(_, _, ps, body, _) => {
                names.extend(ps.iter().map(|p| p.field.name.clone()));
                declarations(body, names, mutable, cut);
            }
            Item::Group(_, _, body) | Item::Over(_, _, body) => {
                declarations(body, names, mutable, cut)
            }
            Item::Alias(..) => (),
        }
    }
}
fn offset(source: &str, span: Span) -> Option<usize> {
    if span.line == 0 || span.column == 0 {
        return None;
    }
    let line = source.split_inclusive('\n').nth(span.line - 1)?;
    let start = source
        .split_inclusive('\n')
        .take(span.line - 1)
        .map(str::len)
        .sum::<usize>();
    let column = line.char_indices().nth(span.column - 1).map(|(i, _)| i)?;
    Some(start + column)
}
/// Extract a top-level unconditional suffix, starting on a statement's own line.
/// The returned source is never written over the input by this API.
pub fn extract_tail(
    source: &str,
    manifest: Manifest,
    control: &str,
    line: usize,
    helper: &str,
) -> Result<Extraction> {
    let span = Span { line, column: 1 };
    let original = compile(source, manifest.clone())?;
    let unit = syntax::parse_unit(source)?;
    let region = unit
        .regions
        .iter()
        .find(|r| r.name == control)
        .ok_or_else(|| error(span, "unknown source control"))?;
    fn isolated(items: &[Item]) -> bool {
        fn statement(s: &Statement) -> bool {
            match s {
                Statement::Boundary { .. } | Statement::Recover(_) => true,
                Statement::Delta(_, _, arms) => arms.iter().any(|a| {
                    isolated(&a.body) || a.termination.as_ref().is_some_and(|e| statement(&e.1))
                }),
                _ => false,
            }
        }
        items.iter().any(|i| match i {
            Item::Guard(_, ss) => ss.iter().any(|(_, s)| statement(s)),
            Item::Loop(_, _, _, body, _) | Item::Group(_, _, body) | Item::Over(_, _, body) => {
                isolated(body)
            }
            _ => false,
        })
    }
    if region.boundary.is_some() || region.contingency || isolated(&region.body) {
        return Err(error(
            span,
            "extract-tail cannot move a boundary or contingency into an ordinary control",
        ));
    }
    if !region.modern {
        return Err(error(
            span,
            "tail extraction currently requires control syntax",
        ));
    }
    let mut chars = helper.chars();
    if !chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        || !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(error(span, "helper name must be a snake_case identifier"));
    }
    if !region.type_parameters.is_empty() {
        return Err(error(
            span,
            "extract-tail currently requires a non-generic source control",
        ));
    }
    if unit.traits.iter().any(|t| t.name == helper)
        || unit.regions.iter().any(|r| r.name == helper)
        || unit.data.iter().any(|d| d.name == helper)
        || unit.contracts.iter().any(|d| d.name == helper)
        || original.region(helper).is_some()
        || original.manifest().type_def(helper).is_some()
        || manifest.primitives.iter().any(|p| p.name == helper)
    {
        return Err(error(
            span,
            "helper name collides with an existing control, type or primitive",
        ));
    }
    let mut selection = None;
    for (i, item) in region.body.iter().enumerate() {
        match item {
            Item::Guard(mask, ss) => {
                for (j, (at, _)) in ss.iter().enumerate() {
                    if at.line == line {
                        if !matches!(mask,syntax::Mask::Name(n) if n=="entry"||n=="Entry") {
                            return Err(error(
                                span,
                                "selection must start on the unconditional entry path; branch extraction needs an explicit path boundary",
                            ));
                        }
                        let mut tail = vec![Item::Guard(mask.clone(), ss[j..].to_vec())];
                        tail.extend_from_slice(&region.body[i + 1..]);
                        if selection.is_some() {
                            return Err(error(
                                span,
                                "multiple statements start on this line; put the selected statement on its own line",
                            ));
                        }
                        selection = Some((*at, tail));
                    }
                }
            }
            Item::Loop(at, ..) | Item::Alias(at, ..) if at.line == line => {
                if selection.is_some() {
                    return Err(error(
                        span,
                        "multiple items start on this line; put the selected item on its own line",
                    ));
                }
                selection = Some((*at, region.body[i..].to_vec()))
            }
            _ => (),
        }
    }
    let (at,tail)=selection.ok_or_else(||error(span,"select a top-level statement or whole loop; nested and partial selections are unsupported"))?;
    let start = offset(source, span).ok_or_else(|| error(span, "invalid selection line"))?;
    let statement_start =
        offset(source, at).ok_or_else(|| error(span, "invalid source position"))?;
    let prefix = source[start..statement_start]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    if !["", "[entry]:", "[Entry]:"].contains(&prefix.as_str()) {
        return Err(error(
            span,
            "selected statement must begin on its own line, optionally after [entry]:",
        ));
    }
    let end = offset(
        source,
        Span {
            line: region.end.line,
            column: 1,
        },
    )
    .ok_or_else(|| error(span, "invalid closing brace"))?;
    let closing = offset(source, region.end).ok_or_else(|| error(span, "invalid closing brace"))?;
    if end <= start || !source[end..closing].trim().is_empty() {
        return Err(error(
            span,
            "the control's closing brace must be on its own line",
        ));
    }
    let mut names = region
        .parameters
        .iter()
        .map(|p| p.name.clone())
        .collect::<BTreeSet<_>>();
    let mut mutable = BTreeSet::new();
    declarations(&region.body, &mut names, &mut mutable, line);
    let mut free = BTreeSet::new();
    items(&tail, &mut BTreeSet::new(), &mut free);
    free.retain(|n| names.contains(n));
    let checked = original.region(control).unwrap();
    let mut inputs = vec![];
    for name in free {
        let types = checked
            .slots
            .values()
            .filter(|s| s.name == name)
            .map(|s| s.ty.clone())
            .collect::<BTreeSet<_>>();
        if types.len() != 1 {
            return Err(error(
                span,
                format!(
                    "cannot infer one boundary type for `{name}`; move the cut before its declaration (local borrow aliases may have no storage slot)"
                ),
            ));
        }
        let ty = types.into_iter().next().unwrap();
        if ty.contains('$') {
            return Err(error(
                span,
                format!(
                    "`{name}` has a private inferred outcome type; give it a named outcome contract before extracting"
                ),
            ));
        }
        inputs.push(Input {
            mutable_local: mutable.contains(&name),
            name,
            ty,
        });
    }
    // Fresh source names for mutable owning inputs: parameters are immutable,
    // so restore the original mutable binding explicitly inside the helper.
    let mut reserved = names.clone();
    let mut locals = String::new();
    let mut params = vec![];
    for input in &inputs {
        let ty = manifest::display_type(&input.ty);
        let mut name = input.name.clone();
        if input.mutable_local {
            name = format!("extracted_{}", input.name);
            while reserved.contains(&name) {
                name.push('_');
            }
            reserved.insert(name.clone());
            locals.push_str(&format!("    mutable {ty} {} = {name};\n", input.name));
        }
        params.push(format!("{ty} {name}"));
    }
    let contract = if let Some(name) = &region.exit_contract
        && matches!(
            original.manifest().type_def(name),
            Some(manifest::TypeDef::Outcome { .. })
        ) {
        name.clone()
    } else {
        let mut exits = vec![];
        for o in &checked.exits {
            if o.fields.iter().any(|f| f.ty.contains('$')) {
                return Err(error(
                    span,
                    "public payload has a private inferred type; name its outcome contract first",
                ));
            }
            let fields = o
                .fields
                .iter()
                .map(|f| format!("{} {}", manifest::display_type(&f.ty), f.name))
                .collect::<Vec<_>>();
            exits.push(if fields.is_empty() {
                o.name.clone()
            } else {
                format!("{} {{ {} }}", o.name, fields.join(", "))
            });
        }
        format!("{{ {} }}", exits.join(", "))
    };
    let indent = source[start..]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect::<String>();
    let args = inputs
        .iter()
        .map(|i| i.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut output = format!(
        "{}{indent}[entry]: forward {helper}({args});\n{}",
        &source[..start],
        &source[end..]
    );
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push('\n');
    let helper_line = output.lines().count() + 1;
    let contract = format!(
        "{}{}",
        if checked.exits.is_empty() {
            String::new()
        } else {
            format!(" exits {contract}")
        },
        // A private helper infers its own permission from the extracted tail.
        // Fault-only syntax needs the marker because there is no exits clause.
        if checked.exits.is_empty() {
            " faults"
        } else {
            ""
        }
    );
    output.push_str(&format!(
        "control {helper}({}){contract} {{\n{locals}{}}}\n",
        params.join(", "),
        &source[start..end]
    ));
    let candidate = compile(&output, manifest).map_err(|e| {
        error(
            span,
            format!(
                "extracted candidate rejected ({} at {}:{}): {}",
                e.code, e.span.line, e.span.column, e.message
            ),
        )
    })?;
    for before in original.regions() {
        let after = candidate
            .region(&before.name)
            .ok_or_else(|| error(span, "existing control disappeared"))?;
        // Slot IDs can shift when a helper is checked before its caller.
        // Compare public parameter descriptors, not internal identities.
        let parameters_equal = before
            .parameters
            .iter()
            .map(|p| (&p.name, &p.ty))
            .eq(after.parameters.iter().map(|p| (&p.name, &p.ty)));
        if !parameters_equal
            || before.exits != after.exits
            || before.faults != after.faults
            || before.contingent != after.contingent
            || before.boundary != after.boundary
            || before.contingency != after.contingency
            || before.borrow_returns != after.borrow_returns
            || before.stored_returns != after.stored_returns
        {
            return Err(error(
                span,
                format!(
                    "extraction changes the public contract of `{}`",
                    before.name
                ),
            ));
        }
    }
    Ok(Extraction {
        source: output,
        control: control.into(),
        helper: helper.into(),
        start_line: line,
        helper_line,
        inputs,
        claim: "rechecked_tail_extraction_preserving_public_contracts",
    })
}
