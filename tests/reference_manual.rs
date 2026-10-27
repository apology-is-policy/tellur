//! Keep the user manual's complete examples and local navigation checked.
use std::{collections::BTreeMap, fs, path::PathBuf};
use tellur::{compile, manifest::Manifest};

fn manual_files() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![root.join("docs/language-reference.md")];
    let mut pending = vec![root.join("docs/reference")];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "md") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[test]
fn examples_have_explicit_status_and_match_the_compiler() {
    let mut accepted = 0;
    let mut rejected = 0;
    let mut fragments = 0;
    let mut failures = vec![];
    for file in manual_files() {
        let text = fs::read_to_string(&file).unwrap();
        let lines: Vec<_> = text.lines().collect();
        let mut cursor = 0;
        while cursor < lines.len() {
            let start = cursor;
            let fence = lines[cursor].trim();
            cursor += 1;
            if !fence.starts_with("```") {
                continue;
            }
            while cursor < lines.len() && lines[cursor].trim() != "```" {
                cursor += 1;
            }
            assert!(
                cursor < lines.len(),
                "{}:{}: unclosed fence",
                file.display(),
                start + 1
            );
            let end = cursor;
            cursor += 1;
            if fence != "```tellur" {
                continue;
            }
            let location = format!("{}:{}", file.display(), start + 1);
            let mode = start
                .checked_sub(1)
                .and_then(|i| lines[i].strip_prefix("<!-- tellur:"))
                .and_then(|line| line.strip_suffix(" -->"))
                .unwrap_or_else(|| panic!("{location}: Tellur example needs a status annotation"));
            if let Some(reason) = mode.strip_prefix("fragment ") {
                assert!(
                    reason.split_whitespace().count() >= 3,
                    "{location}: explain the required context"
                );
                assert!(
                    start >= 3 && lines[start - 3].starts_with("_Context fragment:"),
                    "{location}: show the fragment context to the reader"
                );
                fragments += 1;
                continue;
            }
            let source = lines[start + 1..end].join("\n");
            let source = match mode {
                "body" => {
                    format!("control main() exits auto {{\n{source}\n[entry]: exit done;\n}}")
                }
                "tail" => format!("control main() exits auto {{\n{source}\n}}"),
                "check" => source,
                _ if mode.starts_with("reject ") => source,
                _ => panic!("{location}: unknown example mode {mode}"),
            };
            let manifest =
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
            let result = compile(&source, manifest);
            if let Some(code) = mode.strip_prefix("reject ") {
                rejected += 1;
                match result {
                    Err(error) if error.code == code => {}
                    Err(error) => {
                        failures.push(format!("{location}: expected {code}, got {error}"))
                    }
                    Ok(_) => failures.push(format!(
                        "{location}: expected {code}, but compilation succeeded"
                    )),
                }
            } else {
                accepted += 1;
                if let Err(error) = result {
                    failures.push(format!("{location}: {error}"));
                }
            }
        }
    }
    eprintln!(
        "Manual: {accepted} accepted examples, {rejected} expected diagnostics, {fragments} context fragments"
    );
    assert!(
        accepted > 0 && rejected > 0,
        "manual needs positive and negative examples"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn without_fences(text: &str) -> String {
    let mut fenced = false;
    text.lines()
        .filter(|line| {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                false
            } else {
                !fenced
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn anchors(text: &str) -> Vec<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    without_fences(text)
        .lines()
        .filter_map(|line| {
            let heading = line
                .strip_prefix('#')?
                .trim_start_matches('#')
                .strip_prefix(' ')?;
            // GitHub-style slugs for the plain headings used in this manual.
            let slug: String = heading
                .to_lowercase()
                .chars()
                .filter_map(|c| {
                    if c.is_alphanumeric() || c == '_' || c == '-' {
                        Some(c)
                    } else if c == ' ' {
                        Some('-')
                    } else {
                        None
                    }
                })
                .collect();
            let count = counts.entry(slug.clone()).or_default();
            let result = if *count == 0 {
                slug
            } else {
                format!("{slug}-{count}")
            };
            *count += 1;
            Some(result)
        })
        .collect()
}

#[test]
fn local_links_and_heading_anchors_resolve() {
    let mut failures = vec![];
    let mut checked = 0;
    for file in manual_files() {
        let text = without_fences(&fs::read_to_string(&file).unwrap());
        for suffix in text.split("](").skip(1) {
            let Some((link, _)) = suffix.split_once(')') else {
                continue;
            };
            if link.contains("://") || link.starts_with("mailto:") {
                continue;
            }
            let (relative, anchor) = link
                .split_once('#')
                .map_or((link, None), |(p, a)| (p, Some(a)));
            let target = if relative.is_empty() {
                file.clone()
            } else {
                file.parent().unwrap().join(relative)
            };
            checked += 1;
            if !target.is_file() {
                failures.push(format!("{}: missing link target {link}", file.display()));
            } else if let Some(anchor) = anchor
                && !anchors(&fs::read_to_string(&target).unwrap())
                    .iter()
                    .any(|a| a == anchor)
            {
                failures.push(format!("{}: missing heading in {link}", file.display()));
            }
        }
    }
    eprintln!("Manual: {checked} local links checked");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn manual_targets_the_current_compiler_version() {
    let reference = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/language-reference.md"),
    )
    .unwrap();
    let marker = format!(
        "<!-- tellur-reference-version: {} -->",
        env!("CARGO_PKG_VERSION")
    );
    assert!(
        reference.contains(&marker),
        "update the manual's version and feature status at release time"
    );
}
