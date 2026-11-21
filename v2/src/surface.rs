//! The Rails surface syntax (`docs/v2/surface.md`) for slice 1: layout,
//! grammar and elaboration into the core text tree ([`crate::syntax::Program`]).
//!
//! Every error names the clause of `docs/v2/surface.md` (or `docs/v2/core.md`)
//! that refuses the program: layout (`[Y1]` to `[Y10]`), grammar (`[G1]` to
//! `[G9]`) or elaboration (`[U1]` to `[U15]`).

use crate::int::Int;
use crate::syntax::{
    Alternative, Arm, Binary, BoundaryDecl, Command, Control, Declaration, Expr, Function,
    LoopParameter, OutcomeDecl as CoreOutcomeDecl, Parameter, Place, PlaceStep, Program,
    ResourceDecl, Selector, Statement, StructDecl, Unary, VariantConstructor, VariantDecl,
};
use crate::typed::{BorrowMode, IntType, Type};
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub control: String,
    pub line: u32,
    pub clause: &'static str,
    pub message: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}: warning: {} [{}]",
            self.control, self.line, self.message, self.clause
        )
    }
}

#[derive(Clone, Debug)]
pub struct Elaborated {
    pub program: Program,
    pub warnings: Vec<Warning>,
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
    /// Comment text on this line (after `┄` or `//`), if any.
    comment: Option<String>,
}

fn split_comment(text: &str) -> (&str, Option<String>) {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut in_string = false;
    let mut escaped = false;
    for (i, &(byte_pos, ch)) in chars.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            continue;
        }
        if ch == '┄' {
            let after = text[byte_pos + '┄'.len_utf8()..].trim_end();
            return (&text[..byte_pos], Some(after.to_string()));
        }
        if ch == '/' && chars.get(i + 1).is_some_and(|(_, next)| *next == '/') {
            let after = text[byte_pos + 2..].trim_end();
            return (&text[..byte_pos], Some(after.to_string()));
        }
    }
    (text, None)
}

fn strip_comment(text: &str) -> &str {
    split_comment(text).0
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

    let (without_comment, comment) = split_comment(rest);
    let indented = without_comment.starts_with(' ') || without_comment.starts_with('\t');
    let content = without_comment.trim().to_string();

    Ok(RailLine {
        line: line_no,
        segments,
        marker,
        indented,
        content,
        comment,
    })
}

#[derive(Clone, Debug)]
enum RawItem {
    Blank,
    Comment(String),
    Simple {
        line: u32,
        text: String,
    },
    Label {
        line: u32,
        inline: bool,
        mask: String,
        body: Vec<RawItem>,
    },
    Test {
        line: u32,
        inline: bool,
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
        || text.starts_with('∀')
        || text.starts_with("for ")
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
    // Check whether there is a `:` after the `Δ` / `delta` token (outside parens/strings).
    let after_delta = if let Some(pos) = text.find('Δ') {
        &text[pos + 'Δ'.len_utf8()..]
    } else if let Some(pos) = text.find("delta ") {
        &text[pos + 6..]
    } else {
        return false;
    };
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for ch in after_delta.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            ':' if depth == 0 => return false,
            _ => {}
        }
    }
    true
}

fn has_real_items(items: &[RawItem]) -> bool {
    items
        .iter()
        .any(|item| !matches!(item, RawItem::Blank | RawItem::Comment(_)))
}

fn check_not_empty_frame(frame: &Frame) -> Result<(), Error> {
    match frame {
        Frame::Control { .. } => Ok(()),
        Frame::Loop { line, items, .. } if !has_real_items(items) => {
            fail(*line, "Y4", "a loop must be followed by its `║` body lines")
        }
        Frame::Label { line, items, .. } if !has_real_items(items) => fail(
            *line,
            "Y5",
            "a label without `:` must be followed by its `┆` block",
        ),
        Frame::Test { line, items, .. } if !has_real_items(items) => fail(
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
        Frame::ArmBlock { line, items, .. } if !has_real_items(items) => fail(
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
                    inline: false,
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
                    inline: false,
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
        // Blank or comment rail line ([Y9]).
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
            let mut target_depth = None;
            for depth in (1..=frames.len()).rev() {
                if !matches!(frames[depth - 1], Frame::Decision { .. })
                    && expected_segments(&frames[..depth]) == line.segments
                {
                    target_depth = Some(depth);
                    break;
                }
            }
            if let Some(depth) = target_depth
                && (depth..frames.len()).all(|d| check_not_empty_frame(&frames[d]).is_ok())
            {
                while frames.len() > depth {
                    pop_one_frame(&mut frames)?;
                }
                let item = match line.comment {
                    Some(text) => RawItem::Comment(text),
                    None => RawItem::Blank,
                };
                frames
                    .last_mut()
                    .and_then(Frame::items_mut)
                    .expect("a body frame")
                    .push(item);
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
                            inline: true,
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
                                inline: true,
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
            inline: true,
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
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
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
    let mut in_string = false;
    let mut escaped = false;
    for (i, &(byte_pos, ch)) in chars.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
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
    StringLit(String),
    Delta,
    Arrow,
    Exit,
    Leave,
    Loop,
    Again,
    ForAll,
    In,
    Move,
    Ref,
    MapArrow,
    EmptyMap,
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
    Struct,
    Variant,
    Resource,
    Boundary,
    Contingency,
    Contingent,
    Supervise,
    Terminate,
    Escalate,
    Recover,
    Capture,
    Fault,
    Assume,
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
        "for" => Tok::ForAll,
        "in" | "from" => Tok::In,
        "move" => Tok::Move,
        "ref" => Tok::Ref,
        "if" => Tok::Test,
        "where" => Tok::Oblige,
        "when" => Tok::When,
        "impossible" => Tok::Impossible,
        "mutable" => Tok::Tilde,
        "control" => Tok::Control,
        "extern" => Tok::Extern,
        "function" => Tok::Function,
        "outcome" => Tok::Outcome,
        "struct" => Tok::Struct,
        "variant" => Tok::Variant,
        "resource" => Tok::Resource,
        "boundary" => Tok::Boundary,
        "contingency" => Tok::Contingency,
        "contingent" => Tok::Contingent,
        "supervise" => Tok::Supervise,
        "terminate" => Tok::Terminate,
        "escalate" => Tok::Escalate,
        "recover" => Tok::Recover,
        "capture" => Tok::Capture,
        "fault" => Tok::Fault,
        "assume" => Tok::Assume,
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
        if ch == '"' {
            index += 1;
            let mut value = String::new();
            let mut closed = false;
            while index < characters.len() {
                let c = characters[index];
                index += 1;
                if c == '"' {
                    closed = true;
                    break;
                }
                if c == '\\' && index < characters.len() {
                    let esc = characters[index];
                    index += 1;
                    match esc {
                        '"' => value.push('"'),
                        '\\' => value.push('\\'),
                        'n' => value.push('\n'),
                        'r' => value.push('\r'),
                        't' => value.push('\t'),
                        other => value.push(other),
                    }
                } else {
                    if c == '\n' {
                        line += 1;
                    }
                    value.push(c);
                }
            }
            if !closed {
                return fail(line, "G8", "unterminated string literal");
            }
            tokens.push(Token {
                tok: Tok::StringLit(value),
                line,
                first_on_line,
            });
            first_on_line = false;
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
                '∀' => (Tok::ForAll, 1),
                '∈' => (Tok::In, 1),
                '▾' => (Tok::Move, 1),
                '⇀' => (Tok::MapArrow, 1),
                '∅' => (Tok::EmptyMap, 1),
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
                '&' => (Tok::Ref, 1),
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
struct OutcomeAlt {
    alt: Alternative,
    status: Option<String>,
}

#[derive(Clone, Debug)]
struct OutcomeDecl {
    public: bool,
    vertical: bool,
    name: String,
    alternatives: Vec<OutcomeAlt>,
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
struct SurfaceVariantDecl {
    public: bool,
    is_copy: bool,
    vertical: bool,
    name: String,
    constructors: Vec<VariantConstructor>,
    line: u32,
}

#[derive(Clone, Debug)]
struct SurfaceBoundaryDecl {
    name: String,
    parameters: Vec<Parameter>,
    retains: Vec<String>,
    body: Vec<SurfaceItem>,
    message: Parameter,
    contingency: Vec<SurfaceItem>,
    line: u32,
}

#[derive(Clone, Debug)]
struct SurfaceControl {
    name: String,
    public: bool,
    external: bool,
    contingent: bool,
    faults: bool,
    multiline_contract: bool,
    parameters: Vec<Parameter>,
    precondition: Option<Expr>,
    contract: SurfaceContract,
    uses: Vec<String>,
    body: Vec<SurfaceItem>,
    line: u32,
}

#[derive(Clone, Debug)]
enum SurfaceCall {
    Direct {
        operation: String,
        arguments: Vec<Expr>,
    },
    Method {
        receiver: Expr,
        method: String,
        arguments: Vec<Expr>,
    },
}

#[derive(Clone, Debug)]
enum SurfaceOperand {
    Call(SurfaceCall),
    Expr(Expr),
}

#[derive(Clone, Debug)]
enum SurfaceItem {
    Blank,
    Comment(String),
    Stmt(SurfaceStmt),
    Label {
        inline: bool,
        mask: Mask,
        body: Vec<SurfaceStmt>,
    },
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
    Blank,
    Comment(String),
    Bind {
        line: u32,
        mutable: bool,
        name: String,
        ty: Option<Type>,
        value: Expr,
    },
    Unpack {
        line: u32,
        struct_name: String,
        names: Vec<String>,
        value: Expr,
    },
    Capture {
        line: u32,
        mutable: bool,
        name: String,
        ty: Option<Type>,
        call: SurfaceCall,
    },
    Assign {
        line: u32,
        name: String,
        value: Expr,
    },
    AssignPlace {
        line: u32,
        target: Place,
        value: Expr,
    },
    Oblige {
        line: u32,
        formula: Expr,
    },
    Test {
        line: u32,
        inline: bool,
        condition: Expr,
        body: Vec<SurfaceStmt>,
    },
    Decide {
        line: u32,
        vertical: bool,
        site: Option<String>,
        adapter: bool,
        operand: SurfaceOperand,
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
    MethodCall {
        line: u32,
        receiver: Expr,
        method: String,
        arguments: Vec<Expr>,
    },
    Loop {
        line: u32,
        name: String,
        parameters: Vec<LoopParameter>,
        invariant: Option<Expr>,
        body: Vec<SurfaceItem>,
    },
    ForAll {
        line: u32,
        var: String,
        collection: Expr,
        body: Vec<SurfaceItem>,
    },
    Supervise {
        line: u32,
        boundary: String,
        arguments: Vec<Expr>,
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
    ExitOutcome {
        line: u32,
        name: String,
    },
    Fault {
        line: u32,
        message: Expr,
        assumed: bool,
    },
    Terminate {
        line: u32,
        value: Option<Expr>,
    },
    Escalate {
        line: u32,
    },
    Recover {
        line: u32,
        loop_name: String,
    },
    Impossible {
        line: u32,
    },
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    allow_brace_literal: bool,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Parser {
        Parser {
            tokens,
            pos: 0,
            allow_brace_literal: true,
        }
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
        if self.eat(&Tok::Ref) {
            let mode = if self.eat(&Tok::Tilde) {
                BorrowMode::Exclusive
            } else {
                BorrowMode::Shared
            };
            let inner = self.map_ty(clause)?;
            return Ok(Type::Ref(mode, Box::new(inner)));
        }
        self.map_ty(clause)
    }

    fn map_ty(&mut self, clause: &'static str) -> Result<Type, Error> {
        let base = self.atom_ty(clause)?;
        if self.eat(&Tok::MapArrow) {
            let value = self.atom_ty(clause)?;
            return Ok(Type::Map(Box::new(base), Box::new(value)));
        }
        Ok(base)
    }

    fn atom_ty(&mut self, clause: &'static str) -> Result<Type, Error> {
        if self.eat(&Tok::LeftBracket) {
            let elem = self.map_ty(clause)?;
            self.expect(Tok::RightBracket, clause, "`]`")?;
            return Ok(Type::Vector(Box::new(elem)));
        }
        if self.eat(&Tok::Outcome) {
            self.expect(Tok::Less, clause, "`<` after `outcome`")?;
            let name = self.name(clause, "an outcome name")?;
            self.expect(Tok::Greater, clause, "`>`")?;
            return Ok(Type::OutcomeNamed(name));
        }
        if let Tok::Name(name) = self.peek().clone() {
            if name == "vector" && self.peek_next() == &Tok::Less {
                self.advance();
                self.advance();
                let elem = self.map_ty(clause)?;
                self.expect(Tok::Greater, clause, "`>`")?;
                return Ok(Type::Vector(Box::new(elem)));
            }
            if name == "map" && self.peek_next() == &Tok::Less {
                self.advance();
                self.advance();
                let key = self.map_ty(clause)?;
                self.expect(Tok::Comma, clause, "`,`")?;
                let value = self.map_ty(clause)?;
                self.expect(Tok::Greater, clause, "`>`")?;
                return Ok(Type::Map(Box::new(key), Box::new(value)));
            }
        }
        let name = self.name(clause, "a type")?;
        Ok(Type::from_name(&name).unwrap_or(Type::Named(name)))
    }

    fn qname(&mut self, clause: &'static str, what: &str) -> Result<String, Error> {
        let mut out = self.name(clause, what)?;
        while self.eat(&Tok::Dot) {
            let part = self.name(clause, what)?;
            out.push('.');
            out.push_str(&part);
        }
        Ok(out)
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
        matches!(get(1), Tok::Name(_)) && get(2) == &Tok::Colon
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

    fn outcome_alt(&mut self) -> Result<OutcomeAlt, Error> {
        let name = self.name("G2", "an alternative")?;
        let fields = self.fields("G2")?;
        let status = if self.eat(&Tok::MapsTo) {
            match self.peek().clone() {
                Tok::Number(digits) => {
                    self.advance();
                    Some(digits)
                }
                _ => {
                    return fail(
                        self.line(),
                        "G4",
                        "`↦` on an outcome alternative requires an exit status number",
                    );
                }
            }
        } else {
            None
        };
        Ok(OutcomeAlt {
            alt: Alternative {
                name,
                fields,
                condition: None,
            },
            status,
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

    fn brace_looks_like_arms(&self) -> bool {
        if self.peek() != &Tok::LeftBrace {
            return false;
        }
        let mut depth = 0i32;
        for tok in &self.tokens[self.pos..] {
            match tok.tok {
                Tok::LeftBrace => depth += 1,
                Tok::RightBrace => {
                    depth -= 1;
                    if depth == 0 {
                        return false;
                    }
                }
                Tok::Arrow | Tok::Bar | Tok::Ellipsis if depth == 1 => return true,
                Tok::End => return false,
                _ => {}
            }
        }
        false
    }

    fn unary(&mut self) -> Result<Expr, Error> {
        if self.eat(&Tok::Not) {
            return Ok(Expr::Unary(Unary::Not, Box::new(self.unary()?)));
        }
        if self.eat(&Tok::Minus) {
            return Ok(Expr::Unary(Unary::Negate, Box::new(self.unary()?)));
        }
        if self.eat(&Tok::Move) {
            return Ok(Expr::Move(Box::new(self.unary()?)));
        }
        if self.eat(&Tok::Ref) {
            let mode = if self.eat(&Tok::Tilde) {
                BorrowMode::Exclusive
            } else {
                BorrowMode::Shared
            };
            return Ok(Expr::Borrow(mode, Box::new(self.unary()?)));
        }
        if self.eat(&Tok::Tilde) {
            return Ok(Expr::Borrow(BorrowMode::Exclusive, Box::new(self.unary()?)));
        }
        let base = match self.peek().clone() {
            Tok::Number(digits) => {
                self.advance();
                Expr::Integer(Int::parse(&digits).expect("digits"))
            }
            Tok::True => {
                self.advance();
                Expr::Boolean(true)
            }
            Tok::False => {
                self.advance();
                Expr::Boolean(false)
            }
            Tok::StringLit(s) => {
                self.advance();
                Expr::String(s)
            }
            Tok::EmptyMap => {
                self.advance();
                Expr::EmptyMap
            }
            Tok::LeftBracket => {
                self.advance();
                let mut items = Vec::new();
                while !self.eat(&Tok::RightBracket) {
                    if !items.is_empty() {
                        self.expect(Tok::Comma, "G8", "`,`")?;
                    }
                    items.push(self.expr()?);
                }
                Expr::Vector(items)
            }
            Tok::LeftParen => {
                self.advance();
                let inner = self.expr()?;
                self.expect(Tok::RightParen, "G8", "`)`")?;
                inner
            }
            Tok::Name(name) => {
                self.advance();
                let next = &self.tokens[self.pos];
                if name == "map"
                    && next.tok == Tok::LeftBrace
                    && !next.first_on_line
                    && self
                        .tokens
                        .get(self.pos + 1)
                        .is_some_and(|t| t.tok == Tok::RightBrace)
                {
                    self.advance();
                    self.advance();
                    Expr::EmptyMap
                } else if name == "len" && next.tok == Tok::LeftParen && !next.first_on_line {
                    self.advance();
                    let arg = self.expr()?;
                    self.expect(Tok::RightParen, "G8", "`)`")?;
                    Expr::Len(Box::new(arg))
                } else if name == "clone" && next.tok == Tok::LeftParen && !next.first_on_line {
                    self.advance();
                    let arg = self.expr()?;
                    self.expect(Tok::RightParen, "G8", "`)`")?;
                    Expr::Clone(Box::new(arg))
                } else if name == "old" && next.tok == Tok::LeftParen && !next.first_on_line {
                    self.advance();
                    let var = self.name("W7", "a parameter name")?;
                    self.expect(Tok::RightParen, "G8", "`)`")?;
                    Expr::Old(var)
                } else if next.tok == Tok::LeftParen && !next.first_on_line {
                    Expr::Call(name, self.arguments()?)
                } else if self.allow_brace_literal
                    && next.tok == Tok::LeftBrace
                    && !next.first_on_line
                    && !self.brace_looks_like_arms()
                {
                    Expr::Struct(name, self.payload()?)
                } else {
                    Expr::Name(name)
                }
            }
            other => {
                return fail(
                    self.line(),
                    "G8",
                    format!("expected an expression, found {}", describe_tok(&other)),
                );
            }
        };
        self.postfix(base)
    }

    fn postfix(&mut self, mut base: Expr) -> Result<Expr, Error> {
        loop {
            let tok = &self.tokens[self.pos];
            if tok.first_on_line {
                break;
            }
            if tok.tok == Tok::Dot {
                self.advance();
                let line = self.line();
                let field = self.name("G8", "a field or method name")?;
                let next = &self.tokens[self.pos];
                if next.tok == Tok::LeftParen && !next.first_on_line {
                    let args = self.arguments()?;
                    if field == "len" && args.is_empty() {
                        base = Expr::Len(Box::new(base));
                    } else if field == "clone" && args.is_empty() {
                        base = Expr::Clone(Box::new(base));
                    } else if field == "trim" && args.is_empty() {
                        base = Expr::Call("trim".to_string(), vec![base]);
                    } else {
                        return fail(
                            line,
                            "U12",
                            format!("`{field}` is not a built-in method callable in an expression"),
                        );
                    }
                } else if self.allow_brace_literal
                    && next.tok == Tok::LeftBrace
                    && !next.first_on_line
                    && !self.brace_looks_like_arms()
                    && let Expr::Name(vname) = base
                {
                    let args = self.payload()?;
                    base = Expr::Variant(vname, field, args);
                } else {
                    base = Expr::Field(Box::new(base), field);
                }
            } else if tok.tok == Tok::LeftBracket {
                self.advance();
                let idx = self.expr()?;
                self.expect(Tok::RightBracket, "G8", "`]`")?;
                base = Expr::Index(Box::new(base), Box::new(idx));
            } else {
                break;
            }
        }
        Ok(base)
    }

    fn try_parse_method_call(&mut self) -> Result<Option<SurfaceCall>, Error> {
        let saved = self.pos;
        if !matches!(self.peek(), Tok::Name(_)) {
            return Ok(None);
        }
        let Tok::Name(first) = self.advance() else {
            unreachable!();
        };
        let mut base = Expr::Name(first);
        while !self.tokens[self.pos].first_on_line {
            if self.eat(&Tok::Dot) {
                let Ok(member) = self.name("G8", "a field or method name") else {
                    self.pos = saved;
                    return Ok(None);
                };
                if self.peek() == &Tok::LeftParen && !self.tokens[self.pos].first_on_line {
                    let args = self.arguments()?;
                    if matches!(
                        self.peek(),
                        Tok::Colon
                            | Tok::LeftBrace
                            | Tok::Semi
                            | Tok::RightBrace
                            | Tok::End
                            | Tok::Bar
                            | Tok::Comma
                    ) {
                        return Ok(Some(SurfaceCall::Method {
                            receiver: base,
                            method: member,
                            arguments: args,
                        }));
                    }
                    self.pos = saved;
                    return Ok(None);
                }
                base = Expr::Field(Box::new(base), member);
            } else if self.eat(&Tok::LeftBracket) {
                let Ok(idx) = self.expr() else {
                    self.pos = saved;
                    return Ok(None);
                };
                if !self.eat(&Tok::RightBracket) {
                    self.pos = saved;
                    return Ok(None);
                }
                base = Expr::Index(Box::new(base), Box::new(idx));
            } else {
                break;
            }
        }
        self.pos = saved;
        Ok(None)
    }

    fn parse_call(&mut self, clause: &'static str) -> Result<SurfaceCall, Error> {
        if let Some(method_call) = self.try_parse_method_call()? {
            return Ok(method_call);
        }
        let operation = self.name(clause, "an operation name")?;
        let arguments = self.arguments()?;
        Ok(SurfaceCall::Direct {
            operation,
            arguments,
        })
    }

    fn parse_operand(&mut self, allow_braced_arms: bool) -> Result<SurfaceOperand, Error> {
        if let Some(method_call) = self.try_parse_method_call()? {
            return Ok(SurfaceOperand::Call(method_call));
        }
        let prev = self.allow_brace_literal;
        if allow_braced_arms {
            self.allow_brace_literal = false;
        }
        let expr = self.expr();
        self.allow_brace_literal = prev;
        let expr = expr?;
        if let Expr::Call(operation, arguments) = expr {
            return Ok(SurfaceOperand::Call(SurfaceCall::Direct {
                operation,
                arguments,
            }));
        }
        Ok(SurfaceOperand::Expr(expr))
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
            if self.eat(&Tok::Move) {
                let name = self.name("U14", "a captured outcome name")?;
                return Ok(ArmTarget::Stmt(Box::new(SurfaceStmt::ExitOutcome {
                    line,
                    name,
                })));
            }
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
            Tok::Leave
            | Tok::Again
            | Tok::Forward
            | Tok::Fault
            | Tok::Assume
            | Tok::Supervise
            | Tok::Terminate
            | Tok::Escalate
            | Tok::Recover => Ok(ArmTarget::Stmt(Box::new(self.simple_stmt()?))),
            Tok::Name(_) => {
                if matches!(
                    self.peek_next(),
                    Tok::Assign
                        | Tok::Bind
                        | Tok::Colon
                        | Tok::LeftParen
                        | Tok::Dot
                        | Tok::LeftBracket
                ) {
                    return Ok(ArmTarget::Stmt(Box::new(self.simple_stmt()?)));
                }
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
        let operand = self.parse_operand(allow_braced_arms)?;
        let (vertical, arms) = if self.eat(&Tok::Colon) {
            (false, self.horizontal_arms(adapter, false)?)
        } else if allow_braced_arms && self.eat(&Tok::LeftBrace) {
            let arms = self.horizontal_arms(adapter, true)?;
            let has_block = arms
                .iter()
                .any(|arm| matches!(arm.target, ArmTarget::Block(_)));
            (has_block, arms)
        } else {
            return fail(
                line,
                "Y7",
                "a decision requires `: arms` or vertical `├` and `└` arms",
            );
        };
        Ok(SurfaceStmt::Decide {
            line,
            vertical,
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
            Tok::Fault => {
                self.advance();
                Ok(SurfaceStmt::Fault {
                    line,
                    message: self.expr()?,
                    assumed: false,
                })
            }
            Tok::Assume => {
                self.advance();
                self.expect(Tok::Impossible, "F4", "`⊥` after `assume`")?;
                Ok(SurfaceStmt::Fault {
                    line,
                    message: self.expr()?,
                    assumed: true,
                })
            }
            Tok::Supervise => {
                self.advance();
                let boundary = self.name("G12", "a boundary name")?;
                let arguments = self.arguments()?;
                Ok(SurfaceStmt::Supervise {
                    line,
                    boundary,
                    arguments,
                })
            }
            Tok::Terminate => {
                self.advance();
                let value = if matches!(
                    self.peek(),
                    Tok::End | Tok::Semi | Tok::RightBrace | Tok::Bar
                ) {
                    None
                } else {
                    Some(self.expr()?)
                };
                Ok(SurfaceStmt::Terminate { line, value })
            }
            Tok::Escalate => {
                self.advance();
                Ok(SurfaceStmt::Escalate { line })
            }
            Tok::Recover => {
                self.advance();
                self.expect(Tok::Again, "G8", "`↑` after `recover`")?;
                let loop_name = self.name("G8", "a loop name")?;
                Ok(SurfaceStmt::Recover { line, loop_name })
            }
            Tok::Exit => {
                self.advance();
                if self.eat(&Tok::Delta) {
                    return self.decision_after_prefix(line, None, true, true);
                }
                if self.eat(&Tok::Move) {
                    let name = self.name("U14", "a captured outcome name")?;
                    return Ok(SurfaceStmt::ExitOutcome { line, name });
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
                let (inline, body) = if self.peek() == &Tok::LeftBrace {
                    (false, self.plain_stmts_braced()?)
                } else {
                    (true, vec![self.simple_stmt()?])
                };
                Ok(SurfaceStmt::Test {
                    line,
                    inline,
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
                if self.eat(&Tok::Capture) {
                    let call = self.parse_call("G8")?;
                    return Ok(SurfaceStmt::Capture {
                        line,
                        mutable: true,
                        name,
                        ty,
                        call,
                    });
                }
                let value = self.expr()?;
                Ok(SurfaceStmt::Bind {
                    line,
                    mutable: true,
                    name,
                    ty,
                    value,
                })
            }
            Tok::Name(_) => {
                // First check method-call statement `p.m(ā)` ([U12]).
                if let Some(SurfaceCall::Method {
                    receiver,
                    method,
                    arguments,
                }) = self.try_parse_method_call()?
                {
                    return Ok(SurfaceStmt::MethodCall {
                        line,
                        receiver,
                        method,
                        arguments,
                    });
                }
                let Tok::Name(first) = self.advance() else {
                    unreachable!();
                };
                // Struct unpacking `S {x̄} := e` ([E-Unpack]).
                if self.peek() == &Tok::LeftBrace {
                    let saved = self.pos;
                    self.advance();
                    let mut names = Vec::new();
                    let mut ok = true;
                    while !self.eat(&Tok::RightBrace) {
                        if !names.is_empty() && !self.eat(&Tok::Comma) {
                            ok = false;
                            break;
                        }
                        if let Ok(field_var) = self.name("G8", "a variable name") {
                            names.push(field_var);
                        } else {
                            ok = false;
                            break;
                        }
                    }
                    if ok && self.eat(&Tok::Bind) {
                        let value = self.expr()?;
                        return Ok(SurfaceStmt::Unpack {
                            line,
                            struct_name: first,
                            names,
                            value,
                        });
                    }
                    self.pos = saved;
                }
                // Place assignment `p.f ← e` or `p[i] ← e` ([E-Assign]).
                if matches!(self.peek(), Tok::Dot | Tok::LeftBracket) {
                    let mut steps = Vec::new();
                    while !self.tokens[self.pos].first_on_line {
                        if self.eat(&Tok::Dot) {
                            let field = self.name("G8", "a field name")?;
                            steps.push(PlaceStep::Field(field));
                        } else if self.eat(&Tok::LeftBracket) {
                            let idx = self.expr()?;
                            self.expect(Tok::RightBracket, "G8", "`]`")?;
                            steps.push(PlaceStep::Index(idx));
                        } else {
                            break;
                        }
                    }
                    self.expect(Tok::Assign, "G8", "`←` after place")?;
                    let value = self.expr()?;
                    return Ok(SurfaceStmt::AssignPlace {
                        line,
                        target: Place { root: first, steps },
                        value,
                    });
                }
                if self.eat(&Tok::Assign) {
                    return Ok(SurfaceStmt::Assign {
                        line,
                        name: first,
                        value: self.expr()?,
                    });
                }
                if self.eat(&Tok::Bind) {
                    if self.eat(&Tok::Capture) {
                        let call = self.parse_call("G8")?;
                        return Ok(SurfaceStmt::Capture {
                            line,
                            mutable: false,
                            name: first,
                            ty: None,
                            call,
                        });
                    }
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
                    if self.eat(&Tok::Capture) {
                        let call = self.parse_call("G8")?;
                        return Ok(SurfaceStmt::Capture {
                            line,
                            mutable: false,
                            name: first,
                            ty: Some(ty),
                            call,
                        });
                    }
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

    fn forall_header(&mut self) -> Result<(u32, String, Expr), Error> {
        let line = self.line();
        self.expect(Tok::ForAll, "Y4", "`∀`")?;
        let var = self.name("U10", "an element name")?;
        self.expect(Tok::In, "U10", "`∈`")?;
        let collection = self.expr()?;
        Ok((line, var, collection))
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
                let (inline, body) = if self.peek() == &Tok::LeftBrace {
                    (false, self.plain_stmts_braced()?)
                } else {
                    let mut inline_stmts = Vec::new();
                    let label_line = self.line();
                    while self.peek() != &Tok::RightBrace
                        && self.peek() != &Tok::LeftBracket
                        && self.peek() != &Tok::End
                    {
                        inline_stmts.push(self.plain_stmt()?);
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
                    (true, inline_stmts)
                };
                items.push(SurfaceItem::Label { inline, mask, body });
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
        if self.peek() == &Tok::ForAll {
            let prev = self.allow_brace_literal;
            self.allow_brace_literal = false;
            let header = self.forall_header();
            self.allow_brace_literal = prev;
            let (line, var, collection) = header?;
            let body = self.plain_body_items()?;
            return Ok(SurfaceStmt::ForAll {
                line,
                var,
                collection,
                body,
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
    let first_stmt_idx = items
        .iter()
        .position(|item| !matches!(item, SurfaceItem::Blank | SurfaceItem::Comment(_)))?;
    if matches!(
        items.get(first_stmt_idx),
        Some(SurfaceItem::Stmt(SurfaceStmt::Oblige { .. }))
    ) {
        let SurfaceItem::Stmt(SurfaceStmt::Oblige { formula, .. }) = items.remove(first_stmt_idx)
        else {
            unreachable!();
        };
        return Some(formula);
    }
    None
}

fn parse_raw_stmt(item: RawItem) -> Result<SurfaceStmt, Error> {
    match item {
        RawItem::Blank => Ok(SurfaceStmt::Blank),
        RawItem::Comment(text) => Ok(SurfaceStmt::Comment(text)),
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
            inline,
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
                inline,
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
            let operand = parser.parse_operand(false)?;
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
                vertical: true,
                site,
                adapter,
                operand,
                arms: parsed_arms,
            })
        }
        RawItem::Loop { line, header, body } => {
            let mut parser = Parser::new(lex_fragment(&header, line)?);
            if parser.peek() == &Tok::ForAll {
                let (_, var, collection) = parser.forall_header()?;
                if parser.peek() != &Tok::End {
                    return fail(line, "Y4", "unexpected tokens after `∀` header");
                }
                let items = parse_raw_items(body)?;
                return Ok(SurfaceStmt::ForAll {
                    line,
                    var,
                    collection,
                    body: items,
                });
            }
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
            RawItem::Blank => out.push(SurfaceItem::Blank),
            RawItem::Comment(text) => out.push(SurfaceItem::Comment(text)),
            RawItem::Label {
                line,
                inline,
                mask,
                body,
            } => {
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
                    inline,
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
    Blank,
    Comment(String),
    Struct(StructDecl),
    Variant(SurfaceVariantDecl),
    Resource(ResourceDecl),
    Boundary(SurfaceBoundaryDecl),
    Outcome(OutcomeDecl),
    Function(Function),
    Control(SurfaceControl),
}

fn starts_top_decl(trimmed: &str) -> bool {
    [
        "pub ",
        "copy ",
        "struct ",
        "variant ",
        "outcome ",
        "function ",
        "extern ",
        "contingent ",
        "control ",
        "boundary ",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
}

fn read_rails_body_lines(
    lines: &[(u32, &str)],
    index: &mut usize,
    owner_name: &str,
    owner_line: u32,
    clause_close: &'static str,
) -> Result<Vec<SurfaceItem>, Error> {
    let mut rail_lines = Vec::new();
    let mut closed = false;
    while *index < lines.len() {
        let (body_line, body_raw) = lines[*index];
        let body_stripped = strip_comment(body_raw);
        if let Some(after) = body_stripped.strip_prefix('╵') {
            if !after.trim().is_empty() {
                return fail(body_line, "Y3", "`╵` must stand alone on its line");
            }
            *index += 1;
            closed = true;
            break;
        }
        let trimmed = body_stripped.trim();
        if starts_top_decl(trimmed) || trimmed.starts_with("contingency") {
            return fail(
                body_line,
                clause_close,
                format!("the body of `{owner_name}` is not closed with `╵`"),
            );
        }
        rail_lines.push(parse_rail_line(body_line, body_raw)?);
        *index += 1;
    }
    if !closed {
        return fail(
            owner_line,
            clause_close,
            format!("the body of `{owner_name}` is not closed with `╵`"),
        );
    }
    let raw_items = build_rails_body(&rail_lines)?;
    parse_raw_items(raw_items)
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
        if raw.trim().is_empty() {
            decls.push(TopDecl::Blank);
            index += 1;
            continue;
        }
        if raw.trim_start().starts_with("//!") {
            decls.push(TopDecl::Comment(raw.to_string()));
            index += 1;
            continue;
        }
        let (stripped, comment) = split_comment(raw);
        if stripped.trim().is_empty() {
            if let Some(c) = comment {
                decls.push(TopDecl::Comment(format!("┄{}", c)));
            }
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

        // `[pub] [copy] struct name { Field,* }` ([G4b])
        let is_copy = if let Tok::Name(word) = parser.peek()
            && word == "copy"
            && matches!(parser.peek_next(), Tok::Struct | Tok::Variant)
        {
            parser.advance();
            true
        } else {
            false
        };

        if parser.eat(&Tok::Struct) {
            let name = parser.name("G4b", "a struct name")?;
            parser.expect(Tok::LeftBrace, "G4b", "`{`")?;
            let mut fields = Vec::new();
            while !parser.eat(&Tok::RightBrace) {
                if !fields.is_empty() {
                    parser.expect(Tok::Comma, "G4b", "`,`")?;
                    if parser.eat(&Tok::RightBrace) {
                        break;
                    }
                }
                let fname = parser.name("G4b", "a field name")?;
                parser.expect(Tok::Colon, "G4b", "`:`")?;
                let fty = parser.ty("G4b")?;
                fields.push(Parameter {
                    name: fname,
                    ty: fty,
                });
            }
            if parser.peek() != &Tok::End {
                return fail(line_no, "G4b", "unexpected tokens in `struct` declaration");
            }
            decls.push(TopDecl::Struct(StructDecl {
                name,
                public,
                is_copy,
                fields,
                line: line_no,
            }));
            continue;
        }

        if parser.eat(&Tok::Variant) {
            let name = parser.name("G4b", "a variant name")?;
            let mut constructors = Vec::new();
            let mut vertical = false;
            if parser.eat(&Tok::Colon) {
                loop {
                    let cname = parser.name("G4b", "a constructor name")?;
                    let fields = parser.fields("G4b")?;
                    constructors.push(VariantConstructor {
                        name: cname,
                        fields,
                    });
                    if !parser.eat(&Tok::Bar) {
                        break;
                    }
                }
            } else if parser.eat(&Tok::LeftBrace) {
                while !parser.eat(&Tok::RightBrace) {
                    let cname = parser.name("G4b", "a constructor name")?;
                    let fields = parser.fields("G4b")?;
                    constructors.push(VariantConstructor {
                        name: cname,
                        fields,
                    });
                    if !parser.eat(&Tok::Bar) && !parser.eat(&Tok::Comma) {
                        parser.expect(Tok::RightBrace, "G9", "`}`")?;
                        break;
                    }
                }
            } else {
                vertical = true;
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
                        return fail(alt_line, "Y7", "no constructor may follow `└`");
                    }
                    let mut alt_parser = Parser::new(lex_fragment(rest, alt_line)?);
                    let cname = alt_parser.name("G4b", "a constructor name")?;
                    let fields = alt_parser.fields("G4b")?;
                    if alt_parser.peek() != &Tok::End {
                        return fail(alt_line, "G4b", "unexpected tokens in variant constructor");
                    }
                    constructors.push(VariantConstructor {
                        name: cname,
                        fields,
                    });
                    index += 1;
                    if is_last {
                        saw_last = true;
                        break;
                    }
                }
                if constructors.is_empty() || !saw_last {
                    return fail(
                        line_no,
                        "Y7",
                        "vertical variant constructors must use `├` and end with `└`",
                    );
                }
            }
            if parser.peek() != &Tok::End {
                return fail(line_no, "G4b", "unexpected tokens in `variant` declaration");
            }
            if constructors.is_empty() {
                return fail(line_no, "G4b", "a `variant` declaration needs constructors");
            }
            decls.push(TopDecl::Variant(SurfaceVariantDecl {
                public,
                is_copy,
                vertical,
                name,
                constructors,
                line: line_no,
            }));
            continue;
        }

        if parser.eat(&Tok::Boundary) {
            if public {
                return fail(line_no, "G12", "a named `boundary` is not `pub`");
            }
            let name = parser.name("G12", "a boundary name")?;
            let parameters = parser.parameters("G12")?;
            let mut retains = Vec::new();
            if let Tok::Name(word) = parser.peek()
                && word == "retains"
            {
                parser.advance();
                loop {
                    retains.push(parser.name("G12", "a retained parameter name")?);
                    if !parser.eat(&Tok::Comma) {
                        break;
                    }
                }
            }
            if parser.peek() == &Tok::LeftBrace {
                // Plain form ([G9]): `boundary name(...) { ... } contingency (m: string) { ... }`.
                let body = parser.plain_body_items()?;
                if parser.peek() == &Tok::End {
                    // `contingency` may begin on the next line in plain form.
                    while index < lines.len() && strip_comment(lines[index].1).trim().is_empty() {
                        index += 1;
                    }
                    if index >= lines.len() {
                        return fail(
                            line_no,
                            "Y12",
                            format!("boundary `{name}` must be followed at once by `contingency`"),
                        );
                    }
                    let (cont_line, cont_raw) = lines[index];
                    let cont_stripped = strip_comment(cont_raw);
                    if cont_stripped.starts_with(' ') || cont_stripped.starts_with('\t') {
                        return fail(
                            cont_line,
                            "Y12",
                            "`contingency` must start in the first column",
                        );
                    }
                    let mut cont_src = cont_stripped.to_string();
                    let mut balance: i32 = cont_src
                        .chars()
                        .map(|c| match c {
                            '{' => 1,
                            '}' => -1,
                            _ => 0,
                        })
                        .sum();
                    index += 1;
                    while index < lines.len() && balance > 0 {
                        let (_, nr) = lines[index];
                        let ns = strip_comment(nr);
                        for c in ns.chars() {
                            if c == '{' {
                                balance += 1;
                            } else if c == '}' {
                                balance -= 1;
                            }
                        }
                        cont_src.push('\n');
                        cont_src.push_str(ns);
                        index += 1;
                    }
                    let mut cparser = Parser::new(lex_fragment(&cont_src, cont_line)?);
                    cparser.expect(Tok::Contingency, "Y12", "`contingency`")?;
                    let cparams = cparser.parameters("G12")?;
                    if cparams.len() != 1 {
                        return fail(
                            cont_line,
                            "G12",
                            "`contingency` takes exactly one message parameter `(m: string)`",
                        );
                    }
                    let contingency = cparser.plain_body_items()?;
                    if cparser.peek() != &Tok::End {
                        return fail(
                            cparser.line(),
                            "G12",
                            "unexpected tokens after `contingency` body",
                        );
                    }
                    decls.push(TopDecl::Boundary(SurfaceBoundaryDecl {
                        name,
                        parameters,
                        retains,
                        body,
                        message: cparams.into_iter().next().unwrap(),
                        contingency,
                        line: line_no,
                    }));
                    continue;
                }
                parser.expect(Tok::Contingency, "Y12", "`contingency`")?;
                let cparams = parser.parameters("G12")?;
                if cparams.len() != 1 {
                    return fail(
                        line_no,
                        "G12",
                        "`contingency` takes exactly one message parameter `(m: string)`",
                    );
                }
                let contingency = parser.plain_body_items()?;
                if parser.peek() != &Tok::End {
                    return fail(
                        parser.line(),
                        "G12",
                        "unexpected tokens after `contingency` body",
                    );
                }
                decls.push(TopDecl::Boundary(SurfaceBoundaryDecl {
                    name,
                    parameters,
                    retains,
                    body,
                    message: cparams.into_iter().next().unwrap(),
                    contingency,
                    line: line_no,
                }));
                continue;
            }
            if parser.peek() != &Tok::End {
                return fail(
                    parser.line(),
                    "G12",
                    "unexpected tokens in `boundary` header",
                );
            }
            let body = read_rails_body_lines(&lines, &mut index, &name, line_no, "Y12")?;
            // [Y12]: `contingency` follows at once, starting in column 1.
            if index >= lines.len() {
                return fail(
                    line_no,
                    "Y12",
                    format!("boundary `{name}` must be followed at once by `contingency`"),
                );
            }
            let (cont_line, cont_raw) = lines[index];
            let cont_stripped = strip_comment(cont_raw);
            if cont_stripped.trim().is_empty() {
                return fail(
                    cont_line,
                    "Y12",
                    format!(
                        "`contingency` of boundary `{name}` must follow at once without blank lines"
                    ),
                );
            }
            if cont_stripped.starts_with(' ') || cont_stripped.starts_with('\t') {
                return fail(
                    cont_line,
                    "Y12",
                    "`contingency` must start in the first column",
                );
            }
            let mut cparser = Parser::new(lex_fragment(cont_stripped, cont_line)?);
            cparser.expect(Tok::Contingency, "Y12", "`contingency`")?;
            let cparams = cparser.parameters("G12")?;
            if cparams.len() != 1 {
                return fail(
                    cont_line,
                    "G12",
                    "`contingency` takes exactly one message parameter `(m: string)`",
                );
            }
            if cparser.peek() != &Tok::End {
                return fail(
                    cparser.line(),
                    "G12",
                    "unexpected tokens in `contingency` header",
                );
            }
            index += 1;
            let contingency = read_rails_body_lines(
                &lines,
                &mut index,
                &format!("{name} contingency"),
                cont_line,
                "Y12",
            )?;
            decls.push(TopDecl::Boundary(SurfaceBoundaryDecl {
                name,
                parameters,
                retains,
                body,
                message: cparams.into_iter().next().unwrap(),
                contingency,
                line: line_no,
            }));
            continue;
        }

        if parser.eat(&Tok::Outcome) {
            let name = parser.name("G2", "an outcome name")?;
            let mut alternatives = Vec::new();
            let mut vertical = false;
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
                vertical = true;
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
                public,
                vertical,
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
        if external && parser.eat(&Tok::Resource) {
            let name = parser.name("G8", "a resource name")?;
            let on_fault_release = if let Tok::Name(word) = parser.peek()
                && word == "on"
            {
                parser.advance();
                parser.expect(Tok::Fault, "G8", "`fault` after `on`")?;
                let rel = parser.name("G8", "`release` after `on fault`")?;
                if rel != "release" {
                    return fail(line_no, "G8", "expected `release` after `on fault`");
                }
                true
            } else {
                false
            };
            if parser.peek() != &Tok::End {
                return fail(
                    parser.line(),
                    "G8",
                    "unexpected tokens in `extern resource` declaration",
                );
            }
            decls.push(TopDecl::Resource(ResourceDecl {
                name,
                on_fault_release,
                line: line_no,
            }));
            continue;
        }
        let contingent = parser.eat(&Tok::Contingent);
        parser.expect(Tok::Control, "G8", "`control`")?;
        let name = parser.name("G8", "a control name")?;
        let parameters = parser.parameters("G8")?;
        let precondition = if parser.eat(&Tok::Oblige) {
            Some(parser.expr()?)
        } else {
            None
        };
        parser.expect(Tok::Exit, "G1", "`■` and the contract")?;
        let mut multiline_contract = false;
        let contract = if parser.eat(&Tok::Auto) {
            SurfaceContract::Auto
        } else {
            let mut entries = Vec::new();
            parser.contract_entries(&mut entries)?;
            while parser.eat(&Tok::Exit) {
                multiline_contract = true;
                parser.contract_entries(&mut entries)?;
            }
            if entries.is_empty() {
                return fail(line_no, "G1", "a contract must list at least one entry");
            }
            SurfaceContract::Entries(entries)
        };
        let faults = parser.eat(&Tok::Faults);
        let mut uses = Vec::new();
        if parser.eat(&Tok::Uses) {
            loop {
                uses.push(parser.qname("G8", "an effect")?);
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
                contingent,
                faults,
                multiline_contract,
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
                contingent,
                faults,
                multiline_contract,
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
        let body = read_rails_body_lines(&lines, &mut index, &name, line_no, "Y3")?;
        decls.push(TopDecl::Control(SurfaceControl {
            name,
            public,
            external: false,
            contingent,
            faults,
            multiline_contract,
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
    /// Variable types known in this body, for `■ auto` ([G3]), `Δ bool` ([U6]),
    /// variant decisions, method dispatch (`[U12]`) and move marks (`[U11]`).
    vars: HashMap<String, Type>,
    /// Captured outcome contracts keyed by variable name (`[U6]`, `[U14]`).
    captured_contracts: HashMap<String, ResolvedContract>,
}

#[derive(Clone, Debug)]
struct AutoExit {
    line: u32,
    alternative: String,
    field_names: Vec<String>,
    field_types: Vec<Option<Type>>,
}

struct Elaborator {
    structs: HashMap<String, StructDecl>,
    variants: HashMap<String, Vec<VariantConstructor>>,
    resources: HashSet<String>,
    boundaries: HashMap<String, SurfaceBoundaryDecl>,
    outcomes: HashMap<String, Vec<Alternative>>,
    functions: HashMap<String, Type>,
    controls: HashMap<String, SurfaceControl>,
    contracts: HashMap<String, ResolvedContract>,
    resolving_auto: HashSet<String>,
    elaborated_controls: HashMap<String, Control>,
    warnings: Vec<Warning>,
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
        self.elaborated_controls
            .insert(name.to_string(), elaborated);
        Ok(self.contracts.get(name).expect("inferred contract").clone())
    }

    fn holds_obligation(&self, ty: &Type) -> bool {
        fn inner(elab: &Elaborator, ty: &Type, seen: &mut HashSet<String>) -> bool {
            match ty {
                Type::Resource(_, _) | Type::Outcome(_, _) | Type::OutcomeNamed(_) => true,
                Type::Ref(_, _) | Type::Bool | Type::Int(_) | Type::String => false,
                Type::Vector(elem) => inner(elab, elem, seen),
                Type::Map(k, v) => inner(elab, k, seen) || inner(elab, v, seen),
                Type::Named(name) => {
                    if elab.resources.contains(name) || elab.outcomes.contains_key(name) {
                        return true;
                    }
                    if !seen.insert(name.clone()) {
                        return false;
                    }
                    if let Some(st) = elab.structs.get(name) {
                        return st.fields.iter().any(|f| inner(elab, &f.ty, seen));
                    }
                    if let Some(ctors) = elab.variants.get(name) {
                        return ctors
                            .iter()
                            .any(|c| c.fields.iter().any(|f| inner(elab, &f.ty, seen)));
                    }
                    false
                }
                Type::Struct(_, _) | Type::Variant(_, _) => false,
            }
        }
        inner(self, ty, &mut HashSet::new())
    }

    fn is_place_expr(expr: &Expr) -> bool {
        matches!(expr, Expr::Name(_) | Expr::Field(_, _) | Expr::Index(_, _))
    }

    fn place_last_name(expr: &Expr) -> Option<String> {
        match expr {
            Expr::Move(inner) | Expr::Borrow(_, inner) => Self::place_last_name(inner),
            Expr::Name(n) => Some(n.clone()),
            Expr::Field(_, f) => Some(f.clone()),
            Expr::Index(base, _) => Self::place_last_name(base),
            _ => None,
        }
    }

    #[allow(clippy::only_used_in_recursion)]
    fn check_arg(
        &self,
        expr: &Expr,
        expected: &Type,
        scopes: &[BodyScope],
        line: u32,
    ) -> Result<Expr, Error> {
        match expected {
            Type::Ref(BorrowMode::Exclusive, _) => match expr {
                Expr::Borrow(BorrowMode::Exclusive, inner) => {
                    Ok(Expr::Borrow(BorrowMode::Exclusive, inner.clone()))
                }
                _ => fail(
                    line,
                    "U11",
                    "an `&~` borrow argument must be marked with `~`",
                ),
            },
            Type::Ref(BorrowMode::Shared, _) => match expr {
                Expr::Move(_) => fail(line, "U11", "`▾` cannot be used for a `&` borrow argument"),
                Expr::Borrow(BorrowMode::Exclusive, _) => {
                    fail(line, "U11", "`~` cannot be used for a `&` borrow argument")
                }
                other => Ok(other.clone()),
            },
            _ => {
                let ob = self.holds_obligation(expected);
                match expr {
                    Expr::Borrow(BorrowMode::Exclusive, _) => {
                        fail(line, "U11", "`~` is only written for an `&~` borrow slot")
                    }
                    Expr::Move(inner) => {
                        if !ob {
                            return fail(
                                line,
                                "U11",
                                "`▾` is only written when moving a value whose type holds an obligation",
                            );
                        }
                        Ok(Expr::Move(inner.clone()))
                    }
                    Expr::Struct(sname, args) => {
                        if let Some(st) = self.structs.get(sname) {
                            let mut checked = Vec::with_capacity(args.len());
                            for (arg, field) in args.iter().zip(&st.fields) {
                                checked.push(self.check_arg(arg, &field.ty, scopes, line)?);
                            }
                            for arg in args.iter().skip(st.fields.len()) {
                                checked.push(arg.clone());
                            }
                            return Ok(Expr::Struct(sname.clone(), checked));
                        }
                        Ok(expr.clone())
                    }
                    Expr::Variant(vname, cname, args) => {
                        if let Some(ctors) = self.variants.get(vname)
                            && let Some(ctor) = ctors.iter().find(|c| c.name == *cname)
                        {
                            let mut checked = Vec::with_capacity(args.len());
                            for (arg, field) in args.iter().zip(&ctor.fields) {
                                checked.push(self.check_arg(arg, &field.ty, scopes, line)?);
                            }
                            for arg in args.iter().skip(ctor.fields.len()) {
                                checked.push(arg.clone());
                            }
                            return Ok(Expr::Variant(vname.clone(), cname.clone(), checked));
                        }
                        Ok(expr.clone())
                    }
                    Expr::Vector(items) => {
                        if let Type::Vector(elem_ty) = expected {
                            let mut checked = Vec::with_capacity(items.len());
                            for item in items {
                                checked.push(self.check_arg(item, elem_ty, scopes, line)?);
                            }
                            return Ok(Expr::Vector(checked));
                        }
                        Ok(expr.clone())
                    }
                    other => {
                        if ob && Self::is_place_expr(other) {
                            return fail(
                                line,
                                "U11",
                                "moving a value whose type holds an obligation requires the move mark `▾`",
                            );
                        }
                        Ok(other.clone())
                    }
                }
            }
        }
    }

    fn check_untyped_value(
        &self,
        expr: &Expr,
        scopes: &[BodyScope],
        line: u32,
    ) -> Result<Expr, Error> {
        match expr {
            Expr::Move(inner) => {
                if let Some(ty) = self.expr_type(inner, scopes)
                    && !self.holds_obligation(&ty)
                {
                    return fail(
                        line,
                        "U11",
                        "`▾` is only written when moving a value whose type holds an obligation",
                    );
                }
                Ok(expr.clone())
            }
            Expr::Borrow(BorrowMode::Exclusive, _) => fail(
                line,
                "U11",
                "`~` requires an `&~` borrow type annotation on a bind",
            ),
            other => {
                if let Some(ty) = self.expr_type(other, scopes) {
                    return self.check_arg(other, &ty, scopes, line);
                }
                Ok(other.clone())
            }
        }
    }

    fn check_args_against(
        &self,
        arguments: &[Expr],
        param_types: &[Type],
        scopes: &[BodyScope],
        line: u32,
    ) -> Result<Vec<Expr>, Error> {
        let mut out = Vec::with_capacity(arguments.len());
        for (idx, arg) in arguments.iter().enumerate() {
            if let Some(expected) = param_types.get(idx) {
                out.push(self.check_arg(arg, expected, scopes, line)?);
            } else {
                out.push(arg.clone());
            }
        }
        Ok(out)
    }

    fn lookup_captured_contract(
        &self,
        name: &str,
        scopes: &[BodyScope],
    ) -> Option<ResolvedContract> {
        if let Some(contract) = scopes
            .iter()
            .rev()
            .find_map(|s| s.captured_contracts.get(name).cloned())
        {
            return Some(contract);
        }
        let ty = scopes.iter().rev().find_map(|s| s.vars.get(name))?;
        match ty {
            Type::OutcomeNamed(o) => self.outcomes.get(o).map(|alts| ResolvedContract {
                alternatives: alts.clone(),
                groups: vec![o.clone()],
                own_alternatives: Vec::new(),
            }),
            Type::Named(o) => self.outcomes.get(o).map(|alts| ResolvedContract {
                alternatives: alts.clone(),
                groups: vec![o.clone()],
                own_alternatives: Vec::new(),
            }),
            _ => None,
        }
    }

    fn place_type(&self, place: &Place, scopes: &[BodyScope]) -> Option<Type> {
        let mut ty = scopes
            .iter()
            .rev()
            .find_map(|s| s.vars.get(&place.root).cloned())?
            .deref_type()
            .clone();
        for step in &place.steps {
            match step {
                PlaceStep::Field(fname) => {
                    let Type::Named(sname) = ty else {
                        return None;
                    };
                    let st = self.structs.get(&sname)?;
                    ty = st.fields.iter().find(|f| f.name == *fname)?.ty.clone();
                }
                PlaceStep::Index(_) => {
                    let Type::Vector(elem) = ty else {
                        return None;
                    };
                    ty = *elem;
                }
            }
        }
        Some(ty)
    }

    fn expr_type(&self, expr: &Expr, scopes: &[BodyScope]) -> Option<Type> {
        match expr {
            Expr::Integer(_) | Expr::EmptyMap => None,
            Expr::Boolean(_) => Some(Type::Bool),
            Expr::String(_) => Some(Type::String),
            Expr::Name(name) | Expr::Old(name) => scopes
                .iter()
                .rev()
                .find_map(|scope| scope.vars.get(name).cloned()),
            Expr::Move(inner) | Expr::Clone(inner) => self
                .expr_type(inner, scopes)
                .map(|t| t.deref_type().clone()),
            Expr::Borrow(mode, inner) => self
                .expr_type(inner, scopes)
                .map(|t| Type::Ref(*mode, Box::new(t.deref_type().clone()))),
            Expr::Len(_) => Some(Type::Int(IntType::Usize)),
            Expr::Struct(name, _) => Some(Type::Named(name.clone())),
            Expr::Variant(name, _, _) => Some(Type::Named(name.clone())),
            Expr::Vector(items) => items
                .iter()
                .find_map(|i| self.expr_type(i, scopes))
                .map(|t| Type::Vector(Box::new(t))),
            Expr::Field(base, field) => {
                if let Expr::Name(vname) = &**base
                    && scopes.iter().all(|s| !s.vars.contains_key(vname))
                    && self.variants.contains_key(vname)
                {
                    return Some(Type::Named(vname.clone()));
                }
                let base_ty = self.expr_type(base, scopes)?.deref_type().clone();
                let Type::Named(sname) = base_ty else {
                    return None;
                };
                self.structs
                    .get(&sname)?
                    .fields
                    .iter()
                    .find(|f| f.name == *field)
                    .map(|f| f.ty.clone())
            }
            Expr::Index(base, _) => {
                let base_ty = self.expr_type(base, scopes)?.deref_type().clone();
                match base_ty {
                    Type::Vector(elem) => Some(*elem),
                    _ => None,
                }
            }
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
            Expr::Call(func, _) => {
                if !self.functions.contains_key(func) && func == "trim" {
                    return Some(Type::String);
                }
                self.functions.get(func).cloned()
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn resolve_builtin_call(
        &self,
        call: &SurfaceCall,
        scopes: &[BodyScope],
        line: u32,
    ) -> Result<Option<(String, Vec<Expr>, ResolvedContract)>, Error> {
        let single_alt = |name: &str, fields: Vec<Parameter>| ResolvedContract {
            alternatives: vec![Alternative {
                name: name.to_string(),
                fields,
                condition: None,
            }],
            groups: Vec::new(),
            own_alternatives: vec![name.to_string()],
        };
        let two_alts = |a: (&str, Vec<Parameter>), b: (&str, Vec<Parameter>)| ResolvedContract {
            alternatives: vec![
                Alternative {
                    name: a.0.to_string(),
                    fields: a.1,
                    condition: None,
                },
                Alternative {
                    name: b.0.to_string(),
                    fields: b.1,
                    condition: None,
                },
            ],
            groups: Vec::new(),
            own_alternatives: vec![a.0.to_string(), b.0.to_string()],
        };

        match call {
            SurfaceCall::Method {
                receiver,
                method,
                arguments,
            } => {
                if self.controls.contains_key(method) {
                    return fail(
                        line,
                        "U12",
                        format!(
                            "`{method}` is a control of this module and cannot be called in method form; call `{method}(…)` instead"
                        ),
                    );
                }
                let rec_ty = self
                    .expr_type(receiver, scopes)
                    .map(|t| t.deref_type().clone());
                match (method.as_str(), rec_ty) {
                    ("push", Some(Type::Vector(elem))) => {
                        if arguments.len() != 1 {
                            return fail(line, "C1", "`push` takes 1 argument");
                        }
                        let val = self.check_arg(&arguments[0], &elem, scopes, line)?;
                        Ok(Some((
                            "push".to_string(),
                            vec![
                                Expr::Borrow(BorrowMode::Exclusive, Box::new(receiver.clone())),
                                val,
                            ],
                            single_alt("done", Vec::new()),
                        )))
                    }
                    ("pop", Some(Type::Vector(elem))) => {
                        if !arguments.is_empty() {
                            return fail(line, "C1", "`pop` takes 0 arguments");
                        }
                        Ok(Some((
                            "pop".to_string(),
                            vec![Expr::Borrow(
                                BorrowMode::Exclusive,
                                Box::new(receiver.clone()),
                            )],
                            two_alts(
                                (
                                    "item",
                                    vec![Parameter {
                                        name: "value".to_string(),
                                        ty: *elem,
                                    }],
                                ),
                                ("empty", Vec::new()),
                            ),
                        )))
                    }
                    ("take", Some(Type::Vector(elem))) => {
                        if !arguments.is_empty() {
                            return fail(line, "C1", "`take` takes 0 arguments");
                        }
                        let vec_ty = Type::Vector(elem.clone());
                        Ok(Some((
                            "take".to_string(),
                            vec![receiver.clone()],
                            two_alts(
                                (
                                    "item",
                                    vec![
                                        Parameter {
                                            name: "head".to_string(),
                                            ty: *elem,
                                        },
                                        Parameter {
                                            name: "tail".to_string(),
                                            ty: vec_ty,
                                        },
                                    ],
                                ),
                                ("empty", Vec::new()),
                            ),
                        )))
                    }
                    ("get", Some(Type::Map(k, v))) => {
                        if arguments.len() != 1 {
                            return fail(line, "C1", "`get` takes 1 argument");
                        }
                        let k_ref = Type::Ref(BorrowMode::Shared, k);
                        let key = self.check_arg(&arguments[0], &k_ref, scopes, line)?;
                        Ok(Some((
                            "get".to_string(),
                            vec![
                                Expr::Borrow(BorrowMode::Shared, Box::new(receiver.clone())),
                                key,
                            ],
                            two_alts(
                                (
                                    "found",
                                    vec![Parameter {
                                        name: "value".to_string(),
                                        ty: *v,
                                    }],
                                ),
                                ("missing", Vec::new()),
                            ),
                        )))
                    }
                    ("insert", Some(Type::Map(k, v))) => {
                        if arguments.len() != 2 {
                            return fail(line, "C1", "`insert` takes 2 arguments");
                        }
                        let key = self.check_arg(&arguments[0], &k, scopes, line)?;
                        let val = self.check_arg(&arguments[1], &v, scopes, line)?;
                        Ok(Some((
                            "insert".to_string(),
                            vec![
                                Expr::Borrow(BorrowMode::Exclusive, Box::new(receiver.clone())),
                                key,
                                val,
                            ],
                            two_alts(
                                ("inserted", Vec::new()),
                                (
                                    "replaced",
                                    vec![Parameter {
                                        name: "previous".to_string(),
                                        ty: *v,
                                    }],
                                ),
                            ),
                        )))
                    }
                    ("remove", Some(Type::Map(k, v))) => {
                        if arguments.len() != 1 {
                            return fail(line, "C1", "`remove` takes 1 argument");
                        }
                        let k_ref = Type::Ref(BorrowMode::Shared, k);
                        let key = self.check_arg(&arguments[0], &k_ref, scopes, line)?;
                        Ok(Some((
                            "remove".to_string(),
                            vec![
                                Expr::Borrow(BorrowMode::Exclusive, Box::new(receiver.clone())),
                                key,
                            ],
                            two_alts(
                                (
                                    "removed",
                                    vec![Parameter {
                                        name: "previous".to_string(),
                                        ty: *v,
                                    }],
                                ),
                                ("missing", Vec::new()),
                            ),
                        )))
                    }
                    ("entry_at", Some(Type::Map(k, v))) => {
                        if arguments.len() != 1 {
                            return fail(line, "C1", "`entry_at` takes 1 argument");
                        }
                        let pos = self.check_arg(
                            &arguments[0],
                            &Type::Int(IntType::Usize),
                            scopes,
                            line,
                        )?;
                        Ok(Some((
                            "entry_at".to_string(),
                            vec![
                                Expr::Borrow(BorrowMode::Shared, Box::new(receiver.clone())),
                                pos,
                            ],
                            two_alts(
                                (
                                    "item",
                                    vec![
                                        Parameter {
                                            name: "key".to_string(),
                                            ty: *k,
                                        },
                                        Parameter {
                                            name: "value".to_string(),
                                            ty: *v,
                                        },
                                        Parameter {
                                            name: "next".to_string(),
                                            ty: Type::Int(IntType::Usize),
                                        },
                                    ],
                                ),
                                ("finished", Vec::new()),
                            ),
                        )))
                    }
                    (_, other) => fail(
                        line,
                        "U12",
                        format!(
                            "`{method}` is not a built-in method of {}",
                            other
                                .map(|t| format!("`{t}`"))
                                .unwrap_or_else(|| "this receiver".to_string())
                        ),
                    ),
                }
            }
            SurfaceCall::Direct {
                operation,
                arguments,
            } => {
                if self.controls.contains_key(operation) {
                    return Ok(None);
                }
                match operation.as_str() {
                    "emit" => {
                        if arguments.len() != 1 {
                            return fail(line, "C1", "`emit` takes 1 argument");
                        }
                        let s_ref = Type::Ref(BorrowMode::Shared, Box::new(Type::String));
                        let text = self.check_arg(&arguments[0], &s_ref, scopes, line)?;
                        Ok(Some((
                            "emit".to_string(),
                            vec![text],
                            single_alt("done", Vec::new()),
                        )))
                    }
                    "push" => {
                        if arguments.len() != 2 {
                            return fail(line, "C1", "`push` takes 2 arguments");
                        }
                        let Some(Type::Vector(elem)) = self
                            .expr_type(&arguments[0], scopes)
                            .map(|t| t.deref_type().clone())
                        else {
                            return fail(line, "C1", "`push` requires a vector place");
                        };
                        let rec = self.check_arg(
                            &arguments[0],
                            &Type::Ref(BorrowMode::Exclusive, Box::new(Type::Vector(elem.clone()))),
                            scopes,
                            line,
                        )?;
                        let val = self.check_arg(&arguments[1], &elem, scopes, line)?;
                        Ok(Some((
                            "push".to_string(),
                            vec![rec, val],
                            single_alt("done", Vec::new()),
                        )))
                    }
                    "pop" => {
                        if arguments.len() != 1 {
                            return fail(line, "C1", "`pop` takes 1 argument");
                        }
                        let Some(Type::Vector(elem)) = self
                            .expr_type(&arguments[0], scopes)
                            .map(|t| t.deref_type().clone())
                        else {
                            return fail(line, "C1", "`pop` requires a vector place");
                        };
                        let rec = self.check_arg(
                            &arguments[0],
                            &Type::Ref(BorrowMode::Exclusive, Box::new(Type::Vector(elem.clone()))),
                            scopes,
                            line,
                        )?;
                        Ok(Some((
                            "pop".to_string(),
                            vec![rec],
                            two_alts(
                                (
                                    "item",
                                    vec![Parameter {
                                        name: "value".to_string(),
                                        ty: *elem,
                                    }],
                                ),
                                ("empty", Vec::new()),
                            ),
                        )))
                    }
                    "take" => {
                        if arguments.len() != 1 {
                            return fail(line, "C1", "`take` takes 1 argument");
                        }
                        let Some(Type::Vector(elem)) = self
                            .expr_type(&arguments[0], scopes)
                            .map(|t| t.deref_type().clone())
                        else {
                            return fail(line, "C1", "`take` requires a vector");
                        };
                        let vec_ty = Type::Vector(elem.clone());
                        let rec = self.check_arg(&arguments[0], &vec_ty, scopes, line)?;
                        Ok(Some((
                            "take".to_string(),
                            vec![rec],
                            two_alts(
                                (
                                    "item",
                                    vec![
                                        Parameter {
                                            name: "head".to_string(),
                                            ty: *elem,
                                        },
                                        Parameter {
                                            name: "tail".to_string(),
                                            ty: vec_ty,
                                        },
                                    ],
                                ),
                                ("empty", Vec::new()),
                            ),
                        )))
                    }
                    "get" => {
                        if arguments.len() != 2 {
                            return fail(line, "C1", "`get` takes 2 arguments");
                        }
                        let Some(Type::Map(k, v)) = self
                            .expr_type(&arguments[0], scopes)
                            .map(|t| t.deref_type().clone())
                        else {
                            return fail(line, "C1", "`get` requires a map");
                        };
                        let map_ref = Type::Ref(
                            BorrowMode::Shared,
                            Box::new(Type::Map(k.clone(), v.clone())),
                        );
                        let k_ref = Type::Ref(BorrowMode::Shared, k);
                        let m = self.check_arg(&arguments[0], &map_ref, scopes, line)?;
                        let key = self.check_arg(&arguments[1], &k_ref, scopes, line)?;
                        Ok(Some((
                            "get".to_string(),
                            vec![m, key],
                            two_alts(
                                (
                                    "found",
                                    vec![Parameter {
                                        name: "value".to_string(),
                                        ty: *v,
                                    }],
                                ),
                                ("missing", Vec::new()),
                            ),
                        )))
                    }
                    "insert" => {
                        if arguments.len() != 3 {
                            return fail(line, "C1", "`insert` takes 3 arguments");
                        }
                        let Some(Type::Map(k, v)) = self
                            .expr_type(&arguments[0], scopes)
                            .map(|t| t.deref_type().clone())
                        else {
                            return fail(line, "C1", "`insert` requires a map place");
                        };
                        let map_ref = Type::Ref(
                            BorrowMode::Exclusive,
                            Box::new(Type::Map(k.clone(), v.clone())),
                        );
                        let m = self.check_arg(&arguments[0], &map_ref, scopes, line)?;
                        let key = self.check_arg(&arguments[1], &k, scopes, line)?;
                        let val = self.check_arg(&arguments[2], &v, scopes, line)?;
                        Ok(Some((
                            "insert".to_string(),
                            vec![m, key, val],
                            two_alts(
                                ("inserted", Vec::new()),
                                (
                                    "replaced",
                                    vec![Parameter {
                                        name: "previous".to_string(),
                                        ty: *v,
                                    }],
                                ),
                            ),
                        )))
                    }
                    "remove" => {
                        if arguments.len() != 2 {
                            return fail(line, "C1", "`remove` takes 2 arguments");
                        }
                        let Some(Type::Map(k, v)) = self
                            .expr_type(&arguments[0], scopes)
                            .map(|t| t.deref_type().clone())
                        else {
                            return fail(line, "C1", "`remove` requires a map place");
                        };
                        let map_ref = Type::Ref(
                            BorrowMode::Exclusive,
                            Box::new(Type::Map(k.clone(), v.clone())),
                        );
                        let k_ref = Type::Ref(BorrowMode::Shared, k);
                        let m = self.check_arg(&arguments[0], &map_ref, scopes, line)?;
                        let key = self.check_arg(&arguments[1], &k_ref, scopes, line)?;
                        Ok(Some((
                            "remove".to_string(),
                            vec![m, key],
                            two_alts(
                                (
                                    "removed",
                                    vec![Parameter {
                                        name: "previous".to_string(),
                                        ty: *v,
                                    }],
                                ),
                                ("missing", Vec::new()),
                            ),
                        )))
                    }
                    "entry_at" => {
                        if arguments.len() != 2 {
                            return fail(line, "C1", "`entry_at` takes 2 arguments");
                        }
                        let Some(Type::Map(k, v)) = self
                            .expr_type(&arguments[0], scopes)
                            .map(|t| t.deref_type().clone())
                        else {
                            return fail(line, "C1", "`entry_at` requires a map");
                        };
                        let map_ref = Type::Ref(
                            BorrowMode::Shared,
                            Box::new(Type::Map(k.clone(), v.clone())),
                        );
                        let m = self.check_arg(&arguments[0], &map_ref, scopes, line)?;
                        let pos = self.check_arg(
                            &arguments[1],
                            &Type::Int(IntType::Usize),
                            scopes,
                            line,
                        )?;
                        Ok(Some((
                            "entry_at".to_string(),
                            vec![m, pos],
                            two_alts(
                                (
                                    "item",
                                    vec![
                                        Parameter {
                                            name: "key".to_string(),
                                            ty: *k,
                                        },
                                        Parameter {
                                            name: "value".to_string(),
                                            ty: *v,
                                        },
                                        Parameter {
                                            name: "next".to_string(),
                                            ty: Type::Int(IntType::Usize),
                                        },
                                    ],
                                ),
                                ("finished", Vec::new()),
                            ),
                        )))
                    }
                    _ => Ok(None),
                }
            }
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
                contingent: control.contingent,
                faults: control.faults,
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
            captured_contracts: HashMap::new(),
        };
        for param in &control.parameters {
            root_scope.vars.insert(param.name.clone(), param.ty.clone());
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
            declared_routes: Vec::new(),
            used_unqualified_paths: HashSet::new(),
        };
        collect_declared_paths(
            &control.body,
            &mut ctx.all_declared_sites,
            &mut ctx.all_declared_paths,
        );

        let statements = self.elaborate_items(&control.body, &mut ctx)?;

        // [U17]: Warn when an arm introduces a route `→ name` and no later
        // label in the body mentions `name`.
        let mut warned_routes = HashSet::new();
        for (route_line, route_name) in &ctx.declared_routes {
            if !ctx.used_unqualified_paths.contains(route_name)
                && warned_routes.insert((*route_line, route_name.clone()))
            {
                self.warnings.push(Warning {
                    control: control.name.clone(),
                    line: *route_line,
                    clause: "U17",
                    message: format!(
                        "the route `{route_name}` is never mentioned in a later label mask"
                    ),
                });
            }
        }

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
            contingent: control.contingent,
            faults: control.faults,
            parameters: control.parameters.clone(),
            precondition: control.precondition.clone(),
            contract,
            uses: control.uses.clone(),
            body: statements,
            line: control.line,
        })
    }

    fn elaborate_boundary(
        &mut self,
        boundary: &SurfaceBoundaryDecl,
    ) -> Result<BoundaryDecl, Error> {
        let mut body_scope = BodyScope {
            sites: Vec::new(),
            paths: Vec::new(),
            suppressed_defaults: HashSet::new(),
            vars: HashMap::new(),
            captured_contracts: HashMap::new(),
        };
        for param in &boundary.parameters {
            body_scope.vars.insert(param.name.clone(), param.ty.clone());
        }
        let mut body_ctx = ControlElab {
            control_name: boundary.name.clone(),
            is_auto: false,
            used_core_sites: HashSet::new(),
            explicit_sites: HashSet::new(),
            all_declared_sites: HashSet::new(),
            all_declared_paths: HashSet::new(),
            next_hidden: 0,
            scopes: vec![body_scope],
            auto_exits: Vec::new(),
            auto_groups: Vec::new(),
            declared_routes: Vec::new(),
            used_unqualified_paths: HashSet::new(),
        };
        collect_declared_paths(
            &boundary.body,
            &mut body_ctx.all_declared_sites,
            &mut body_ctx.all_declared_paths,
        );
        let body = self.elaborate_items(&boundary.body, &mut body_ctx)?;

        let mut cont_scope = BodyScope {
            sites: Vec::new(),
            paths: Vec::new(),
            suppressed_defaults: HashSet::new(),
            vars: HashMap::new(),
            captured_contracts: HashMap::new(),
        };
        for retained in &boundary.retains {
            if let Some(param) = boundary.parameters.iter().find(|p| &p.name == retained) {
                cont_scope.vars.insert(param.name.clone(), param.ty.clone());
            }
        }
        cont_scope
            .vars
            .insert(boundary.message.name.clone(), boundary.message.ty.clone());
        let mut cont_ctx = ControlElab {
            control_name: format!("{}.contingency", boundary.name),
            is_auto: false,
            used_core_sites: HashSet::new(),
            explicit_sites: HashSet::new(),
            all_declared_sites: HashSet::new(),
            all_declared_paths: HashSet::new(),
            next_hidden: 0,
            scopes: vec![cont_scope],
            auto_exits: Vec::new(),
            auto_groups: Vec::new(),
            declared_routes: Vec::new(),
            used_unqualified_paths: HashSet::new(),
        };
        collect_declared_paths(
            &boundary.contingency,
            &mut cont_ctx.all_declared_sites,
            &mut cont_ctx.all_declared_paths,
        );
        let contingency = self.elaborate_items(&boundary.contingency, &mut cont_ctx)?;

        Ok(BoundaryDecl {
            name: boundary.name.clone(),
            parameters: boundary.parameters.clone(),
            retains: boundary.retains.clone(),
            body,
            message: boundary.message.name.clone(),
            contingency,
            line: boundary.line,
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
                    match (&decided, &item.field_types[field_idx]) {
                        (Some(a), Some(b)) if a != b => {
                            return fail(
                                item.line,
                                "G3",
                                format!(
                                    "exits for `{alt_name}` differ in payload type: {a} and {b}"
                                ),
                            );
                        }
                        (None, Some(b)) => decided = Some(b.clone()),
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
                SurfaceItem::Blank | SurfaceItem::Comment(_) => {}
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

    fn elaborate_mask(&mut self, mask: &Mask, ctx: &mut ControlElab) -> Result<Selector, Error> {
        match mask {
            Mask::And(left, right) => Ok(Selector::And(
                Box::new(self.elaborate_mask(left, ctx)?),
                Box::new(self.elaborate_mask(right, ctx)?),
            )),
            Mask::Or(left, right) => Ok(Selector::Or(
                Box::new(self.elaborate_mask(left, ctx)?),
                Box::new(self.elaborate_mask(right, ctx)?),
            )),
            Mask::Not { site, name, line } => {
                let resolved = self.resolve_path(site.as_deref(), name, *line, ctx)?;
                // [U5] / [U18]: Warn when `¬` is applied to a path of a decision
                // that has only one other continuing path, and suggest that path.
                if let Some(other_path) =
                    self.single_other_continuing_path(site.as_deref(), name, &resolved, ctx)
                {
                    self.warnings.push(Warning {
                        control: ctx.control_name.clone(),
                        line: *line,
                        clause: "U5",
                        message: format!(
                            "`¬{}` on a two-path decision can be written directly as `{other_path}`",
                            match site {
                                Some(s) => format!("{s}.{name}"),
                                None => name.clone(),
                            }
                        ),
                    });
                }
                Ok(Selector::Not(Box::new(resolved)))
            }
            Mask::Path { site, name, line } => self.resolve_path(site.as_deref(), name, *line, ctx),
        }
    }

    fn single_other_continuing_path(
        &self,
        site: Option<&str>,
        negated_name: &str,
        resolved: &Selector,
        ctx: &ControlElab,
    ) -> Option<String> {
        let Selector::Atom(core_site, negated_alt) = resolved else {
            return None;
        };
        // Find all continuing paths in scope for `core_site` whose alternative != `negated_alt`.
        let mut other_paths: Vec<&PathEntry> = ctx
            .scopes
            .iter()
            .flat_map(|scope| &scope.paths)
            .filter(|entry| &entry.core_site == core_site && &entry.alternative != negated_alt)
            .collect();
        other_paths.dedup_by(|a, b| a.path_name == b.path_name && a.alternative == b.alternative);
        if other_paths.len() != 1 {
            return None;
        }
        let other = other_paths[0];
        if other.path_name == negated_name {
            return None;
        }
        if let Some(site_name) = site
            && !other.is_route
        {
            return Some(format!("{site_name}.{}", other.path_name));
        }
        Some(other.path_name.clone())
    }

    fn resolve_path(
        &self,
        site: Option<&str>,
        name: &str,
        line: u32,
        ctx: &mut ControlElab,
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
        ctx.used_unqualified_paths.insert(name.to_string());
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
            SurfaceStmt::Blank | SurfaceStmt::Comment(_) => {}
            SurfaceStmt::Bind {
                line,
                mutable,
                name,
                ty,
                value,
            } => {
                let checked_value = if let Some(declared_ty) = ty {
                    self.check_arg(value, declared_ty, &ctx.scopes, *line)?
                } else {
                    self.check_untyped_value(value, &ctx.scopes, *line)?
                };
                let inferred = ty
                    .clone()
                    .or_else(|| self.expr_type(&checked_value, &ctx.scopes))
                    .unwrap_or(Type::Int(IntType::U32));
                ctx.current_scope_mut().vars.insert(name.clone(), inferred);
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Bind {
                        mutable: *mutable,
                        name: name.clone(),
                        ty: ty.clone(),
                        value: checked_value,
                    },
                    line: *line,
                });
            }
            SurfaceStmt::Unpack {
                line,
                struct_name,
                names,
                value,
            } => {
                let checked_value =
                    self.check_arg(value, &Type::Named(struct_name.clone()), &ctx.scopes, *line)?;
                if let Some(st) = self.structs.get(struct_name).cloned() {
                    for (var_name, field) in names.iter().zip(&st.fields) {
                        ctx.current_scope_mut()
                            .vars
                            .insert(var_name.clone(), field.ty.clone());
                    }
                }
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Unpack {
                        struct_name: struct_name.clone(),
                        names: names.clone(),
                        value: checked_value,
                    },
                    line: *line,
                });
            }
            SurfaceStmt::Capture {
                line,
                mutable,
                name,
                ty,
                call,
            } => {
                let (op_name, core_args, contract) = match call {
                    SurfaceCall::Direct {
                        operation,
                        arguments,
                    } if self.controls.contains_key(operation) => {
                        let callee = self.ensure_contract(operation, *line)?;
                        let param_types: Vec<Type> = self.controls[operation]
                            .parameters
                            .iter()
                            .map(|p| p.ty.clone())
                            .collect();
                        let checked =
                            self.check_args_against(arguments, &param_types, &ctx.scopes, *line)?;
                        (operation.clone(), checked, callee)
                    }
                    other => {
                        let Some(res) = self.resolve_builtin_call(other, &ctx.scopes, *line)?
                        else {
                            let SurfaceCall::Direct { operation, .. } = other else {
                                unreachable!();
                            };
                            return fail(*line, "C1", format!("`{operation}` is not a control"));
                        };
                        res
                    }
                };
                let outcome_ty = ty
                    .clone()
                    .unwrap_or_else(|| Type::OutcomeNamed(op_name.clone()));
                ctx.current_scope_mut()
                    .vars
                    .insert(name.clone(), outcome_ty);
                ctx.current_scope_mut()
                    .captured_contracts
                    .insert(name.clone(), contract);
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Capture {
                        mutable: *mutable,
                        name: name.clone(),
                        ty: ty.clone(),
                        operation: op_name,
                        arguments: core_args,
                    },
                    line: *line,
                });
            }
            SurfaceStmt::Assign { line, name, value } => {
                let checked_value = if let Some(var_ty) = ctx
                    .scopes
                    .iter()
                    .rev()
                    .find_map(|s| s.vars.get(name).cloned())
                {
                    self.check_arg(value, var_ty.deref_type(), &ctx.scopes, *line)?
                } else {
                    self.check_untyped_value(value, &ctx.scopes, *line)?
                };
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Assign {
                        name: name.clone(),
                        value: checked_value,
                    },
                    line: *line,
                });
            }
            SurfaceStmt::AssignPlace {
                line,
                target,
                value,
            } => {
                let checked_value = if let Some(place_ty) = self.place_type(target, &ctx.scopes) {
                    self.check_arg(value, &place_ty, &ctx.scopes, *line)?
                } else {
                    self.check_untyped_value(value, &ctx.scopes, *line)?
                };
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::AssignPlace {
                        target: target.clone(),
                        value: checked_value,
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
            SurfaceStmt::Fault {
                line,
                message,
                assumed,
            } => {
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Fault {
                        message: message.clone(),
                        assumed: *assumed,
                    },
                    line: *line,
                });
            }
            SurfaceStmt::Terminate { line, value } => {
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Terminate(value.clone()),
                    line: *line,
                });
            }
            SurfaceStmt::Escalate { line } => {
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Escalate,
                    line: *line,
                });
            }
            SurfaceStmt::Recover { line, loop_name } => {
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Recover {
                        loop_name: loop_name.clone(),
                    },
                    line: *line,
                });
            }
            SurfaceStmt::Supervise {
                line,
                boundary,
                arguments,
            } => {
                let checked_args = if let Some(bdef) = self.boundaries.get(boundary) {
                    let param_types: Vec<Type> =
                        bdef.parameters.iter().map(|p| p.ty.clone()).collect();
                    self.check_args_against(arguments, &param_types, &ctx.scopes, *line)?
                } else {
                    arguments.clone()
                };
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Supervise {
                        boundary: boundary.clone(),
                        arguments: checked_args,
                    },
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
                let mut payload = Vec::with_capacity(outputs.len());
                for item in outputs {
                    payload.push(self.check_untyped_value(&item.value, &ctx.scopes, *line)?);
                }
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
                let checked_payload = if !ctx.is_auto
                    && let Some(caller) = self.contracts.get(&ctx.control_name)
                    && let Some(entry) = caller.alternatives.iter().find(|a| a.name == *alternative)
                {
                    let field_types: Vec<Type> =
                        entry.fields.iter().map(|f| f.ty.clone()).collect();
                    self.check_args_against(payload, &field_types, &ctx.scopes, *line)?
                } else {
                    let mut out_p = Vec::with_capacity(payload.len());
                    for item in payload {
                        out_p.push(self.check_untyped_value(item, &ctx.scopes, *line)?);
                    }
                    out_p
                };
                self.record_exit(ctx, *line, alternative, &checked_payload, None, None)?;
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Exit {
                        alternative: alternative.clone(),
                        payload: checked_payload,
                    },
                    line: *line,
                });
            }
            SurfaceStmt::ExitOutcome { line, name } => {
                // [U14]: `■ ▾x` forwards a captured outcome under each alternative's own name.
                let Some(callee) = self.lookup_captured_contract(name, &ctx.scopes) else {
                    return fail(
                        *line,
                        "U14",
                        format!("`{name}` is not a captured outcome here"),
                    );
                };
                self.check_forward_compatibility(&format!("■ ▾{name}"), &callee, ctx, *line)?;
                let core_site = ctx.allocate_site(None, name, *line)?;
                let mut core_arms = Vec::new();
                let mut exits = Vec::new();
                for alt in &callee.alternatives {
                    let vars: Vec<String> = (0..alt.fields.len())
                        .map(|_| ctx.fresh_hidden_var())
                        .collect();
                    let payload: Vec<Expr> = vars.iter().cloned().map(Expr::Name).collect();
                    let field_names = Some(alt.fields.iter().map(|f| f.name.clone()).collect());
                    let field_types = Some(alt.fields.iter().map(|f| Some(f.ty.clone())).collect());
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
                        operation: "outcome".to_string(),
                        arguments: vec![Expr::Name(name.clone())],
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
            SurfaceStmt::Test {
                line,
                condition,
                body,
                ..
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
            SurfaceStmt::MethodCall {
                line,
                receiver,
                method,
                arguments,
            } => {
                let call = SurfaceCall::Method {
                    receiver: receiver.clone(),
                    method: method.clone(),
                    arguments: arguments.clone(),
                };
                let (op_name, core_args, contract) = self
                    .resolve_builtin_call(&call, &ctx.scopes, *line)?
                    .expect("method call resolves or errors");
                if contract.alternatives.len() != 1 {
                    return fail(
                        *line,
                        "U6",
                        format!(
                            "`{method}` has {} alternatives and must be decided with `Δ`",
                            contract.alternatives.len()
                        ),
                    );
                }
                let single = &contract.alternatives[0];
                let core_site = ctx.allocate_site(None, &op_name, *line)?;
                let hidden_names: Vec<String> = (0..single.fields.len())
                    .map(|_| ctx.fresh_hidden_var())
                    .collect();
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Decide {
                        site: core_site,
                        operation: op_name,
                        arguments: core_args,
                        arms: vec![Arm {
                            alternative: single.name.clone(),
                            names: hidden_names,
                        }],
                    },
                    line: *line,
                });
            }
            SurfaceStmt::Call {
                line,
                operation,
                arguments,
            } => {
                // [U6]: ω(ā) is a decision on an operation that has exactly one
                // alternative, and its payload, if any, is dropped.
                let (core_op, core_args, contract) = if self.controls.contains_key(operation) {
                    let contract = self.ensure_contract(operation, *line)?;
                    let param_types: Vec<Type> = self.controls[operation]
                        .parameters
                        .iter()
                        .map(|p| p.ty.clone())
                        .collect();
                    let checked =
                        self.check_args_against(arguments, &param_types, &ctx.scopes, *line)?;
                    (operation.clone(), checked, contract)
                } else if let Some(res) = self.resolve_builtin_call(
                    &SurfaceCall::Direct {
                        operation: operation.clone(),
                        arguments: arguments.clone(),
                    },
                    &ctx.scopes,
                    *line,
                )? {
                    res
                } else {
                    return fail(*line, "C1", format!("`{operation}` is not a control"));
                };
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
                let core_site = ctx.allocate_site(None, &core_op, *line)?;
                let hidden_names: Vec<String> = (0..single.fields.len())
                    .map(|_| ctx.fresh_hidden_var())
                    .collect();
                out.push(Statement {
                    selector: selection.clone(),
                    command: Command::Decide {
                        site: core_site,
                        operation: core_op,
                        arguments: core_args,
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
                let param_types: Vec<Type> = self.controls[operation]
                    .parameters
                    .iter()
                    .map(|p| p.ty.clone())
                    .collect();
                let checked_args =
                    self.check_args_against(arguments, &param_types, &ctx.scopes, *line)?;
                self.check_forward_compatibility(
                    &format!("forward {operation}"),
                    &callee,
                    ctx,
                    *line,
                )?;

                let core_site = ctx.allocate_site(None, operation, *line)?;
                let mut core_arms = Vec::new();
                let mut exits = Vec::new();
                for alt in &callee.alternatives {
                    let vars: Vec<String> = (0..alt.fields.len())
                        .map(|_| ctx.fresh_hidden_var())
                        .collect();
                    let payload: Vec<Expr> = vars.iter().cloned().map(Expr::Name).collect();
                    let field_names = Some(alt.fields.iter().map(|f| f.name.clone()).collect());
                    let field_types = Some(alt.fields.iter().map(|f| Some(f.ty.clone())).collect());
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
                        arguments: checked_args,
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
                ..
            } => match operand {
                SurfaceOperand::Call(call) => {
                    if let SurfaceCall::Direct {
                        operation,
                        arguments,
                    } = call
                        && self.controls.contains_key(operation)
                    {
                        let callee = self.ensure_contract(operation, *line)?;
                        let param_types: Vec<Type> = self.controls[operation]
                            .parameters
                            .iter()
                            .map(|p| p.ty.clone())
                            .collect();
                        let checked_args =
                            self.check_args_against(arguments, &param_types, &ctx.scopes, *line)?;
                        self.elaborate_resolved_decision(
                            *line,
                            site.as_deref(),
                            *adapter,
                            operation,
                            operation,
                            &checked_args,
                            &callee,
                            arms,
                            selection,
                            ctx,
                            out,
                        )?;
                    } else if let Some((op_name, core_args, contract)) =
                        self.resolve_builtin_call(call, &ctx.scopes, *line)?
                    {
                        self.elaborate_resolved_decision(
                            *line,
                            site.as_deref(),
                            *adapter,
                            &op_name,
                            &op_name,
                            &core_args,
                            &contract,
                            arms,
                            selection,
                            ctx,
                            out,
                        )?;
                    } else if let SurfaceCall::Direct {
                        operation,
                        arguments,
                    } = call
                        && self.functions.contains_key(operation)
                    {
                        if *adapter {
                            return fail(
                                *line,
                                "U13",
                                "`■ Δ` requires a control or operation call as its operand",
                            );
                        }
                        self.elaborate_bool_decision(
                            *line,
                            site.as_deref(),
                            &Expr::Call(operation.clone(), arguments.clone()),
                            arms,
                            selection,
                            ctx,
                            out,
                        )?;
                    } else {
                        let op_name = match call {
                            SurfaceCall::Direct { operation, .. } => operation.as_str(),
                            SurfaceCall::Method { method, .. } => method.as_str(),
                        };
                        return fail(*line, "C1", format!("`{op_name}` is not a control"));
                    }
                }
                SurfaceOperand::Expr(expr) => {
                    // Check captured outcome (`Δ ▾x` [U6]) first.
                    if let Expr::Move(inner) = expr
                        && let Expr::Name(var_name) = &**inner
                        && let Some(contract) = self.lookup_captured_contract(var_name, &ctx.scopes)
                    {
                        self.elaborate_resolved_decision(
                            *line,
                            site.as_deref(),
                            *adapter,
                            var_name,
                            "outcome",
                            &[Expr::Name(var_name.clone())],
                            &contract,
                            arms,
                            selection,
                            ctx,
                            out,
                        )?;
                    } else if let Expr::Name(var_name) = expr
                        && self
                            .lookup_captured_contract(var_name, &ctx.scopes)
                            .is_some()
                    {
                        return fail(
                            *line,
                            "U11",
                            format!(
                                "deciding on a captured outcome requires `▾` (`Δ ▾{var_name}`)"
                            ),
                        );
                    } else if let Some(Type::Named(vname)) = self
                        .expr_type(
                            match expr {
                                Expr::Move(inner) | Expr::Borrow(_, inner) => inner,
                                other => other,
                            },
                            &ctx.scopes,
                        )
                        .map(|t| t.deref_type().clone())
                        && let Some(ctors) = self.variants.get(&vname).cloned()
                    {
                        // Variant decision (`Δ p`, `Δ ~p`, `Δ ▾p` [U6]).
                        let default_site =
                            Self::place_last_name(expr).unwrap_or_else(|| vname.clone());
                        let contract = ResolvedContract {
                            alternatives: ctors
                                .iter()
                                .map(|c| Alternative {
                                    name: c.name.clone(),
                                    fields: c.fields.clone(),
                                    condition: None,
                                })
                                .collect(),
                            groups: Vec::new(),
                            own_alternatives: ctors.iter().map(|c| c.name.clone()).collect(),
                        };
                        self.elaborate_resolved_decision(
                            *line,
                            site.as_deref(),
                            *adapter,
                            &default_site,
                            &vname,
                            std::slice::from_ref(expr),
                            &contract,
                            arms,
                            selection,
                            ctx,
                            out,
                        )?;
                    } else {
                        if *adapter {
                            return fail(
                                *line,
                                "U13",
                                "`■ Δ` requires a control, outcome or variant as its operand",
                            );
                        }
                        self.elaborate_bool_decision(
                            *line,
                            site.as_deref(),
                            expr,
                            arms,
                            selection,
                            ctx,
                            out,
                        )?;
                    }
                }
            },
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
            SurfaceStmt::ForAll {
                line,
                var,
                collection,
                body,
            } => {
                self.elaborate_forall(*line, var, collection, body, selection, ctx, out)?;
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
                    ctx.declared_routes.push((arm.line, route.clone()));
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

    fn check_forward_compatibility(
        &self,
        label: &str,
        callee: &ResolvedContract,
        ctx: &mut ControlElab,
        line: u32,
    ) -> Result<(), Error> {
        if ctx.is_auto {
            for group in &callee.groups {
                if !ctx.auto_groups.contains(group) {
                    ctx.auto_groups.push(group.clone());
                }
            }
            return Ok(());
        }
        let caller = &self.contracts[&ctx.control_name];
        for group in &callee.groups {
            if !caller.groups.contains(group) {
                return fail(
                    line,
                    "U14",
                    format!(
                        "`{label}` requires `{}` to include the outcome group `…{group}`",
                        ctx.control_name
                    ),
                );
            }
        }
        for own in &callee.own_alternatives {
            if !caller.own_alternatives.contains(own) {
                return fail(
                    line,
                    "U14",
                    format!(
                        "`{label}` requires `{own}` to be an own entry of `{}`",
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
                    line,
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
                    line,
                    "U14",
                    format!(
                        "the payload of `{}` in `{label}` does not match `{}`",
                        callee_alt.name, ctx.control_name
                    ),
                );
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn elaborate_resolved_decision(
        &mut self,
        line: u32,
        explicit_site: Option<&str>,
        adapter: bool,
        default_site: &str,
        core_operation: &str,
        arguments: &[Expr],
        callee: &ResolvedContract,
        arms: &[SurfaceArm],
        selection: &Selector,
        ctx: &mut ControlElab,
        out: &mut Vec<Statement>,
    ) -> Result<(), Error> {
        let operation = default_site;
        let surface_site = explicit_site.unwrap_or(default_site).to_string();
        let core_site = ctx.allocate_site(explicit_site, default_site, line)?;

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
                    if let Some(written_ty) = &pat.ty
                        && *written_ty != field.ty
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
                        .insert(pat.name.clone(), field.ty.clone());
                    if pat.mutable {
                        let raw = ctx.fresh_hidden_var();
                        arm_vars.push(raw.clone());
                        mutable_rebinds.push((pat.name.clone(), field.ty.clone(), raw));
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
                operation: core_operation.to_string(),
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
                        let field_types = Some(
                            contract_alt
                                .fields
                                .iter()
                                .map(|f| Some(f.ty.clone()))
                                .collect(),
                        );
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
                    ctx.declared_routes.push((exp.line, route.clone()));
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
                    let field_types = Some(
                        contract_alt
                            .fields
                            .iter()
                            .map(|f| Some(f.ty.clone()))
                            .collect(),
                    );
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
            captured_contracts: HashMap::new(),
        };
        for param in parameters {
            loop_scope.vars.insert(param.name.clone(), param.ty.clone());
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
                    if let Some(ty) = finished_loop_scope.vars.get(&output.name).cloned() {
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

    #[allow(clippy::too_many_arguments)]
    fn elaborate_forall(
        &mut self,
        line: u32,
        var: &str,
        collection: &Expr,
        body: &[SurfaceItem],
        selection: &Selector,
        ctx: &mut ControlElab,
        out: &mut Vec<Statement>,
    ) -> Result<(), Error> {
        // [U10]: `∀ x ∈ p {B}` elaborates into a loop over indices `0..len(p)`.
        let elem_ty = match self
            .expr_type(collection, &ctx.scopes)
            .map(|t| t.deref_type().clone())
        {
            Some(Type::Vector(elem)) => *elem,
            other => {
                return fail(
                    line,
                    "U10",
                    format!(
                        "`∀ {var} ∈ …` requires a vector, found {}",
                        other
                            .map(|t| format!("`{t}`"))
                            .unwrap_or_else(|| "an unknown type".to_string())
                    ),
                );
            }
        };

        let loop_name = ctx.fresh_hidden_var();
        let core_site = ctx.allocate_site(None, &loop_name, line)?;
        let idx_var = ctx.fresh_hidden_var();
        let end_site = ctx.fresh_test_site();
        let done_alt = format!("_done_{}", ctx.next_hidden);
        ctx.next_hidden += 1;

        // Collect user `□` leaves in `body` so `B` may leave with named alternatives ([U10]).
        let mut leaves = Vec::new();
        collect_loop_leaves(body, &mut leaves);
        let mut named_order: Vec<String> = Vec::new();
        let mut named_fields: HashMap<String, Vec<String>> = HashMap::new();
        for (leave_line, alt, outputs) in &leaves {
            let Some(alt_name) = alt else {
                return fail(
                    *leave_line,
                    "U10",
                    "a `□` inside `∀` must name its alternative",
                );
            };
            let field_names: Vec<String> = outputs.iter().map(|o| o.name.clone()).collect();
            if let Some(existing) = named_fields.get(alt_name) {
                if *existing != field_names {
                    return fail(
                        *leave_line,
                        "U9",
                        format!("all `□ {alt_name}` of `∀` must give the same field names"),
                    );
                }
            } else {
                named_order.push(alt_name.clone());
                named_fields.insert(alt_name.clone(), field_names);
            }
        }

        let mut loop_scope = BodyScope {
            sites: Vec::new(),
            paths: Vec::new(),
            suppressed_defaults: HashSet::new(),
            vars: HashMap::new(),
            captured_contracts: HashMap::new(),
        };
        loop_scope
            .vars
            .insert(idx_var.clone(), Type::Int(IntType::Usize));
        loop_scope.vars.insert(
            var.to_string(),
            Type::Ref(BorrowMode::Shared, Box::new(elem_ty)),
        );
        ctx.scopes.push(loop_scope);

        let user_stmts = self.elaborate_items(body, ctx)?;
        let finished_loop_scope = ctx.scopes.pop().expect("a forall loop scope");

        let mut core_body = vec![
            Statement {
                selector: Selector::Top,
                command: Command::Test {
                    site: end_site.clone(),
                    condition: Expr::Binary(
                        Binary::Equal,
                        Box::new(Expr::Name(idx_var.clone())),
                        Box::new(Expr::Len(Box::new(collection.clone()))),
                    ),
                },
                line,
            },
            Statement {
                selector: Selector::Atom(end_site, "true".to_string()),
                command: Command::Leave {
                    alternative: done_alt.clone(),
                    payload: Vec::new(),
                },
                line,
            },
            Statement {
                selector: Selector::Top,
                command: Command::Bind {
                    mutable: false,
                    name: var.to_string(),
                    ty: None,
                    value: Expr::Borrow(
                        BorrowMode::Shared,
                        Box::new(Expr::Index(
                            Box::new(collection.clone()),
                            Box::new(Expr::Name(idx_var.clone())),
                        )),
                    ),
                },
                line,
            },
        ];
        core_body.extend(user_stmts);
        core_body.push(Statement {
            selector: Selector::Top,
            command: Command::Again {
                name: loop_name.clone(),
                arguments: vec![Expr::Binary(
                    Binary::Add,
                    Box::new(Expr::Name(idx_var.clone())),
                    Box::new(Expr::Integer(Int::from_u64(1))),
                )],
            },
            line,
        });

        let mut core_arms = vec![Arm {
            alternative: done_alt,
            names: Vec::new(),
        }];
        for alt_name in &named_order {
            core_arms.push(Arm {
                alternative: alt_name.clone(),
                names: named_fields[alt_name].clone(),
            });
            ctx.current_scope_mut().paths.push(PathEntry {
                core_site: core_site.clone(),
                alternative: alt_name.clone(),
                path_name: alt_name.clone(),
                is_route: false,
            });
        }
        for (_, alt, outputs) in &leaves {
            if alt.is_some() {
                for output in outputs {
                    if let Some(ty) = finished_loop_scope.vars.get(&output.name).cloned() {
                        ctx.current_scope_mut().vars.insert(output.name.clone(), ty);
                    }
                }
            }
        }
        if !named_order.is_empty() {
            ctx.current_scope_mut().sites.push(SiteRecord {
                surface_name: loop_name.clone(),
                core_name: core_site.clone(),
                alternatives: named_order,
            });
        }

        out.push(Statement {
            selector: selection.clone(),
            command: Command::Loop {
                site: core_site,
                name: loop_name,
                parameters: vec![LoopParameter {
                    mutable: false,
                    name: idx_var.clone(),
                    ty: Type::Int(IntType::Usize),
                    initial: Expr::Integer(Int::zero()),
                }],
                invariant: Some(Expr::Binary(
                    Binary::LessEqual,
                    Box::new(Expr::Name(idx_var)),
                    Box::new(Expr::Len(Box::new(collection.clone()))),
                )),
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
    declared_routes: Vec<(u32, String)>,
    used_unqualified_paths: HashSet<String>,
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
            SurfaceItem::Blank | SurfaceItem::Comment(_) => {}
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
            } else {
                match operand {
                    SurfaceOperand::Call(SurfaceCall::Direct { operation, .. }) => {
                        sites.insert(operation.clone());
                    }
                    SurfaceOperand::Call(SurfaceCall::Method { method, .. }) => {
                        sites.insert(method.clone());
                    }
                    SurfaceOperand::Expr(expr) => {
                        if let Some(n) = Elaborator::place_last_name(expr) {
                            sites.insert(n);
                        }
                    }
                }
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
        SurfaceStmt::ForAll { body, .. } => {
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
            SurfaceItem::Blank | SurfaceItem::Comment(_) => {}
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
        SurfaceStmt::Loop { .. } | SurfaceStmt::ForAll { .. } => {}
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
        Expr::Integer(_) | Expr::Boolean(_) | Expr::String(_) | Expr::EmptyMap => {}
        Expr::Name(name) | Expr::Old(name) => {
            if let Some(new_name) = renames.get(name) {
                *name = new_name.clone();
            }
        }
        Expr::Unary(_, inner)
        | Expr::Move(inner)
        | Expr::Borrow(_, inner)
        | Expr::Field(inner, _)
        | Expr::Len(inner)
        | Expr::Clone(inner) => rename_expr(inner, renames),
        Expr::Binary(_, left, right) | Expr::Index(left, right) => {
            rename_expr(left, renames);
            rename_expr(right, renames);
        }
        Expr::Call(_, args)
        | Expr::Struct(_, args)
        | Expr::Variant(_, _, args)
        | Expr::Vector(args) => {
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
            _ => {}
        }
    }
}

/// Reads a Rails (or plain-form) surface program, elaborates it into the
/// core text tree ([`syntax::Program`]), and collects surface warnings
/// (`[U5]`, `[U17]`, `[U18]`).
pub fn elaborate_with_warnings(source: &str) -> Result<Elaborated, Error> {
    let top_decls = parse_surface_file(source)?;
    let mut structs = HashMap::new();
    let mut variants = HashMap::new();
    let mut resources = HashSet::new();
    let mut boundaries = HashMap::new();
    let mut outcomes = HashMap::new();
    let mut functions = HashMap::new();
    let mut controls = HashMap::new();

    for decl in &top_decls {
        match decl {
            TopDecl::Blank | TopDecl::Comment(_) => {}
            TopDecl::Struct(st) => {
                structs.insert(st.name.clone(), st.clone());
            }
            TopDecl::Variant(var) => {
                variants.insert(var.name.clone(), var.constructors.clone());
            }
            TopDecl::Resource(res) => {
                resources.insert(res.name.clone());
            }
            TopDecl::Boundary(bnd) => {
                boundaries.insert(bnd.name.clone(), bnd.clone());
            }
            TopDecl::Outcome(outcome) => {
                let mut seen = HashSet::new();
                for oa in &outcome.alternatives {
                    if !seen.insert(&oa.alt.name) {
                        return fail(
                            outcome.line,
                            "G2",
                            format!(
                                "the outcome `{}` lists `{}` twice",
                                outcome.name, oa.alt.name
                            ),
                        );
                    }
                }
                let alts: Vec<Alternative> = outcome
                    .alternatives
                    .iter()
                    .map(|oa| oa.alt.clone())
                    .collect();
                if outcomes.insert(outcome.name.clone(), alts).is_some() {
                    return fail(
                        outcome.line,
                        "G2",
                        format!("the outcome `{}` is declared twice", outcome.name),
                    );
                }
            }
            TopDecl::Function(function) => {
                functions.insert(function.name.clone(), function.result.clone());
            }
            TopDecl::Control(control) => {
                controls.insert(control.name.clone(), control.clone());
            }
        }
    }

    let mut elaborator = Elaborator {
        structs,
        variants,
        resources,
        boundaries,
        outcomes,
        functions,
        controls,
        contracts: HashMap::new(),
        resolving_auto: HashSet::new(),
        elaborated_controls: HashMap::new(),
        warnings: Vec::new(),
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
            TopDecl::Blank | TopDecl::Comment(_) => {}
            TopDecl::Struct(st) => {
                declarations.push(Declaration::Struct(st));
            }
            TopDecl::Variant(var) => {
                declarations.push(Declaration::Variant(VariantDecl {
                    name: var.name,
                    public: var.public,
                    is_copy: var.is_copy,
                    constructors: var.constructors,
                    line: var.line,
                }));
            }
            TopDecl::Resource(res) => {
                declarations.push(Declaration::Resource(res));
            }
            TopDecl::Outcome(outcome) => {
                declarations.push(Declaration::Outcome(CoreOutcomeDecl {
                    name: outcome.name,
                    public: outcome.public,
                    alternatives: outcome.alternatives.into_iter().map(|oa| oa.alt).collect(),
                    line: outcome.line,
                }));
            }
            TopDecl::Boundary(bnd) => {
                declarations.push(Declaration::Boundary(elaborator.elaborate_boundary(&bnd)?));
            }
            TopDecl::Function(function) => {
                declarations.push(Declaration::Function(function));
            }
            TopDecl::Control(control) => {
                let elaborated =
                    if let Some(cached) = elaborator.elaborated_controls.remove(&control.name) {
                        cached
                    } else {
                        elaborator.elaborate_control(&control)?
                    };
                declarations.push(Declaration::Control(elaborated));
            }
        }
    }

    // [F3]: The effect of a private defined control is inferred as the union
    // of the effects of the operations it calls.
    infer_private_effects(&mut declarations);

    Ok(Elaborated {
        program: Program { declarations },
        warnings: elaborator.warnings,
    })
}

/// Reads a Rails (or plain-form) surface program and elaborates it into the
/// core text tree ([`syntax::Program`]).
pub fn elaborate(source: &str) -> Result<Program, Error> {
    Ok(elaborate_with_warnings(source)?.program)
}

// ---------------------------------------------------------------------------
// Stage 4: Canonical Rails formatter ([G10])
// ---------------------------------------------------------------------------

/// Formats a Rails, word-form or plain-form surface program into canonical
/// Rails glyph text ([G10]).
pub fn format(source: &str) -> Result<String, Error> {
    // Reject invalid programs before formatting so `format` only formats valid
    // surface text.
    let _ = elaborate(source)?;
    let top_decls = parse_surface_file(source)?;
    let mut lines: Vec<String> = Vec::new();

    for decl in &top_decls {
        match decl {
            TopDecl::Blank => {
                if lines.last().is_some_and(|l| !l.is_empty()) {
                    lines.push(String::new());
                }
            }
            TopDecl::Comment(c) => {
                lines.push(c.clone());
            }
            TopDecl::Struct(st) => {
                format_struct(st, &mut lines);
            }
            TopDecl::Variant(var) => {
                format_variant(var, &mut lines);
            }
            TopDecl::Resource(res) => {
                format_resource(res, &mut lines);
            }
            TopDecl::Boundary(bnd) => {
                format_boundary(bnd, &mut lines);
            }
            TopDecl::Outcome(outcome) => {
                format_outcome(outcome, &mut lines);
            }
            TopDecl::Function(function) => {
                format_function(function, &mut lines);
            }
            TopDecl::Control(control) => {
                format_control(control, &mut lines);
            }
        }
    }

    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut out = lines.join("\n");
    out.push('\n');
    Ok(out)
}

fn format_params(parameters: &[Parameter]) -> String {
    let items: Vec<String> = parameters
        .iter()
        .map(|p| format!("{}: {}", p.name, p.ty))
        .collect();
    items.join(", ")
}

fn format_fields(fields: &[Parameter]) -> String {
    if fields.is_empty() {
        String::new()
    } else {
        format!(" {{{}}}", format_params(fields))
    }
}

fn format_outcome_alt(oa: &OutcomeAlt) -> String {
    let mut s = format!("{}{}", oa.alt.name, format_fields(&oa.alt.fields));
    if let Some(status) = &oa.status {
        s.push_str(&format!(" ↦ {status}"));
    }
    s
}

fn format_struct(st: &StructDecl, lines: &mut Vec<String>) {
    let mut head = String::new();
    if st.public {
        head.push_str("pub ");
    }
    if st.is_copy {
        head.push_str("copy ");
    }
    head.push_str(&format!(
        "struct {} {{{}}}",
        st.name,
        format_params(&st.fields)
    ));
    lines.push(head);
}

fn format_variant(var: &SurfaceVariantDecl, lines: &mut Vec<String>) {
    let mut prefix = String::new();
    if var.public {
        prefix.push_str("pub ");
    }
    if var.is_copy {
        prefix.push_str("copy ");
    }
    let ctors: Vec<String> = var
        .constructors
        .iter()
        .map(|c| format!("{}{}", c.name, format_fields(&c.fields)))
        .collect();
    let one_line = format!("{prefix}variant {}: {}", var.name, ctors.join(" │ "));
    if !var.vertical && one_line.chars().count() <= 100 {
        lines.push(one_line);
    } else {
        lines.push(format!("{prefix}variant {}", var.name));
        for (idx, c) in ctors.iter().enumerate() {
            let marker = if idx + 1 == ctors.len() { "└" } else { "├" };
            lines.push(format!("{marker} {c}"));
        }
    }
}

fn format_resource(res: &ResourceDecl, lines: &mut Vec<String>) {
    if res.on_fault_release {
        lines.push(format!("extern resource {} on fault release", res.name));
    } else {
        lines.push(format!("extern resource {}", res.name));
    }
}

fn format_boundary(bnd: &SurfaceBoundaryDecl, lines: &mut Vec<String>) {
    let mut head = format!("boundary {}({})", bnd.name, format_params(&bnd.parameters));
    if !bnd.retains.is_empty() {
        head.push_str(&format!(" retains {}", bnd.retains.join(", ")));
    }
    lines.push(head);
    format_items(&bnd.body, "│ ", false, lines);
    lines.push("╵".to_string());
    lines.push(format!(
        "contingency ({}: {})",
        bnd.message.name, bnd.message.ty
    ));
    format_items(&bnd.contingency, "│ ", false, lines);
    lines.push("╵".to_string());
}

fn format_outcome(outcome: &OutcomeDecl, lines: &mut Vec<String>) {
    let pub_prefix = if outcome.public { "pub " } else { "" };
    let alts: Vec<String> = outcome
        .alternatives
        .iter()
        .map(format_outcome_alt)
        .collect();
    let one_line = format!("{pub_prefix}outcome {}: {}", outcome.name, alts.join(" │ "));
    let has_status = outcome.alternatives.iter().any(|oa| oa.status.is_some());
    if !outcome.vertical && !has_status && one_line.chars().count() <= 100 {
        lines.push(one_line);
    } else {
        lines.push(format!("{pub_prefix}outcome {}", outcome.name));
        for (idx, alt) in alts.iter().enumerate() {
            let marker = if idx + 1 == alts.len() { "└" } else { "├" };
            lines.push(format!("{marker} {alt}"));
        }
    }
}

fn format_function(function: &Function, lines: &mut Vec<String>) {
    let mut s = format!(
        "function {}({})",
        function.name,
        format_params(&function.parameters)
    );
    if let Some(pre) = &function.precondition {
        s.push_str(&format!(" ⊢ {pre}"));
    }
    s.push_str(&format!(": {} := {}", function.result, function.body));
    lines.push(s);
}

fn format_contract_entry(entry: &SurfaceContractEntry) -> String {
    match entry {
        SurfaceContractEntry::Group(group, _) => format!("…{group}"),
        SurfaceContractEntry::Alt(alt) => {
            let mut s = format!("{}{}", alt.name, format_fields(&alt.fields));
            if let Some(cond) = &alt.condition {
                s.push_str(&format!(" ⇐ {cond}"));
            }
            s
        }
    }
}

fn format_control(control: &SurfaceControl, lines: &mut Vec<String>) {
    let mut head = String::new();
    if control.public {
        head.push_str("pub ");
    }
    if control.external {
        head.push_str("extern ");
    }
    if control.contingent {
        head.push_str("contingent ");
    }
    head.push_str(&format!(
        "control {}({})",
        control.name,
        format_params(&control.parameters)
    ));

    let pre_part = control.precondition.as_ref().map(|pre| format!("⊢ {pre}"));
    let mut tail_parts = Vec::new();
    if control.faults {
        tail_parts.push("faults".to_string());
    }
    if !control.uses.is_empty() {
        tail_parts.push(format!("uses {}", control.uses.join(", ")));
    }
    let uses_part = if tail_parts.is_empty() {
        None
    } else {
        Some(tail_parts.join(" "))
    };

    match &control.contract {
        SurfaceContract::Auto => {
            let mut single = head.clone();
            if let Some(pre) = &pre_part {
                single.push(' ');
                single.push_str(pre);
            }
            single.push_str(" ■ auto");
            if let Some(uses) = &uses_part {
                single.push(' ');
                single.push_str(uses);
            }
            if single.chars().count() <= 100 {
                lines.push(single);
            } else {
                lines.push(head);
                if let Some(pre) = &pre_part {
                    lines.push(format!("    {pre}"));
                }
                let mut auto_line = "    ■ auto".to_string();
                if let Some(uses) = &uses_part {
                    auto_line.push(' ');
                    auto_line.push_str(uses);
                }
                lines.push(auto_line);
            }
        }
        SurfaceContract::Entries(entries) => {
            let formatted_entries: Vec<String> =
                entries.iter().map(format_contract_entry).collect();
            let any_cond = entries.iter().any(|e| {
                matches!(
                    e,
                    SurfaceContractEntry::Alt(Alternative {
                        condition: Some(_),
                        ..
                    })
                )
            });
            let contract_one_line = format!("■ {}", formatted_entries.join(" │ "));
            let mut single = head.clone();
            if let Some(pre) = &pre_part {
                single.push(' ');
                single.push_str(pre);
            }
            single.push(' ');
            single.push_str(&contract_one_line);
            if let Some(uses) = &uses_part {
                single.push(' ');
                single.push_str(uses);
            }

            if !control.multiline_contract && !any_cond && single.chars().count() <= 100 {
                lines.push(single);
            } else {
                lines.push(head);
                if let Some(pre) = &pre_part {
                    lines.push(format!("    {pre}"));
                }
                for (idx, entry_str) in formatted_entries.iter().enumerate() {
                    let mut entry_line = format!("    ■ {entry_str}");
                    if idx + 1 == formatted_entries.len()
                        && let Some(uses) = &uses_part
                    {
                        if entry_line.chars().count() + 1 + uses.chars().count() <= 100 {
                            entry_line.push(' ');
                            entry_line.push_str(uses);
                        } else {
                            lines.push(entry_line);
                            lines.push(format!("    {uses}"));
                            continue;
                        }
                    }
                    lines.push(entry_line);
                }
            }
        }
    }

    if !control.external {
        format_items(&control.body, "│ ", false, lines);
        lines.push("╵".to_string());
    }
}

fn format_mask(mask: &Mask) -> String {
    format_mask_prec(mask, 0)
}

fn format_mask_prec(mask: &Mask, parent_prec: u8) -> String {
    match mask {
        Mask::Path { site, name, .. } => match site {
            Some(s) => format!("{s}.{name}"),
            None => name.clone(),
        },
        Mask::Not { site, name, .. } => match site {
            Some(s) => format!("¬{s}.{name}"),
            None => format!("¬{name}"),
        },
        Mask::And(l, r) => {
            let s = format!("{} ∧ {}", format_mask_prec(l, 2), format_mask_prec(r, 2));
            if parent_prec > 2 { format!("({s})") } else { s }
        }
        Mask::Or(l, r) => {
            let s = format!("{} ∨ {}", format_mask_prec(l, 1), format_mask_prec(r, 1));
            if parent_prec > 1 { format!("({s})") } else { s }
        }
    }
}

fn format_pattern(p: &Pattern) -> String {
    let mut s = String::new();
    if p.mutable {
        s.push('~');
    }
    s.push_str(&p.name);
    if let Some(ty) = &p.ty {
        s.push_str(&format!(": {ty}"));
    }
    s
}

fn format_arm_head(arm: &SurfaceArm) -> String {
    if arm.is_group {
        return format!("…{}", arm.name);
    }
    let mut s = arm.name.clone();
    if let Some(patterns) = &arm.patterns
        && !patterns.is_empty()
    {
        let pats: Vec<String> = patterns.iter().map(format_pattern).collect();
        s.push_str(&format!(" {{{}}}", pats.join(", ")));
    }
    s
}

fn format_expr_list(exprs: &[Expr]) -> String {
    let items: Vec<String> = exprs.iter().map(ToString::to_string).collect();
    items.join(", ")
}

fn format_payload(payload: &[Expr]) -> String {
    if payload.is_empty() {
        String::new()
    } else {
        format!(" {{{}}}", format_expr_list(payload))
    }
}

fn format_leave_outputs(outputs: &[LeaveOut]) -> String {
    if outputs.is_empty() {
        return String::new();
    }
    let items: Vec<String> = outputs
        .iter()
        .map(|o| match &o.value {
            Expr::Name(n) if n == &o.name => o.name.clone(),
            other => format!("{}: {other}", o.name),
        })
        .collect();
    format!(" {{{}}}", items.join(", "))
}

fn format_arm_target_inline(target: &ArmTarget) -> Option<String> {
    match target {
        ArmTarget::None => None,
        ArmTarget::Name(name) => Some(name.clone()),
        ArmTarget::AdaptExit(alt, payload) => Some(format!("{alt}{}", format_payload(payload))),
        ArmTarget::SameExit => Some("■".to_string()),
        ArmTarget::Stmt(stmt) => Some(format_simple_stmt_inline(stmt)),
        ArmTarget::Block(_) => None,
    }
}

fn format_surface_call(call: &SurfaceCall) -> String {
    match call {
        SurfaceCall::Direct {
            operation,
            arguments,
        } => format!("{operation}({})", format_expr_list(arguments)),
        SurfaceCall::Method {
            receiver,
            method,
            arguments,
        } => format!("{receiver}.{method}({})", format_expr_list(arguments)),
    }
}

fn format_surface_operand(operand: &SurfaceOperand) -> String {
    match operand {
        SurfaceOperand::Call(call) => format_surface_call(call),
        SurfaceOperand::Expr(expr) => expr.to_string(),
    }
}

fn format_place_inline(place: &Place) -> String {
    let mut s = place.root.clone();
    for step in &place.steps {
        match step {
            PlaceStep::Field(f) => {
                s.push('.');
                s.push_str(f);
            }
            PlaceStep::Index(idx) => {
                s.push_str(&format!("[{idx}]"));
            }
        }
    }
    s
}

fn format_simple_stmt_inline(stmt: &SurfaceStmt) -> String {
    match stmt {
        SurfaceStmt::Bind {
            mutable,
            name,
            ty,
            value,
            ..
        } => {
            let mut s = String::new();
            if *mutable {
                s.push('~');
            }
            s.push_str(name);
            if let Some(ty) = ty {
                s.push_str(&format!(": {ty}"));
            }
            s.push_str(&format!(" := {value}"));
            s
        }
        SurfaceStmt::Unpack {
            struct_name,
            names,
            value,
            ..
        } => format!("{struct_name} {{{}}} := {value}", names.join(", ")),
        SurfaceStmt::Capture {
            mutable,
            name,
            ty,
            call,
            ..
        } => {
            let mut s = String::new();
            if *mutable {
                s.push('~');
            }
            s.push_str(name);
            if let Some(ty) = ty {
                s.push_str(&format!(": {ty}"));
            }
            s.push_str(&format!(" := capture {}", format_surface_call(call)));
            s
        }
        SurfaceStmt::Assign { name, value, .. } => format!("{name} ← {value}"),
        SurfaceStmt::AssignPlace { target, value, .. } => {
            format!("{} ← {value}", format_place_inline(target))
        }
        SurfaceStmt::Oblige { formula, .. } => format!("⊢ {formula}"),
        SurfaceStmt::Forward {
            operation,
            arguments,
            ..
        } => format!("forward {operation}({})", format_expr_list(arguments)),
        SurfaceStmt::Call {
            operation,
            arguments,
            ..
        } => format!("{operation}({})", format_expr_list(arguments)),
        SurfaceStmt::MethodCall {
            receiver,
            method,
            arguments,
            ..
        } => format!("{receiver}.{method}({})", format_expr_list(arguments)),
        SurfaceStmt::Supervise {
            boundary,
            arguments,
            ..
        } => format!("supervise {boundary}({})", format_expr_list(arguments)),
        SurfaceStmt::Again {
            name, arguments, ..
        } => {
            if arguments.is_empty() {
                format!("↑ {name}")
            } else {
                format!("↑ {name}({})", format_expr_list(arguments))
            }
        }
        SurfaceStmt::Leave {
            alternative,
            outputs,
            ..
        } => match alternative {
            Some(alt) => format!("□ {alt}{}", format_leave_outputs(outputs)),
            None => "□".to_string(),
        },
        SurfaceStmt::Exit {
            alternative,
            payload,
            ..
        } => format!("■ {alternative}{}", format_payload(payload)),
        SurfaceStmt::ExitOutcome { name, .. } => format!("■ ▾{name}"),
        SurfaceStmt::Fault {
            message, assumed, ..
        } => {
            if *assumed {
                format!("assume ⊥ {message}")
            } else {
                format!("fault {message}")
            }
        }
        SurfaceStmt::Terminate { value, .. } => match value {
            Some(v) => format!("terminate {v}"),
            None => "terminate".to_string(),
        },
        SurfaceStmt::Escalate { .. } => "escalate".to_string(),
        SurfaceStmt::Recover { loop_name, .. } => format!("recover ↑ {loop_name}"),
        SurfaceStmt::Impossible { .. } => "⊥".to_string(),
        SurfaceStmt::Test {
            condition, body, ..
        } => {
            let inner = format_simple_stmt_inline(&body[0]);
            format!("? {condition} → {inner}")
        }
        SurfaceStmt::Decide {
            site,
            adapter,
            operand,
            arms,
            ..
        } => {
            let mut head = String::new();
            if *adapter {
                head.push_str("■ ");
            } else if let Some(s) = site {
                head.push_str(&format!("{s}: "));
            }
            head.push_str(&format!("Δ {}", format_surface_operand(operand)));
            let arm_strs: Vec<String> = arms
                .iter()
                .map(|arm| {
                    let h = format_arm_head(arm);
                    if let Some(t) = format_arm_target_inline(&arm.target) {
                        format!("{h} → {t}")
                    } else {
                        h
                    }
                })
                .collect();
            format!("{head}: {}", arm_strs.join(" │ "))
        }
        SurfaceStmt::Loop { .. }
        | SurfaceStmt::ForAll { .. }
        | SurfaceStmt::Blank
        | SurfaceStmt::Comment(_) => String::new(),
    }
}

fn is_short_inline_stmt(stmt: &SurfaceStmt) -> bool {
    matches!(
        stmt,
        SurfaceStmt::Bind { .. }
            | SurfaceStmt::Unpack { .. }
            | SurfaceStmt::Capture { .. }
            | SurfaceStmt::Assign { .. }
            | SurfaceStmt::AssignPlace { .. }
            | SurfaceStmt::Oblige { .. }
            | SurfaceStmt::Forward { .. }
            | SurfaceStmt::Call { .. }
            | SurfaceStmt::MethodCall { .. }
            | SurfaceStmt::Supervise { .. }
            | SurfaceStmt::Again { .. }
            | SurfaceStmt::Leave { .. }
            | SurfaceStmt::Exit { .. }
            | SurfaceStmt::ExitOutcome { .. }
            | SurfaceStmt::Fault { .. }
            | SurfaceStmt::Terminate { .. }
            | SurfaceStmt::Escalate { .. }
            | SurfaceStmt::Recover { .. }
            | SurfaceStmt::Impossible { .. }
    )
}

fn is_inline_test_stmt(stmt: &SurfaceStmt) -> bool {
    match stmt {
        SurfaceStmt::Test { inline, body, .. } => {
            *inline && body.len() == 1 && is_inline_test_stmt(&body[0])
        }
        SurfaceStmt::Decide { vertical, arms, .. } => {
            !*vertical
                && !arms
                    .iter()
                    .any(|arm| matches!(arm.target, ArmTarget::Block(_)))
        }
        other => is_short_inline_stmt(other),
    }
}

fn format_items(items: &[SurfaceItem], prefix: &str, in_loop: bool, lines: &mut Vec<String>) {
    let mut idx = 0usize;
    while idx < items.len() {
        match &items[idx] {
            SurfaceItem::Blank => {
                lines.push(prefix.trim_end().to_string());
                idx += 1;
            }
            SurfaceItem::Comment(c) => {
                lines.push(format!("{prefix}┄{c}"));
                idx += 1;
            }
            SurfaceItem::Stmt(stmt) => {
                format_stmt(stmt, prefix, lines);
                idx += 1;
            }
            SurfaceItem::Label {
                inline, mask, body, ..
            } => {
                let can_inline = *inline
                    && !body.is_empty()
                    && body.len() <= 2
                    && body.iter().all(is_short_inline_stmt);
                if can_inline {
                    // Align consecutive short inline labels (`[G10]`).
                    let start = idx;
                    let mut end = idx + 1;
                    while let Some(SurfaceItem::Label {
                        inline: next_inline,
                        body: next_body,
                        ..
                    }) = items.get(end)
                    {
                        if *next_inline
                            && !next_body.is_empty()
                            && next_body.len() <= 2
                            && next_body.iter().all(is_short_inline_stmt)
                        {
                            end += 1;
                        } else {
                            break;
                        }
                    }
                    let label_prefix = replace_last_segment_with_label(prefix, in_loop);
                    let masks: Vec<String> = items[start..end]
                        .iter()
                        .map(|item| {
                            let SurfaceItem::Label { mask: m, .. } = item else {
                                unreachable!();
                            };
                            format_mask(m)
                        })
                        .collect();
                    let max_mask = masks.iter().map(|m| m.chars().count()).max().unwrap_or(0);
                    for (offset, mask_str) in masks.iter().enumerate() {
                        let SurfaceItem::Label { body: b, .. } = &items[start + offset] else {
                            unreachable!();
                        };
                        let stmts: Vec<String> = b.iter().map(format_simple_stmt_inline).collect();
                        let pad = " ".repeat(max_mask.saturating_sub(mask_str.chars().count()));
                        lines.push(format!(
                            "{label_prefix}{mask_str}:{pad} {}",
                            stmts.join("; ")
                        ));
                    }
                    idx = end;
                } else {
                    let label_prefix = replace_last_segment_with_label(prefix, in_loop);
                    let mask_str = format_mask(mask);
                    lines.push(format!("{label_prefix}{mask_str}"));
                    let dash_prefix = format!("{prefix}┆ ");
                    for stmt in body {
                        format_stmt(stmt, &dash_prefix, lines);
                    }
                    idx += 1;
                }
            }
        }
    }
}

fn replace_last_segment_with_label(prefix: &str, in_loop: bool) -> String {
    if in_loop && let Some(before) = prefix.strip_suffix("║ ") {
        return format!("{before}╠═ ");
    }
    if let Some(before) = prefix.strip_suffix("│ ") {
        return format!("{before}╞═ ");
    }
    format!("{prefix}╞═ ")
}

fn format_stmt(stmt: &SurfaceStmt, prefix: &str, lines: &mut Vec<String>) {
    match stmt {
        SurfaceStmt::Blank => {
            lines.push(prefix.trim_end().to_string());
        }
        SurfaceStmt::Comment(c) => {
            lines.push(format!("{prefix}┄{c}"));
        }
        SurfaceStmt::Test {
            inline,
            condition,
            body,
            ..
        } => {
            if *inline && body.len() == 1 && is_inline_test_stmt(&body[0]) {
                lines.push(format!(
                    "{prefix}? {condition} → {}",
                    format_simple_stmt_inline(&body[0])
                ));
            } else {
                lines.push(format!("{prefix}? {condition} →"));
                let dash_prefix = format!("{prefix}┆ ");
                for child in body {
                    format_stmt(child, &dash_prefix, lines);
                }
            }
        }
        SurfaceStmt::Decide {
            vertical,
            site,
            adapter,
            operand,
            arms,
            ..
        } => {
            let mut head = String::new();
            if *adapter {
                head.push_str("■ ");
            } else if let Some(s) = site {
                head.push_str(&format!("{s}: "));
            }
            head.push_str(&format!("Δ {}", format_surface_operand(operand)));

            let has_block = arms
                .iter()
                .any(|arm| matches!(arm.target, ArmTarget::Block(_)));
            if !*vertical && !has_block {
                let arm_strs: Vec<String> = arms
                    .iter()
                    .map(|arm| {
                        let h = format_arm_head(arm);
                        if let Some(t) = format_arm_target_inline(&arm.target) {
                            format!("{h} → {t}")
                        } else {
                            h
                        }
                    })
                    .collect();
                let one_line = format!("{prefix}{head}: {}", arm_strs.join(" │ "));
                if one_line.chars().count() <= 100 {
                    lines.push(one_line);
                    return;
                }
            }

            // Vertical decision (`├ ` / `└ `) with aligned `→` arrows.
            lines.push(format!("{prefix}{head}"));
            let arm_heads: Vec<String> = arms.iter().map(format_arm_head).collect();
            let max_head = arms
                .iter()
                .zip(&arm_heads)
                .filter(|(arm, _)| !matches!(arm.target, ArmTarget::None | ArmTarget::Block(_)))
                .map(|(_, h)| h.chars().count())
                .max()
                .unwrap_or(0);

            for (idx, (arm, arm_head)) in arms.iter().zip(&arm_heads).enumerate() {
                let is_last = idx + 1 == arms.len();
                let marker = if is_last { "└" } else { "├" };
                match &arm.target {
                    ArmTarget::None => {
                        lines.push(format!("{prefix}{marker} {arm_head}"));
                    }
                    ArmTarget::Block(block_stmts) => {
                        lines.push(format!("{prefix}{marker} {arm_head} →"));
                        let block_prefix = if is_last {
                            format!("{prefix}  ┆ ")
                        } else {
                            format!("{prefix}│ ┆ ")
                        };
                        for child in block_stmts {
                            format_stmt(child, &block_prefix, lines);
                        }
                    }
                    other => {
                        let target_str = format_arm_target_inline(other).unwrap();
                        let pad = " ".repeat(max_head.saturating_sub(arm_head.chars().count()));
                        lines.push(format!("{prefix}{marker} {arm_head}{pad} → {target_str}"));
                    }
                }
            }
        }
        SurfaceStmt::Loop {
            name,
            parameters,
            invariant,
            body,
            ..
        } => {
            if parameters.is_empty() {
                lines.push(format!("{prefix}↻ {name}"));
            } else {
                let params: Vec<String> = parameters
                    .iter()
                    .map(|p| {
                        let tilde = if p.mutable { "~" } else { "" };
                        format!("{tilde}{}: {} := {}", p.name, p.ty, p.initial)
                    })
                    .collect();
                lines.push(format!("{prefix}↻ {name}({})", params.join(", ")));
            }
            let loop_prefix = format!("{prefix}║ ");
            if let Some(inv) = invariant {
                lines.push(format!("{loop_prefix}⊢ {inv}"));
            }
            format_items(body, &loop_prefix, true, lines);
        }
        SurfaceStmt::ForAll {
            var,
            collection,
            body,
            ..
        } => {
            lines.push(format!("{prefix}∀ {var} ∈ {collection}"));
            let loop_prefix = format!("{prefix}║ ");
            format_items(body, &loop_prefix, true, lines);
        }
        other => {
            lines.push(format!("{prefix}{}", format_simple_stmt_inline(other)));
        }
    }
}

fn collect_called_operations(statements: &[Statement], callees: &mut Vec<String>) {
    for statement in statements {
        match &statement.command {
            Command::Decide { operation, .. } | Command::Capture { operation, .. } => {
                if !callees.contains(operation) {
                    callees.push(operation.clone());
                }
            }
            Command::Loop { body, .. } => collect_called_operations(body, callees),
            Command::Boundary {
                body, contingency, ..
            } => {
                collect_called_operations(body, callees);
                collect_called_operations(contingency, callees);
            }
            Command::Supervise { boundary, .. } => {
                let tag = format!("@boundary:{boundary}");
                if !callees.contains(&tag) {
                    callees.push(tag);
                }
            }
            _ => {}
        }
    }
}

fn infer_private_effects(declarations: &mut [Declaration]) {
    let mut boundary_callees: HashMap<String, Vec<String>> = HashMap::new();
    for declaration in declarations.iter() {
        if let Declaration::Boundary(bnd) = declaration {
            let mut cs = Vec::new();
            collect_called_operations(&bnd.body, &mut cs);
            collect_called_operations(&bnd.contingency, &mut cs);
            boundary_callees.insert(bnd.name.clone(), cs);
        }
    }
    let declared_controls: HashSet<String> = declarations
        .iter()
        .filter_map(|d| match d {
            Declaration::Control(c) => Some(c.name.clone()),
            _ => None,
        })
        .collect();
    loop {
        let mut known_uses: HashMap<String, Vec<String>> = HashMap::new();
        for declaration in declarations.iter() {
            if let Declaration::Control(control) = declaration {
                known_uses.insert(control.name.clone(), control.uses.clone());
            }
        }
        if !declared_controls.contains("emit") {
            known_uses.insert("emit".to_string(), vec!["console.write".to_string()]);
        }
        for (bname, cs) in &boundary_callees {
            let mut b_effects = Vec::new();
            for c in cs {
                if let Some(effs) = known_uses.get(c) {
                    for e in effs {
                        if !b_effects.contains(e) {
                            b_effects.push(e.clone());
                        }
                    }
                }
            }
            known_uses.insert(format!("@boundary:{bname}"), b_effects);
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
