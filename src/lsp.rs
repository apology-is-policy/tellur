//! Small LSP 3.17 service sharing the compiler's parser and checker.
//! Each document is a compilation unit, matching `tellurc check`.
use crate::{
    Diagnostic, Span,
    manifest::{Manifest, display_type},
    syntax,
};
use lsp_server::{Connection, Message, Notification, Request, Response};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use url::Url;

#[derive(Clone)]
struct Document {
    text: String,
    version: i64,
}
#[derive(Default)]
pub struct Server {
    documents: BTreeMap<String, Document>,
    roots: Vec<PathBuf>,
    manifest_path: String,
    dirty: bool,
}
fn file_path(uri: &str) -> Option<PathBuf> {
    Url::parse(uri).ok()?.to_file_path().ok()
}
fn position(text: &str, span: Span) -> Value {
    let line = span.line.saturating_sub(1);
    let contents = text
        .split('\n')
        .nth(line)
        .unwrap_or("")
        .trim_end_matches('\r');
    let character: usize = contents
        .chars()
        .take(span.column.saturating_sub(1))
        .map(char::len_utf16)
        .sum();
    json!({"line":line,"character":character})
}
fn scalar_span(text: &str, line: u64, character: u64) -> Option<Span> {
    let line = usize::try_from(line).ok()?;
    let target = usize::try_from(character).ok()?;
    let contents = text.split('\n').nth(line)?.trim_end_matches('\r');
    let mut units = 0;
    let mut column = 1;
    for c in contents.chars() {
        if units == target {
            return Some(Span {
                line: line + 1,
                column,
            });
        }
        units += c.len_utf16();
        column += 1;
        if units > target {
            return None;
        }
    }
    (units == target).then_some(Span {
        line: line + 1,
        column,
    })
}
pub fn diagnostic_range(text: &str, span: Span) -> Value {
    let start = position(text, span);
    let line = text
        .split('\n')
        .nth(span.line.saturating_sub(1))
        .unwrap_or("")
        .trim_end_matches('\r');
    let rest: String = line.chars().skip(span.column.saturating_sub(1)).collect();
    let width: usize = if rest.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        rest.chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .map(char::len_utf16)
            .sum()
    } else {
        rest.chars().next().map(char::len_utf16).unwrap_or(0)
    };
    json!({"start":start,"end":{"line":start["line"],"character":start["character"].as_u64().unwrap_or(0) + width as u64}})
}
fn source_range(text: &str, error: &Diagnostic) -> Value {
    match error.end.as_deref() {
        Some(end) => json!({"start":position(text,error.span),"end":position(text,*end)}),
        None => diagnostic_range(text, error.span),
    }
}
fn diagnostic(text: &str, error: Diagnostic, severity: u8) -> Value {
    let mut result = json!({"range":source_range(text,&error),"severity":severity,"code":error.code,"source":"tellur","message":error.message});
    if let Some(witness) = error.witness {
        result["data"] = json!({"schema":"tellur.diagnostic.v1","witness":witness});
    }
    result
}
impl Server {
    pub fn initialize(&mut self, params: &Value) {
        if let Some(folders) = params["workspaceFolders"].as_array() {
            self.roots = folders
                .iter()
                .filter_map(|f| f["uri"].as_str().and_then(file_path))
                .collect();
        } else if let Some(root) = params["rootUri"].as_str().and_then(file_path) {
            self.roots.push(root);
        }
        if let Some(path) = params["initializationOptions"]["manifestPath"].as_str() {
            self.manifest_path = path.into();
        }
    }
    fn root(&self, source: Option<&Path>) -> Option<&Path> {
        self.roots
            .iter()
            .filter(|r| source.is_some_and(|s| s.starts_with(r)))
            .max_by_key(|r| r.components().count())
            .or_else(|| self.roots.first())
            .map(PathBuf::as_path)
    }
    fn manifest(&self, uri: &str) -> std::result::Result<Manifest, Diagnostic> {
        let source = file_path(uri);
        let root = self.root(source.as_deref());
        let configured = PathBuf::from(&self.manifest_path);
        let candidate = if !self.manifest_path.is_empty() {
            if configured.is_absolute() {
                Some(configured)
            } else {
                root.or_else(|| source.as_deref().and_then(Path::parent))
                    .map(|r| r.join(configured))
            }
        } else {
            let start = source.as_deref().and_then(Path::parent).or(root);
            let mut found = None;
            if let Some(start) = start {
                for dir in start.ancestors() {
                    let path = dir.join("manifest.json");
                    if path.is_file() {
                        found = Some(path);
                        break;
                    }
                    if Some(dir) == root {
                        break;
                    }
                }
            }
            found
        };
        let path = candidate.ok_or_else(|| Diagnostic::new("E_MANIFEST", Span { line:1,column:1 },
            "No primitive manifest found. Place manifest.json beside this source (or in a parent folder), or set tellurLanguage.manifestPath. Only syntax was checked."))?;
        let text = fs::read_to_string(&path).map_err(|e| {
            Diagnostic::new(
                "E_MANIFEST",
                Span { line: 1, column: 1 },
                format!(
                    "Cannot read manifest {}: {e}. Only syntax was checked.",
                    path.display()
                ),
            )
        })?;
        Manifest::from_json_at(&text, &path).map_err(|e| {
            Diagnostic::new(
                "E_MANIFEST",
                Span { line: 1, column: 1 },
                format!(
                    "Invalid manifest {}: {}. Only syntax was checked.",
                    path.display(),
                    e.message
                ),
            )
        })
    }
    fn checked(
        &self,
        uri: &str,
    ) -> std::result::Result<crate::CheckedProgram, crate::modules::ModuleError> {
        let path = file_path(uri).unwrap_or_else(|| PathBuf::from(uri));
        let manifest = self
            .manifest(uri)
            .map_err(|diagnostic| crate::modules::ModuleError {
                path: path.clone(),
                diagnostic,
            })?;
        self.checked_with_manifest(uri, manifest)
    }
    fn source_text(&self, path: &Path) -> std::io::Result<String> {
        let mut found: Option<&str> = None;
        for (uri, doc) in &self.documents {
            if let Some(open) = file_path(uri)
                && crate::modules::source_identity(&open).unwrap_or(open) == path
            {
                if found.is_some_and(|text| text != doc.text) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "conflicting open buffers refer to the same module file",
                    ));
                }
                found = Some(&doc.text);
            }
        }
        found
            .map(|text| Ok(text.to_owned()))
            .unwrap_or_else(|| fs::read_to_string(path))
    }
    fn checked_with_manifest(
        &self,
        uri: &str,
        manifest: Manifest,
    ) -> std::result::Result<crate::CheckedProgram, crate::modules::ModuleError> {
        self.check_diagnostics_with_manifest(uri, manifest)
            .map_err(crate::Diagnostics::first)
    }
    fn check_diagnostics_with_manifest(
        &self,
        uri: &str,
        manifest: Manifest,
    ) -> crate::CheckResult<crate::CheckedProgram, crate::modules::ModuleError> {
        let doc = &self.documents[uri];
        if let Some(path) = file_path(uri) {
            crate::modules::check_with_loader(&path, manifest, |requested| {
                self.source_text(requested)
            })
        } else {
            crate::check_source(&doc.text, manifest).map_err(|errors| {
                errors.map(|diagnostic| crate::modules::ModuleError {
                    path: PathBuf::from(uri),
                    diagnostic,
                })
            })
        }
    }
    fn module_diagnostic(&self, uri: &str, mut error: crate::modules::ModuleError) -> Value {
        let doc = &self.documents[uri];
        let root = file_path(uri).map(|p| crate::modules::source_identity(&p).unwrap_or(p));
        let mut related = error.diagnostic.notes.iter().filter_map(|note| {
            let (target_uri, text) = if let Some(path) = &note.path {
                let target_uri = if root.as_ref() == Some(path) {
                    uri.to_owned()
                } else {
                    self.documents.keys().find(|open_uri| {
                        file_path(open_uri).is_some_and(|p| crate::modules::source_identity(&p).unwrap_or(p) == *path)
                    }).cloned().or_else(|| Url::from_file_path(path).ok().map(|u| u.to_string()))?
                };
                (target_uri, self.source_text(path).unwrap_or_default())
            } else {
                (uri.to_owned(), doc.text.clone())
            };
            Some(json!({"location":{"uri":target_uri,"range":diagnostic_range(&text,note.span)},"message":note.message}))
        }).collect::<Vec<_>>();
        let mut result = if root.as_ref().is_some_and(|p| p != &error.path) {
            let target_uri = self
                .documents
                .keys()
                .find(|uri| {
                    file_path(uri).is_some_and(|p| {
                        crate::modules::source_identity(&p).unwrap_or(p) == error.path
                    })
                })
                .cloned()
                .or_else(|| Url::from_file_path(&error.path).ok().map(|u| u.to_string()));
            let text = self.source_text(&error.path).unwrap_or_default();
            let target_range = source_range(&text, &error.diagnostic);
            let message = format!(
                "Imported {}:{}:{}: {}",
                error.path.display(),
                error.diagnostic.span.line,
                error.diagnostic.span.column,
                error.diagnostic.message
            );
            error.diagnostic.span = syntax::parse_unit(&doc.text)
                .ok()
                .and_then(|u| u.imports.first().map(|i| i.span))
                .unwrap_or(Span { line: 1, column: 1 });
            error.diagnostic.message = message.clone();
            error.diagnostic.end = None;
            if let Some(target_uri) = target_uri {
                related.insert(
                    0,
                    json!({"location":{"uri":target_uri,"range":target_range},"message":message}),
                );
            }
            diagnostic(&doc.text, error.diagnostic, 1)
        } else {
            diagnostic(&doc.text, error.diagnostic, 1)
        };
        if !related.is_empty() {
            result["relatedInformation"] = json!(related);
        }
        result
    }
    pub fn diagnostics(&self, uri: &str) -> Value {
        let Some(doc) = self.documents.get(uri) else {
            return json!({"uri":uri,"diagnostics":[]});
        };
        let diagnostics = if let Err(e) = syntax::parse(&doc.text) {
            vec![diagnostic(&doc.text, e, 1)]
        } else {
            match self.manifest(uri) {
                Ok(manifest) => self
                    .check_diagnostics_with_manifest(uri, manifest)
                    .err()
                    .map(|mut errors| {
                        if errors.limit_reached
                            && let Some(last) = errors.errors.last_mut()
                        {
                            last.diagnostic.message.push_str(
                                " (diagnostic limit reached; remaining controls may be unchecked)",
                            );
                        }
                        errors
                            .errors
                            .into_iter()
                            .map(|e| self.module_diagnostic(uri, e))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                Err(e) => vec![diagnostic(&doc.text, e, 2)],
            }
        };
        json!({"uri":uri,"version":doc.version,"diagnostics":diagnostics})
    }
    pub fn symbols(&self, uri: &str) -> Value {
        let Some(doc) = self.documents.get(uri) else {
            return json!([]);
        };
        let Ok(unit) = syntax::parse_unit(&doc.text) else {
            return json!([]);
        };
        let checked = self.checked(uri).ok();
        let mut symbols = unit.regions.iter().map(|region| {
            let params = region.parameters.iter().map(|p|format!("{} {}",display_type(&p.ty),p.name)).collect::<Vec<_>>().join(", ");
            let exits = checked.as_ref().and_then(|p| p.region(&region.name).map(|r| checked_exit_description(&r.exits, region)).or_else(|| p.generic_controls().iter().find(|r| r.name == region.name).map(|r| checked_exit_description(&r.exits, region))))
                .unwrap_or_else(|| if region.infer_exits { "auto (unresolved)".into() } else { region.exits.iter().map(|e| format!("{}{}", e.name, if region.infer_payloads.contains(&e.name) {"... (unresolved)"} else {""})).collect::<Vec<_>>().join(" | ") });
            json!({"name":region.name,"detail":format!("{}({params}) exits {exits}",bounded_type_parameters(&region.type_parameters, &region.type_bounds)),"kind":12,
                "range":{"start":position(&doc.text,region.span),"end":position(&doc.text,Span{column:region.end.column+1,..region.end})},
                "selectionRange":diagnostic_range(&doc.text,region.name_span)})
        }).collect::<Vec<_>>();
        for data in &unit.data {
            let kind = if matches!(data.body, syntax::DataBody::Struct(_)) {
                23
            } else {
                10
            };
            symbols.push(json!({"name":data.name,"detail":data_description(data),"kind":kind,
                "range":{"start":position(&doc.text,data.span),"end":position(&doc.text,Span{column:data.end.column+1,..data.end})},
                "selectionRange":diagnostic_range(&doc.text,data.name_span)}));
        }
        for contract in &unit.contracts {
            let detail = checked
                .as_ref()
                .and_then(|p| p.manifest().outcomes(&contract.name))
                .map(|o| exit_description(&o))
                .unwrap_or_else(|| {
                    format!(
                        "outcome {}{}",
                        contract.name,
                        type_parameters(&contract.type_parameters)
                    )
                });
            symbols.push(json!({"name":contract.name,"detail":detail,"kind":10,
                "range":{"start":position(&doc.text,contract.span),"end":position(&doc.text,Span{column:contract.end.column+1,..contract.end})},
                "selectionRange":diagnostic_range(&doc.text,contract.name_span)}));
        }
        for declaration in &unit.traits {
            symbols.push(json!({"name":declaration.name,"detail":"trait: explicit static conformance","kind":11,
                "range":{"start":position(&doc.text,declaration.span),"end":position(&doc.text,Span{column:declaration.end.column+1,..declaration.end})},
                "selectionRange":diagnostic_range(&doc.text,declaration.name_span),
                "children":trait_members(&doc.text,&declaration.associated,&declaration.methods)}));
        }
        for implementation in &unit.implementations {
            symbols.push(json!({"name":format!("implement {} for {}",implementation.trait_name,display_type(&implementation.target)),"kind":19,
                "range":{"start":position(&doc.text,implementation.span),"end":position(&doc.text,Span{column:implementation.end.column+1,..implementation.end})},
                "selectionRange":diagnostic_range(&doc.text,implementation.span),
                "children":trait_members(&doc.text,&implementation.associated,&implementation.methods)}));
        }
        if let Some(program) = &checked {
            for symbol in &mut symbols {
                if let Some(detail) = symbol["detail"].as_str() {
                    symbol["detail"] = json!(source_spelling(uri, &unit, program, detail));
                }
            }
        }
        symbols.sort_by_key(|s| {
            (
                s["range"]["start"]["line"].as_u64(),
                s["range"]["start"]["character"].as_u64(),
            )
        });
        json!(symbols)
    }
    /// Declaration hover exposes the checked contract, including inferred payload types.
    /// Format the current buffer, including untitled files, without semantic checking.
    pub fn formatting(&self, uri: &str, options: crate::format::Options) -> Value {
        let Some(document) = self.documents.get(uri) else {
            return json!([]);
        };
        let Ok(formatted) = crate::format::source(&document.text, options) else {
            return json!([]);
        };
        if formatted == document.text {
            return json!([]);
        }
        let mut lines = document.text.split('\n');
        let last = lines.next_back().unwrap_or("");
        let end = json!({"line": lines.count(), "character": last.trim_end_matches('\r').encode_utf16().count()});
        json!([{"range":{"start":{"line":0,"character":0},"end":end},"newText":formatted}])
    }
    pub fn hover(&self, uri: &str, line: u64, character: u64) -> Value {
        let Some(doc) = self.documents.get(uri) else {
            return Value::Null;
        };
        let Ok(unit) = syntax::parse_unit(&doc.text) else {
            return Value::Null;
        };
        let hit = |span| {
            let r = diagnostic_range(&doc.text, span);
            r["start"]["line"] == line
                && character >= r["start"]["character"].as_u64().unwrap()
                && character < r["end"]["character"].as_u64().unwrap()
        };
        let auto_sites = syntax::auto_binding_spans(&doc.text).unwrap_or_default();
        if auto_sites.iter().any(|(_, keyword, _)| hit(*keyword)) {
            let Ok(program) = self.checked(uri) else {
                return Value::Null;
            };
            let root = file_path(uri).map(|p| crate::modules::source_identity(&p).unwrap_or(p));
            let offset = program
                .source_files()
                .iter()
                .find(|s| Some(&s.path) == root.as_ref())
                .map(|s| s.start_line - 1)
                .unwrap_or(0);
            for (start, keyword, name) in &auto_sites {
                if !hit(*keyword) {
                    continue;
                }
                let span = Span {
                    line: start.line + offset,
                    column: start.column,
                };
                if let Some(binding) = program
                    .inferred_bindings
                    .iter()
                    .find(|b| b.span == span && b.name == *name)
                {
                    let ty = source_spelling(uri, &unit, &program, &binding.display_type);
                    return json!({"contents":{"kind":"markdown","value":format!(
                        "```tellur\n{ty} {name}\n```\n\nInferred binding type from the checked source."
                    )},"range":diagnostic_range(&doc.text,*keyword)});
                }
            }
            return Value::Null;
        }
        let mut trait_hover = None;
        for d in &unit.traits {
            if hit(d.name_span) {
                trait_hover = Some((
                    d.name_span,
                    format!(
                        "trait {} {{\n{}\n}}",
                        d.name,
                        d.associated
                            .iter()
                            .map(|a| format!("    {}", associated_description(a)))
                            .chain(
                                d.methods
                                    .iter()
                                    .map(|m| format!("    {};", source_signature(m)))
                            )
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                ));
            }
            for a in &d.associated {
                if hit(a.span) {
                    trait_hover = Some((
                        a.span,
                        format!(
                            "{} // associated type of {}",
                            associated_description(a),
                            d.name
                        ),
                    ));
                }
            }
            for m in &d.methods {
                if hit(m.name_span) {
                    trait_hover = Some((m.name_span, source_signature(m)));
                }
            }
        }
        for i in &unit.implementations {
            if hit(i.span) {
                trait_hover = Some((
                    i.span,
                    format!("implement {} for {}", i.trait_name, display_type(&i.target)),
                ));
            }
            for a in &i.associated {
                if hit(a.span) {
                    trait_hover = Some((
                        a.span,
                        format!(
                            "type {} = {};",
                            a.name,
                            display_type(a.value.as_deref().unwrap_or("?"))
                        ),
                    ));
                }
            }
            for m in &i.methods {
                if hit(m.name_span) {
                    trait_hover = Some((m.name_span, source_signature(m)));
                }
            }
        }
        if let Some((at, description)) = trait_hover {
            if self.checked(uri).is_err() {
                return Value::Null;
            }
            return json!({"contents":{"kind":"markdown","value":format!("```tellur\n{description}\n```\n\nExplicit trait/type selection uses `trait_name<type>.member`. Implementations are checked for ownership, exit contracts, coherence and privacy; calls dispatch statically.")},"range":diagnostic_range(&doc.text,at)});
        }
        for data in &unit.data {
            let range = diagnostic_range(&doc.text, data.name_span);
            if range["start"]["line"] == line
                && character >= range["start"]["character"].as_u64().unwrap()
                && character < range["end"]["character"].as_u64().unwrap()
            {
                let Ok(program) = self.checked(uri) else {
                    return Value::Null;
                };
                let m = program.manifest();
                if !data.type_parameters.is_empty() {
                    return json!({"contents":{"kind":"markdown","value":format!("```tellur\n{}\n```\n\nGeneric nominal data. Copy and must-complete properties are derived from each specialization's fields. Type arguments are explicit.",data_description(data))},"range":range});
                }
                let description = format!(
                    "```tellur\n{}\n```\n\nNominal data type. Copy: {}. Must-complete contents: {}.\n\nOrdinary data has no outcome-handling obligation of its own; contained resources and outcomes retain theirs.",
                    data_description(data),
                    m.is_copy(&data.name),
                    m.linear(&data.name)
                );
                return json!({"contents":{"kind":"markdown","value":description},"range":range});
            }
        }
        for contract in &unit.contracts {
            let range = diagnostic_range(&doc.text, contract.name_span);
            if range["start"]["line"] == line
                && character >= range["start"]["character"].as_u64().unwrap()
                && character < range["end"]["character"].as_u64().unwrap()
            {
                let Ok(program) = self.checked(uri) else {
                    return Value::Null;
                };
                let outcomes = program
                    .manifest()
                    .outcomes(&contract.name)
                    .unwrap_or_else(|| {
                        contract
                            .outcomes
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
                    });
                let name = format!(
                    "{}{}",
                    contract.name,
                    type_parameters(&contract.type_parameters)
                );
                let (name, description) = if contract.origin_parameters.is_empty() {
                    (name, exit_description(&outcomes))
                } else {
                    let groups = contract
                        .origin_parameters
                        .iter()
                        .map(|g| format!("'{g}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let alternatives = contract
                        .outcomes
                        .iter()
                        .map(|o| {
                            if o.fields.is_empty() {
                                o.name.clone()
                            } else {
                                format!(
                                    "{} {{ {} }}",
                                    o.name,
                                    o.fields
                                        .iter()
                                        .map(|f| source_field(f, true))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    (format!("{name}[{groups}]"), alternatives)
                };
                let statuses = if contract.exit_codes.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n\nProcess exit defaults (only at the executable boundary): {}.",
                        contract
                            .exit_codes
                            .iter()
                            .map(|(name, code)| format!("`{name}` → `{code}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                return json!({"contents":{"kind":"markdown","value":format!("```tellur\noutcome {} {{ {} }}\n```\n\nA named closed contract. Every stored value must be handled or transferred, even with an empty payload.{}",name,description,statuses)},"range":range});
            }
        }
        let auto_exits = syntax::auto_exit_spans(&doc.text).unwrap_or_default();
        for region in &unit.regions {
            let at = auto_exits
                .iter()
                .copied()
                .find(|s| {
                    region.infer_exits
                        && hit(*s)
                        && (s.line, s.column) > (region.name_span.line, region.name_span.column)
                        && (s.line, s.column) < (region.end.line, region.end.column)
                })
                .unwrap_or(region.name_span);
            let range = diagnostic_range(&doc.text, at);
            if range["start"]["line"] != line
                || character < range["start"]["character"].as_u64().unwrap()
                || character >= range["end"]["character"].as_u64().unwrap()
            {
                continue;
            }
            let Ok(program) = self.checked(uri) else {
                return Value::Null;
            };
            if let Some(generic) = program
                .generic_controls()
                .iter()
                .find(|g| g.name == region.name)
            {
                let params = region
                    .parameters
                    .iter()
                    .map(|p| format!("{} {}", display_type(&p.ty), p.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                return json!({"contents":{"kind":"markdown","value":format!("```tellur\ncontrol {}{}({params}) exits {}\n```\n\nGeneric definition checked using only its declared capabilities. Concrete type arguments are explicit. Unconstrained values can be borrowed or transferred. Sealed bounds grant permissions: copy implies clone, and clone implies drop.",region.name,bounded_type_parameters(&region.type_parameters, &region.type_bounds),source_spelling(uri,&unit,&program,&checked_exit_description(&generic.exits,region)))},"range":range});
            }
            let Some(checked) = program.region(&region.name) else {
                return Value::Null;
            };
            let params = region
                .parameters
                .iter()
                .map(|p| format!("{} {}", display_type(&p.ty), p.name))
                .collect::<Vec<_>>()
                .join(", ");
            let inferred = if region.infer_exits || !region.infer_payloads.is_empty() {
                "\n\nInferred from the checked body. All outcomes still require explicit handling."
            } else {
                ""
            };
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
            let borrows = if borrows.is_empty() {
                borrows
            } else {
                format!("\n\nChecked return lifetimes:{borrows}")
            };
            let contract = region.exit_contract.as_ref().filter(|name| {
                matches!(
                    program.manifest().type_def(name),
                    Some(crate::manifest::TypeDef::Outcome { .. })
                )
            });
            let exits = contract
                .map(|name| {
                    if region.exit_origins.is_empty() {
                        name.clone()
                    } else {
                        format!(
                            "{name}[{}]",
                            region
                                .exit_origins
                                .iter()
                                .map(|o| o.display())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                })
                .unwrap_or_else(|| checked_exit_description(&checked.exits, region));
            let details = contract
                .map(|_| {
                    format!(
                        "\n\nContract alternatives: `{}`.",
                        exit_description(&checked.exits)
                    )
                })
                .unwrap_or_default();
            return json!({"contents":{"kind":"markdown","value":format!("```tellur\ncontrol {}({params}) exits {exits}\n```{inferred}{details}{borrows}",region.name)},"range":range});
        }
        self.selection_hover(uri, &unit, line, character)
    }
    fn selection_hover(&self, uri: &str, unit: &syntax::Unit, line: u64, character: u64) -> Value {
        let doc = &self.documents[uri];
        let Some(at) = scalar_span(&doc.text, line, character) else {
            return Value::Null;
        };
        let Some(site) = syntax::selection_site_at(unit, &doc.text, at) else {
            return Value::Null;
        };
        let Ok(program) = self.checked(uri) else {
            return Value::Null;
        };
        let root = file_path(uri).map(|p| crate::modules::source_identity(&p).unwrap_or(p));
        let offset = program
            .source_files()
            .iter()
            .find(|s| Some(&s.path) == root.as_ref())
            .map(|s| s.start_line - 1)
            .unwrap_or(0);
        let anchor = Span {
            line: site.anchor.line + offset,
            column: site.anchor.column,
        };
        let Some(history) = program
            .selected_histories
            .iter()
            .find(|h| h.span == anchor && h.kind == site.kind)
        else {
            return Value::Null;
        };
        json!({"contents":{"kind":"markdown","value":history.markdown(offset)},
            "range":{"start":position(&doc.text,site.range.start),"end":position(&doc.text,site.range.end)}})
    }
    /// Returns an immediate publication only for close; other changes are debounced.
    pub fn notification(&mut self, method: &str, params: &Value) -> Option<Value> {
        match method {
            "textDocument/didOpen" => {
                let doc = &params["textDocument"];
                if let (Some(uri), Some(text), Some(version)) = (
                    doc["uri"].as_str(),
                    doc["text"].as_str(),
                    doc["version"].as_i64(),
                ) {
                    self.documents.insert(
                        uri.into(),
                        Document {
                            text: text.into(),
                            version,
                        },
                    );
                    self.dirty = true;
                }
            }
            "textDocument/didChange" => {
                let doc = &params["textDocument"];
                if let (Some(uri), Some(version), Some(changes)) = (
                    doc["uri"].as_str(),
                    doc["version"].as_i64(),
                    params["contentChanges"].as_array(),
                ) && let Some(current) = self.documents.get_mut(uri)
                    && version > current.version
                {
                    // We advertise full-document synchronization; do not treat a ranged patch as a full document.
                    if let Some(change) = changes.last().filter(|c| c.get("range").is_none())
                        && let Some(text) = change["text"].as_str()
                    {
                        current.text = text.into();
                        current.version = version;
                        self.dirty = true;
                    }
                }
            }
            "textDocument/didClose" => {
                if let Some(uri) = params["textDocument"]["uri"].as_str() {
                    self.documents.remove(uri);
                    self.dirty = true;
                    return Some(json!({"uri":uri,"diagnostics":[]}));
                }
            }
            "textDocument/didSave" | "workspace/didChangeWatchedFiles" => self.dirty = true,
            "workspace/didChangeConfiguration" => {
                let settings = params["settings"]
                    .get("tellurLanguage")
                    .or_else(|| params["settings"].get("pathLanguage"))
                    .unwrap_or(&params["settings"]);
                if let Some(path) = settings["manifestPath"].as_str() {
                    self.manifest_path = path.into();
                }
                self.dirty = true;
            }
            _ => {}
        }
        None
    }
    fn publish(
        &mut self,
        connection: &Connection,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        if self.dirty {
            for uri in self.documents.keys() {
                connection.sender.send(
                    Notification::new(
                        "textDocument/publishDiagnostics".into(),
                        self.diagnostics(uri),
                    )
                    .into(),
                )?;
            }
            self.dirty = false;
        }
        Ok(())
    }
}
fn data_description(data: &syntax::DataDeclaration) -> String {
    let groups = if data.origin_parameters.is_empty() {
        String::new()
    } else {
        format!(
            "[{}]",
            data.origin_parameters
                .iter()
                .map(|n| format!("'{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let name = format!(
        "{}{}{groups}",
        data.name,
        type_parameters(&data.type_parameters)
    );
    match &data.body {
        syntax::DataBody::Struct(fields) => format!(
            "struct {} {{ {} }}",
            name,
            fields
                .iter()
                .map(|f| format!("{};", source_field(f, true)))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        syntax::DataBody::Variant(alternatives) => format!(
            "variant {} {{ {} }}",
            name,
            alternatives
                .iter()
                .map(|o| if o.fields.is_empty() {
                    o.name.clone()
                } else {
                    format!(
                        "{} {{ {} }}",
                        o.name,
                        o.fields
                            .iter()
                            .map(|f| source_field(f, true))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
fn exit_description(exits: &[crate::manifest::Outcome]) -> String {
    exit_description_with_origins(exits, &Default::default())
}
fn exit_description_with_origins(
    exits: &[crate::manifest::Outcome],
    origins: &std::collections::BTreeMap<(String, usize), String>,
) -> String {
    exits
        .iter()
        .map(|o| {
            if o.fields.is_empty() {
                o.name.clone()
            } else {
                format!(
                    "{} {{ {} }}",
                    o.name,
                    o.fields
                        .iter()
                        .enumerate()
                        .map(|(i, f)| format!(
                            "{} {}{}",
                            display_type(&f.ty),
                            f.name,
                            origins
                                .get(&(o.name.clone(), i))
                                .map(|p| format!(" from {p}"))
                                .unwrap_or_default()
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
fn request(server: &Server, request: Request) -> Response {
    match request.method.as_str() {
        "textDocument/formatting" => match (
            request.params["textDocument"]["uri"].as_str(),
            request.params["options"]["tabSize"].as_u64(),
            request.params["options"]["insertSpaces"].as_bool(),
        ) {
            (Some(uri), Some(tab_size @ 1..=16), Some(insert_spaces)) => Response::new_ok(
                request.id,
                server.formatting(
                    uri,
                    crate::format::Options {
                        tab_size: tab_size as usize,
                        insert_spaces,
                    },
                ),
            ),
            _ => Response::new_err(
                request.id,
                -32602,
                "formatting needs a document URI, tabSize from 1 through 16, and insertSpaces"
                    .into(),
            ),
        },
        "textDocument/documentSymbol" => match request.params["textDocument"]["uri"].as_str() {
            Some(uri) => Response::new_ok(request.id, server.symbols(uri)),
            None => Response::new_err(request.id, -32602, "missing document URI".into()),
        },
        "textDocument/hover" => match (
            request.params["textDocument"]["uri"].as_str(),
            request.params["position"]["line"].as_u64(),
            request.params["position"]["character"].as_u64(),
        ) {
            (Some(uri), Some(line), Some(character)) => {
                Response::new_ok(request.id, server.hover(uri, line, character))
            }
            _ => Response::new_err(
                request.id,
                -32602,
                "missing document URI or position".into(),
            ),
        },
        _ => Response::new_err(
            request.id,
            -32601,
            format!("method not supported: {}", request.method),
        ),
    }
}
pub fn serve(connection: Connection) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (id, params) = connection.initialize_start()?;
    let mut server = Server::default();
    server.initialize(&params);
    connection.initialize_finish(id,json!({"capabilities":{
        "positionEncoding":"utf-16", "textDocumentSync":{"openClose":true,"change":1,"save":{"includeText":false}},
        "documentSymbolProvider":true, "hoverProvider":true, "documentFormattingProvider":true
    },"serverInfo":{"name":"tellur-lsp","version":env!("CARGO_PKG_VERSION")}}))?;
    let mut last_publication = Instant::now();
    loop {
        match connection.receiver.recv_timeout(Duration::from_millis(150)) {
            Ok(Message::Request(r)) => {
                if connection.handle_shutdown(&r)? {
                    break;
                }
                connection.sender.send(request(&server, r).into())?;
            }
            Ok(Message::Notification(n)) => {
                if n.method == "exit" {
                    return Err("exit received without shutdown".into());
                }
                if let Some(params) = server.notification(&n.method, &n.params) {
                    connection.sender.send(
                        Notification::new("textDocument/publishDiagnostics".into(), params).into(),
                    )?;
                }
            }
            Ok(Message::Response(_)) => {}
            Err(e) if e.is_timeout() => {
                server.publish(&connection)?;
                last_publication = Instant::now();
            }
            Err(_) => break,
        }
        // Avoid starving diagnostics while a client continuously sends changes.
        if last_publication.elapsed() >= Duration::from_millis(750) {
            server.publish(&connection)?;
            last_publication = Instant::now();
        }
    }
    Ok(())
}

fn type_parameters(parameters: &[String]) -> String {
    if parameters.is_empty() {
        String::new()
    } else {
        format!("<{}>", parameters.join(", "))
    }
}

fn bounded_type_parameters(
    parameters: &[String],
    bounds: &std::collections::BTreeMap<String, Vec<String>>,
) -> String {
    type_parameters(
        &parameters
            .iter()
            .map(|p| match bounds.get(p) {
                Some(bs) if !bs.is_empty() => format!("{p}: {}", bs.join(" + ")),
                _ => p.clone(),
            })
            .collect::<Vec<_>>(),
    )
}

fn source_signature(r: &syntax::Region) -> String {
    let parameters = r
        .parameters
        .iter()
        .map(|p| format!("{} {}", display_type(&p.ty), p.name))
        .collect::<Vec<_>>()
        .join(", ");
    let exits = r
        .exit_contract
        .as_ref()
        .map(|t| display_type(t))
        .unwrap_or_else(|| {
            r.exits
                .iter()
                .map(|o| {
                    if r.infer_payloads.contains(&o.name) {
                        format!("{}...", o.name)
                    } else if o.fields.is_empty() {
                        o.name.clone()
                    } else {
                        format!(
                            "{} {{ {} }}",
                            o.name,
                            o.fields
                                .iter()
                                .enumerate()
                                .map(|(i, f)| format!(
                                    "{} {}{}",
                                    field_type(f),
                                    f.name,
                                    r.return_origins
                                        .get(&(o.name.clone(), i))
                                        .map(|p| format!(" from {p}"))
                                        .unwrap_or_default()
                                ))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        });
    format!("control {}({parameters}) exits {exits}", r.name)
}
fn trait_members(
    text: &str,
    associated: &[syntax::AssociatedType],
    methods: &[syntax::Region],
) -> Vec<Value> {
    let mut children=associated.iter().map(|a|json!({"name":a.name,"kind":26,"detail":a.value.as_ref().map(|v|display_type(v)).unwrap_or_else(||"associated type".into()),"range":diagnostic_range(text,a.span),"selectionRange":diagnostic_range(text,a.span)})).collect::<Vec<_>>();
    children.extend(methods.iter().map(|m|json!({"name":m.name,"kind":6,"detail":source_signature(m),"range":{"start":position(text,m.span),"end":position(text,Span{column:m.end.column+1,..m.end})},"selectionRange":diagnostic_range(text,m.name_span)})));
    children
}

fn associated_description(a: &syntax::AssociatedType) -> String {
    let bounds = if a.bounds.is_empty() {
        String::new()
    } else {
        format!(": {}", a.bounds.join(" + "))
    };
    let value = a
        .value
        .as_ref()
        .map(|v| format!(" = {}", display_type(v)))
        .unwrap_or_default();
    format!("type {}{bounds}{value};", a.name)
}

fn source_spelling(
    uri: &str,
    unit: &syntax::Unit,
    program: &crate::CheckedProgram,
    text: &str,
) -> String {
    let mut text = text.to_owned();
    if let Some(root) = file_path(uri)
        && let Some(parent) = root.parent()
    {
        for import in &unit.imports {
            if let Ok(target) = crate::modules::source_identity(&parent.join(&import.path)) {
                for source in program.source_files().iter().filter(|s| s.path == target) {
                    for (global, local) in &source.names {
                        if global != local {
                            text = text.replace(global, &format!("{}.{local}", import.alias));
                        }
                    }
                }
            }
        }
    }
    text
}

fn field_type(field: &syntax::Field) -> String {
    let ty = display_type(&field.ty);
    if field.origins.is_empty() {
        ty
    } else {
        format!(
            "{ty}[{}]",
            field
                .origins
                .iter()
                .map(syntax::OriginArgument::display)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}
fn source_field(field: &syntax::Field, declaration: bool) -> String {
    if declaration
        && crate::manifest::reference_inner(&field.ty).is_some()
        && field.origins.len() == 1
    {
        format!(
            "{} {} <- {}",
            if field.referent_origins.is_empty() {
                display_type(&field.ty)
            } else {
                format!(
                    "{}[{}]",
                    display_type(&field.ty),
                    field
                        .referent_origins
                        .iter()
                        .map(syntax::OriginArgument::display)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
            field.name,
            field.origins[0].display()
        )
    } else {
        format!("{} {}", field_type(field), field.name)
    }
}

fn checked_exit_description(
    checked: &[crate::manifest::Outcome],
    region: &syntax::Region,
) -> String {
    let mut exits = checked.to_vec();
    for o in &mut exits {
        if let Some(source) = region.exits.iter().find(|s| s.name == o.name) {
            for (f, source) in o.fields.iter_mut().zip(&source.fields) {
                if !source.origins.is_empty() {
                    f.ty = format!(
                        "{}[{}]",
                        f.ty,
                        source
                            .origins
                            .iter()
                            .map(syntax::OriginArgument::display)
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
        }
    }
    exit_description_with_origins(&exits, &region.return_origins)
}
