//! Bezel Backspace-at-start chain as pure ops.

use block_markdown::BlockType;

use crate::types::{BlockOp, Cursor, Part};
use gpui_component_block_view::BlockSnapshot;

/// Emit ops for Backspace at offset 0 (Bezel `merge_back` order).
///
/// Pure function of projected snapshots + cursor — no Loro mutation here.
#[must_use]
pub fn backspace_at_start(snapshots: &[BlockSnapshot], cursor: &Cursor) -> Vec<BlockOp> {
    if matches!(cursor.part, Part::Cell { .. }) {
        return Vec::new();
    }
    let Some(ix) = snapshots.iter().position(|b| b.id == cursor.id) else {
        return Vec::new();
    };
    let block = &snapshots[ix];

    if block.indent > 0 {
        return vec![BlockOp::Outdent {
            id: cursor.id.clone(),
        }];
    }

    if cursor.part == Part::Caption
        && matches!(block.block_type, BlockType::Image)
        && block.plain.is_empty()
    {
        return vec![BlockOp::DeleteBlock {
            id: cursor.id.clone(),
        }];
    }

    let unwrap = matches!(
        block.block_type,
        BlockType::Heading { .. }
            | BlockType::Quote
            | BlockType::Bullet
            | BlockType::Ordered
            | BlockType::Task
            | BlockType::Code
    );
    if unwrap {
        return vec![BlockOp::UnwrapToParagraph {
            id: cursor.id.clone(),
        }];
    }

    if ix == 0 {
        return Vec::new();
    }

    let previous = &snapshots[ix - 1];
    if previous.block_type.has_body() {
        return vec![BlockOp::MergeWithPrevious {
            id: cursor.id.clone(),
        }];
    }

    // Previous has a last part (fence/caption) but not a body merge — if this
    // body is empty, delete it; otherwise no structural op (caret move is view).
    if matches!(
        previous.block_type,
        BlockType::Code | BlockType::Image | BlockType::Table
    ) {
        if block.plain.is_empty() && matches!(block.block_type, BlockType::Paragraph) {
            return vec![BlockOp::DeleteBlock {
                id: cursor.id.clone(),
            }];
        }
        return Vec::new();
    }

    // Previous is a rule (no parts) → delete the rule.
    if previous.block_type == BlockType::Rule {
        return vec![BlockOp::DeleteBlock {
            id: previous.id.clone(),
        }];
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use block_markdown::BlockId;
    use gpui_component_block_view::BlockSnapshot;
    use loro::TextDelta;

    fn snap(id: &str, kind: BlockType, indent: i64, plain: &str) -> BlockSnapshot {
        BlockSnapshot {
            id: BlockId::from(id),
            block_type: kind,
            indent,
            plain: plain.into(),
            runs: if plain.is_empty() {
                vec![]
            } else {
                vec![TextDelta::Insert {
                    insert: plain.into(),
                    attributes: None,
                }]
            },
            props: Default::default(),
            checked: None,
            language: None,
            url: None,
            form: None,
            width: None,
            number: None,
            table: None,
        }
    }

    #[test]
    fn cell_at_start_is_noop() {
        let snaps = [snap("t", BlockType::Table, 0, "")];
        let ops = backspace_at_start(
            &snaps,
            &Cursor::new(BlockId::from("t"), Part::Cell { row: 0, column: 0 }, 0),
        );
        assert!(ops.is_empty());
    }

    #[test]
    fn indent_outdents() {
        let snaps = [snap("b", BlockType::Bullet, 1, "x")];
        let ops = backspace_at_start(&snaps, &Cursor::new(BlockId::from("b"), Part::Body, 0));
        assert!(matches!(ops.as_slice(), [BlockOp::Outdent { .. }]));
    }

    #[test]
    fn heading_unwraps() {
        let snaps = [snap("h", BlockType::Heading { level: 1 }, 0, "Hi")];
        let ops = backspace_at_start(&snaps, &Cursor::new(BlockId::from("h"), Part::Body, 0));
        assert!(matches!(
            ops.as_slice(),
            [BlockOp::UnwrapToParagraph { .. }]
        ));
    }

    #[test]
    fn empty_caption_deletes_image() {
        let snaps = [snap("i", BlockType::Image, 0, "")];
        let ops = backspace_at_start(&snaps, &Cursor::new(BlockId::from("i"), Part::Caption, 0));
        assert!(matches!(ops.as_slice(), [BlockOp::DeleteBlock { .. }]));
    }

    #[test]
    fn empty_para_after_rule_deletes_rule() {
        let snaps = [
            snap("r", BlockType::Rule, 0, ""),
            snap("p", BlockType::Paragraph, 0, ""),
        ];
        let ops = backspace_at_start(&snaps, &Cursor::new(BlockId::from("p"), Part::Body, 0));
        assert!(matches!(
            ops.as_slice(),
            [BlockOp::DeleteBlock { id }] if id.as_str() == "r"
        ));
    }

    #[test]
    fn merge_with_previous_body() {
        let snaps = [
            snap("a", BlockType::Paragraph, 0, "hi"),
            snap("b", BlockType::Paragraph, 0, "there"),
        ];
        let ops = backspace_at_start(&snaps, &Cursor::new(BlockId::from("b"), Part::Body, 0));
        assert!(matches!(
            ops.as_slice(),
            [BlockOp::MergeWithPrevious { .. }]
        ));
    }
}
