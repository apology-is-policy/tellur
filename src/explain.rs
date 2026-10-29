//! Checked, bounded source queries. Reports are observations, never execution certificates.
use crate::{CheckedProgram, SourceRange, Span, absorption, manifest::Manifest, modules, syntax};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Selection coordinates are one-based Unicode scalar positions in the entry file.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Query {
    Selection { line: usize, column: usize },
    Outcome { control: String, outcome: String },
}
#[derive(Clone, Debug, Serialize)]
pub struct DecisionArm {
    pub name: String,
    pub outcome: String,
    pub implicit: bool,
    pub location: Span,
}
#[derive(Clone, Debug, Serialize)]
pub struct SelectionLimits {
    pub condition_nodes: usize,
    pub condition_depth: usize,
    pub decision_arms: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct Selection {
    pub kind: String,
    pub condition: String,
    pub structurally_empty: bool,
    pub iteration: Option<String>,
    pub decision_arms: Vec<DecisionArm>,
    pub condition_truncated: bool,
    pub origins_truncated: bool,
    pub limits: SelectionLimits,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Details {
    Selection {
        range: SourceRange,
        selection: Selection,
    },
    Outcome {
        control: String,
        outcome: String,
        evidence: absorption::Evidence,
        analysis: absorption::Analysis,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub format: String,
    pub version: u32,
    pub source: PathBuf,
    /// Navigation targets, not portable snapshot identities or ABI names.
    pub modules: BTreeMap<String, PathBuf>,
    pub query: Query,
    pub result: Details,
    pub markdown: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Target {
    pub control: String,
    pub outcome: String,
}

/// Read and check one coherent source graph before answering the query.
pub fn file(path: impl AsRef<Path>, manifest: Manifest, query: Query) -> Result<Report, String> {
    with_loader(path, manifest, query, |p| std::fs::read_to_string(p))
}
pub fn with_loader(
    path: impl AsRef<Path>,
    manifest: Manifest,
    query: Query,
    mut read: impl FnMut(&Path) -> std::io::Result<String>,
) -> Result<Report, String> {
    let mut texts = BTreeMap::new();
    let program = modules::check_with_loader(path, manifest, |p| {
        let text = read(p)?;
        texts.insert(p.to_owned(), text.clone());
        Ok(text)
    })
    .map_err(|e| e.to_string())?;
    let source = &program.source_files()[0].path;
    checked(&program, &texts[source], query)
}

/// Ordinary concrete controls declared in the entry file, including private ones.
pub fn targets(program: &CheckedProgram) -> Vec<Target> {
    let Some(source) = program.source_files().first() else {
        return vec![];
    };
    let mut result = vec![];
    for (internal, local) in &source.names {
        if let Some(region) = program.region(internal) {
            result.extend(region.exits.iter().map(|o| Target {
                control: local.clone(),
                outcome: o.name.clone(),
            }));
        }
    }
    result.sort_by(|a, b| (&a.control, &a.outcome).cmp(&(&b.control, &b.outcome)));
    result
}
fn link(path: &Path, span: Span) -> String {
    let mut uri = url::Url::from_file_path(path).expect("checked source paths are absolute");
    // Markdown editors disagree on column encoding; link to the exact line and
    // show the compiler scalar column in the label.
    uri.set_fragment(Some(&format!("L{}", span.line)));
    format!("[source {}:{}]({uri})", span.line, span.column)
}
pub(crate) fn checked(
    program: &CheckedProgram,
    text: &str,
    query: Query,
) -> Result<Report, String> {
    let index = absorption::SourceIndex::new(program).map_err(|e| e.to_string())?;
    let source = &program.source_files()[0];
    let modules: BTreeMap<_, _> = index
        .modules
        .iter()
        .cloned()
        .zip(program.source_files().iter().map(|s| s.path.clone()))
        .collect();
    let (result, markdown) = match &query {
        Query::Selection { line, column } => {
            if *line == 0 || *column == 0 {
                return Err("selection positions are one-based line:column coordinates".into());
            }
            let at = Span {
                line: *line,
                column: *column,
            };
            let unit = syntax::parse_unit(text).map_err(|e| e.to_string())?;
            let site = syntax::selection_site_at(&unit, text, at)
                .ok_or("no path selection at this position; select a statement, path label, alias or delta arm")?;
            let anchor = Span {
                line: site.anchor.line + source.start_line - 1,
                column: site.anchor.column,
            };
            let history = program
                .selected_histories
                .iter()
                .find(|h| h.span == anchor && h.kind == site.kind)
                .ok_or("no checked history is available for this selection")?;
            let selection = history.summary(source.start_line - 1);
            let mut markdown = format!(
                "# Tellur path selection\n\n{}\n\n{}\n",
                link(&source.path, site.range.start),
                history.markdown(source.start_line - 1)
            );
            if !selection.decision_arms.is_empty() {
                markdown.push_str("\n## Decision source links\n\n");
                for arm in &selection.decision_arms {
                    markdown.push_str(&format!(
                        "- `{}`: {}\n",
                        arm.name,
                        link(&source.path, arm.location)
                    ));
                }
            }
            markdown.push_str(&format!("\nLimits: {} captured condition nodes, depth {}, {} displayed decision origins. This report is fixed at the time of the request.\n", selection.limits.condition_nodes, selection.limits.condition_depth, selection.limits.decision_arms));
            (
                Details::Selection {
                    range: site.range,
                    selection,
                },
                markdown,
            )
        }
        Query::Outcome { control, outcome } => {
            let internal = source.names.iter().find(|(_, local)| *local == control).map(|(global,_)| global)
                .ok_or_else(|| format!("no control `{control}` is declared in this source file; query an imported file directly"))?;
            if program
                .generic_controls()
                .iter()
                .any(|g| &g.name == internal)
            {
                return Err("generic definition root contributor queries are not supported yet; query a concrete calling control (generic source selections are available)".into());
            }
            if program.region(internal).is_none() {
                return Err(format!("`{control}` is not an ordinary concrete control"));
            }
            let evidence = index
                .outcomes(internal)
                .map_err(|e| e.to_string())?
                .remove(outcome)
                .ok_or_else(|| format!("control `{control}` has no outcome `{outcome}`"))?;
            let name = index.name(internal);
            let mut markdown = format!(
                "# Tellur outcome contributors\n\nControl `{name}`, outcome `{outcome}`.\n\nRecorded structural contributors (a set, not an execution sequence):\n\n"
            );
            for c in &evidence.contributors {
                let action = match c.kind {
                    absorption::Kind::Declared => "declared",
                    absorption::Kind::Produced => "produced",
                    absorption::Kind::Constructed => "constructed",
                    absorption::Kind::Received => "received",
                    absorption::Kind::Forwarded => "forwarded",
                    absorption::Kind::Omitted => unreachable!(),
                };
                let operation = c
                    .operation
                    .as_ref()
                    .map(|op| format!(" from `{op}`"))
                    .unwrap_or_default();
                let at = link(
                    &modules[&c.location.module],
                    Span {
                        line: c.location.line,
                        column: c.location.column,
                    },
                );
                markdown.push_str(&format!(
                    "- `{}` {action}{operation} in `{}` — {at} (`{}`).\n",
                    c.outcome, c.control, c.site
                ));
            }
            if evidence.contributors.is_empty() {
                markdown.push_str(
                    "No contributors were recorded. This does not establish unreachability.\n",
                );
            }
            if evidence.truncated {
                markdown.push_str("\n**Truncated: additional contributors were omitted. Absence from this report proves nothing.**\n");
            }
            markdown.push_str(&format!("\nAt most {} contributors per outcome. This evidence does not prove feasible inputs, complete execution histories, or behavioral equivalence. Loop-carried values, indirect storage, dynamic collection contents, cross-call payload lineage and uninstantiated generic roots are outside this analysis. Declarations are contract facts, not evidence that an outcome is produced. The report is fixed at the time of the request.\n", absorption::LIMIT));
            (
                Details::Outcome {
                    control: name,
                    outcome: outcome.clone(),
                    evidence,
                    analysis: Default::default(),
                },
                markdown,
            )
        }
    };
    Ok(Report {
        format: "tellur.explanation".into(),
        version: 1,
        source: source.path.clone(),
        modules,
        query,
        result,
        markdown,
    })
}
