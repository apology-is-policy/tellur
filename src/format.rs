//! Parser-directed whitespace formatting. No punctuation or comments are rewritten.
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Diagnostic, Result, Span,
    syntax::{self, Kind, Token},
};

#[derive(Default)]
pub(crate) struct Layout {
    pub blocks: BTreeSet<usize>,
    pub starts: BTreeSet<usize>,
    pub indents: Vec<(usize, usize)>,
    pub angles: Vec<(usize, usize)>,
    pub label_statements: Vec<(usize, usize)>,
    pub contracts: Vec<(usize, usize)>,
    pub conditions: Vec<(usize, usize)>,
    pub arm_results: Vec<(usize, usize)>,
}

/// Indentation settings shared by the command line and language server.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub tab_size: usize,
    pub insert_spaces: bool,
    /// Soft target in Unicode scalar columns; tabs advance to a tab stop.
    pub max_width: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            tab_size: 4,
            insert_spaces: true,
            max_width: 100,
        }
    }
}

struct Source<'a> {
    text: &'a str,
    offsets: Vec<Vec<usize>>,
}
impl<'a> Source<'a> {
    fn new(text: &'a str) -> Self {
        let mut offsets = vec![vec![]];
        for (byte, c) in text.char_indices() {
            offsets.last_mut().unwrap().push(byte);
            if c == '\n' {
                offsets.push(vec![]);
            }
        }
        offsets.last_mut().unwrap().push(text.len());
        Self { text, offsets }
    }
    fn offset(&self, at: Span) -> usize {
        self.offsets[at.line - 1][at.column - 1]
    }
    fn raw(&self, token: &Token) -> &'a str {
        &self.text[self.offset(token.span)..self.offset(token.end)]
    }
}

struct Writer<'a> {
    text: String,
    newline: &'a str,
    indent: String,
    tab_size: usize,
    row: usize,
    column: usize,
}
impl Writer<'_> {
    fn append(&mut self, text: &str) {
        for c in text.chars() {
            match c {
                '\n' => {
                    self.row += 1;
                    self.column = 0;
                }
                '\r' => {}
                '\t' => self.column += self.tab_size - self.column % self.tab_size,
                _ => self.column += 1,
            }
        }
        self.text.push_str(text);
    }
    fn line(&mut self, count: usize) {
        if self.text.is_empty() {
            return;
        }
        let existing = self
            .text
            .chars()
            .rev()
            .take_while(|c| *c == '\r' || *c == '\n')
            .filter(|c| *c == '\n')
            .count();
        for _ in existing..count {
            self.append(self.newline);
        }
    }
    fn put(&mut self, text: &str, depth: usize, space: bool) {
        if self.text.is_empty() || self.text.ends_with('\n') {
            for _ in 0..depth {
                self.text.push_str(&self.indent);
                self.column += self.tab_size;
            }
        } else if space {
            self.append(" ");
        }
        self.append(text);
    }
    fn trivia(&mut self, gap: &str, depth: usize) {
        let mut rest = gap;
        while let Some(start) = rest.find("//") {
            let before = &rest[..start];
            let lines = before.bytes().filter(|b| *b == b'\n').count();
            if lines > 0 {
                self.line(lines.min(2));
            }
            let end = rest[start..].find('\n').map_or(rest.len(), |n| start + n);
            // CR belongs to the line delimiter, not the comment's spelling.
            let comment = rest[start..end]
                .strip_suffix('\r')
                .unwrap_or(&rest[start..end]);
            self.put(comment, depth, lines == 0);
            self.line(1);
            rest = &rest[end..];
        }
        if rest.bytes().filter(|b| *b == b'\n').count() > 1 {
            self.line(2);
        }
    }
}

fn space_before(previous: &str, current: &str, i: usize, angles: &BTreeSet<usize>) -> bool {
    if [",", ";", ")", "]", ".", ":", "..."].contains(&current)
        || ["(", "[", ".", "'", "@", "!"].contains(&previous)
        || (current == "}" && previous == "{")
        || (current == "<" && angles.contains(&i))
        || (current == ">" && angles.contains(&i))
        || (previous == "<" && angles.contains(&(i - 1)))
    {
        return false;
    }
    if current == "(" {
        if previous == ">" && angles.contains(&(i - 1)) {
            return false;
        }
        return [
            "delta", "Δ", "&", "|", "=", "=>", "&&", "||", "!=", "==", "<", ">", "<=", ">=",
            "borrow", "deref", "mutable",
        ]
        .contains(&previous);
    }
    if current == "[" {
        return ["=", "=>", "->", ",", ":", "(", "{", "contingency"].contains(&previous);
    }
    true
}

/// A list or condition can wrap only at token boundaries already permitted by
/// the parser. Contract lists have a virtual delimiter at `exits` and the body.
struct Group {
    open: usize,
    end: usize,
    breaks: BTreeSet<usize>,
    contract: bool,
    condition: bool,
}

fn groups(
    raw: &[&str],
    gaps: &[&str],
    layout: &Layout,
    angles: &BTreeSet<usize>,
) -> (Vec<Group>, BTreeSet<usize>) {
    let mut pairs = BTreeMap::new();
    let mut parent = vec![None; raw.len()];
    let mut stack = vec![];
    for (i, token) in raw.iter().enumerate() {
        parent[i] = stack.last().copied();
        if ["}", "]", ")"].contains(token) || (*token == ">" && angles.contains(&i)) {
            if let Some(open) = stack.pop() {
                pairs.insert(open, i);
            }
        } else if ["{", "[", "("].contains(token) || (*token == "<" && angles.contains(&i)) {
            stack.push(i);
        }
    }
    let mut result = vec![];
    let mut expanded = BTreeSet::new();
    for (open, end, contract) in pairs
        .into_iter()
        .filter(|(open, end)| !layout.blocks.contains(open) && *end > *open + 1)
        .map(|(open, end)| (open, end, false))
        .chain(
            layout
                .contracts
                .iter()
                .map(|(open, end)| (*open, *end, true)),
        )
    {
        let direct = |i: usize| {
            if contract {
                parent[i] == parent[open]
            } else {
                parent[i] == Some(open)
            }
        };
        let mut breaks: BTreeSet<usize> = (open + 1..end)
            .filter(|&i| direct(i) && (raw[i] == "&&" || raw[i] == "||"))
            .collect();
        // Conditions keep the first operand beside their opener and put logical
        // operators at the start of continuation lines. Lists put each item on
        // its own line, including a single long/nested argument.
        if breaks.is_empty() && !contract {
            breaks.insert(open + 1);
        }
        for (i, token) in raw.iter().enumerate().take(end).skip(open + 1) {
            if direct(i) && *token == "," {
                breaks.insert(i + 1);
            }
        }
        if !contract || raw[end] != ";" {
            breaks.insert(end);
        }
        let authored = (open + 1..=end).any(|i| (contract || direct(i)) && gaps[i].contains('\n'));
        if authored {
            expanded.insert(result.len());
        }
        result.push(Group {
            open,
            end,
            breaks,
            contract,
            condition: false,
        });
    }
    for &(open, end) in &layout.conditions {
        // A sole parenthesized condition already owns a delimiter continuation.
        if parent[open].is_some_and(|p| {
            p + 1 == open && parent[end] == Some(p) && [")", "]", "}"].contains(&raw[end])
        }) {
            continue;
        }
        let breaks: BTreeSet<_> = (open + 1..end)
            .filter(|&i| parent[i] == parent[open] && ["&&", "||"].contains(&raw[i]))
            .collect();
        if breaks.is_empty() {
            continue;
        }
        if breaks.iter().any(|&i| gaps[i].contains('\n')) {
            expanded.insert(result.len());
        }
        result.push(Group {
            open,
            end,
            breaks,
            contract: false,
            condition: true,
        });
    }
    (result, expanded)
}

struct Rendered {
    text: String,
    rows: Vec<usize>,
    long_rows: BTreeSet<usize>,
}

#[allow(clippy::too_many_arguments)]
fn render(
    raw: &[&str],
    gaps: &[&str],
    layout: &Layout,
    angles: &BTreeSet<usize>,
    groups: &[Group],
    expanded: &BTreeSet<usize>,
    options: Options,
    newline: &str,
) -> Rendered {
    let mut writer = Writer {
        text: String::new(),
        newline,
        indent: if options.insert_spaces {
            " ".repeat(options.tab_size)
        } else {
            "\t".into()
        },
        tab_size: options.tab_size,
        row: 0,
        column: 0,
    };
    let mut breaks: BTreeSet<usize> = BTreeSet::new();
    let mut changes = vec![0_isize; raw.len() + 1];
    for &(start, end) in &layout.indents {
        changes[start] += 1;
        changes[end] -= 1;
    }
    let mut inline = BTreeSet::new();
    for &(start, end) in &layout.label_statements {
        if !gaps[start].contains('\n') && !gaps[start].contains("//") {
            inline.insert(start);
            // A statement alongside its label owns the same indentation as that
            // label; later selected statements still sit one level beneath it.
            changes[start] -= 1;
            changes[end] += 1;
        }
    }
    for &(start, end) in &layout.arm_results {
        if gaps[start].contains('\n') {
            changes[start] += 1;
            changes[end] -= 1;
            breaks.insert(start);
        }
    }
    for &index in expanded {
        let group = &groups[index];
        breaks.extend(&group.breaks);
        if group.contract || group.condition {
            changes[group.open + 1] += 1;
            changes[group.end] -= 1;
        }
    }
    let mut stack: Vec<(usize, bool)> = vec![];
    let mut selected_depth = 0_isize;
    let mut after_break = false;
    let mut rows = vec![0; raw.len()];
    let mut long_rows = BTreeSet::new();
    for (i, &current) in raw.iter().enumerate() {
        let closing = ["}", "]", ")"].contains(&current) || (current == ">" && angles.contains(&i));
        let closing_block = closing && stack.last().is_some_and(|(_, block)| *block);
        let empty_block = closing_block
            && stack.last().is_some_and(|(open, _)| *open + 1 == i)
            && !gaps[i].contains("//");
        selected_depth += changes[i];
        let comment_depth = (stack.len() as isize + selected_depth) as usize;
        let mut indent = comment_depth - usize::from(closing);
        writer.trivia(gaps[i], comment_depth);
        if current.is_empty() {
            break;
        }
        let hard_break = !empty_block
            && (after_break
                || (layout.starts.contains(&i) && !inline.contains(&i))
                || closing_block
                || breaks.contains(&i));
        if hard_break {
            writer.line(1);
        } else if !empty_block && !layout.blocks.contains(&i) && gaps[i].contains('\n') {
            writer.line(1);
            if !closing && !stack.iter().any(|(_, block)| !block) {
                indent += 1;
            }
        }
        rows[i] = writer.row;
        writer.put(
            current,
            indent,
            i > 0 && space_before(raw[i - 1], current, i, angles),
        );
        if writer.column > options.max_width {
            long_rows.insert(writer.row);
        }
        after_break = false;
        if closing {
            stack.pop();
        }
        if ["{", "[", "("].contains(&current) || (current == "<" && angles.contains(&i)) {
            let block = layout.blocks.contains(&i);
            stack.push((i, block));
            if block {
                after_break = true;
            }
        } else if current == ";"
            || (current == "," && stack.last().is_some_and(|(_, block)| *block))
        {
            after_break = true;
        }
    }
    while writer.text.ends_with('\n') || writer.text.ends_with('\r') {
        writer.text.pop();
    }
    writer.line(1);
    Rendered {
        text: writer.text,
        rows,
        long_rows,
    }
}

/// Format a single source file without loading imports or a primitive manifest.
/// A syntax error leaves the caller's source untouched. Tokens, raw literal bytes,
/// comments and their order are checked again before any result is returned.
pub fn source(text: &str, options: Options) -> Result<String> {
    if !(1..=16).contains(&options.tab_size) {
        return Err(Diagnostic::new(
            "E_FORMAT",
            Span::default(),
            "tab size must be from 1 through 16",
        ));
    }
    if !(40..=240).contains(&options.max_width) {
        return Err(Diagnostic::new(
            "E_FORMAT",
            Span::default(),
            "line width must be from 40 through 240",
        ));
    }
    let tokens = syntax::lex(text)?;
    let (tokens, layout) = if tokens.len() == 1 {
        (tokens, Layout::default())
    } else {
        let (_, tokens, layout) = syntax::parse_with_layout(text)?;
        (tokens, layout)
    };
    let original = Source::new(text);
    let raw: Vec<_> = tokens.iter().map(|t| original.raw(t)).collect();
    // Choose a line ending from trivia, never from a multiline literal.
    let mut end = 0;
    let mut newline = None;
    for token in &tokens {
        let gap = &text[end..original.offset(token.span)];
        if let Some(n) = gap.find('\n') {
            newline.get_or_insert(if n > 0 && gap.as_bytes()[n - 1] == b'\r' {
                "\r\n"
            } else {
                "\n"
            });
        }
        end = original.offset(token.end);
    }
    let mut previous_end = 0;
    let gaps: Vec<_> = tokens
        .iter()
        .map(|token| {
            let gap = &text[previous_end..original.offset(token.span)];
            previous_end = original.offset(token.end);
            gap
        })
        .collect();
    let angles: BTreeSet<_> = layout.angles.iter().flat_map(|(a, b)| [*a, *b]).collect();
    let (groups, mut expanded) = groups(&raw, &gaps, &layout, &angles);
    let formatted = loop {
        // A multiline payload makes its enclosing exit list multiline too,
        // including when the payload was expanded by width rather than authored.
        let contracts: Vec<_> = groups
            .iter()
            .enumerate()
            .filter(|(index, group)| {
                group.contract
                    && !expanded.contains(index)
                    && expanded.iter().any(|&child| {
                        groups[child].open > group.open && groups[child].end < group.end
                    })
            })
            .map(|(index, _)| index)
            .collect();
        expanded.extend(contracts);
        let rendered = render(
            &raw,
            &gaps,
            &layout,
            &angles,
            &groups,
            &expanded,
            options,
            newline.unwrap_or("\n"),
        );
        let mut candidates = BTreeMap::new();
        // Prefer the outer list on each long line before subdividing its items.
        // Walk group spans once, rather than rescanning every group per long line.
        for (index, group) in groups
            .iter()
            .enumerate()
            .filter(|(i, _)| !expanded.contains(i))
        {
            for &row in &rendered.rows[group.open..=group.end] {
                if rendered.long_rows.contains(&row) {
                    let candidate = (group.end - group.open, index);
                    let best = candidates.entry(row).or_insert(candidate);
                    *best = (*best).max(candidate);
                }
            }
        }
        let additions: BTreeSet<_> = candidates.into_values().map(|(_, index)| index).collect();
        // Each pass expands at least one group, so this process terminates.
        if additions.is_empty() {
            break rendered.text;
        }
        expanded.extend(additions);
    };
    if fingerprint(text)? != fingerprint(&formatted)? {
        return Err(Diagnostic::new(
            "E_FORMAT",
            Span::default(),
            "formatting would change tokens or comments; source was left unchanged",
        ));
    }
    if tokens.len() > 1 {
        syntax::parse_unit(&formatted)?;
    }
    Ok(formatted)
}

fn fingerprint(text: &str) -> Result<Vec<String>> {
    let source = Source::new(text);
    let mut pieces = vec![];
    let mut end = 0;
    for token in syntax::lex(text)? {
        let gap = &text[end..source.offset(token.span)];
        for line in gap.split('\n') {
            if let Some(start) = line.find("//") {
                pieces.push(format!(
                    "comment:{}",
                    line[start..].strip_suffix('\r').unwrap_or(&line[start..])
                ));
            }
        }
        if !matches!(token.kind, Kind::End) {
            pieces.push(format!("token:{}", source.raw(&token)));
        }
        end = source.offset(token.end);
    }
    Ok(pieces)
}
