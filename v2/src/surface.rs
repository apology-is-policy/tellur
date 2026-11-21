//! The Rails surface syntax (`docs/v2/surface.md`) for slice 1: layout,
//! grammar and elaboration into the core text tree ([`crate::syntax::Program`]).
//!
//! Every error names the clause of `docs/v2/surface.md` (or `docs/v2/core.md`)
//! that refuses the program: layout (`[Y1]` to `[Y10]`), grammar (`[G1]` to
//! `[G9]`) or elaboration (`[U1]` to `[U15]`).

use crate::int::Int;
use crate::syntax::{
    Alternative, Arm, Binary, Command, Control, Declaration, Expr, Function, LoopParameter,
    Parameter, Program, Selector, Statement, Unary,
};
use crate::typed::{IntType, Type};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Clone, Debug)]
pub struct Error {
    pub line: u32,
    pub clause: &'static str,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "line {}: {} [{}]",
            self.line, self.message, self.clause
        )
    }
}

fn fail<T>(line: u32, clause: &'static str, message: impl Into<String>) -> Result<T, Error> {
    Err(Error {
        line,
        clause,
        message: message.into(),
    })
}

// ---------------------------------------------------------------------------
// Stage 1: Layout ([Y1] to [Y10]) and line structure
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seg {
    /// `│ `
    Bar,
    /// `║ `
    Loop,
    /// `┆ `
    Dash,
    /// Two spaces `  ` standing for a finished `└ ` arm rail before `┆ `.
    Blank,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    None,
    /// `╞═` on a control rail.
    ControlLabel,
    /// `╠═` on a loop rail.
    LoopLabel,
    /// `├ ` vertical arm.
    ArmMid,
    /// `└ ` last vertical arm.
    ArmLast,
}

#[derive(Clone, Debug)]
struct RailLine {
    line: u32,
    segments: Vec<Seg>,
    marker: Marker,
    /// Whether the content after the prefix and marker began with whitespace.
    indented: bool,
    /// Trimmed content with trailing comments stripped.
    content: String,
}

fn strip_comment(text: &str) -> &str {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (i, &(byte_pos, ch)) in chars.iter().enumerate() {
        if ch == '┄' {
            return &text[..byte_pos];
        }
        if ch == '/' && chars.get(i + 1).is_some_and(|(_, next)| *next == '/') {
            return &text[..byte_pos];
        }
    }
    text
}

/// Whether `rest` starts with zero or more `  ` / `│ ` segments followed by `┆`.
fn leads_to_dash(mut rest: &str) -> bool {
    loop {
        if rest.starts_with('┆') {
            return true;
        }
        if let Some(after) = rest.strip_prefix("  ").or_else(|| rest.strip_prefix("│ ")) {
            rest = after;
        } else {
            return false;
        }
    }
}

/// Parses one line inside a Rails control body (before the closing `╵`).
fn parse_rail_line(line_no: u32, raw: &str) -> Result<RailLine, Error> {
    if raw.trim().is_empty() {
        return fail(
            line_no,
            "Y9",
            "a blank line in a body must carry its rail prefix",
        );
    }
    let mut rest = raw;
    let mut segments = Vec::new();
    let mut marker = Marker::None;

    loop {
        if rest.is_empty() {
            break;
        }
        if let Some(after) = rest.strip_prefix('╞') {
            let Some(after_eq) = after.strip_prefix('═') else {
                return fail(line_no, "Y2", "`╞` must be followed by `═`");
            };
            if !after_eq.is_empty() && !after_eq.starts_with(' ') {
                return fail(line_no, "Y2", "`╞═` must be followed by a space");
            }
            marker = Marker::ControlLabel;
            rest = after_eq.strip_prefix(' ').unwrap_or(after_eq);
            break;
        }
        if let Some(after) = rest.strip_prefix('╠') {
            let Some(after_eq) = after.strip_prefix('═') else {
                return fail(line_no, "Y2", "`╠` must be followed by `═`");
            };
            if !after_eq.is_empty() && !after_eq.starts_with(' ') {
                return fail(line_no, "Y2", "`╠═` must be followed by a space");
            }
            marker = Marker::LoopLabel;
            rest = after_eq.strip_prefix(' ').unwrap_or(after_eq);
            break;
        }
        if let Some(after) = rest.strip_prefix('├') {
            if !after.starts_with(' ') {
                return fail(line_no, "Y2", "`├` must be followed by a space");
            }
            marker = Marker::ArmMid;
            rest = &after[1..];
            break;
        }
        if let Some(after) = rest.strip_prefix('└') {
            if !after.starts_with(' ') {
                return fail(line_no, "Y2", "`└` must be followed by a space");
            }
            marker = Marker::ArmLast;
            rest = &after[1..];
            break;
        }
        if let Some((ch, seg)) = [('│', Seg::Bar), ('║', Seg::Loop), ('┆', Seg::Dash)]
            .into_iter()
            .find(|(ch, _)| rest.starts_with(*ch))
        {
            let after = &rest[ch.len_utf8()..];
            if after.trim().is_empty() {
                segments.push(seg);
                rest = "";
                break;
            }
            if !after.starts_with(' ') {
                return fail(
                    line_no,
                    "Y2",
                    format!("`{ch}` in a rail prefix must be followed by a space"),
                );
            }
            segments.push(seg);
            rest = &after[1..];
            continue;
        }
        if !segments.is_empty()
            && let Some(after) = rest.strip_prefix("  ")
            && leads_to_dash(after)
        {
            segments.push(Seg::Blank);
            rest = after;
            continue;
        }
        break;
    }

    if segments.is_empty() && marker == Marker::None {
        return fail(
            line_no,
            "Y3",
            "a line of a control body must start with a rail or close with `╵`",
        );
    }

    let without_comment = strip_comment(rest);
    let indented = without_comment.starts_with(' ') || without_comment.starts_with('\t');
    let content = without_comment.trim().to_string();

    Ok(RailLine {
        line: line_no,
        segments,
        marker,
        indented,
        content,
    })
}

#[derive(Clone, Debug)]
enum RawItem {
    Simple {
        line: u32,
        text: String,
    },
    Label {
        line: u32,
        mask: String,
        body: Vec<RawItem>,
    },
    Test {
        line: u32,
        condition: String,
        body: Vec<RawItem>,
    },
    Decision {
        line: u32,
        header: String,
        arms: Vec<RawArm>,
    },
    Loop {
        line: u32,
        header: String,
        body: Vec<RawItem>,
    },
}

#[derive(Clone, Debug)]
struct RawArm {
    line: u32,
    header: String,
    block: Option<Vec<RawItem>>,
}

#[derive(Clone, Debug)]
enum Frame {
    Control {
        items: Vec<RawItem>,
    },
    Loop {
        line: u32,
        header: String,
        items: Vec<RawItem>,
    },
    Label {
        line: u32,
        mask: String,
        items: Vec<RawItem>,
    },
    Test {
        line: u32,
        condition: String,
        items: Vec<RawItem>,
    },
    Decision {
        line: u32,
        header: String,
        saw_last: bool,
        arms: Vec<RawArm>,
    },
    ArmBlock {
        line: u32,
        header: String,
        is_last: bool,
        items: Vec<RawItem>,
    },
}

impl Frame {
    fn segments(&self) -> &'static [Seg] {
        match self {
            Frame::Control { .. } => &[Seg::Bar],
            Frame::Loop { .. } => &[Seg::Loop],
            Frame::Label { .. } | Frame::Test { .. } => &[Seg::Dash],
            Frame::Decision { .. } => &[],
            Frame::ArmBlock { is_last: false, .. } => &[Seg::Bar, Seg::Dash],
            Frame::ArmBlock { is_last: true, .. } => &[Seg::Blank, Seg::Dash],
        }
    }

    fn items_mut(&mut self) -> Option<&mut Vec<RawItem>> {
        match self {
            Frame::Control { items }
            | Frame::Loop { items, .. }
            | Frame::Label { items, .. }
            | Frame::Test { items, .. }
            | Frame::ArmBlock { items, .. } => Some(items),
            Frame::Decision { .. } => None,
        }
    }
}

fn expected_segments(frames: &[Frame]) -> Vec<Seg> {
    let mut out = Vec::new();
    for frame in frames {
        out.extend_from_slice(frame.segments());
    }
    out
}

fn strip_trailing_arrow(text: &str) -> Option<&str> {
    let trimmed = text.trim_end();
    trimmed
        .strip_suffix('→')
        .or_else(|| trimmed.strip_suffix("=>"))
        .or_else(|| trimmed.strip_suffix("->"))
        .map(str::trim_end)
}

fn starts_loop(text: &str) -> bool {
    text.starts_with('↻')
        || text == "loop"
        || text.starts_with("loop ")
        || text.starts_with("loop(")
}

fn starts_test(text: &str) -> Option<&str> {
    if let Some(rest) = text.strip_prefix('?') {
        return Some(rest.trim_start());
    }
    if let Some(rest) = text.strip_prefix("if ") {
        return Some(rest.trim_start());
    }
    if text.starts_with("if(") {
        return Some(&text[2..]);
    }
    None
}

/// Whether `text` is a decision header without horizontal `: arms`.
fn is_vertical_decision(text: &str) -> bool {
    // Either `Δ ...`, `delta ...`, `■ Δ ...`, `exit delta ...`, or `name : Δ ...`.
    let has_delta = text.starts_with('Δ')
        || text.starts_with("delta ")
        || text.starts_with("■ Δ")
        || text.starts_with("exit Δ")
        || text.starts_with("exit delta ")
        || text.find(':').is_some_and(|colon| {
            let before = text[..colon].trim();
            let after = text[colon + 1..].trim_start();
            !before.is_empty()
                && before
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
                && (after.starts_with('Δ') || after.starts_with("delta "))
        });
    if !has_delta {
        return false;
    }
    // Check whether there is a `:` after the `Δ` / `delta` token (outside parens).
    let after_delta = if let Some(pos) = text.find('Δ') {
        &text[pos + 'Δ'.len_utf8()..]
    } else if let Some(pos) = text.find("delta ") {
        &text[pos + 6..]
    } else {
        return false;
    };
    let mut depth = 0i32;
    for ch in after_delta.chars() {
        match ch {
            '(' | '{' => depth += 1,
            ')' | '}' => depth -= 1,
            ':' if depth == 0 => return false,
            _ => {}
        }
    }
    true
}

fn check_not_empty_frame(frame: &Frame) -> Result<(), Error> {
    match frame {
        Frame::Control { .. } => Ok(()),
        Frame::Loop { line, items, .. } if items.is_empty() => {
            fail(*line, "Y4", "a loop must be followed by its `║` body lines")
        }
        Frame::Label { line, items, .. } if items.is_empty() => fail(
            *line,
            "Y5",
            "a label without `:` must be followed by its `┆` block",
        ),
        Frame::Test { line, items, .. } if items.is_empty() => fail(
            *line,
            "Y6",
            "a test ending in `→` must be followed by its `┆` block",
        ),
        Frame::Decision {
            line,
            arms,
            saw_last,
            ..
        } => {
            if arms.is_empty() {
                return fail(
                    *line,
                    "Y7",
                    "a decision without `:` must be followed by `├` and `└` arms",
                );
            }
            if !*saw_last {
                return fail(*line, "Y7", "vertical arms must end with `└`");
            }
            Ok(())
        }
        Frame::ArmBlock { line, items, .. } if items.is_empty() => fail(
            *line,
            "Y7",
            "an arm ending in `→` must be followed by its `┆` block",
        ),
        _ => Ok(()),
    }
}

fn pop_one_frame(frames: &mut Vec<Frame>) -> Result<(), Error> {
    let popped = frames.pop().expect("a frame to pop");
    check_not_empty_frame(&popped)?;
    match popped {
        Frame::Control { .. } => unreachable!(),
        Frame::Loop {
            line,
            header,
            items,
        } => {
            frames
                .last_mut()
                .and_then(Frame::items_mut)
                .expect("a parent block")
                .push(RawItem::Loop {
                    line,
                    header,
                    body: items,
                });
        }
        Frame::Label { line, mask, items } => {
            frames
                .last_mut()
                .and_then(Frame::items_mut)
                .expect("a parent block")
                .push(RawItem::Label {
                    line,
                    mask,
                    body: items,
                });
        }
        Frame::Test {
            line,
            condition,
            items,
        } => {
            frames
                .last_mut()
                .and_then(Frame::items_mut)
                .expect("a parent block")
                .push(RawItem::Test {
                    line,
                    condition,
                    body: items,
                });
        }
        Frame::Decision {
            line, header, arms, ..
        } => {
            frames
                .last_mut()
                .and_then(Frame::items_mut)
                .expect("a parent block")
                .push(RawItem::Decision { line, header, arms });
        }
        Frame::ArmBlock {
            line,
            header,
            items,
            ..
        } => {
            let Frame::Decision { arms, .. } = frames.last_mut().expect("a decision frame") else {
                unreachable!();
            };
            arms.push(RawArm {
                line,
                header,
                block: Some(items),
            });
        }
    }
    Ok(())
}

/// Joins `[Y8]` continuation lines and builds the block tree of a Rails
/// control body.
fn build_rails_body(raw_lines: &[RailLine]) -> Result<Vec<RawItem>, Error> {
    // First pass: join [Y8] continuation lines into their preceding line when
    // the preceding line has the exact same rail prefix and can be continued.
    let mut joined: Vec<RailLine> = Vec::new();
    for line in raw_lines {
        if line.content.is_empty() && line.marker == Marker::None {
            joined.push(line.clone());
            continue;
        }
        if line.indented {
            if line.marker != Marker::None {
                return fail(line.line, "Y8", "a marker line cannot be a continuation");
            }
            let Some(prev) = joined.last_mut() else {
                return fail(
                    line.line,
                    "Y8",
                    "a continuation line has no statement above it",
                );
            };
            if prev.content.is_empty()
                || prev.segments != line.segments
                || strip_trailing_arrow(&prev.content).is_some()
                || starts_loop(&prev.content)
            {
                return fail(
                    line.line,
                    "Y8",
                    "a continuation line does not match a statement on the line above",
                );
            }
            prev.content.push(' ');
            prev.content.push_str(&line.content);
            continue;
        }
        joined.push(line.clone());
    }

    let mut frames = vec![Frame::Control { items: Vec::new() }];

    for line in joined {
        // Blank rail line ([Y9]).
        if line.content.is_empty() && line.marker == Marker::None {
            let expected = expected_segments(&frames);
            let matches_prefix = line.segments.len() <= expected.len()
                && line.segments.as_slice() == &expected[..line.segments.len()];
            if !matches_prefix {
                return fail(
                    line.line,
                    "Y10",
                    "the rail prefix of this blank line does not fit the open blocks",
                );
            }
            continue;
        }

        match line.marker {
            Marker::ControlLabel | Marker::LoopLabel => {
                if line.content.is_empty() {
                    return fail(line.line, "Y5", "a label junction needs a mask");
                }
                // A label stands directly on the rail of a control (`╞═`) or of
                // a loop (`╠═`), never inside a `┆` block ([Y5]).
                let mut target_depth = None;
                for depth in (1..=frames.len()).rev() {
                    let parent_segs = expected_segments(&frames[..depth - 1]);
                    if parent_segs == line.segments {
                        match &frames[depth - 1] {
                            Frame::Control { .. } | Frame::Loop { .. } => {
                                target_depth = Some(depth);
                                break;
                            }
                            Frame::Label { .. } | Frame::Test { .. } | Frame::ArmBlock { .. } => {
                                return fail(
                                    line.line,
                                    "Y5",
                                    "a label cannot stand inside a `┆` block",
                                );
                            }
                            Frame::Decision { .. } => {}
                        }
                    }
                }
                let Some(depth) = target_depth else {
                    return fail(
                        line.line,
                        "Y5",
                        "a label must stand directly on the rail of a control or a loop",
                    );
                };
                let is_control = matches!(frames[depth - 1], Frame::Control { .. });
                if is_control && line.marker != Marker::ControlLabel {
                    return fail(line.line, "Y5", "use `╞═` on a control's rail");
                }
                if !is_control && line.marker != Marker::LoopLabel {
                    return fail(line.line, "Y5", "use `╠═` on a loop's rail");
                }
                while frames.len() > depth {
                    pop_one_frame(&mut frames)?;
                }
                if let Some((mask, inline)) = split_short_label(&line.content) {
                    let mut body = Vec::new();
                    for part in inline.split(';') {
                        let trimmed = part.trim();
                        if !trimmed.is_empty() {
                            if is_vertical_decision(trimmed)
                                || starts_loop(trimmed)
                                || (starts_test(trimmed).is_some()
                                    && strip_trailing_arrow(trimmed).is_some())
                            {
                                return fail(
                                    line.line,
                                    "Y5",
                                    "a short label on one line takes simple statements only",
                                );
                            }
                            body.push(RawItem::Simple {
                                line: line.line,
                                text: trimmed.to_string(),
                            });
                        }
                    }
                    if body.is_empty() {
                        return fail(line.line, "Y5", "expected a statement after `:` in label");
                    }
                    frames
                        .last_mut()
                        .and_then(Frame::items_mut)
                        .expect("a body frame")
                        .push(RawItem::Label {
                            line: line.line,
                            mask: mask.to_string(),
                            body,
                        });
                } else {
                    frames.push(Frame::Label {
                        line: line.line,
                        mask: line.content,
                        items: Vec::new(),
                    });
                }
            }
            Marker::ArmMid | Marker::ArmLast => {
                // Find the open Decision frame at `line.segments`.
                let mut decision_depth = None;
                for depth in (1..=frames.len()).rev() {
                    if matches!(frames[depth - 1], Frame::Decision { .. })
                        && expected_segments(&frames[..depth]) == line.segments
                    {
                        decision_depth = Some(depth);
                        break;
                    }
                }
                let Some(depth) = decision_depth else {
                    return fail(
                        line.line,
                        "Y7",
                        "an arm marker `├` or `└` does not match an open vertical decision",
                    );
                };
                while frames.len() > depth {
                    pop_one_frame(&mut frames)?;
                }
                let is_last = line.marker == Marker::ArmLast;
                let Frame::Decision { saw_last, arms, .. } = frames.last_mut().unwrap() else {
                    unreachable!();
                };
                if *saw_last {
                    return fail(line.line, "Y7", "no arm may follow the last arm `└`");
                }
                if is_last {
                    *saw_last = true;
                }
                if line.content.is_empty() {
                    return fail(line.line, "Y7", "expected an arm after `├` or `└`");
                }
                if let Some(arm_header) = strip_trailing_arrow(&line.content) {
                    frames.push(Frame::ArmBlock {
                        line: line.line,
                        header: arm_header.to_string(),
                        is_last,
                        items: Vec::new(),
                    });
                } else {
                    arms.push(RawArm {
                        line: line.line,
                        header: line.content,
                        block: None,
                    });
                }
            }
            Marker::None => {
                // Find the open non-Decision frame whose expected prefix matches
                // `line.segments`.
                let mut target_depth = None;
                for depth in (1..=frames.len()).rev() {
                    if !matches!(frames[depth - 1], Frame::Decision { .. })
                        && expected_segments(&frames[..depth]) == line.segments
                    {
                        target_depth = Some(depth);
                        break;
                    }
                }
                let Some(depth) = target_depth else {
                    // Check if the top frame was an empty mandatory block so we
                    // report its specific clause ([Y4], [Y5], [Y6], [Y7]).
                    check_not_empty_frame(frames.last().unwrap())?;
                    return fail(
                        line.line,
                        "Y10",
                        "the rail prefix of this line does not fit the open blocks",
                    );
                };
                while frames.len() > depth {
                    pop_one_frame(&mut frames)?;
                }

                if starts_loop(&line.content) {
                    frames.push(Frame::Loop {
                        line: line.line,
                        header: line.content,
                        items: Vec::new(),
                    });
                } else if let Some(after_q) = starts_test(&line.content) {
                    if let Some(cond) = strip_trailing_arrow(after_q) {
                        frames.push(Frame::Test {
                            line: line.line,
                            condition: cond.to_string(),
                            items: Vec::new(),
                        });
                    } else if let Some((cond, simple)) = split_top_arrow(after_q) {
                        if is_vertical_decision(simple) || starts_loop(simple) {
                            return fail(
                                line.line,
                                "Y6",
                                "a test on one line takes a simple statement after `→`",
                            );
                        }
                        let body = parse_inline_test_chain(line.line, simple)?;
                        frames
                            .last_mut()
                            .and_then(Frame::items_mut)
                            .expect("a body frame")
                            .push(RawItem::Test {
                                line: line.line,
                                condition: cond.to_string(),
                                body,
                            });
                    } else {
                        return fail(line.line, "Y6", "a test `?` requires `→`");
                    }
                } else if is_vertical_decision(&line.content) {
                    frames.push(Frame::Decision {
                        line: line.line,
                        header: line.content,
                        saw_last: false,
                        arms: Vec::new(),
                    });
                } else {
                    frames
                        .last_mut()
                        .and_then(Frame::items_mut)
                        .expect("a body frame")
                        .push(RawItem::Simple {
                            line: line.line,
                            text: line.content,
                        });
                }
            }
        }
    }

    while frames.len() > 1 {
        pop_one_frame(&mut frames)?;
    }
    let Frame::Control { items } = frames.pop().unwrap() else {
        unreachable!();
    };
    Ok(items)
}

fn parse_inline_test_chain(line: u32, text: &str) -> Result<Vec<RawItem>, Error> {
    if let Some(after_q) = starts_test(text) {
        let Some((cond, rest)) = split_top_arrow(after_q) else {
            return fail(line, "Y6", "a test `?` requires `→`");
        };
        let inner = parse_inline_test_chain(line, rest)?;
        return Ok(vec![RawItem::Test {
            line,
            condition: cond.to_string(),
            body: inner,
        }]);
    }
    Ok(vec![RawItem::Simple {
        line,
        text: text.to_string(),
    }])
}

/// Splits `mask : statements` at the first top-level `:`.
fn split_short_label(text: &str) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    for (idx, ch) in text.char_indices() {
        match ch {
            '(' | '{' => depth += 1,
            ')' | '}' => depth -= 1,
            ':' if depth == 0 => {
                let after = &text[idx + 1..];
                if !after.starts_with('=') {
                    return Some((text[..idx].trim(), after.trim()));
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits `left → right` (or `=>` / `->`) at top-level.
fn split_top_arrow(text: &str) -> Option<(&str, &str)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut depth = 0i32;
    for (i, &(byte_pos, ch)) in chars.iter().enumerate() {
        match ch {
            '(' | '{' => depth += 1,
            ')' | '}' => depth -= 1,
            '→' if depth == 0 => {
                return Some((
                    text[..byte_pos].trim(),
                    text[byte_pos + '→'.len_utf8()..].trim(),
                ));
            }
            '=' | '-' if depth == 0 && chars.get(i + 1).is_some_and(|(_, next)| *next == '>') => {
                return Some((text[..byte_pos].trim(), text[byte_pos + 2..].trim()));
            }
            _ => {}
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Stage 2: Surface tokens, grammar ([G1] to [G9]) and AST
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Name(String),
    Number(String),
    Delta,
    Arrow,
    Exit,
    Leave,
    Loop,
    Again,
    Test,
    Oblige,
    When,
    Impossible,
    Assign,
    Bind,
    MapsTo,
    Ellipsis,
    Bar,
    Colon,
    Semi,
    Comma,
    Dot,
    Tilde,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Plus,
    Minus,
    Star,
    Slash,
    And,
    Or,
    Not,
    Control,
    Extern,
    Function,
    Outcome,
    Forward,
    Auto,
    Pub,
    Uses,
    Faults,
    Mod,
    True,
    False,
    End,
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    line: u32,
    first_on_line: bool,
}

fn word_token(word: &str) -> Tok {
    match word {
        "delta" => Tok::Delta,
        "exit" | "exits" => Tok::Exit,
        "leave" => Tok::Leave,
        "loop" => Tok::Loop,
        "again" => Tok::Again,
        "if" => Tok::Test,
        "where" => Tok::Oblige,
        "when" => Tok::When,
        "impossible" => Tok::Impossible,
        "mutable" => Tok::Tilde,
        "control" => Tok::Control,
        "extern" => Tok::Extern,
        "function" => Tok::Function,
        "outcome" => Tok::Outcome,
        "forward" => Tok::Forward,
        "auto" => Tok::Auto,
        "pub" => Tok::Pub,
        "uses" => Tok::Uses,
        "faults" => Tok::Faults,
        "mod" => Tok::Mod,
        "true" => Tok::True,
        "false" => Tok::False,
        _ => Tok::Name(word.to_string()),
    }
}

fn lex_fragment(source: &str, base_line: u32) -> Result<Vec<Token>, Error> {
    let characters: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = base_line;
    let mut first_on_line = true;
    while index < characters.len() {
        let ch = characters[index];
        let next = characters.get(index + 1).copied();
        let third = characters.get(index + 2).copied();
        if ch == '\n' {
            line += 1;
            first_on_line = true;
            index += 1;
            continue;
        }
        if ch.is_whitespace() {
            index += 1;
            continue;
        }
        if ch == '┄' || (ch == '/' && next == Some('/')) {
            while index < characters.len() && characters[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if matches!(ch, '│' | '║' | '┆' | '╞' | '╠' | '├' | '└' | '╵') {
            if ch == '│' {
                tokens.push(Token {
                    tok: Tok::Bar,
                    line,
                    first_on_line,
                });
                first_on_line = false;
                index += 1;
                continue;
            }
            return fail(
                line,
                "G9",
                format!("rail character `{ch}` cannot appear inside content or plain form"),
            );
        }
        let start = index;
        let tok = if ch.is_ascii_alphabetic() || ch == '_' {
            while index < characters.len()
                && (characters[index].is_ascii_alphanumeric() || characters[index] == '_')
            {
                index += 1;
            }
            word_token(&characters[start..index].iter().collect::<String>())
        } else if ch.is_ascii_digit() {
            while index < characters.len() && characters[index].is_ascii_digit() {
                index += 1;
            }
            Tok::Number(characters[start..index].iter().collect())
        } else {
            let pair = |c: char| next == Some(c);
            let (tok, len) = match ch {
                'Δ' => (Tok::Delta, 1),
                '→' => (Tok::Arrow, 1),
                '■' => (Tok::Exit, 1),
                '□' => (Tok::Leave, 1),
                '↻' => (Tok::Loop, 1),
                '↑' => (Tok::Again, 1),
                '?' => (Tok::Test, 1),
                '⊢' => (Tok::Oblige, 1),
                '⇐' => (Tok::When, 1),
                '⊥' => (Tok::Impossible, 1),
                '←' => (Tok::Assign, 1),
                '↦' => (Tok::MapsTo, 1),
                '…' => (Tok::Ellipsis, 1),
                '≠' => (Tok::NotEqual, 1),
                '≤' => (Tok::LessEqual, 1),
                '≥' => (Tok::GreaterEqual, 1),
                '∧' => (Tok::And, 1),
                '∨' => (Tok::Or, 1),
                '¬' => (Tok::Not, 1),
                '÷' => (Tok::Slash, 1),
                '~' => (Tok::Tilde, 1),
                ',' => (Tok::Comma, 1),
                ';' => (Tok::Semi, 1),
                '(' => (Tok::LeftParen, 1),
                ')' => (Tok::RightParen, 1),
                '{' => (Tok::LeftBrace, 1),
                '}' => (Tok::RightBrace, 1),
                '[' => (Tok::LeftBracket, 1),
                ']' => (Tok::RightBracket, 1),
                '+' => (Tok::Plus, 1),
                '-' if pair('>') => (Tok::Arrow, 2),
                '-' => (Tok::Minus, 1),
                '*' => (Tok::Star, 1),
                '/' => (Tok::Slash, 1),
                '.' if pair('.') => (Tok::Ellipsis, 2),
                '.' => (Tok::Dot, 1),
                ':' if pair('=') => (Tok::Bind, 2),
                ':' => (Tok::Colon, 1),
                '|' if pair('-') && third == Some('>') => (Tok::MapsTo, 3),
                '|' if pair('|') => (Tok::Or, 2),
                '|' => (Tok::Bar, 1),
                '&' if pair('&') => (Tok::And, 2),
                '=' if pair('>') => (Tok::Arrow, 2),
                '=' if pair('=') => (Tok::Equal, 2),
                '=' => (Tok::Equal, 1),
                '>' if pair('=') => (Tok::GreaterEqual, 2),
                '>' => (Tok::Greater, 1),
                '<' if pair('-') => (Tok::Assign, 2),
                '<' if pair('=') => (Tok::LessEqual, 2),
                '<' => (Tok::Less, 1),
                '!' if pair('=') => (Tok::NotEqual, 2),
                '!' => (Tok::Not, 1),
                other => {
                    return fail(
                        line,
                        "G8",
                        format!("`{other}` is not a token of the surface language"),
                    );
                }
            };
            index += len;
            tok
        };
        tokens.push(Token {
            tok,
            line,
            first_on_line,
        });
        first_on_line = false;
    }
    tokens.push(Token {
        tok: Tok::End,
        line,
        first_on_line: true,
    });
    Ok(tokens)
}

#[derive(Clone, Debug)]
struct OutcomeDecl {
    name: String,
    alternatives: Vec<Alternative>,
    line: u32,
}

#[derive(Clone, Debug)]
enum SurfaceContractEntry {
    Alt(Alternative),
    Group(String, u32),
}

#[derive(Clone, Debug)]
enum SurfaceContract {
    Auto,
    Entries(Vec<SurfaceContractEntry>),
}

#[derive(Clone, Debug)]
struct SurfaceControl {
    name: String,
    public: bool,
    external: bool,
    parameters: Vec<Parameter>,
    precondition: Option<Expr>,
    contract: SurfaceContract,
    uses: Vec<String>,
    body: Vec<SurfaceItem>,
    line: u32,
}

#[derive(Clone, Debug)]
enum SurfaceItem {
    Stmt(SurfaceStmt),
    Label { mask: Mask, body: Vec<SurfaceStmt> },
}

#[derive(Clone, Debug)]
enum Mask {
    Path {
        site: Option<String>,
        name: String,
        line: u32,
    },
    Not {
        site: Option<String>,
        name: String,
        line: u32,
    },
    And(Box<Mask>, Box<Mask>),
    Or(Box<Mask>, Box<Mask>),
}

#[derive(Clone, Debug)]
struct Pattern {
    mutable: bool,
    name: String,
    ty: Option<Type>,
}

#[derive(Clone, Debug)]
enum ArmTarget {
    None,
    /// `→ name` in a normal decision (`[U2]`), or `→ name` without payload in an adapter (`[U13]`).
    Name(String),
    /// `→ name { args }` in an adapter (`[U13]`).
    AdaptExit(String, Vec<Expr>),
    /// `→ ■` alone (`[U8]`).
    SameExit,
    /// `→ Terminator` or `→ forward ...` (`[U8]`).
    Stmt(Box<SurfaceStmt>),
    /// `→ { Stmt* }` (`[U8]`).
    Block(Vec<SurfaceStmt>),
}

#[derive(Clone, Debug)]
struct SurfaceArm {
    line: u32,
    is_group: bool,
    name: String,
    patterns: Option<Vec<Pattern>>,
    target: ArmTarget,
}

#[derive(Clone, Debug)]
struct LeaveOut {
    name: String,
    value: Expr,
}

#[derive(Clone, Debug)]
enum SurfaceStmt {
    Bind {
        line: u32,
        mutable: bool,
        name: String,
        ty: Option<Type>,
        value: Expr,
    },
    Assign {
        line: u32,
        name: String,
        value: Expr,
    },
    Oblige {
        line: u32,
        formula: Expr,
    },
    Test {
        line: u32,
        condition: Expr,
        body: Vec<SurfaceStmt>,
    },
    Decide {
        line: u32,
        site: Option<String>,
        adapter: bool,
        operand: Expr,
        arms: Vec<SurfaceArm>,
    },
    Forward {
        line: u32,
        operation: String,
        arguments: Vec<Expr>,
    },
    Call {
        line: u32,
        operation: String,
        arguments: Vec<Expr>,
    },
    Loop {
        line: u32,
        name: String,
        parameters: Vec<LoopParameter>,
        invariant: Option<Expr>,
        body: Vec<SurfaceItem>,
    },
    Again {
        line: u32,
        name: String,
        arguments: Vec<Expr>,
    },
    Leave {
        line: u32,
        alternative: Option<String>,
        outputs: Vec<LeaveOut>,
    },
    Exit {
        line: u32,
        alternative: String,
        payload: Vec<Expr>,
    },
    Impossible {
        line: u32,
    },
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Parser {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> &Tok {
        &self.tokens[self.pos].tok
    }

    fn peek_next(&self) -> &Tok {
        &self.tokens[(self.pos + 1).min(self.tokens.len() - 1)].tok
    }

    fn line(&self) -> u32 {
        self.tokens[self.pos].line
    }

    fn advance(&mut self) -> Tok {
        let tok = self.tokens[self.pos].tok.clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn eat(&mut self, expected: &Tok) -> bool {
        if self.peek() == expected {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: Tok, clause: &'static str, what: &str) -> Result<(), Error> {
        if self.eat(&expected) {
            Ok(())
        } else {
            fail(
                self.line(),
                clause,
                format!("expected {what}, found {}", describe_tok(self.peek())),
            )
        }
    }

    fn name(&mut self, clause: &'static str, what: &str) -> Result<String, Error> {
        match self.peek().clone() {
            Tok::Name(name) => {
                self.advance();
                Ok(name)
            }
            Tok::True if what == "an alternative" => {
                self.advance();
                Ok("true".to_string())
            }
            Tok::False if what == "an alternative" => {
                self.advance();
                Ok("false".to_string())
            }
            other => fail(
                self.line(),
                clause,
                format!("expected {what}, found {}", describe_tok(&other)),
            ),
        }
    }

    fn ty(&mut self, clause: &'static str) -> Result<Type, Error> {
        let line = self.line();
        let name = self.name(clause, "a type")?;
        Type::from_name(&name).ok_or(Error {
            line,
            clause,
            message: format!("`{name}` is not a type of slice 1"),
        })
    }

    fn parameters(&mut self, clause: &'static str) -> Result<Vec<Parameter>, Error> {
        let mut out = Vec::new();
        self.expect(Tok::LeftParen, clause, "`(`")?;
        while !self.eat(&Tok::RightParen) {
            if !out.is_empty() {
                self.expect(Tok::Comma, clause, "`,`")?;
            }
            let name = self.name(clause, "a parameter name")?;
            self.expect(Tok::Colon, clause, "`:`")?;
            let ty = self.ty(clause)?;
            out.push(Parameter { name, ty });
        }
        Ok(out)
    }

    fn is_field_list_ahead(&self) -> bool {
        if self.peek() != &Tok::LeftBrace {
            return false;
        }
        let get = |offset: usize| &self.tokens[(self.pos + offset).min(self.tokens.len() - 1)].tok;
        if get(1) == &Tok::RightBrace {
            return true;
        }
        matches!(get(1), Tok::Name(_))
            && get(2) == &Tok::Colon
            && matches!(get(3), Tok::Name(_))
            && matches!(get(4), Tok::Comma | Tok::RightBrace)
    }

    fn fields(&mut self, clause: &'static str) -> Result<Vec<Parameter>, Error> {
        let mut out = Vec::new();
        if self.is_field_list_ahead() && self.eat(&Tok::LeftBrace) {
            while !self.eat(&Tok::RightBrace) {
                if !out.is_empty() {
                    self.expect(Tok::Comma, clause, "`,`")?;
                }
                let name = self.name(clause, "a field name")?;
                self.expect(Tok::Colon, clause, "`:`")?;
                let ty = self.ty(clause)?;
                out.push(Parameter { name, ty });
            }
        }
        Ok(out)
    }

    fn outcome_alt(&mut self) -> Result<Alternative, Error> {
        let name = self.name("G2", "an alternative")?;
        let fields = self.fields("G2")?;
        if self.eat(&Tok::MapsTo) {
            match self.peek() {
                Tok::Number(_) => {
                    self.advance();
                }
                _ => {
                    return fail(
                        self.line(),
                        "G4",
                        "`↦` on an outcome alternative requires an exit status number",
                    );
                }
            }
        }
        Ok(Alternative {
            name,
            fields,
            condition: None,
        })
    }

    fn contract_entries(&mut self, into: &mut Vec<SurfaceContractEntry>) -> Result<(), Error> {
        let braced = self.eat(&Tok::LeftBrace);
        loop {
            let line = self.line();
            if self.eat(&Tok::Ellipsis) {
                let group = self.name("G2", "an outcome group name")?;
                into.push(SurfaceContractEntry::Group(group, line));
            } else {
                let name = self.name("G1", "an alternative")?;
                let fields = self.fields("G1")?;
                if self.eat(&Tok::MapsTo) {
                    return fail(
                        self.line(),
                        "G4",
                        "`↦` belongs on an `outcome` declaration, not a control contract",
                    );
                }
                let condition = if self.eat(&Tok::When) {
                    Some(self.expr()?)
                } else {
                    None
                };
                into.push(SurfaceContractEntry::Alt(Alternative {
                    name,
                    fields,
                    condition,
                }));
            }
            if !self.eat(&Tok::Bar) {
                break;
            }
        }
        if braced {
            self.expect(Tok::RightBrace, "G1", "`}`")?;
        }
        Ok(())
    }

    fn mask(&mut self) -> Result<Mask, Error> {
        let mut left = self.mask_and()?;
        while self.eat(&Tok::Or) {
            left = Mask::Or(Box::new(left), Box::new(self.mask_and()?));
        }
        Ok(left)
    }

    fn mask_and(&mut self) -> Result<Mask, Error> {
        let mut left = self.mask_atom()?;
        while self.eat(&Tok::And) {
            left = Mask::And(Box::new(left), Box::new(self.mask_atom()?));
        }
        Ok(left)
    }

    fn mask_atom(&mut self) -> Result<Mask, Error> {
        let line = self.line();
        if self.eat(&Tok::Not) {
            if self.peek() == &Tok::LeftParen {
                return fail(
                    line,
                    "G5",
                    "`¬` in a mask applies to a single path, not a parenthesized mask",
                );
            }
            let (site, name) = self.path_atom()?;
            return Ok(Mask::Not { site, name, line });
        }
        if self.eat(&Tok::LeftParen) {
            let inner = self.mask()?;
            self.expect(Tok::RightParen, "G5", "`)`")?;
            return Ok(inner);
        }
        let (site, name) = self.path_atom()?;
        Ok(Mask::Path { site, name, line })
    }

    fn path_atom(&mut self) -> Result<(Option<String>, String), Error> {
        let first = self.name("G5", "a path name")?;
        if self.eat(&Tok::Dot) {
            let second = self.name("G5", "an alternative name")?;
            Ok((Some(first), second))
        } else {
            Ok((None, first))
        }
    }

    fn expr(&mut self) -> Result<Expr, Error> {
        self.binary(1)
    }

    fn binary_op(&self) -> Option<Binary> {
        Some(match self.peek() {
            Tok::Or => Binary::Or,
            Tok::And => Binary::And,
            Tok::Equal => Binary::Equal,
            Tok::NotEqual => Binary::NotEqual,
            Tok::Less => Binary::Less,
            Tok::LessEqual => Binary::LessEqual,
            Tok::Greater => Binary::Greater,
            Tok::GreaterEqual => Binary::GreaterEqual,
            Tok::Plus => Binary::Add,
            Tok::Minus => Binary::Subtract,
            Tok::Star => Binary::Multiply,
            Tok::Slash => Binary::Divide,
            Tok::Mod => Binary::Remainder,
            _ => return None,
        })
    }

    fn binary(&mut self, level: u8) -> Result<Expr, Error> {
        if level > 5 {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        while let Some(op) = self.binary_op() {
            if op.level() != level {
                break;
            }
            self.advance();
            let right = self.binary(level + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
            if level == 3 {
                if self.binary_op().is_some_and(|next| next.level() == 3) {
                    return fail(self.line(), "G8", "comparisons do not chain; use `∧`");
                }
                break;
            }
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, Error> {
        if self.eat(&Tok::Not) {
            return Ok(Expr::Unary(Unary::Not, Box::new(self.unary()?)));
        }
        if self.eat(&Tok::Minus) {
            return Ok(Expr::Unary(Unary::Negate, Box::new(self.unary()?)));
        }
        match self.peek().clone() {
            Tok::Number(digits) => {
                self.advance();
                Ok(Expr::Integer(Int::parse(&digits).expect("digits")))
            }
            Tok::True => {
                self.advance();
                Ok(Expr::Boolean(true))
            }
            Tok::False => {
                self.advance();
                Ok(Expr::Boolean(false))
            }
            Tok::LeftParen => {
                self.advance();
                let inner = self.expr()?;
                self.expect(Tok::RightParen, "G8", "`)`")?;
                Ok(inner)
            }
            Tok::Name(name) => {
                self.advance();
                let next = &self.tokens[self.pos];
                if next.tok == Tok::LeftParen && !next.first_on_line {
                    return Ok(Expr::Call(name, self.arguments()?));
                }
                Ok(Expr::Name(name))
            }
            other => fail(
                self.line(),
                "G8",
                format!("expected an expression, found {}", describe_tok(&other)),
            ),
        }
    }

    fn arguments(&mut self) -> Result<Vec<Expr>, Error> {
        let mut out = Vec::new();
        self.expect(Tok::LeftParen, "G8", "`(`")?;
        while !self.eat(&Tok::RightParen) {
            if !out.is_empty() {
                self.expect(Tok::Comma, "G8", "`,`")?;
            }
            out.push(self.expr()?);
        }
        Ok(out)
    }

    fn payload(&mut self) -> Result<Vec<Expr>, Error> {
        let mut out = Vec::new();
        if self.eat(&Tok::LeftBrace) {
            while !self.eat(&Tok::RightBrace) {
                if !out.is_empty() {
                    self.expect(Tok::Comma, "G8", "`,`")?;
                }
                out.push(self.expr()?);
            }
        }
        Ok(out)
    }

    fn leave_outputs(&mut self) -> Result<Vec<LeaveOut>, Error> {
        let mut out = Vec::new();
        if self.eat(&Tok::LeftBrace) {
            while !self.eat(&Tok::RightBrace) {
                if !out.is_empty() {
                    self.expect(Tok::Comma, "U9", "`,`")?;
                }
                let field = self.name("U9", "a leave field name")?;
                let value = if self.eat(&Tok::Colon) {
                    self.expr()?
                } else {
                    Expr::Name(field.clone())
                };
                out.push(LeaveOut { name: field, value });
            }
        }
        Ok(out)
    }

    fn patterns(&mut self) -> Result<Option<Vec<Pattern>>, Error> {
        if !self.eat(&Tok::LeftBrace) {
            return Ok(None);
        }
        let mut out = Vec::new();
        while !self.eat(&Tok::RightBrace) {
            if !out.is_empty() {
                self.expect(Tok::Comma, "U8", "`,`")?;
            }
            let mutable = self.eat(&Tok::Tilde);
            let name = self.name("U8", "a payload name")?;
            let ty = if self.eat(&Tok::Colon) {
                Some(self.ty("U8")?)
            } else {
                None
            };
            out.push(Pattern { mutable, name, ty });
        }
        Ok(Some(out))
    }

    #[allow(clippy::type_complexity)]
    fn arm_head(&mut self) -> Result<(u32, bool, String, Option<Vec<Pattern>>), Error> {
        let line = self.line();
        if self.eat(&Tok::Ellipsis) {
            let group = self.name("U7", "an outcome group name")?;
            return Ok((line, true, group, None));
        }
        let name = self.name("U7", "an alternative")?;
        let patterns = self.patterns()?;
        Ok((line, false, name, patterns))
    }

    fn arm_after_arrow(&mut self, adapter: bool, allow_block: bool) -> Result<ArmTarget, Error> {
        let line = self.line();
        if self.peek() == &Tok::LeftBrace {
            if !allow_block {
                return fail(
                    line,
                    "Y7",
                    "an arm in the horizontal form cannot open a block",
                );
            }
            if adapter {
                return fail(
                    line,
                    "U13",
                    "an adapter arm must exit and cannot open a block",
                );
            }
            return Ok(ArmTarget::Block(self.plain_stmts_braced()?));
        }
        if self.eat(&Tok::Exit) {
            if matches!(
                self.peek(),
                Tok::Bar | Tok::Comma | Tok::RightBrace | Tok::Semi | Tok::End
            ) {
                return Ok(ArmTarget::SameExit);
            }
            let alt = self.name("U15", "an alternative")?;
            let payload = self.payload()?;
            return Ok(ArmTarget::Stmt(Box::new(SurfaceStmt::Exit {
                line,
                alternative: alt,
                payload,
            })));
        }
        if self.eat(&Tok::Impossible) {
            return Ok(ArmTarget::Stmt(Box::new(SurfaceStmt::Impossible { line })));
        }
        if adapter {
            let alt = self.name("U13", "an exit alternative")?;
            if self.peek() == &Tok::LeftBrace {
                let payload = self.payload()?;
                return Ok(ArmTarget::AdaptExit(alt, payload));
            }
            return Ok(ArmTarget::Name(alt));
        }
        match self.peek() {
            Tok::Leave | Tok::Again | Tok::Forward => {
                Ok(ArmTarget::Stmt(Box::new(self.simple_stmt()?)))
            }
            Tok::Name(_) => {
                let route = self.name("U2", "a route name")?;
                if self.peek() == &Tok::LeftBrace {
                    return fail(
                        line,
                        "U8",
                        "an exit in a decision arm requires `■`; only an adapter `■ Δ` omits `■`",
                    );
                }
                Ok(ArmTarget::Name(route))
            }
            _ => Ok(ArmTarget::Stmt(Box::new(self.simple_stmt()?))),
        }
    }

    fn horizontal_arms(&mut self, adapter: bool, braced: bool) -> Result<Vec<SurfaceArm>, Error> {
        let mut arms = Vec::new();
        loop {
            if braced && self.eat(&Tok::RightBrace) {
                break;
            }
            let (line, is_group, name, patterns) = self.arm_head()?;
            let target = if self.eat(&Tok::Arrow) {
                self.arm_after_arrow(adapter, braced)?
            } else {
                ArmTarget::None
            };
            arms.push(SurfaceArm {
                line,
                is_group,
                name,
                patterns,
                target,
            });
            if self.eat(&Tok::Bar) || (braced && self.eat(&Tok::Comma)) {
                continue;
            }
            if braced {
                self.expect(Tok::RightBrace, "G9", "`}`")?;
            }
            break;
        }
        Ok(arms)
    }

    fn decision_after_prefix(
        &mut self,
        line: u32,
        site: Option<String>,
        adapter: bool,
        allow_braced_arms: bool,
    ) -> Result<SurfaceStmt, Error> {
        let operand = self.expr()?;
        let arms = if self.eat(&Tok::Colon) {
            self.horizontal_arms(adapter, false)?
        } else if allow_braced_arms && self.eat(&Tok::LeftBrace) {
            self.horizontal_arms(adapter, true)?
        } else {
            return fail(
                line,
                "Y7",
                "a decision requires `: arms` or vertical `├` and `└` arms",
            );
        };
        Ok(SurfaceStmt::Decide {
            line,
            site,
            adapter,
            operand,
            arms,
        })
    }

    fn simple_stmt(&mut self) -> Result<SurfaceStmt, Error> {
        let line = self.line();
        match self.peek().clone() {
            Tok::Oblige => {
                self.advance();
                Ok(SurfaceStmt::Oblige {
                    line,
                    formula: self.expr()?,
                })
            }
            Tok::Impossible => {
                self.advance();
                Ok(SurfaceStmt::Impossible { line })
            }
            Tok::Exit => {
                self.advance();
                if self.eat(&Tok::Delta) {
                    return self.decision_after_prefix(line, None, true, true);
                }
                let alternative = self.name("U15", "an alternative")?;
                let payload = self.payload()?;
                Ok(SurfaceStmt::Exit {
                    line,
                    alternative,
                    payload,
                })
            }
            Tok::Leave => {
                self.advance();
                let alternative = match self.peek() {
                    Tok::Name(_) => Some(self.name("U9", "an alternative")?),
                    _ => None,
                };
                let outputs = if alternative.is_some() {
                    self.leave_outputs()?
                } else {
                    Vec::new()
                };
                Ok(SurfaceStmt::Leave {
                    line,
                    alternative,
                    outputs,
                })
            }
            Tok::Again => {
                self.advance();
                let name = self.name("W3", "a loop name")?;
                let arguments = if self.peek() == &Tok::LeftParen {
                    self.arguments()?
                } else {
                    Vec::new()
                };
                Ok(SurfaceStmt::Again {
                    line,
                    name,
                    arguments,
                })
            }
            Tok::Forward => {
                self.advance();
                let operation = self.name("U14", "a control name")?;
                let arguments = self.arguments()?;
                Ok(SurfaceStmt::Forward {
                    line,
                    operation,
                    arguments,
                })
            }
            Tok::Delta => {
                self.advance();
                self.decision_after_prefix(line, None, false, true)
            }
            Tok::Test => {
                self.advance();
                let condition = self.expr()?;
                self.expect(Tok::Arrow, "Y6", "`→` after test condition")?;
                let body = if self.peek() == &Tok::LeftBrace {
                    self.plain_stmts_braced()?
                } else {
                    vec![self.simple_stmt()?]
                };
                Ok(SurfaceStmt::Test {
                    line,
                    condition,
                    body,
                })
            }
            Tok::Tilde => {
                self.advance();
                let name = self.name("G8", "a variable name")?;
                let ty = if self.eat(&Tok::Colon) {
                    Some(self.ty("G8")?)
                } else {
                    None
                };
                self.expect(Tok::Bind, "G8", "`:=`")?;
                let value = self.expr()?;
                Ok(SurfaceStmt::Bind {
                    line,
                    mutable: true,
                    name,
                    ty,
                    value,
                })
            }
            Tok::Name(first) => {
                self.advance();
                if self.eat(&Tok::Assign) {
                    return Ok(SurfaceStmt::Assign {
                        line,
                        name: first,
                        value: self.expr()?,
                    });
                }
                if self.eat(&Tok::Bind) {
                    return Ok(SurfaceStmt::Bind {
                        line,
                        mutable: false,
                        name: first,
                        ty: None,
                        value: self.expr()?,
                    });
                }
                if self.eat(&Tok::Colon) {
                    if self.eat(&Tok::Delta) {
                        return self.decision_after_prefix(line, Some(first), false, true);
                    }
                    let ty = self.ty("G8")?;
                    self.expect(Tok::Bind, "G8", "`:=`")?;
                    let value = self.expr()?;
                    return Ok(SurfaceStmt::Bind {
                        line,
                        mutable: false,
                        name: first,
                        ty: Some(ty),
                        value,
                    });
                }
                if self.peek() == &Tok::LeftParen {
                    let arguments = self.arguments()?;
                    return Ok(SurfaceStmt::Call {
                        line,
                        operation: first,
                        arguments,
                    });
                }
                fail(
                    line,
                    "G8",
                    format!(
                        "expected `:=`, `←`, `:` or `(` after `{first}`, found {}",
                        describe_tok(self.peek())
                    ),
                )
            }
            other => fail(
                line,
                "G8",
                format!("expected a statement, found {}", describe_tok(&other)),
            ),
        }
    }

    fn loop_header(&mut self) -> Result<(u32, String, Vec<LoopParameter>), Error> {
        let line = self.line();
        self.expect(Tok::Loop, "Y4", "`↻`")?;
        let name = self.name("Y4", "a loop name")?;
        let mut parameters = Vec::new();
        if self.eat(&Tok::LeftParen) {
            while !self.eat(&Tok::RightParen) {
                if !parameters.is_empty() {
                    self.expect(Tok::Comma, "Y4", "`,`")?;
                }
                let mutable = self.eat(&Tok::Tilde);
                let param_name = self.name("Y4", "a loop parameter")?;
                self.expect(Tok::Colon, "Y4", "`:`")?;
                let ty = self.ty("Y4")?;
                self.expect(Tok::Bind, "Y4", "`:=`")?;
                let initial = self.expr()?;
                parameters.push(LoopParameter {
                    mutable,
                    name: param_name,
                    ty,
                    initial,
                });
            }
        }
        Ok((line, name, parameters))
    }

    fn plain_stmts_braced(&mut self) -> Result<Vec<SurfaceStmt>, Error> {
        self.expect(Tok::LeftBrace, "G9", "`{`")?;
        let mut out = Vec::new();
        while !self.eat(&Tok::RightBrace) {
            if self.eat(&Tok::Semi) {
                continue;
            }
            if self.peek() == &Tok::End {
                return fail(self.line(), "G9", "unclosed `{` block");
            }
            if self.peek() == &Tok::LeftBracket {
                return fail(
                    self.line(),
                    "Y5",
                    "a label `[mask]:` cannot stand inside a test, arm or label block",
                );
            }
            out.push(self.plain_stmt()?);
        }
        Ok(out)
    }

    fn plain_body_items(&mut self) -> Result<Vec<SurfaceItem>, Error> {
        self.expect(Tok::LeftBrace, "G9", "`{`")?;
        let mut items = Vec::new();
        while !self.eat(&Tok::RightBrace) {
            if self.eat(&Tok::Semi) {
                continue;
            }
            if self.peek() == &Tok::End {
                return fail(self.line(), "G9", "unclosed `{` body");
            }
            if self.eat(&Tok::LeftBracket) {
                let _ = self.line();
                let mask = self.mask()?;
                self.expect(Tok::RightBracket, "G9", "`]`")?;
                self.expect(Tok::Colon, "G9", "`:` after `[mask]`")?;
                let body = if self.peek() == &Tok::LeftBrace {
                    self.plain_stmts_braced()?
                } else {
                    let mut inline = Vec::new();
                    let label_line = self.line();
                    while self.peek() != &Tok::RightBrace
                        && self.peek() != &Tok::LeftBracket
                        && self.peek() != &Tok::End
                    {
                        inline.push(self.plain_stmt()?);
                        if self.eat(&Tok::Semi) {
                            if self.line() != label_line {
                                break;
                            }
                            continue;
                        }
                        if self.line() != label_line {
                            break;
                        }
                    }
                    inline
                };
                items.push(SurfaceItem::Label { mask, body });
            } else {
                items.push(SurfaceItem::Stmt(self.plain_stmt()?));
            }
        }
        Ok(items)
    }

    fn plain_stmt(&mut self) -> Result<SurfaceStmt, Error> {
        if self.peek() == &Tok::Loop {
            let (line, name, parameters) = self.loop_header()?;
            let mut raw_items = self.plain_body_items()?;
            let invariant = extract_loop_invariant(&mut raw_items);
            return Ok(SurfaceStmt::Loop {
                line,
                name,
                parameters,
                invariant,
                body: raw_items,
            });
        }
        self.simple_stmt()
    }
}

fn describe_tok(tok: &Tok) -> String {
    match tok {
        Tok::Name(name) => format!("`{name}`"),
        Tok::Number(digits) => format!("`{digits}`"),
        Tok::End => "the end of the line".to_string(),
        other => format!("{other:?}").to_lowercase(),
    }
}

/// `[G6]`: A `⊢` line that is the first statement of a loop body is the
/// loop's invariant.
fn extract_loop_invariant(items: &mut Vec<SurfaceItem>) -> Option<Expr> {
    if let Some(SurfaceItem::Stmt(SurfaceStmt::Oblige { .. })) = items.first() {
        let Some(SurfaceItem::Stmt(SurfaceStmt::Oblige { formula, .. })) = items.drain(..1).next()
        else {
            unreachable!();
        };
        return Some(formula);
    }
    None
}

fn parse_raw_stmt(item: RawItem) -> Result<SurfaceStmt, Error> {
    match item {
        RawItem::Simple { line, text } => {
            let mut parser = Parser::new(lex_fragment(&text, line)?);
            let stmt = parser.simple_stmt()?;
            if parser.peek() != &Tok::End {
                return fail(
                    line,
                    "G8",
                    format!("unexpected {} after statement", describe_tok(parser.peek())),
                );
            }
            Ok(stmt)
        }
        RawItem::Test {
            line,
            condition,
            body,
        } => {
            let mut parser = Parser::new(lex_fragment(&condition, line)?);
            let cond = parser.expr()?;
            if parser.peek() != &Tok::End {
                return fail(line, "Y6", "unexpected tokens in test condition");
            }
            let mut stmts = Vec::new();
            for child in body {
                if matches!(child, RawItem::Label { .. }) {
                    return fail(line, "Y5", "a label cannot stand inside a test block");
                }
                stmts.push(parse_raw_stmt(child)?);
            }
            Ok(SurfaceStmt::Test {
                line,
                condition: cond,
                body: stmts,
            })
        }
        RawItem::Decision { line, header, arms } => {
            let mut parser = Parser::new(lex_fragment(&header, line)?);
            let adapter = parser.eat(&Tok::Exit);
            let site = if !adapter
                && let Tok::Name(name) = parser.peek().clone()
                && parser.peek_next() == &Tok::Colon
            {
                parser.advance();
                parser.advance();
                Some(name)
            } else {
                None
            };
            parser.expect(Tok::Delta, "Y7", "`Δ`")?;
            let operand = parser.expr()?;
            if parser.peek() != &Tok::End {
                return fail(line, "Y7", "unexpected tokens in decision header");
            }
            let mut parsed_arms = Vec::new();
            for raw_arm in arms {
                let mut arm_parser = Parser::new(lex_fragment(&raw_arm.header, raw_arm.line)?);
                let (arm_line, is_group, name, patterns) = arm_parser.arm_head()?;
                let target = if let Some(block_items) = raw_arm.block {
                    if arm_parser.peek() != &Tok::End {
                        return fail(arm_line, "Y7", "unexpected tokens before arm block");
                    }
                    if adapter {
                        return fail(
                            arm_line,
                            "U13",
                            "an adapter arm must exit and cannot open a block",
                        );
                    }
                    let mut stmts = Vec::new();
                    for child in block_items {
                        stmts.push(parse_raw_stmt(child)?);
                    }
                    ArmTarget::Block(stmts)
                } else if arm_parser.eat(&Tok::Arrow) {
                    let target = arm_parser.arm_after_arrow(adapter, false)?;
                    if arm_parser.peek() != &Tok::End {
                        return fail(arm_line, "U8", "unexpected tokens after arm target");
                    }
                    target
                } else {
                    if arm_parser.peek() != &Tok::End {
                        return fail(arm_line, "U8", "unexpected tokens in arm");
                    }
                    ArmTarget::None
                };
                parsed_arms.push(SurfaceArm {
                    line: arm_line,
                    is_group,
                    name,
                    patterns,
                    target,
                });
            }
            Ok(SurfaceStmt::Decide {
                line,
                site,
                adapter,
                operand,
                arms: parsed_arms,
            })
        }
        RawItem::Loop { line, header, body } => {
            let mut parser = Parser::new(lex_fragment(&header, line)?);
            let (_, name, parameters) = parser.loop_header()?;
            if parser.peek() != &Tok::End {
                return fail(line, "Y4", "unexpected tokens after loop header");
            }
            let mut items = parse_raw_items(body)?;
            let invariant = extract_loop_invariant(&mut items);
            Ok(SurfaceStmt::Loop {
                line,
                name,
                parameters,
                invariant,
                body: items,
            })
        }
        RawItem::Label { line, .. } => fail(line, "Y5", "a label cannot stand inside a `┆` block"),
    }
}

fn parse_raw_items(raw: Vec<RawItem>) -> Result<Vec<SurfaceItem>, Error> {
    let mut out = Vec::new();
    for item in raw {
        match item {
            RawItem::Label { line, mask, body } => {
                let mut parser = Parser::new(lex_fragment(&mask, line)?);
                let parsed_mask = parser.mask()?;
                if parser.peek() != &Tok::End {
                    return fail(line, "G5", "unexpected tokens in label mask");
                }
                let mut stmts = Vec::new();
                for child in body {
                    stmts.push(parse_raw_stmt(child)?);
                }
                out.push(SurfaceItem::Label {
                    mask: parsed_mask,
                    body: stmts,
                });
            }
            other => out.push(SurfaceItem::Stmt(parse_raw_stmt(other)?)),
        }
    }
    Ok(out)
}

#[derive(Clone, Debug)]
enum TopDecl {
    Outcome(OutcomeDecl),
    Function(Function),
    Control(SurfaceControl),
}

fn starts_top_decl(trimmed: &str) -> bool {
    ["pub ", "outcome ", "function ", "extern ", "control "]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

fn parse_surface_file(source: &str) -> Result<Vec<TopDecl>, Error> {
    if let Some(pos) = source.find('\r') {
        let line = 1 + source[..pos].bytes().filter(|b| *b == b'\n').count() as u32;
        return fail(
            line,
            "Y1",
            "lines must end with a line feed `\\n`, not `\\r`",
        );
    }
    let lines: Vec<(u32, &str)> = source
        .lines()
        .enumerate()
        .map(|(idx, line)| (idx as u32 + 1, line))
        .collect();
    let mut index = 0usize;
    let mut decls = Vec::new();

    while index < lines.len() {
        let (line_no, raw) = lines[index];
        let stripped = strip_comment(raw);
        if stripped.trim().is_empty() {
            index += 1;
            continue;
        }
        // [Y1]: A declaration starts in the first column.
        if stripped.starts_with(' ') || stripped.starts_with('\t') {
            return fail(
                line_no,
                "Y1",
                "a declaration must start in the first column",
            );
        }
        let trimmed = stripped.trim();
        if !starts_top_decl(trimmed) {
            return fail(
                line_no,
                "Y1",
                format!("expected a declaration in column 1, found `{trimmed}`"),
            );
        }

        // Gather the declaration header and any 4-space-indented signature
        // continuation lines ([Y1]).
        let mut header = stripped.to_string();
        index += 1;
        while index < lines.len() {
            let (cont_line, cont_raw) = lines[index];
            let cont_stripped = strip_comment(cont_raw);
            if cont_stripped.trim().is_empty() {
                break;
            }
            if cont_stripped.starts_with("    ") {
                header.push('\n');
                header.push_str(cont_stripped);
                index += 1;
            } else if cont_stripped.starts_with(' ') || cont_stripped.starts_with('\t') {
                return fail(
                    cont_line,
                    "Y1",
                    "a signature continuation line must be indented by at least four spaces",
                );
            } else {
                break;
            }
        }

        // Plain braced form ([G9]): if the header opens `{` without closing it,
        // or if the next line starts with `{`, read until matching `}`!
        let mut brace_balance = 0i32;
        let mut saw_open_brace = false;
        let mut in_contract_braces = false;
        // Check whether `{` after the contract (or in `outcome name { ... }`) opens a plain body.
        for ch in header.chars() {
            if ch == '{' {
                brace_balance += 1;
                saw_open_brace = true;
            } else if ch == '}' {
                brace_balance -= 1;
            }
        }
        if brace_balance > 0 || (header.trim_end().ends_with('{')) {
            let _ = (saw_open_brace, &mut in_contract_braces);
            while index < lines.len() && brace_balance > 0 {
                let (_, next_raw) = lines[index];
                let next_stripped = strip_comment(next_raw);
                for ch in next_stripped.chars() {
                    if ch == '{' {
                        brace_balance += 1;
                    } else if ch == '}' {
                        brace_balance -= 1;
                    }
                }
                header.push('\n');
                header.push_str(next_stripped);
                index += 1;
            }
        }

        let mut parser = Parser::new(lex_fragment(&header, line_no)?);
        let public = parser.eat(&Tok::Pub);

        if parser.eat(&Tok::Outcome) {
            let name = parser.name("G2", "an outcome name")?;
            let mut alternatives = Vec::new();
            if parser.eat(&Tok::Colon) {
                loop {
                    alternatives.push(parser.outcome_alt()?);
                    if !parser.eat(&Tok::Bar) {
                        break;
                    }
                }
            } else if parser.eat(&Tok::LeftBrace) {
                while !parser.eat(&Tok::RightBrace) {
                    alternatives.push(parser.outcome_alt()?);
                    if !parser.eat(&Tok::Bar) && !parser.eat(&Tok::Comma) {
                        parser.expect(Tok::RightBrace, "G9", "`}`")?;
                        break;
                    }
                }
            } else {
                // Vertical outcome alternatives: `├ ` and `└ ` in column 1.
                let mut saw_last = false;
                while index < lines.len() {
                    let (alt_line, alt_raw) = lines[index];
                    let alt_stripped = strip_comment(alt_raw);
                    if alt_stripped.trim().is_empty() {
                        index += 1;
                        continue;
                    }
                    let (is_last, rest) = if let Some(after) = alt_stripped.strip_prefix('├') {
                        if !after.starts_with(' ') {
                            return fail(alt_line, "Y2", "`├` must be followed by a space");
                        }
                        (false, after[1..].trim())
                    } else if let Some(after) = alt_stripped.strip_prefix('└') {
                        if !after.starts_with(' ') {
                            return fail(alt_line, "Y2", "`└` must be followed by a space");
                        }
                        (true, after[1..].trim())
                    } else {
                        break;
                    };
                    if saw_last {
                        return fail(alt_line, "Y7", "no alternative may follow `└`");
                    }
                    let mut alt_parser = Parser::new(lex_fragment(rest, alt_line)?);
                    alternatives.push(alt_parser.outcome_alt()?);
                    if alt_parser.peek() != &Tok::End {
                        return fail(alt_line, "G2", "unexpected tokens in outcome alternative");
                    }
                    index += 1;
                    if is_last {
                        saw_last = true;
                        break;
                    }
                }
                if alternatives.is_empty() || !saw_last {
                    return fail(
                        line_no,
                        "Y7",
                        "vertical outcome alternatives must use `├` and end with `└`",
                    );
                }
            }
            if parser.peek() != &Tok::End {
                return fail(line_no, "G2", "unexpected tokens in `outcome` declaration");
            }
            if alternatives.is_empty() {
                return fail(line_no, "G2", "an `outcome` declaration needs alternatives");
            }
            decls.push(TopDecl::Outcome(OutcomeDecl {
                name,
                alternatives,
                line: line_no,
            }));
            continue;
        }

        if parser.eat(&Tok::Function) {
            let name = parser.name("G8", "a function name")?;
            let parameters = parser.parameters("G8")?;
            let precondition = if parser.eat(&Tok::Oblige) {
                Some(parser.expr()?)
            } else {
                None
            };
            parser.expect(Tok::Colon, "G8", "`:` and result type")?;
            let result = parser.ty("G8")?;
            parser.expect(Tok::Bind, "G8", "`:=`")?;
            let body = parser.expr()?;
            if parser.peek() != &Tok::End {
                return fail(line_no, "G8", "unexpected tokens after function body");
            }
            decls.push(TopDecl::Function(Function {
                name,
                parameters,
                precondition,
                result,
                body,
                line: line_no,
            }));
            continue;
        }

        let external = parser.eat(&Tok::Extern);
        parser.expect(Tok::Control, "G8", "`control`")?;
        let name = parser.name("G8", "a control name")?;
        let parameters = parser.parameters("G8")?;
        let precondition = if parser.eat(&Tok::Oblige) {
            Some(parser.expr()?)
        } else {
            None
        };
        parser.expect(Tok::Exit, "G1", "`■` and the contract")?;
        let contract = if parser.eat(&Tok::Auto) {
            SurfaceContract::Auto
        } else {
            let mut entries = Vec::new();
            parser.contract_entries(&mut entries)?;
            while parser.eat(&Tok::Exit) {
                parser.contract_entries(&mut entries)?;
            }
            if entries.is_empty() {
                return fail(line_no, "G1", "a contract must list at least one entry");
            }
            SurfaceContract::Entries(entries)
        };
        if parser.peek() == &Tok::Faults {
            return fail(
                parser.line(),
                "G8",
                "`faults` belongs to slice 2 and is not read yet",
            );
        }
        let mut uses = Vec::new();
        if parser.eat(&Tok::Uses) {
            loop {
                uses.push(parser.name("G8", "an effect")?);
                if !parser.eat(&Tok::Comma) {
                    break;
                }
            }
        }

        if external {
            if parser.peek() != &Tok::End {
                return fail(line_no, "G8", "an extern control has no body");
            }
            decls.push(TopDecl::Control(SurfaceControl {
                name,
                public,
                external: true,
                parameters,
                precondition,
                contract,
                uses,
                body: Vec::new(),
                line: line_no,
            }));
            continue;
        }

        // Plain form ([G9]) if the body opens with `{` on the signature line.
        if parser.peek() == &Tok::LeftBrace {
            let body = parser.plain_body_items()?;
            if parser.peek() != &Tok::End {
                return fail(parser.line(), "G9", "unexpected tokens after `}`");
            }
            decls.push(TopDecl::Control(SurfaceControl {
                name,
                public,
                external: false,
                parameters,
                precondition,
                contract,
                uses,
                body,
                line: line_no,
            }));
            continue;
        }

        if parser.peek() != &Tok::End {
            return fail(
                parser.line(),
                "G1",
                format!(
                    "unexpected {} in control signature",
                    describe_tok(parser.peek())
                ),
            );
        }

        // Rails form ([Y3]): read lines until `╵` in column 1.
        let mut rail_lines = Vec::new();
        let mut closed = false;
        while index < lines.len() {
            let (body_line, body_raw) = lines[index];
            let body_stripped = strip_comment(body_raw);
            if let Some(after) = body_stripped.strip_prefix('╵') {
                if !after.trim().is_empty() {
                    return fail(body_line, "Y3", "`╵` must stand alone on its line");
                }
                index += 1;
                closed = true;
                break;
            }
            if starts_top_decl(body_stripped.trim()) {
                return fail(
                    body_line,
                    "Y3",
                    format!("the body of `{name}` is not closed with `╵`"),
                );
            }
            rail_lines.push(parse_rail_line(body_line, body_raw)?);
            index += 1;
        }
        if !closed {
            return fail(
                line_no,
                "Y3",
                format!("the body of `{name}` is not closed with `╵`"),
            );
        }
        let raw_items = build_rails_body(&rail_lines)?;
        let body = parse_raw_items(raw_items)?;
        decls.push(TopDecl::Control(SurfaceControl {
            name,
            public,
            external: false,
            parameters,
            precondition,
            contract,
            uses,
            body,
            line: line_no,
        }));
    }

    Ok(decls)
}

// ---------------------------------------------------------------------------
// Stage 3: Elaboration ([G2], [G3], [U1] to [U15]) into syntax::Program
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct ResolvedContract {
    alternatives: Vec<Alternative>,
    /// Outcome groups included by name (`…g` or `■ g`).
    groups: Vec<String>,
    /// Alternatives listed directly outside any group.
    own_alternatives: Vec<String>,
}

#[derive(Clone, Debug)]
struct SiteRecord {
    surface_name: String,
    core_name: String,
    alternatives: Vec<String>,
}

#[derive(Clone, Debug)]
struct PathEntry {
    core_site: String,
    alternative: String,
    path_name: String,
    is_route: bool,
}

#[derive(Clone, Debug)]
struct BodyScope {
    sites: Vec<SiteRecord>,
    paths: Vec<PathEntry>,
    /// Alternative names that had a route, terminator or block and therefore
    /// have no default unqualified path name ([U2]).
    suppressed_defaults: HashSet<String>,
    /// Variable types known in this body, for `■ auto` ([G3]) and `Δ bool` ([U6]).
    vars: HashMap<String, Type>,
}

#[derive(Clone, Debug)]
struct AutoExit {
    line: u32,
    alternative: String,
    field_names: Vec<String>,
    field_types: Vec<Option<Type>>,
}

struct Elaborator {
    outcomes: HashMap<String, Vec<Alternative>>,
    functions: HashMap<String, Type>,
    controls: HashMap<String, SurfaceControl>,
    contracts: HashMap<String, ResolvedContract>,
    resolving_auto: HashSet<String>,
}

impl Elaborator {
    fn resolve_written_contract(
        &self,
        entries: &[SurfaceContractEntry],
        line: u32,
    ) -> Result<ResolvedContract, Error> {
        // [G2]: A contract that is one name declared as an outcome is that
        // declaration's contract.
        if let [SurfaceContractEntry::Alt(single)] = entries
            && single.fields.is_empty()
            && single.condition.is_none()
            && let Some(group_alts) = self.outcomes.get(&single.name)
        {
            return Ok(ResolvedContract {
                alternatives: group_alts.clone(),
                groups: vec![single.name.clone()],
                own_alternatives: Vec::new(),
            });
        }

        let mut alternatives: Vec<Alternative> = Vec::new();
        let mut groups = Vec::new();
        let mut own_alternatives = Vec::new();

        for entry in entries {
            match entry {
                SurfaceContractEntry::Group(group, entry_line) => {
                    let Some(group_alts) = self.outcomes.get(group) else {
                        return fail(
                            *entry_line,
                            "G2",
                            format!("`{group}` is not a declared outcome"),
                        );
                    };
                    if !groups.contains(group) {
                        groups.push(group.clone());
                    }
                    for alt in group_alts {
                        if alternatives.iter().any(|a| a.name == alt.name) {
                            return fail(
                                *entry_line,
                                "W7",
                                format!("the contract lists `{}` twice", alt.name),
                            );
                        }
                        alternatives.push(alt.clone());
                    }
                }
                SurfaceContractEntry::Alt(alt) => {
                    if alternatives.iter().any(|a| a.name == alt.name) {
                        return fail(
                            line,
                            "W7",
                            format!("the contract lists `{}` twice", alt.name),
                        );
                    }
                    own_alternatives.push(alt.name.clone());
                    alternatives.push(alt.clone());
                }
            }
        }

        Ok(ResolvedContract {
            alternatives,
            groups,
            own_alternatives,
        })
    }

    fn ensure_contract(&mut self, name: &str, call_line: u32) -> Result<ResolvedContract, Error> {
        if let Some(contract) = self.contracts.get(name) {
            return Ok(contract.clone());
        }
        let Some(control) = self.controls.get(name).cloned() else {
            return fail(call_line, "C1", format!("`{name}` is not a control"));
        };
        if !self.resolving_auto.insert(name.to_string()) {
            return fail(
                control.line,
                "G3",
                format!(
                    "`{name}` has `■ auto` and reaches itself through `■ auto` controls; write its contract"
                ),
            );
        }
        let elaborated = self.elaborate_control(&control)?;
        self.resolving_auto.remove(name);
        let _ = elaborated;
        Ok(self.contracts.get(name).expect("inferred contract").clone())
    }

    fn expr_type(&self, expr: &Expr, scopes: &[BodyScope]) -> Option<Type> {
        match expr {
            Expr::Integer(_) => None,
            Expr::Boolean(_) => Some(Type::Bool),
            Expr::Name(name) => scopes
                .iter()
                .rev()
                .find_map(|scope| scope.vars.get(name).copied()),
            Expr::Unary(Unary::Not, _) => Some(Type::Bool),
            Expr::Unary(Unary::Negate, inner) => self.expr_type(inner, scopes),
            Expr::Binary(op, left, right) => {
                if op.level() <= 3 {
                    Some(Type::Bool)
                } else {
                    self.expr_type(left, scopes)
                        .or_else(|| self.expr_type(right, scopes))
                }
            }
            Expr::Call(func, _) => self.functions.get(func).copied(),
        }
    }

    fn elaborate_control(&mut self, control: &SurfaceControl) -> Result<Control, Error> {
        let is_auto = matches!(control.contract, SurfaceContract::Auto);
        if is_auto {
            if control.public {
                return fail(
                    control.line,
                    "G3",
                    "`■ auto` is not allowed on a `pub` control",
                );
            }
            if control.external {
                return fail(
                    control.line,
                    "G3",
                    "`■ auto` is not allowed on an `extern` control",
                );
            }
        } else if !self.contracts.contains_key(&control.name) {
            let SurfaceContract::Entries(entries) = &control.contract else {
                unreachable!();
            };
            let resolved = self.resolve_written_contract(entries, control.line)?;
            self.contracts.insert(control.name.clone(), resolved);
        }

        if control.external {
            let contract = self.contracts[&control.name].alternatives.clone();
            return Ok(Control {
                name: control.name.clone(),
                public: control.public,
                external: true,
                parameters: control.parameters.clone(),
                precondition: control.precondition.clone(),
                contract,
                uses: control.uses.clone(),
                body: Vec::new(),
                line: control.line,
            });
        }

        let mut root_scope = BodyScope {
            sites: Vec::new(),
            paths: Vec::new(),
            suppressed_defaults: HashSet::new(),
            vars: HashMap::new(),
        };
        for param in &control.parameters {
            root_scope.vars.insert(param.name.clone(), param.ty);
        }

        let mut ctx = ControlElab {
            control_name: control.name.clone(),
            is_auto,
            used_core_sites: HashSet::new(),
            explicit_sites: HashSet::new(),
            all_declared_sites: HashSet::new(),
            all_declared_paths: HashSet::new(),
            next_hidden: 0,
            scopes: vec![root_scope],
            auto_exits: Vec::new(),
            auto_groups: Vec::new(),
        };
        collect_declared_paths(
            &control.body,
            &mut ctx.all_declared_sites,
            &mut ctx.all_declared_paths,
        );

        let statements = self.elaborate_items(&control.body, &mut ctx)?;

        if is_auto {
            let inferred =
                self.build_auto_contract(control.line, &ctx.auto_exits, &ctx.auto_groups)?;
            self.contracts.insert(control.name.clone(), inferred);
        }

        let contract = self.contracts[&control.name].alternatives.clone();
        Ok(Control {
            name: control.name.clone(),
            public: control.public,
            external: false,
            parameters: control.parameters.clone(),
            precondition: control.precondition.clone(),
            contract,
            uses: control.uses.clone(),
            body: statements,
            line: control.line,
        })
    }

    fn build_auto_contract(
        &self,
        line: u32,
        exits: &[AutoExit],
        groups: &[String],
    ) -> Result<ResolvedContract, Error> {
        if exits.is_empty() {
            return fail(
                line,
                "G3",
                "a control with `■ auto` must have at least one exit",
            );
        }
        let mut order: Vec<String> = Vec::new();
        let mut by_alt: HashMap<String, Vec<&AutoExit>> = HashMap::new();
        for exit in exits {
            if !by_alt.contains_key(&exit.alternative) {
                order.push(exit.alternative.clone());
            }
            by_alt
                .entry(exit.alternative.clone())
                .or_default()
                .push(exit);
        }

        let mut alternatives = Vec::new();
        for alt_name in &order {
            let group = &by_alt[alt_name];
            let arity = group[0].field_types.len();
            for item in group.iter().skip(1) {
                if item.field_types.len() != arity {
                    return fail(
                        item.line,
                        "G3",
                        format!("exits for `{alt_name}` differ in payload field count"),
                    );
                }
            }
            let mut fields = Vec::new();
            for field_idx in 0..arity {
                let mut decided: Option<Type> = None;
                for item in group {
                    match (decided, item.field_types[field_idx]) {
                        (Some(a), Some(b)) if a != b => {
                            return fail(
                                item.line,
                                "G3",
                                format!(
                                    "exits for `{alt_name}` differ in payload type: {a} and {b}"
                                ),
                            );
                        }
                        (None, Some(b)) => decided = Some(b),
                        _ => {}
                    }
                }
                let ty = decided.unwrap_or(Type::Int(IntType::U32));
                let name = group[0]
                    .field_names
                    .get(field_idx)
                    .cloned()
                    .unwrap_or_else(|| format!("v{field_idx}"));
                fields.push(Parameter { name, ty });
            }
            alternatives.push(Alternative {
                name: alt_name.clone(),
                fields,
                condition: None,
            });
        }

        let mut kept_groups = Vec::new();
        for group_name in groups {
            if let Some(group_alts) = self.outcomes.get(group_name)
                && group_alts
                    .iter()
                    .all(|ga| alternatives.iter().any(|a| a.name == ga.name))
                && !kept_groups.contains(group_name)
            {
                kept_groups.push(group_name.clone());
            }
        }
        let own_alternatives = order
            .into_iter()
            .filter(|alt_name| {
                !kept_groups
                    .iter()
                    .any(|g| self.outcomes[g].iter().any(|ga| &ga.name == alt_name))
            })
            .collect();

        Ok(ResolvedContract {
            alternatives,
            groups: kept_groups,
            own_alternatives,
        })
    }

    fn record_exit(
        &self,
        ctx: &mut ControlElab,
        line: u32,
        alternative: &str,
        payload: &[Expr],
        field_names: Option<Vec<String>>,
        field_types: Option<Vec<Option<Type>>>,
    ) -> Result<(), Error> {
        let types = match field_types {
            Some(types) => types,
            None => payload
                .iter()
                .map(|e| self.expr_type(e, &ctx.scopes))
                .collect(),
        };
        let names =
            field_names.unwrap_or_else(|| (0..types.len()).map(|idx| format!("v{idx}")).collect());
        if ctx.is_auto {
            ctx.auto_exits.push(AutoExit {
                line,
                alternative: alternative.to_string(),
                field_names: names,
                field_types: types,
            });
            return Ok(());
        }
        let caller = &self.contracts[&ctx.control_name];
        let Some(entry) = caller.alternatives.iter().find(|a| a.name == alternative) else {
            return fail(
                line,
                "U15",
                format!(
                    "`{alternative}` is not an alternative of `{}`",
                    ctx.control_name
                ),
            );
        };
        if entry.fields.len() != types.len() {
            return fail(
                line,
                "U15",
                format!(
                    "`{alternative}` takes {} payload values, and {} are given",
                    entry.fields.len(),
                    types.len()
                ),
            );
        }
        Ok(())
    }

    fn elaborate_items(
        &mut self,
        items: &[SurfaceItem],
        ctx: &mut ControlElab,
    ) -> Result<Vec<Statement>, Error> {
        let mut out = Vec::new();
        for item in items {
            match item {
                SurfaceItem::Stmt(stmt) => {
                    self.elaborate_stmt(stmt, &Selector::Top, ctx, &mut out)?;
                }
                SurfaceItem::Label { mask, body, .. } => {
                    let selector = self.elaborate_mask(mask, ctx)?;
                    for stmt in body {
                        self.elaborate_stmt(stmt, &selector, ctx, &mut out)?;
                    }
                }
            }
        }
        Ok(out)
    }

    fn elaborate_mask(&self, mask: &Mask, ctx: &ControlElab) -> Result<Selector, Error> {
        match mask {
            Mask::And(left, right) => Ok(Selector::And(
                Box::new(self.elaborate_mask(left, ctx)?),
                Box::new(self.elaborate_mask(right, ctx)?),
            )),
            Mask::Or(left, right) => Ok(Selector::Or(
                Box::new(self.elaborate_mask(left, ctx)?),
                Box::new(self.elaborate_mask(right, ctx)?),
            )),
            Mask::Not { site, name, line } => Ok(Selector::Not(Box::new(self.resolve_path(
                site.as_deref(),
                name,
                *line,
                ctx,
            )?))),
            Mask::Path { site, name, line } => self.resolve_path(site.as_deref(), name, *line, ctx),
        }
    }

    fn resolve_path(
        &self,
        site: Option<&str>,
        name: &str,
        line: u32,
        ctx: &ControlElab,
    ) -> Result<Selector, Error> {
        if let Some(site_name) = site {
            let matching_sites: Vec<&SiteRecord> = ctx
                .scopes
                .iter()
                .flat_map(|scope| &scope.sites)
                .filter(|record| record.surface_name == site_name)
                .collect();
            if matching_sites.is_empty() {
                if ctx.all_declared_sites.contains(site_name) {
                    return fail(
                        line,
                        "U4",
                        format!("the site `{site_name}` does not precede this label"),
                    );
                }
                return fail(
                    line,
                    "U3",
                    format!("no site `{site_name}` is in scope here"),
                );
            }
            if matching_sites.len() > 1 {
                return fail(
                    line,
                    "U1",
                    format!(
                        "two decisions in scope are named `{site_name}`; give each an explicit site name `name : Δ`"
                    ),
                );
            }
            let record = matching_sites[0];
            if !record.alternatives.iter().any(|alt| alt == name) {
                return fail(
                    line,
                    "U3",
                    format!("the site `{site_name}` has no alternative `{name}`"),
                );
            }
            return Ok(Selector::Atom(record.core_name.clone(), name.to_string()));
        }

        let entries: Vec<&PathEntry> = ctx
            .scopes
            .iter()
            .flat_map(|scope| &scope.paths)
            .filter(|entry| entry.path_name == name)
            .collect();
        if entries.is_empty() {
            let was_suppressed = ctx
                .scopes
                .iter()
                .any(|scope| scope.suppressed_defaults.contains(name));
            if was_suppressed {
                return fail(
                    line,
                    "U2",
                    format!(
                        "`{name}` was renamed by a route or ended in a terminator or block, so it is not a path name"
                    ),
                );
            }
            if ctx.all_declared_paths.contains(name) {
                return fail(
                    line,
                    "U4",
                    format!("the path `{name}` does not precede this label"),
                );
            }
            return fail(line, "U3", format!("`{name}` is not a path name here"));
        }
        let default_count = entries.iter().filter(|e| !e.is_route).count();
        if entries.len() > 1 && default_count > 1 {
            return fail(
                line,
                "U3",
                format!(
                    "`{name}` is the default name of more than one arm; qualify it as `site.{name}`"
                ),
            );
        }
        let mut iter = entries
            .into_iter()
            .map(|e| Selector::Atom(e.core_site.clone(), e.alternative.clone()));
        let first = iter.next().expect("at least one entry");
        Ok(iter.fold(first, |acc, atom| {
            Selector::Or(Box::new(acc), Box::new(atom))
        }))
    }

    fn elaborate_stmt(
        &mut self,
        stmt: &SurfaceStmt,
        selection: &Selector,
        ctx: &mut ControlElab,
        out: &mut Vec<Statement>,
    ) -> Result<(), Error> {
        match stmt {
            SurfaceStmt::Bind {
                line,
                mutable,
                name,
                ty,
                value,
            } => {
                let inferred = ty
                    .or_else(|| self.expr_type(value, &ctx.scopes))
                    .unwrap_or(Type::Int(IntType::U32));
                ctx.current_scope_mut().vars.insert(name.clone(), inferred);
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Bind {
                        mutable: *mutable,
                        name: name.clone(),
                        ty: *ty,
                        value: value.clone(),
                    },
                    line: *line,
                });
            }
            SurfaceStmt::Assign { line, name, value } => {
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Assign {
                        name: name.clone(),
                        value: value.clone(),
                    },
                    line: *line,
                });
            }
            SurfaceStmt::Oblige { line, formula } => {
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Oblige(formula.clone()),
                    line: *line,
                });
            }
            SurfaceStmt::Impossible { line } => {
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Impossible,
                    line: *line,
                });
            }
            SurfaceStmt::Again {
                line,
                name,
                arguments,
            } => {
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Again {
                        name: name.clone(),
                        arguments: arguments.clone(),
                    },
                    line: *line,
                });
            }
            SurfaceStmt::Leave {
                line,
                alternative,
                outputs,
            } => {
                let alt_name = alternative.clone().unwrap_or_default();
                let payload = outputs.iter().map(|item| item.value.clone()).collect();
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Leave {
                        alternative: alt_name,
                        payload,
                    },
                    line: *line,
                });
            }
            SurfaceStmt::Exit {
                line,
                alternative,
                payload,
            } => {
                self.record_exit(ctx, *line, alternative, payload, None, None)?;
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Exit {
                        alternative: alternative.clone(),
                        payload: payload.clone(),
                    },
                    line: *line,
                });
            }
            SurfaceStmt::Test {
                line,
                condition,
                body,
            } => {
                let site = ctx.fresh_test_site();
                ctx.current_scope_mut().sites.push(SiteRecord {
                    surface_name: site.clone(),
                    core_name: site.clone(),
                    alternatives: vec!["true".to_string(), "false".to_string()],
                });
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Test {
                        site: site.clone(),
                        condition: condition.clone(),
                    },
                    line: *line,
                });
                let inner_sel = Selector::Atom(site, "true".to_string());
                for child in body {
                    self.elaborate_stmt(child, &inner_sel, ctx, out)?;
                }
            }
            SurfaceStmt::Call {
                line,
                operation,
                arguments,
            } => {
                // [U6]: ω(ā) is a decision on an operation that has exactly one
                // alternative, and its payload, if any, is dropped.
                let contract = self.ensure_contract(operation, *line)?;
                if contract.alternatives.len() != 1 {
                    return fail(
                        *line,
                        "U6",
                        format!(
                            "`{operation}` has {} alternatives and must be decided with `Δ`",
                            contract.alternatives.len()
                        ),
                    );
                }
                let single = &contract.alternatives[0];
                let core_site = ctx.allocate_site(None, operation, *line)?;
                let hidden_names: Vec<String> = (0..single.fields.len())
                    .map(|_| ctx.fresh_hidden_var())
                    .collect();
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Decide {
                        site: core_site,
                        operation: operation.clone(),
                        arguments: arguments.clone(),
                        arms: vec![Arm {
                            alternative: single.name.clone(),
                            names: hidden_names,
                        }],
                    },
                    line: *line,
                });
            }
            SurfaceStmt::Forward {
                line,
                operation,
                arguments,
            } => {
                // [U14]: forward ω(ā) requires every entry of the callee's
                // contract (single alternative or group included by name) to be
                // an entry of the caller's contract with the same payload types.
                let callee = self.ensure_contract(operation, *line)?;
                if ctx.is_auto {
                    for group in &callee.groups {
                        if !ctx.auto_groups.contains(group) {
                            ctx.auto_groups.push(group.clone());
                        }
                    }
                } else {
                    let caller = &self.contracts[&ctx.control_name];
                    for group in &callee.groups {
                        if !caller.groups.contains(group) {
                            return fail(
                                *line,
                                "U14",
                                format!(
                                    "`forward {operation}` requires `{}` to include the outcome group `…{group}`",
                                    ctx.control_name
                                ),
                            );
                        }
                    }
                    for own in &callee.own_alternatives {
                        if !caller.own_alternatives.contains(own) {
                            return fail(
                                *line,
                                "U14",
                                format!(
                                    "`forward {operation}` requires `{own}` to be an own entry of `{}`",
                                    ctx.control_name
                                ),
                            );
                        }
                    }
                    for callee_alt in &callee.alternatives {
                        let Some(caller_alt) = caller
                            .alternatives
                            .iter()
                            .find(|a| a.name == callee_alt.name)
                        else {
                            return fail(
                                *line,
                                "U14",
                                format!(
                                    "`{}` is not an alternative of `{}`",
                                    callee_alt.name, ctx.control_name
                                ),
                            );
                        };
                        let same_types = caller_alt.fields.len() == callee_alt.fields.len()
                            && caller_alt
                                .fields
                                .iter()
                                .zip(&callee_alt.fields)
                                .all(|(a, b)| a.ty == b.ty);
                        if !same_types {
                            return fail(
                                *line,
                                "U14",
                                format!(
                                    "the payload of `{}` in `{operation}` does not match `{}`",
                                    callee_alt.name, ctx.control_name
                                ),
                            );
                        }
                    }
                }

                let core_site = ctx.allocate_site(None, operation, *line)?;
                let mut core_arms = Vec::new();
                let mut exits = Vec::new();
                for alt in &callee.alternatives {
                    let vars: Vec<String> = (0..alt.fields.len())
                        .map(|_| ctx.fresh_hidden_var())
                        .collect();
                    let payload: Vec<Expr> = vars.iter().cloned().map(Expr::Name).collect();
                    let field_names = Some(alt.fields.iter().map(|f| f.name.clone()).collect());
                    let field_types = Some(alt.fields.iter().map(|f| Some(f.ty)).collect());
                    self.record_exit(ctx, *line, &alt.name, &payload, field_names, field_types)?;
                    core_arms.push(Arm {
                        alternative: alt.name.clone(),
                        names: vars,
                    });
                    exits.push((alt.name.clone(), payload));
                }
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Decide {
                        site: core_site.clone(),
                        operation: operation.clone(),
                        arguments: arguments.clone(),
                        arms: core_arms,
                    },
                    line: *line,
                });
                for (alt_name, payload) in exits {
                    out.push(Statement {
                        selector: Selector::Atom(core_site.clone(), alt_name.clone()),
                        command: Command::Exit {
                            alternative: alt_name,
                            payload,
                        },
                        line: *line,
                    });
                }
            }
            SurfaceStmt::Decide {
                line,
                site,
                adapter,
                operand,
                arms,
            } => {
                // Distinguish a control decision `Δ op(args)` from a Boolean
                // decision `Δ bool_expr` ([U6]).
                if let Expr::Call(op_name, arguments) = operand
                    && self.controls.contains_key(op_name)
                {
                    self.elaborate_control_decision(
                        *line,
                        site.as_deref(),
                        *adapter,
                        op_name,
                        arguments,
                        arms,
                        selection,
                        ctx,
                        out,
                    )?;
                } else {
                    if *adapter {
                        return fail(*line, "U13", "`■ Δ` requires a control call as its operand");
                    }
                    self.elaborate_bool_decision(
                        *line,
                        site.as_deref(),
                        operand,
                        arms,
                        selection,
                        ctx,
                        out,
                    )?;
                }
            }
            SurfaceStmt::Loop {
                line,
                name,
                parameters,
                invariant,
                body,
            } => {
                self.elaborate_loop(
                    *line,
                    name,
                    parameters,
                    invariant.as_ref(),
                    body,
                    selection,
                    ctx,
                    out,
                )?;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn elaborate_bool_decision(
        &mut self,
        line: u32,
        explicit_site: Option<&str>,
        operand: &Expr,
        arms: &[SurfaceArm],
        selection: &Selector,
        ctx: &mut ControlElab,
        out: &mut Vec<Statement>,
    ) -> Result<(), Error> {
        let default_name = match operand {
            Expr::Name(var) => var.as_str(),
            _ => "_bool",
        };
        let surface_site = explicit_site.unwrap_or(default_name).to_string();
        let core_site = if default_name == "_bool" && explicit_site.is_none() {
            ctx.fresh_test_site()
        } else {
            ctx.allocate_site(explicit_site, default_name, line)?
        };

        // [U6], [U7]: A decision on a `bool` has the alternatives `true` and
        // `false`, and each needs a target.
        let mut saw_true = false;
        let mut saw_false = false;
        for arm in arms {
            if arm.is_group {
                return fail(arm.line, "U7", "a Boolean decision has no outcome group");
            }
            if arm.patterns.as_ref().is_some_and(|p| !p.is_empty()) {
                return fail(arm.line, "U8", "a Boolean alternative has no payload");
            }
            match arm.name.as_str() {
                "true" if !saw_true => saw_true = true,
                "false" if !saw_false => saw_false = true,
                "true" | "false" => {
                    return fail(
                        arm.line,
                        "U7",
                        format!("the alternative `{}` is listed twice", arm.name),
                    );
                }
                other => {
                    return fail(
                        arm.line,
                        "U7",
                        format!("`{other}` is not an alternative of `bool` (`true` or `false`)"),
                    );
                }
            }
            if matches!(arm.target, ArmTarget::None) {
                return fail(
                    arm.line,
                    "U6",
                    "each arm of a decision on a `bool` needs a target (`→`)",
                );
            }
        }
        if !saw_true || !saw_false {
            return fail(
                line,
                "U7",
                "a decision on a `bool` must list both `true` and `false`",
            );
        }

        ctx.current_scope_mut().sites.push(SiteRecord {
            surface_name: surface_site,
            core_name: core_site.clone(),
            alternatives: vec!["true".to_string(), "false".to_string()],
        });
        out.push(Statement {
            selector: selection.clone(),
            command: Command::Test {
                site: core_site.clone(),
                condition: operand.clone(),
            },
            line,
        });

        for arm in arms {
            let arm_sel = Selector::Atom(core_site.clone(), arm.name.clone());
            match &arm.target {
                ArmTarget::Name(route) => {
                    ctx.current_scope_mut().paths.push(PathEntry {
                        core_site: core_site.clone(),
                        alternative: arm.name.clone(),
                        path_name: route.clone(),
                        is_route: true,
                    });
                }
                ArmTarget::SameExit => {
                    self.record_exit(ctx, arm.line, &arm.name, &[], None, None)?;
                    out.push(Statement {
                        selector: arm_sel,
                        command: Command::Exit {
                            alternative: arm.name.clone(),
                            payload: Vec::new(),
                        },
                        line: arm.line,
                    });
                }
                ArmTarget::Stmt(stmt) => {
                    self.elaborate_stmt(stmt, &arm_sel, ctx, out)?;
                }
                ArmTarget::Block(stmts) => {
                    for stmt in stmts {
                        self.elaborate_stmt(stmt, &arm_sel, ctx, out)?;
                    }
                }
                ArmTarget::None | ArmTarget::AdaptExit(_, _) => unreachable!(),
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn elaborate_control_decision(
        &mut self,
        line: u32,
        explicit_site: Option<&str>,
        adapter: bool,
        operation: &str,
        arguments: &[Expr],
        arms: &[SurfaceArm],
        selection: &Selector,
        ctx: &mut ControlElab,
        out: &mut Vec<Statement>,
    ) -> Result<(), Error> {
        let callee = self.ensure_contract(operation, line)?;
        let surface_site = explicit_site.unwrap_or(operation).to_string();
        let core_site = ctx.allocate_site(explicit_site, operation, line)?;

        // [U7] Acknowledgement: check explicit arms first, then expand group arms.
        let mut explicit_names = HashSet::new();
        for arm in arms {
            if !arm.is_group {
                if !callee.alternatives.iter().any(|a| a.name == arm.name) {
                    return fail(
                        arm.line,
                        "U7",
                        format!("`{}` is not an alternative of `{operation}`", arm.name),
                    );
                }
                if !explicit_names.insert(arm.name.clone()) {
                    return fail(
                        arm.line,
                        "U7",
                        format!("the alternative `{}` is listed twice", arm.name),
                    );
                }
            }
        }

        struct ExpandedArm {
            line: u32,
            alternative: String,
            patterns: Option<Vec<Pattern>>,
            target: ArmTarget,
            from_group: Option<String>,
        }

        let mut expanded = Vec::new();
        let mut covered = HashSet::new();
        for arm in arms {
            if arm.is_group {
                if !callee.groups.contains(&arm.name) {
                    return fail(
                        arm.line,
                        "U7",
                        format!(
                            "`{operation}` does not include the outcome group `{}` in its contract",
                            arm.name
                        ),
                    );
                }
                let group_alts = &self.outcomes[&arm.name];
                let mut any_in_group = false;
                for group_alt in group_alts {
                    if explicit_names.contains(&group_alt.name) {
                        continue;
                    }
                    if !covered.insert(group_alt.name.clone()) {
                        return fail(
                            arm.line,
                            "U7",
                            format!(
                                "the alternative `{}` is covered by more than one group arm",
                                group_alt.name
                            ),
                        );
                    }
                    any_in_group = true;
                    expanded.push(ExpandedArm {
                        line: arm.line,
                        alternative: group_alt.name.clone(),
                        patterns: None,
                        target: arm.target.clone(),
                        from_group: Some(arm.name.clone()),
                    });
                }
                if !any_in_group {
                    return fail(
                        arm.line,
                        "U7",
                        format!(
                            "every alternative of `…{}` is already listed explicitly",
                            arm.name
                        ),
                    );
                }
            } else {
                covered.insert(arm.name.clone());
                expanded.push(ExpandedArm {
                    line: arm.line,
                    alternative: arm.name.clone(),
                    patterns: arm.patterns.clone(),
                    target: arm.target.clone(),
                    from_group: None,
                });
            }
        }

        for alt in &callee.alternatives {
            if !covered.contains(&alt.name) {
                return fail(
                    line,
                    "U7",
                    format!(
                        "the decision on `{operation}` does not acknowledge `{}`",
                        alt.name
                    ),
                );
            }
        }

        ctx.current_scope_mut().sites.push(SiteRecord {
            surface_name: surface_site,
            core_name: core_site.clone(),
            alternatives: expanded.iter().map(|a| a.alternative.clone()).collect(),
        });

        let mut core_arms = Vec::new();
        #[allow(clippy::type_complexity)]
        let mut post_actions: Vec<(ExpandedArm, Vec<String>, Vec<(String, Type, String)>)> =
            Vec::new();

        for exp in expanded {
            let contract_alt = callee
                .alternatives
                .iter()
                .find(|a| a.name == exp.alternative)
                .expect("a contract alternative");
            let mut arm_vars = Vec::new();
            let mut mutable_rebinds = Vec::new();

            if let Some(patterns) = &exp.patterns {
                if patterns.len() != contract_alt.fields.len() {
                    return fail(
                        exp.line,
                        "U8",
                        format!(
                            "`{}` has {} payload fields, and {} patterns are given",
                            exp.alternative,
                            contract_alt.fields.len(),
                            patterns.len()
                        ),
                    );
                }
                for (pat, field) in patterns.iter().zip(&contract_alt.fields) {
                    if let Some(written_ty) = pat.ty
                        && written_ty != field.ty
                    {
                        return fail(
                            exp.line,
                            "U8",
                            format!(
                                "`{}` has type {} in the contract of `{operation}`, not {written_ty}",
                                pat.name, field.ty
                            ),
                        );
                    }
                    ctx.current_scope_mut()
                        .vars
                        .insert(pat.name.clone(), field.ty);
                    if pat.mutable {
                        let raw = ctx.fresh_hidden_var();
                        arm_vars.push(raw.clone());
                        mutable_rebinds.push((pat.name.clone(), field.ty, raw));
                    } else {
                        arm_vars.push(pat.name.clone());
                    }
                }
            } else {
                for _ in &contract_alt.fields {
                    arm_vars.push(ctx.fresh_hidden_var());
                }
            }

            core_arms.push(Arm {
                alternative: exp.alternative.clone(),
                names: arm_vars.clone(),
            });
            post_actions.push((exp, arm_vars, mutable_rebinds));
        }

        out.push(Statement {
            selector: selection.clone(),
            command: Command::Decide {
                site: core_site.clone(),
                operation: operation.to_string(),
                arguments: arguments.to_vec(),
                arms: core_arms,
            },
            line,
        });

        for (exp, arm_vars, mutable_rebinds) in post_actions {
            let arm_sel = Selector::Atom(core_site.clone(), exp.alternative.clone());
            for (user_name, ty, raw_name) in mutable_rebinds {
                out.push(Statement {
                    selector: arm_sel.clone(),
                    command: Command::Bind {
                        mutable: true,
                        name: user_name,
                        ty: Some(ty),
                        value: Expr::Name(raw_name),
                    },
                    line: exp.line,
                });
            }

            let contract_alt = callee
                .alternatives
                .iter()
                .find(|a| a.name == exp.alternative)
                .unwrap();

            if adapter {
                // [U13]: Every arm of `■ Δ` exits.
                match exp.target {
                    ArmTarget::None | ArmTarget::SameExit => {
                        if let Some(group) = &exp.from_group
                            && !ctx.auto_groups.contains(group)
                        {
                            ctx.auto_groups.push(group.clone());
                        }
                        let payload: Vec<Expr> = arm_vars.iter().cloned().map(Expr::Name).collect();
                        let field_names =
                            Some(contract_alt.fields.iter().map(|f| f.name.clone()).collect());
                        let field_types =
                            Some(contract_alt.fields.iter().map(|f| Some(f.ty)).collect());
                        self.record_exit(
                            ctx,
                            exp.line,
                            &exp.alternative,
                            &payload,
                            field_names,
                            field_types,
                        )?;
                        out.push(Statement {
                            selector: arm_sel,
                            command: Command::Exit {
                                alternative: exp.alternative,
                                payload,
                            },
                            line: exp.line,
                        });
                    }
                    ArmTarget::Name(target_alt) => {
                        self.record_exit(ctx, exp.line, &target_alt, &[], None, None)?;
                        out.push(Statement {
                            selector: arm_sel,
                            command: Command::Exit {
                                alternative: target_alt,
                                payload: Vec::new(),
                            },
                            line: exp.line,
                        });
                    }
                    ArmTarget::AdaptExit(target_alt, payload) => {
                        self.record_exit(ctx, exp.line, &target_alt, &payload, None, None)?;
                        out.push(Statement {
                            selector: arm_sel,
                            command: Command::Exit {
                                alternative: target_alt,
                                payload,
                            },
                            line: exp.line,
                        });
                    }
                    ArmTarget::Stmt(stmt) => {
                        if !matches!(
                            *stmt,
                            SurfaceStmt::Exit { .. } | SurfaceStmt::Impossible { .. }
                        ) {
                            return fail(exp.line, "U13", "an adapter arm must exit or be `⊥`");
                        }
                        self.elaborate_stmt(&stmt, &arm_sel, ctx, out)?;
                    }
                    ArmTarget::Block(_) => {
                        return fail(exp.line, "U13", "an adapter arm cannot open a block");
                    }
                }
                continue;
            }

            // Normal decision ([U2], [U8]).
            match exp.target {
                ArmTarget::None => {
                    ctx.current_scope_mut().paths.push(PathEntry {
                        core_site: core_site.clone(),
                        alternative: exp.alternative.clone(),
                        path_name: exp.alternative,
                        is_route: false,
                    });
                }
                ArmTarget::Name(route) => {
                    if route != exp.alternative {
                        ctx.current_scope_mut()
                            .suppressed_defaults
                            .insert(exp.alternative.clone());
                    }
                    ctx.current_scope_mut().paths.push(PathEntry {
                        core_site: core_site.clone(),
                        alternative: exp.alternative,
                        path_name: route,
                        is_route: true,
                    });
                }
                ArmTarget::SameExit => {
                    ctx.current_scope_mut()
                        .suppressed_defaults
                        .insert(exp.alternative.clone());
                    if let Some(group) = &exp.from_group
                        && !ctx.auto_groups.contains(group)
                    {
                        ctx.auto_groups.push(group.clone());
                    }
                    let payload: Vec<Expr> = arm_vars.iter().cloned().map(Expr::Name).collect();
                    let field_names =
                        Some(contract_alt.fields.iter().map(|f| f.name.clone()).collect());
                    let field_types =
                        Some(contract_alt.fields.iter().map(|f| Some(f.ty)).collect());
                    self.record_exit(
                        ctx,
                        exp.line,
                        &exp.alternative,
                        &payload,
                        field_names,
                        field_types,
                    )?;
                    out.push(Statement {
                        selector: arm_sel,
                        command: Command::Exit {
                            alternative: exp.alternative,
                            payload,
                        },
                        line: exp.line,
                    });
                }
                ArmTarget::Stmt(stmt) => {
                    ctx.current_scope_mut()
                        .suppressed_defaults
                        .insert(exp.alternative);
                    self.elaborate_stmt(&stmt, &arm_sel, ctx, out)?;
                }
                ArmTarget::Block(stmts) => {
                    ctx.current_scope_mut()
                        .suppressed_defaults
                        .insert(exp.alternative);
                    for stmt in stmts {
                        self.elaborate_stmt(&stmt, &arm_sel, ctx, out)?;
                    }
                }
                ArmTarget::AdaptExit(_, _) => unreachable!(),
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn elaborate_loop(
        &mut self,
        line: u32,
        name: &str,
        parameters: &[LoopParameter],
        invariant: Option<&Expr>,
        body: &[SurfaceItem],
        selection: &Selector,
        ctx: &mut ControlElab,
        out: &mut Vec<Statement>,
    ) -> Result<(), Error> {
        let core_site = ctx.allocate_site(Some(name), name, line)?;

        // Collect all `□` (`Leave`) statements of this loop (not inside nested
        // loops) and check [U9].
        let mut leaves = Vec::new();
        collect_loop_leaves(body, &mut leaves);

        let mut bare_count = 0usize;
        let mut named_order: Vec<String> = Vec::new();
        let mut named_fields: HashMap<String, Vec<String>> = HashMap::new();

        for (leave_line, alt, outputs) in &leaves {
            match alt {
                None => bare_count += 1,
                Some(alt_name) => {
                    let field_names: Vec<String> = outputs.iter().map(|o| o.name.clone()).collect();
                    for (idx, field) in field_names.iter().enumerate() {
                        if field_names[..idx].contains(field) {
                            return fail(
                                *leave_line,
                                "U9",
                                format!("`□ {alt_name}` lists the field `{field}` twice"),
                            );
                        }
                    }
                    if let Some(existing) = named_fields.get(alt_name) {
                        if *existing != field_names {
                            return fail(
                                *leave_line,
                                "U9",
                                format!(
                                    "all `□ {alt_name}` of the loop `{name}` must give the same field names"
                                ),
                            );
                        }
                    } else {
                        named_order.push(alt_name.clone());
                        named_fields.insert(alt_name.clone(), field_names);
                    }
                }
            }
        }

        if bare_count > 0 && !named_order.is_empty() {
            return fail(
                line,
                "U9",
                format!(
                    "the loop `{name}` mixes bare `□` with named `□`; either every `□` has a name or none does"
                ),
            );
        }

        // Open the loop's scope.
        let mut loop_scope = BodyScope {
            sites: Vec::new(),
            paths: Vec::new(),
            suppressed_defaults: HashSet::new(),
            vars: HashMap::new(),
        };
        for param in parameters {
            loop_scope.vars.insert(param.name.clone(), param.ty);
        }
        ctx.scopes.push(loop_scope);

        let mut core_body = self.elaborate_items(body, ctx)?;
        let finished_loop_scope = ctx.scopes.pop().expect("a loop scope");

        // Determine the core loop arms and rename any inner variable of the
        // loop body that collides with a loop output field name ([U9], [W4]).
        let mut exported_names = HashSet::new();
        let mut core_arms = Vec::new();
        if bare_count > 0 {
            let hidden_alt = "_done".to_string();
            replace_bare_leaves(&mut core_body, &hidden_alt);
            core_arms.push(Arm {
                alternative: hidden_alt,
                names: Vec::new(),
            });
        } else {
            for alt_name in &named_order {
                let fields = &named_fields[alt_name];
                for field in fields {
                    exported_names.insert(field.clone());
                }
                core_arms.push(Arm {
                    alternative: alt_name.clone(),
                    names: fields.clone(),
                });
            }
        }

        // Infer types of exported loop fields and record them in the outer scope.
        for (_, alt, outputs) in &leaves {
            if let Some(alt_name) = alt {
                let _ = alt_name;
                for output in outputs {
                    if let Some(ty) = finished_loop_scope.vars.get(&output.name).copied() {
                        ctx.current_scope_mut().vars.insert(output.name.clone(), ty);
                    }
                }
            }
        }

        // [U9]: If the loop body has a name of its own that is the same as an
        // output field name bound after the loop, rename the inner one so [W4]
        // holds in the core.
        let mut inner_declared = HashSet::new();
        for param in parameters {
            inner_declared.insert(param.name.clone());
        }
        collect_inner_declarations(&core_body, &mut inner_declared);

        let mut renames = HashMap::new();
        for exported in &exported_names {
            if inner_declared.contains(exported) {
                renames.insert(exported.clone(), ctx.fresh_renamed_var(exported));
            }
        }

        let mut core_params = parameters.to_vec();
        let mut core_invariant = invariant.cloned();
        if !renames.is_empty() {
            for param in &mut core_params {
                if let Some(new_name) = renames.get(&param.name) {
                    param.name = new_name.clone();
                }
            }
            if let Some(inv) = &mut core_invariant {
                rename_expr(inv, &renames);
            }
            rename_statements(&mut core_body, &renames);
        }

        // [U2]: Each named `□ a` gives the alternative `a` of its loop the path
        // name `a` in the body that contains the loop.
        ctx.current_scope_mut().sites.push(SiteRecord {
            surface_name: name.to_string(),
            core_name: core_site.clone(),
            alternatives: named_order.clone(),
        });
        for alt_name in &named_order {
            ctx.current_scope_mut().paths.push(PathEntry {
                core_site: core_site.clone(),
                alternative: alt_name.clone(),
                path_name: alt_name.clone(),
                is_route: false,
            });
        }

        out.push(Statement {
            selector: selection.clone(),
            command: Command::Loop {
                site: core_site,
                name: name.to_string(),
                parameters: core_params,
                invariant: core_invariant,
                body: core_body,
                arms: core_arms,
            },
            line,
        });
        Ok(())
    }
}

struct ControlElab {
    control_name: String,
    is_auto: bool,
    used_core_sites: HashSet<String>,
    explicit_sites: HashSet<String>,
    all_declared_sites: HashSet<String>,
    all_declared_paths: HashSet<String>,
    next_hidden: usize,
    scopes: Vec<BodyScope>,
    auto_exits: Vec<AutoExit>,
    auto_groups: Vec<String>,
}

impl ControlElab {
    fn current_scope_mut(&mut self) -> &mut BodyScope {
        self.scopes.last_mut().expect("an open body scope")
    }

    fn allocate_site(
        &mut self,
        explicit: Option<&str>,
        default_name: &str,
        line: u32,
    ) -> Result<String, Error> {
        if let Some(name) = explicit
            && !self.explicit_sites.insert(name.to_string())
        {
            return fail(
                line,
                "U1",
                format!(
                    "the site name `{name}` is used twice in `{}`",
                    self.control_name
                ),
            );
        }
        let base = explicit.unwrap_or(default_name);
        if self.used_core_sites.insert(base.to_string()) {
            return Ok(base.to_string());
        }
        loop {
            self.next_hidden += 1;
            let candidate = format!("{base}_{}", self.next_hidden);
            if self.used_core_sites.insert(candidate.clone()) {
                return Ok(candidate);
            }
        }
    }

    fn fresh_test_site(&mut self) -> String {
        loop {
            let candidate = format!("_t{}", self.next_hidden);
            self.next_hidden += 1;
            if self.used_core_sites.insert(candidate.clone()) {
                return candidate;
            }
        }
    }

    fn fresh_hidden_var(&mut self) -> String {
        let name = format!("_v{}", self.next_hidden);
        self.next_hidden += 1;
        name
    }

    fn fresh_renamed_var(&mut self, original: &str) -> String {
        let name = format!("_inner_{original}_{}", self.next_hidden);
        self.next_hidden += 1;
        name
    }
}

fn collect_declared_paths(
    items: &[SurfaceItem],
    sites: &mut HashSet<String>,
    paths: &mut HashSet<String>,
) {
    for item in items {
        match item {
            SurfaceItem::Stmt(stmt) => collect_stmt_paths(stmt, sites, paths),
            SurfaceItem::Label { body, .. } => {
                for stmt in body {
                    collect_stmt_paths(stmt, sites, paths);
                }
            }
        }
    }
}

fn collect_stmt_paths(
    stmt: &SurfaceStmt,
    sites: &mut HashSet<String>,
    paths: &mut HashSet<String>,
) {
    match stmt {
        SurfaceStmt::Decide {
            site,
            operand,
            arms,
            ..
        } => {
            if let Some(s) = site {
                sites.insert(s.clone());
            } else if let Expr::Call(op, _) = operand {
                sites.insert(op.clone());
            } else if let Expr::Name(n) = operand {
                sites.insert(n.clone());
            }
            for arm in arms {
                match &arm.target {
                    ArmTarget::None => {
                        paths.insert(arm.name.clone());
                    }
                    ArmTarget::Name(route) => {
                        paths.insert(route.clone());
                    }
                    ArmTarget::Stmt(inner) => collect_stmt_paths(inner, sites, paths),
                    ArmTarget::Block(stmts) => {
                        for s in stmts {
                            collect_stmt_paths(s, sites, paths);
                        }
                    }
                    _ => {}
                }
            }
        }
        SurfaceStmt::Test { body, .. } => {
            for s in body {
                collect_stmt_paths(s, sites, paths);
            }
        }
        SurfaceStmt::Loop { name, body, .. } => {
            sites.insert(name.clone());
            collect_declared_paths(body, sites, paths);
        }
        SurfaceStmt::Leave {
            alternative: Some(alt),
            ..
        } => {
            paths.insert(alt.clone());
        }
        _ => {}
    }
}

fn collect_loop_leaves(items: &[SurfaceItem], out: &mut Vec<(u32, Option<String>, Vec<LeaveOut>)>) {
    for item in items {
        match item {
            SurfaceItem::Stmt(stmt) => collect_stmt_leaves(stmt, out),
            SurfaceItem::Label { body, .. } => {
                for stmt in body {
                    collect_stmt_leaves(stmt, out);
                }
            }
        }
    }
}

fn collect_stmt_leaves(stmt: &SurfaceStmt, out: &mut Vec<(u32, Option<String>, Vec<LeaveOut>)>) {
    match stmt {
        SurfaceStmt::Leave {
            line,
            alternative,
            outputs,
        } => {
            out.push((*line, alternative.clone(), outputs.clone()));
        }
        SurfaceStmt::Test { body, .. } => {
            for child in body {
                collect_stmt_leaves(child, out);
            }
        }
        SurfaceStmt::Decide { arms, .. } => {
            for arm in arms {
                match &arm.target {
                    ArmTarget::Stmt(inner) => collect_stmt_leaves(inner, out),
                    ArmTarget::Block(stmts) => {
                        for child in stmts {
                            collect_stmt_leaves(child, out);
                        }
                    }
                    _ => {}
                }
            }
        }
        // Do not recurse into a nested loop: its `□` statements belong to it ([W3]).
        SurfaceStmt::Loop { .. } => {}
        _ => {}
    }
}

fn replace_bare_leaves(statements: &mut [Statement], hidden_alt: &str) {
    for statement in statements {
        if let Command::Leave { alternative, .. } = &mut statement.command
            && alternative.is_empty()
        {
            *alternative = hidden_alt.to_string();
        }
    }
}

fn collect_inner_declarations(statements: &[Statement], out: &mut HashSet<String>) {
    for statement in statements {
        match &statement.command {
            Command::Bind { name, .. } => {
                out.insert(name.clone());
            }
            Command::Decide { arms, .. } => {
                for arm in arms {
                    for var in &arm.names {
                        out.insert(var.clone());
                    }
                }
            }
            Command::Loop { arms, .. } => {
                for arm in arms {
                    for var in &arm.names {
                        out.insert(var.clone());
                    }
                }
            }
            _ => {}
        }
    }
}

fn rename_expr(expr: &mut Expr, renames: &HashMap<String, String>) {
    match expr {
        Expr::Integer(_) | Expr::Boolean(_) => {}
        Expr::Name(name) => {
            if let Some(new_name) = renames.get(name) {
                *name = new_name.clone();
            }
        }
        Expr::Unary(_, inner) => rename_expr(inner, renames),
        Expr::Binary(_, left, right) => {
            rename_expr(left, renames);
            rename_expr(right, renames);
        }
        Expr::Call(_, args) => {
            for arg in args {
                rename_expr(arg, renames);
            }
        }
    }
}

fn rename_statements(statements: &mut [Statement], renames: &HashMap<String, String>) {
    for statement in statements {
        match &mut statement.command {
            Command::Bind { name, value, .. } => {
                if let Some(new_name) = renames.get(name) {
                    *name = new_name.clone();
                }
                rename_expr(value, renames);
            }
            Command::Assign { name, value } => {
                if let Some(new_name) = renames.get(name) {
                    *name = new_name.clone();
                }
                rename_expr(value, renames);
            }
            Command::Oblige(formula) => rename_expr(formula, renames),
            Command::Test { condition, .. } => rename_expr(condition, renames),
            Command::Decide {
                arguments, arms, ..
            } => {
                for arg in arguments {
                    rename_expr(arg, renames);
                }
                for arm in arms {
                    for var in &mut arm.names {
                        if let Some(new_name) = renames.get(var) {
                            *var = new_name.clone();
                        }
                    }
                }
            }
            Command::Loop {
                parameters,
                invariant,
                body,
                arms,
                ..
            } => {
                for param in parameters {
                    rename_expr(&mut param.initial, renames);
                }
                if let Some(inv) = invariant {
                    rename_expr(inv, renames);
                }
                rename_statements(body, renames);
                for arm in arms {
                    for var in &mut arm.names {
                        if let Some(new_name) = renames.get(var) {
                            *var = new_name.clone();
                        }
                    }
                }
            }
            Command::Again { arguments, .. } => {
                for arg in arguments {
                    rename_expr(arg, renames);
                }
            }
            Command::Leave { payload, .. } | Command::Exit { payload, .. } => {
                for item in payload {
                    rename_expr(item, renames);
                }
            }
            Command::Impossible => {}
        }
    }
}

/// Reads a Rails (or plain-form) surface program and elaborates it into the
/// core text tree ([`syntax::Program`]).
pub fn elaborate(source: &str) -> Result<Program, Error> {
    let top_decls = parse_surface_file(source)?;
    let mut outcomes = HashMap::new();
    let mut functions = HashMap::new();
    let mut controls = HashMap::new();

    for decl in &top_decls {
        match decl {
            TopDecl::Outcome(outcome) => {
                let mut seen = HashSet::new();
                for alt in &outcome.alternatives {
                    if !seen.insert(&alt.name) {
                        return fail(
                            outcome.line,
                            "G2",
                            format!("the outcome `{}` lists `{}` twice", outcome.name, alt.name),
                        );
                    }
                }
                if outcomes
                    .insert(outcome.name.clone(), outcome.alternatives.clone())
                    .is_some()
                {
                    return fail(
                        outcome.line,
                        "G2",
                        format!("the outcome `{}` is declared twice", outcome.name),
                    );
                }
            }
            TopDecl::Function(function) => {
                functions.insert(function.name.clone(), function.result);
            }
            TopDecl::Control(control) => {
                controls.insert(control.name.clone(), control.clone());
            }
        }
    }

    let mut elaborator = Elaborator {
        outcomes,
        functions,
        controls,
        contracts: HashMap::new(),
        resolving_auto: HashSet::new(),
    };

    // Resolve all written contracts first so callers see them regardless of
    // declaration order.
    for decl in &top_decls {
        if let TopDecl::Control(control) = decl
            && let SurfaceContract::Entries(entries) = &control.contract
        {
            let resolved = elaborator.resolve_written_contract(entries, control.line)?;
            elaborator.contracts.insert(control.name.clone(), resolved);
        }
    }

    let mut declarations = Vec::new();
    for decl in top_decls {
        match decl {
            TopDecl::Outcome(_) => {}
            TopDecl::Function(function) => {
                declarations.push(Declaration::Function(function));
            }
            TopDecl::Control(control) => {
                let elaborated = elaborator.elaborate_control(&control)?;
                declarations.push(Declaration::Control(elaborated));
            }
        }
    }

    // [F3]: The effect of a private defined control is inferred as the union
    // of the effects of the operations it calls.
    infer_private_effects(&mut declarations);

    Ok(Program { declarations })
}

fn collect_called_operations(statements: &[Statement], callees: &mut Vec<String>) {
    for statement in statements {
        match &statement.command {
            Command::Decide { operation, .. } => {
                if !callees.contains(operation) {
                    callees.push(operation.clone());
                }
            }
            Command::Loop { body, .. } => collect_called_operations(body, callees),
            _ => {}
        }
    }
}

fn infer_private_effects(declarations: &mut [Declaration]) {
    loop {
        let mut known_uses: HashMap<String, Vec<String>> = HashMap::new();
        for declaration in declarations.iter() {
            if let Declaration::Control(control) = declaration {
                known_uses.insert(control.name.clone(), control.uses.clone());
            }
        }
        let mut changed = false;
        for declaration in declarations.iter_mut() {
            if let Declaration::Control(control) = declaration
                && !control.public
                && !control.external
            {
                let mut callees = Vec::new();
                collect_called_operations(&control.body, &mut callees);
                for callee in callees {
                    if let Some(effects) = known_uses.get(&callee) {
                        for effect in effects {
                            if !control.uses.contains(effect) {
                                control.uses.push(effect.clone());
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
}
