//! Source-tooling descriptions computed while checked layouts are available.
pub(crate) fn return_lifetimes(
    manifest: &crate::manifest::Manifest,
    checked: &crate::ir::Region,
) -> String {
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
                        let (suffix, field_ty) = manifest.borrow_field(&ty, *index).unwrap();
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
                ty = &manifest.field_at(ty, *index).unwrap().ty;
            }
            let tagged = result
                .sum_leaf
                .as_ref()
                .and_then(|leaf| leaf.reference_type(ty, manifest));
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
