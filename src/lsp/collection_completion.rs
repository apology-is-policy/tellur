//! Dot is an editor gesture. Replace the whole receiver with an ordinary Tellur call.
use super::*;
use crate::collection_support::{Collection, generic};
use crate::manifest::reference_inner;
use syntax::{ExprKind, Item, Kind, Mask, Region, Statement, Unit};

fn point(span: Span) -> (usize, usize) {
    (span.line, span.column)
}

struct Site<'a> {
    name: &'a str,
    prefix: &'a str,
    start: Span,
    end: Span,
    bytes: std::ops::Range<usize>,
}

impl<'a> Site<'a> {
    fn at(source: &'a str, line: u64, character: u64) -> Option<Self> {
        let cursor = scalar_span(source, line, character)?;
        let mut lines = source.split_inclusive('\n');
        let line_offset: usize = lines.by_ref().take(cursor.line - 1).map(str::len).sum();
        let current = lines.next()?;
        let offset = line_offset
            + current
                .chars()
                .take(cursor.column - 1)
                .map(char::len_utf8)
                .sum::<usize>();
        let before = &source[..offset];
        let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
        let prefix = &before[before.trim_end_matches(ident).len()..];
        let left = before.strip_suffix(prefix)?.strip_suffix('.')?;
        let start_offset = left.trim_end_matches(ident).len();
        let name = &left[start_offset..];
        let tokens = syntax::lex(&before[..offset - prefix.len()]).ok()?;
        let [prior @ .., receiver, dot, _end] = tokens.as_slice() else {
            return None;
        };
        if !matches!(&receiver.kind, Kind::Word(n) if n == name)
            || !dot.is(".")
            || point(dot.end) != (cursor.line, cursor.column - prefix.len())
            || prior.last().is_some_and(|t| t.is("."))
        {
            return None;
        }
        let after = &source[offset..];
        let suffix_len = after.len() - after.trim_start_matches(ident).len();
        Some(Self {
            name,
            prefix,
            start: receiver.span,
            end: Span {
                line: cursor.line,
                column: cursor.column + suffix_len,
            },
            bytes: start_offset..offset + suffix_len,
        })
    }

    fn unit(&self, source: &str) -> Option<Unit> {
        // The cursor may precede existing arms, punctuation, or an unfinished line.
        // Parse a transient buffer. No edit or diagnostic is published for it.
        let tail = &source[self.bytes.end..];
        let head = &source[..self.bytes.start];
        for hole in [
            "completion_probe()",
            "completion_probe() {}",
            "completion_probe();",
            "completion_probe() {};",
        ] {
            if let Ok(unit) = syntax::parse_unit(&format!("{head}{hole}{tail}")) {
                return Some(unit);
            }
        }
        // The author may still be writing the enclosing control. Parse the prefix
        // with temporary block terminators to recover its declarations and imports.
        let tokens = syntax::lex(head).ok()?;
        let mut depth = 0usize;
        for token in tokens {
            if token.is("{") {
                depth += 1;
            }
            if token.is("}") {
                depth = depth.checked_sub(1)?;
            }
        }
        let close = "}".repeat(depth);
        for hole in ["completion_probe() {}", "completion_probe();"] {
            if let Ok(unit) = syntax::parse_unit(&format!("{head}{hole}{close}")) {
                return Some(unit);
            }
        }
        None
    }
}

#[derive(Clone)]
struct Receiver {
    ty: String,
    mutable: bool,
}

fn same_mask(a: &Mask, b: &Mask) -> bool {
    match (a, b) {
        (Mask::Name(x), Mask::Name(y)) => x == y,
        (Mask::And(al, ar), Mask::And(bl, br)) | (Mask::Or(al, ar), Mask::Or(bl, br)) => {
            same_mask(al, bl) && same_mask(ar, br)
        }
        _ => false,
    }
}

fn receiver(region: &Region, site: &Site) -> Option<Receiver> {
    let mut bindings: BTreeMap<_, _> = region
        .parameters
        .iter()
        .map(|p| {
            (
                p.name.clone(),
                Receiver {
                    ty: p.ty.clone(),
                    mutable: false,
                },
            )
        })
        .collect();
    let guards = region.body.iter().filter_map(|item| match item {
        Item::Guard(mask, stmts) => Some((mask, stmts)),
        _ => None,
    });
    let current_mask = guards
        .clone()
        .filter(|(_, stmts)| stmts.iter().any(|(s, _)| point(*s) <= point(site.start)))
        .next_back()
        .map(|(m, _)| m);
    for (mask, stmts) in guards {
        if !matches!(mask,Mask::Name(n) if n.eq_ignore_ascii_case("entry"))
            && !current_mask.is_some_and(|m| same_mask(m, mask))
        {
            continue;
        }
        for (span, stmt) in stmts {
            if point(*span) >= point(site.start) {
                break;
            }
            if let Statement::Bind(name, annotation, expr, mutable) = stmt {
                let ty = annotation
                    .as_ref()
                    .map(|a| a.ty.clone())
                    .or_else(|| match &expr.kind {
                        ExprKind::Name(n) => bindings.get(n).map(|r| r.ty.clone()),
                        _ => None,
                    });
                if let Some(ty) = ty {
                    bindings.insert(
                        name.clone(),
                        Receiver {
                            ty,
                            mutable: *mutable,
                        },
                    );
                }
            }
        }
    }
    bindings.remove(site.name)
}

struct ModuleScope<'a> {
    path: &'a Path,
    unit: &'a Unit,
}

impl ModuleScope<'_> {
    fn import(&self, alias: &str) -> Option<PathBuf> {
        let import = self.unit.imports.iter().find(|i| i.alias == alias)?;
        if Path::new(&import.path).is_absolute() || import.path.is_empty() {
            return None;
        }
        crate::modules::source_identity(&self.path.parent()?.join(&import.path)).ok()
    }

    fn type_owner(&self, name: &str) -> Option<PathBuf> {
        if let Some((alias, _)) = name.split_once('.') {
            return self.import(alias);
        }
        self.unit
            .data
            .iter()
            .any(|d| d.name == name)
            .then(|| self.path.to_owned())
    }
}

// Bind type variables in caller spelling. Concrete nominal types must belong
// to the same module, even when both files happen to declare a type named row.
fn match_type(
    pattern: &str,
    actual: &str,
    params: &[String],
    bindings: &mut BTreeMap<String, String>,
    definition: &ModuleScope,
    caller: &ModuleScope,
) -> bool {
    if params.iter().any(|p| p == pattern) {
        return bindings
            .entry(pattern.into())
            .or_insert_with(|| actual.to_owned())
            == actual;
    }
    let (p, ps) = generic(pattern).unwrap_or((pattern, vec![]));
    let (a, args) = generic(actual).unwrap_or((actual, vec![]));
    let same = match (definition.type_owner(p), caller.type_owner(a)) {
        (Some(left), Some(right)) => left == right && p.rsplit('.').next() == a.rsplit('.').next(),
        (None, None) => p == a && !p.contains('.'),
        _ => false,
    };
    same && ps.len() == args.len()
        && ps
            .into_iter()
            .zip(args)
            .all(|(p, a)| match_type(p, a, params, bindings, definition, caller))
}

fn record_fields(
    unit: &Unit,
    name: &str,
    actual: &str,
    public_only: bool,
    site: &Site,
    range: &Value,
) -> Vec<Value> {
    let Some(decl) = unit
        .data
        .iter()
        .find(|d| d.name == name && (!public_only || d.public))
    else {
        return vec![];
    };
    let syntax::DataBody::Struct(fields) = &decl.body else {
        return vec![];
    };
    let args = generic(actual).map(|(_, args)| args).unwrap_or_default();
    let types = decl
        .type_parameters
        .iter()
        .cloned()
        .zip(args.into_iter().map(str::to_owned))
        .collect();
    fields.iter().filter(|f|f.name.starts_with(site.prefix) && (!public_only || decl.public_fields.contains(&f.name))).map(|f|{
        let field=format!("{}.{}",site.name,f.name);
        json!({"label":f.name,"kind":5,"detail":display_type(&crate::generics::substitute(&f.ty,&types)),"textEdit":{"range":range,"newText":field},"filterText":field})
    }).collect()
}

impl Server {
    pub fn completions(&self, uri: &str, line: u64, character: u64) -> Value {
        let mut items = Vec::new();
        if let Some(doc) = self.documents.get(uri)
            && let Some(site) = Site::at(&doc.text, line, character)
            && let Some(unit) = site.unit(&doc.text)
            && let Some(region) = unit.regions.iter().find(|r| {
                point(r.name_span) < point(site.start) && point(site.start) < point(r.end)
            })
            && let Some(value) = receiver(region, &site)
        {
            let range =
                json!({"start":position(&doc.text,site.start),"end":position(&doc.text,site.end)});
            let base = reference_inner(&value.ty).map_or(value.ty.as_str(), |(_, t)| t);
            let (type_name, _) = generic(base).unwrap_or((base, Vec::new()));
            if Collection::parse(base).is_none() && !type_name.contains('.') {
                return json!(record_fields(&unit, type_name, base, false, &site, &range));
            }
            let Some(path) = file_path(uri).and_then(|p| crate::modules::source_identity(&p).ok())
            else {
                return json!([]);
            };
            let caller = ModuleScope {
                path: &path,
                unit: &unit,
            };
            for import in &unit.imports {
                if let Some((alias, _)) = type_name.split_once('.')
                    && alias != import.alias
                {
                    continue;
                }
                let Some(path) = caller.import(&import.alias) else {
                    continue;
                };
                let Some(module) = self
                    .source_text(&path)
                    .ok()
                    .and_then(|s| syntax::parse_unit(&s).ok())
                else {
                    continue;
                };
                if let Some((_, name)) = type_name.split_once('.') {
                    return json!(record_fields(&module, name, base, true, &site, &range));
                }
                let definition = ModuleScope {
                    path: &path,
                    unit: &module,
                };
                for control in module
                    .regions
                    .iter()
                    .filter(|c| c.public && c.name.starts_with(site.prefix))
                {
                    if let Some(mut item) = control_item(
                        control,
                        &definition,
                        &caller,
                        &site,
                        &value,
                        self.snippets,
                        &import.alias,
                    ) {
                        item["textEdit"]["range"] = range.clone();
                        items.push(item);
                    }
                }
            }
        }
        json!(items)
    }
}

fn control_item(
    control: &Region,
    definition: &ModuleScope,
    caller: &ModuleScope,
    site: &Site,
    value: &Receiver,
    snippets: bool,
    module: &str,
) -> Option<Value> {
    if control.asynchronous
        || control.boundary.is_some()
        || control.contingency
        || control.type_bounds.values().any(|b| !b.is_empty())
    {
        return None;
    }
    let (first, rest) = control.parameters.split_first()?;
    let mode = reference_inner(&first.ty);
    let formal = mode.map_or(first.ty.as_str(), |(_, t)| t);
    let actual_ref = reference_inner(&value.ty);
    let actual = actual_ref.map_or(value.ty.as_str(), |(_, t)| t);
    if Collection::parse(formal).is_none() {
        return None;
    }
    let mut bindings = BTreeMap::new();
    if !match_type(
        formal,
        actual,
        &control.type_parameters,
        &mut bindings,
        definition,
        caller,
    ) {
        return None;
    }
    let args = control
        .type_parameters
        .iter()
        .map(|p| bindings.get(p).map(|t| display_type(t)))
        .collect::<Option<Vec<_>>>()?;
    let generics = type_parameters(&args);
    let (effect, receiver) = match (mode, actual_ref) {
        (None, None) if matches!(Collection::parse(actual), Some(Collection::Array(_, _))) => {
            ("passes by value", site.name.to_owned())
        }
        (None, None) => ("moves", site.name.to_owned()),
        (Some((false, _)), None) => ("borrows", format!("borrow {}", site.name)),
        (Some((true, _)), None) if value.mutable => (
            "borrows exclusively",
            format!("borrow mutable {}", site.name),
        ),
        (Some((false, _)), Some(_)) => ("borrows", site.name.to_owned()),
        (Some((true, _)), Some((true, _))) => ("borrows exclusively", site.name.to_owned()),
        _ => return None,
    };
    let mut arguments = vec![receiver];
    for (i, f) in rest.iter().enumerate() {
        let arg = match reference_inner(&f.ty) {
            Some((true, _)) => format!("borrow mutable {}", f.name),
            Some((false, _)) => format!("borrow {}", f.name),
            None => f.name.clone(),
        };
        arguments.push(if snippets {
            format!("${{{}:{arg}}}", i + 1)
        } else {
            arg
        });
    }
    let name = format!("{module}.{}{generics}", control.name);
    let call = format!("{name}({})", arguments.join(", "));
    Some(
        json!({"label":control.name,"kind":3,"documentation":source_signature(control),"detail":format!("{effect} {} · {name}",site.name),"labelDetails":{"description":format!("{effect} {} · {module}",site.name)},"filterText":format!("{}.{}",site.name,control.name),"textEdit":{"newText":call},"insertTextFormat":if snippets {2} else {1}}),
    )
}
