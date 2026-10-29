//! Parser-directed whitespace formatting. No punctuation or comments are rewritten.
use std::collections::BTreeSet;

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
}

/// Indentation settings shared by the command line and language server.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub tab_size: usize,
    pub insert_spaces: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            tab_size: 4,
            insert_spaces: true,
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
}
impl Writer<'_> {
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
            self.text.push_str(self.newline);
        }
    }
    fn put(&mut self, text: &str, depth: usize, space: bool) {
        if self.text.is_empty() || self.text.ends_with('\n') {
            for _ in 0..depth {
                self.text.push_str(&self.indent);
            }
        } else if space {
            self.text.push(' ');
        }
        self.text.push_str(text);
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
        return ["=", "=>", "->", ",", ":", "(", "{"].contains(&previous);
    }
    true
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
    let mut writer = Writer {
        text: String::with_capacity(text.len()),
        newline: newline.unwrap_or("\n"),
        indent: if options.insert_spaces {
            " ".repeat(options.tab_size)
        } else {
            "\t".into()
        },
    };
    let angles: BTreeSet<_> = layout.angles.iter().flat_map(|(a, b)| [*a, *b]).collect();
    let mut stack: Vec<(usize, bool)> = vec![];
    let mut indent_changes = vec![0_isize; tokens.len() + 1];
    for (start, end) in layout.indents {
        indent_changes[start] += 1;
        indent_changes[end] -= 1;
    }
    let mut selected_depth = 0_isize;
    let mut previous_end = 0;
    let mut after_break = false;
    for (i, token) in tokens.iter().enumerate() {
        let current = raw[i];
        let closing = matches!(&token.kind, Kind::Symbol(s) if ["}", "]", ")"].contains(&s.as_str()))
            || (current == ">" && angles.contains(&i));
        let closing_block = closing && stack.last().is_some_and(|(_, block)| *block);
        let empty_block = closing_block
            && stack.last().is_some_and(|(open, _)| *open + 1 == i)
            && !text[previous_end..original.offset(token.span)].contains("//");
        selected_depth += indent_changes[i];
        let comment_depth = stack.len() + selected_depth as usize;
        let mut indent = comment_depth - usize::from(closing);
        let gap = &text[previous_end..original.offset(token.span)];
        writer.trivia(gap, comment_depth);
        if matches!(token.kind, Kind::End) {
            break;
        }
        let hard_break =
            !empty_block && (after_break || layout.starts.contains(&i) || closing_block);
        if hard_break {
            writer.line(1);
        } else if !empty_block && !layout.blocks.contains(&i) && gap.contains('\n') {
            // Retain authored wrapping; automatic width-based reflow is separate.
            writer.line(1);
            if !closing && !stack.iter().any(|(_, block)| !block) {
                indent += 1;
            }
        }
        writer.put(
            current,
            indent,
            i > 0 && space_before(raw[i - 1], current, i, &angles),
        );
        after_break = false;
        if closing {
            stack.pop();
        }
        if matches!(&token.kind, Kind::Symbol(s) if ["{", "[", "("].contains(&s.as_str()))
            || (current == "<" && angles.contains(&i))
        {
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
        previous_end = original.offset(token.end);
    }
    // Final whitespace is ours; literal and comment spelling is never trimmed.
    while writer.text.ends_with('\n') || writer.text.ends_with('\r') {
        writer.text.pop();
    }
    writer.line(1);
    let formatted = writer.text;
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
