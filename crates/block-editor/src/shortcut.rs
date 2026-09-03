//! Markdown prefix and inline-rule emitters as [`BlockOp`] lists.
//!
//! Ported from Bezel `edit::shortcut` / `inline_rule`. Catena's rule: trigger
//! on the completing character and do **not** insert that character when the
//! shortcut fires — use [`try_prefix`] / [`try_inline`].

use std::ops::Range;

use block_markdown::{BlockId, BlockType};

use crate::types::{BlockOp, LwwValue};

/// A markdown prefix typed at the start of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixKind {
    Heading(u8),
    Bullet,
    Ordered,
    Task { checked: bool },
    Quote,
    Code,
    Rule,
}

impl PrefixKind {
    #[must_use]
    pub fn block_type(self) -> BlockType {
        match self {
            Self::Heading(level) => BlockType::Heading { level },
            Self::Bullet => BlockType::Bullet,
            Self::Ordered => BlockType::Ordered,
            Self::Task { .. } => BlockType::Task,
            Self::Quote => BlockType::Quote,
            Self::Code => BlockType::Code,
            Self::Rule => BlockType::Rule,
        }
    }
}

/// Match a markdown prefix at the start of `text`, returning it and how many
/// bytes it occupied. Order matters — a task marker is a bullet with more on
/// the end.
#[must_use]
pub fn match_prefix(text: &str) -> Option<(PrefixKind, usize)> {
    const PREFIXES: &[(&str, PrefixKind)] = &[
        ("- [ ] ", PrefixKind::Task { checked: false }),
        ("- [x] ", PrefixKind::Task { checked: true }),
        ("[] ", PrefixKind::Task { checked: false }),
        ("[x] ", PrefixKind::Task { checked: true }),
        ("###### ", PrefixKind::Heading(6)),
        ("##### ", PrefixKind::Heading(5)),
        ("#### ", PrefixKind::Heading(4)),
        ("### ", PrefixKind::Heading(3)),
        ("## ", PrefixKind::Heading(2)),
        ("# ", PrefixKind::Heading(1)),
        ("- ", PrefixKind::Bullet),
        ("* ", PrefixKind::Bullet),
        ("+ ", PrefixKind::Bullet),
        ("1. ", PrefixKind::Ordered),
        ("> ", PrefixKind::Quote),
        ("```", PrefixKind::Code),
        ("---", PrefixKind::Rule),
    ];
    PREFIXES
        .iter()
        .find(|(prefix, _)| text.starts_with(prefix))
        .map(|(prefix, kind)| (*kind, prefix.len()))
}

fn prefix_ops_for(id: BlockId, kind: PrefixKind, prefix_len: usize) -> Vec<BlockOp> {
    let mut ops = vec![
        BlockOp::DeleteRange {
            id: id.clone(),
            start: 0,
            end: prefix_len,
        },
        BlockOp::SetType {
            id: id.clone(),
            kind: kind.block_type(),
        },
    ];
    match kind {
        PrefixKind::Task { checked: true } => {
            ops.push(BlockOp::SetProp {
                id,
                key: "checked",
                value: LwwValue::Bool(true),
            });
        }
        PrefixKind::Ordered => {
            ops.push(BlockOp::SetProp {
                id,
                key: "number",
                value: LwwValue::I64(1),
            });
        }
        _ => {}
    }
    ops
}

/// Emit `DeleteRange` of the prefix plus `SetType` (and props) for a match at
/// the start of `plain`. Caller should only invoke when the caret is at or past
/// the prefix (Bezel post-insert path).
#[must_use]
pub fn prefix_ops(id: BlockId, plain: &str) -> Option<Vec<BlockOp>> {
    let (kind, len) = match_prefix(plain)?;
    Some(prefix_ops_for(id, kind, len))
}

/// Catena completing-character path: if `plain + typed` is exactly a prefix,
/// return ops that delete the already-present portion and set the type. The
/// caller must **not** insert `typed`.
#[must_use]
pub fn try_prefix(id: BlockId, plain: &str, typed: &str) -> Option<Vec<BlockOp>> {
    if typed.is_empty() {
        return None;
    }
    let combined = format!("{plain}{typed}");
    let (kind, len) = match_prefix(&combined)?;
    // Only fire when the completing character finishes the prefix exactly —
    // do not convert mid-sentence hyphens or pastes that already include body.
    if combined.len() != len {
        return None;
    }
    // Delete what was already in the block (everything before `typed`).
    let mut ops = Vec::new();
    if !plain.is_empty() {
        ops.push(BlockOp::DeleteRange {
            id: id.clone(),
            start: 0,
            end: plain.len(),
        });
    }
    ops.push(BlockOp::SetType {
        id: id.clone(),
        kind: kind.block_type(),
    });
    match kind {
        PrefixKind::Task { checked: true } => {
            ops.push(BlockOp::SetProp {
                id,
                key: "checked",
                value: LwwValue::Bool(true),
            });
        }
        PrefixKind::Ordered => {
            ops.push(BlockOp::SetProp {
                id,
                key: "number",
                value: LwwValue::I64(1),
            });
        }
        _ => {}
    }
    Some(ops)
}

/// Closing inline delimiter just typed, and the run it closes.
///
/// Returns the opening delimiter's range and the text between it and the caret;
/// the closing delimiter is `inner.end..caret`.
#[must_use]
pub fn match_inline(
    text: &str,
    caret: usize,
) -> Option<(Range<usize>, Range<usize>, &'static str)> {
    let head = text.get(..caret)?;
    for (delimiter, mark) in [
        ("**", "bold"),
        ("~~", "strike"),
        ("`", "code"),
        ("_", "italic"),
        ("*", "italic"),
    ] {
        let Some(closes) = head.strip_suffix(delimiter) else {
            continue;
        };
        let Some(open) = closes.rfind(delimiter) else {
            continue;
        };
        let inner = open + delimiter.len()..closes.len();
        let Some(body) = text.get(inner.clone()).filter(|body| !body.is_empty()) else {
            continue;
        };
        if body.starts_with(char::is_whitespace) || body.ends_with(char::is_whitespace) {
            continue;
        }
        if delimiter == "_"
            && text[..open]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
        {
            continue;
        }
        return Some((open..open + delimiter.len(), inner, mark));
    }
    None
}

/// Emit deletes of both delimiters plus `ToggleMark` on the inner run.
/// Assumes `plain` already includes the closing delimiter (Bezel post-insert).
#[must_use]
pub fn inline_ops(id: BlockId, plain: &str, caret: usize) -> Option<Vec<BlockOp>> {
    let (open, inner, mark) = match_inline(plain, caret)?;
    let width = open.len();
    // Closing first — taking the opening one would move every offset after it.
    Some(vec![
        BlockOp::DeleteRange {
            id: id.clone(),
            start: inner.end,
            end: caret,
        },
        BlockOp::DeleteRange {
            id: id.clone(),
            start: open.start,
            end: open.end,
        },
        BlockOp::ToggleMark {
            id,
            start: inner.start - width,
            end: inner.end - width,
            mark,
        },
    ])
}

/// Catena completing-character path for inline rules: if appending `typed`
/// (the **full** closing delimiter) would close a mark, emit ops against
/// `plain` that strip the opening delimiter and toggle the mark. Caller skips
/// insert. Char-by-char closes use insert-then-[`inline_ops`] instead.
#[must_use]
pub fn try_inline(id: BlockId, plain: &str, typed: &str) -> Option<Vec<BlockOp>> {
    if typed.is_empty() {
        return None;
    }
    if match_inline(plain, plain.len()).is_some() {
        return None;
    }
    let combined = format!("{plain}{typed}");
    let caret = combined.len();
    let (open, inner, mark) = match_inline(&combined, caret)?;
    let close = &combined[inner.end..caret];
    if close != typed {
        return None;
    }
    let width = open.len();
    Some(vec![
        BlockOp::DeleteRange {
            id: id.clone(),
            start: open.start,
            end: open.end,
        },
        BlockOp::ToggleMark {
            id,
            start: inner.start - width,
            end: inner.end - width,
            mark,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> BlockId {
        BlockId::from("block-1")
    }

    #[test]
    fn prefix_heading_bullet_task_code() {
        assert_eq!(match_prefix("## hello"), Some((PrefixKind::Heading(2), 3)));
        assert_eq!(match_prefix("- x"), Some((PrefixKind::Bullet, 2)));
        assert_eq!(match_prefix("1. x"), Some((PrefixKind::Ordered, 3)));
        assert_eq!(
            match_prefix("- [ ] x"),
            Some((PrefixKind::Task { checked: false }, 6))
        );
        assert_eq!(
            match_prefix("- [x] x"),
            Some((PrefixKind::Task { checked: true }, 6))
        );
        assert_eq!(match_prefix("> q"), Some((PrefixKind::Quote, 2)));
        assert_eq!(match_prefix("```"), Some((PrefixKind::Code, 3)));
        assert_eq!(match_prefix("---"), Some((PrefixKind::Rule, 3)));
        assert_eq!(match_prefix("nope"), None);
    }

    #[test]
    fn prefix_ops_delete_and_set_type() {
        let ops = prefix_ops(id(), "## ").expect("match");
        assert!(matches!(
            &ops[0],
            BlockOp::DeleteRange {
                start: 0,
                end: 3,
                ..
            }
        ));
        assert!(matches!(
            &ops[1],
            BlockOp::SetType {
                kind: BlockType::Heading { level: 2 },
                ..
            }
        ));
    }

    #[test]
    fn try_prefix_skips_insert_of_space() {
        let ops = try_prefix(id(), "##", " ").expect("complete");
        assert!(matches!(
            &ops[0],
            BlockOp::DeleteRange {
                start: 0,
                end: 2,
                ..
            }
        ));
        assert!(matches!(
            &ops[1],
            BlockOp::SetType {
                kind: BlockType::Heading { level: 2 },
                ..
            }
        ));
        // Incomplete — do not fire.
        assert!(try_prefix(id(), "#", "#").is_none());
        // Body already present — do not fire on trailing space.
        assert!(try_prefix(id(), "## hello", " ").is_none());
    }

    #[test]
    fn try_prefix_task_and_fence() {
        let ops = try_prefix(id(), "- [ ]", " ").expect("task");
        assert!(ops.iter().any(|o| matches!(
            o,
            BlockOp::SetType {
                kind: BlockType::Task,
                ..
            }
        )));
        let ops = try_prefix(id(), "``", "`").expect("code");
        assert!(matches!(
            ops.last(),
            Some(BlockOp::SetType {
                kind: BlockType::Code,
                ..
            })
        ));
    }

    #[test]
    fn inline_bold_and_try() {
        let plain = "**bold**";
        let ops = inline_ops(id(), plain, plain.len()).expect("bold");
        assert_eq!(ops.len(), 3);
        assert!(matches!(
            &ops[2],
            BlockOp::ToggleMark {
                mark: "bold",
                start: 0,
                end: 4,
                ..
            }
        ));

        let ops = try_inline(id(), "**bold", "**").expect("complete");
        assert!(matches!(
            &ops[0],
            BlockOp::DeleteRange {
                start: 0,
                end: 2,
                ..
            }
        ));
        assert!(matches!(
            &ops[1],
            BlockOp::ToggleMark {
                mark: "bold",
                start: 0,
                end: 4,
                ..
            }
        ));
        // Partial close is not enough for try_inline — use inline_ops after insert.
        assert!(try_inline(id(), "**bold*", "*").is_none());
    }

    #[test]
    fn inline_rejects_snake_case_underscore() {
        assert!(match_inline("snake_case_", "snake_case_".len()).is_none());
        assert!(match_inline("* spaced *", "* spaced *".len()).is_none());
    }

    #[test]
    fn bracket_task_prefix() {
        assert_eq!(
            match_prefix("[] "),
            Some((PrefixKind::Task { checked: false }, 3))
        );
        let ops = try_prefix(id(), "[]", " ").expect("task");
        assert!(ops.iter().any(|o| matches!(
            o,
            BlockOp::SetType {
                kind: BlockType::Task,
                ..
            }
        )));
    }
}
