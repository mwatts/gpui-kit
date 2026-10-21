//! The block model's types: mutations, carets, and the paint-free projection
//! of one block. Ported from Bezel `select.rs` / `render.rs`, keyed by stable
//! [`BlockId`] rather than list index.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use std::collections::HashMap;

use block_markdown::{BlockId, BlockType, CommentState, Form};
use loro::TextDelta;

/// Which editable part of a block the caret sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub enum Part {
    #[default]
    Body,
    Code,
    Caption,
    Cell {
        row: usize,
        column: usize,
    },
}

/// Caret position keyed by stable block id.
///
/// `id` is the content identity ([`BlockId`] as an ObjectId). When the caret
/// sits on a composed Child occurrence, `occurrence` is that relation id so
/// two placements of the same content remain distinct for paint and structure.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Cursor {
    pub id: BlockId,
    pub part: Part,
    pub offset: usize,
    pub occurrence: Option<BlockId>,
}

impl Cursor {
    #[must_use]
    pub fn new(id: BlockId, part: Part, offset: usize) -> Self {
        Self {
            id,
            part,
            offset,
            occurrence: None,
        }
    }

    /// Bind this caret to a Child occurrence identity.
    #[must_use]
    pub fn with_occurrence(mut self, occurrence: impl Into<BlockId>) -> Self {
        self.occurrence = Some(occurrence.into());
        self
    }

    /// Move within the same block and Child occurrence.
    #[must_use]
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Row identity used for paint and hit-testing: occurrence, else content.
    #[must_use]
    pub fn paint_id(&self) -> &BlockId {
        self.occurrence.as_ref().unwrap_or(&self.id)
    }
}

/// Two-cursor selection (anchor / focus).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Cursor,
    pub focus: Cursor,
}

impl Selection {
    #[must_use]
    pub fn caret(cursor: Cursor) -> Self {
        Self {
            anchor: cursor.clone(),
            focus: cursor,
        }
    }

    #[must_use]
    pub fn new(anchor: Cursor, focus: Cursor) -> Self {
        Self { anchor, focus }
    }

    #[must_use]
    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.focus
    }

    /// The focus end (where typing lands).
    #[must_use]
    pub fn head(&self) -> &Cursor {
        &self.focus
    }

    /// Document-order ends `(start, end)`.
    #[must_use]
    pub fn ordered(&self) -> (Cursor, Cursor) {
        if Self::cursor_le(&self.anchor, &self.focus) {
            (self.anchor.clone(), self.focus.clone())
        } else {
            (self.focus.clone(), self.anchor.clone())
        }
    }

    /// Keep the anchor; move the focus.
    #[must_use]
    pub fn extend_to(&self, focus: Cursor) -> Self {
        Self {
            anchor: self.anchor.clone(),
            focus,
        }
    }

    fn cursor_le(a: &Cursor, b: &Cursor) -> bool {
        // Without document order, compare paint id then part then offset.
        // Callers that need true document order should clamp against snapshots.
        (a.paint_id().0.as_str(), a.part, a.offset) <= (b.paint_id().0.as_str(), b.part, b.offset)
    }
}

/// Immutable projection of one block for paint / tests.
#[derive(Debug, Clone)]
pub struct BlockSnapshot {
    /// Content identity (`BlockId` as the object's ObjectId).
    pub id: BlockId,
    /// Child occurrence identity when this row is a composed placement.
    /// Repeated placements share [`Self::id`] and keep distinct occurrence ids.
    pub occurrence_id: Option<BlockId>,
    pub block_type: BlockType,
    pub indent: i64,
    pub plain: String,
    pub runs: Vec<TextDelta>,
    pub props: HashMap<String, String>,
    pub checked: Option<bool>,
    pub number: Option<i64>,
    pub language: Option<String>,
    pub url: Option<String>,
    pub form: Option<Form>,
    pub width: Option<i64>,
    /// Table cells as plain strings `[row][column]`; row 0 is the header when present.
    pub table: Option<TableData>,
    /// Unsupported or gated kinds paint without accepting input.
    pub read_only: bool,
}

impl BlockSnapshot {
    /// Row identity used for paint and hit-testing: occurrence, else content.
    #[must_use]
    pub fn paint_id(&self) -> &BlockId {
        self.occurrence_id.as_ref().unwrap_or(&self.id)
    }
}

/// Frameless table projection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableData {
    pub align: Vec<Align>,
    pub rows: Vec<Vec<String>>,
}

/// GFM column alignment (view copy of the codec enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// Comment identity (UUID string).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommentId(pub String);

impl CommentId {
    #[must_use]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for CommentId {
    fn default() -> Self {
        Self::new()
    }
}

/// LWW prop values for [`BlockOp::SetProp`].
#[derive(Debug, Clone, PartialEq)]
pub enum LwwValue {
    Null,
    Bool(bool),
    I64(i64),
    String(String),
}

/// One mutation against the Loro document.
#[derive(Debug, Clone)]
pub enum BlockOp {
    InsertText {
        id: BlockId,
        offset: usize,
        text: String,
    },
    DeleteRange {
        id: BlockId,
        start: usize,
        end: usize,
    },
    SplitBlock {
        id: BlockId,
        offset: usize,
    },
    MergeWithPrevious {
        id: BlockId,
    },
    DeleteBlock {
        id: BlockId,
    },
    DeleteCrossBlock {
        anchor: Cursor,
        focus: Cursor,
    },
    SetType {
        id: BlockId,
        kind: BlockType,
    },
    ToggleMark {
        id: BlockId,
        start: usize,
        end: usize,
        mark: String,
    },
    SetLink {
        id: BlockId,
        start: usize,
        end: usize,
        url: String,
    },
    RemoveLink {
        id: BlockId,
        start: usize,
        end: usize,
    },
    Indent {
        id: BlockId,
    },
    Outdent {
        id: BlockId,
    },
    Move {
        id: BlockId,
        to: usize,
    },
    ToggleCheck {
        id: BlockId,
    },
    SetProp {
        id: BlockId,
        key: &'static str,
        value: LwwValue,
    },
    UnwrapToParagraph {
        id: BlockId,
    },
    AddComment {
        id: CommentId,
        range: Selection,
        body: String,
    },
    SetCommentBody {
        id: CommentId,
        body: String,
    },
    SetCommentState {
        id: CommentId,
        state: CommentState,
    },
    DeleteComment {
        id: CommentId,
    },
    ImeCommit {
        id: BlockId,
        part: Part,
        offset: usize,
        replace_len: usize,
        text: String,
    },
}

impl BlockOp {
    /// Content or occurrence id named by this op, when it targets a block.
    #[must_use]
    pub fn target_id(&self) -> Option<&BlockId> {
        match self {
            Self::InsertText { id, .. }
            | Self::DeleteRange { id, .. }
            | Self::SplitBlock { id, .. }
            | Self::MergeWithPrevious { id, .. }
            | Self::DeleteBlock { id, .. }
            | Self::SetType { id, .. }
            | Self::ToggleMark { id, .. }
            | Self::SetLink { id, .. }
            | Self::RemoveLink { id, .. }
            | Self::Indent { id, .. }
            | Self::Outdent { id, .. }
            | Self::Move { id, .. }
            | Self::ToggleCheck { id, .. }
            | Self::SetProp { id, .. }
            | Self::UnwrapToParagraph { id, .. }
            | Self::ImeCommit { id, .. } => Some(id),
            Self::DeleteCrossBlock { focus, .. } => Some(&focus.id),
            Self::AddComment { .. }
            | Self::SetCommentBody { .. }
            | Self::SetCommentState { .. }
            | Self::DeleteComment { .. } => None,
        }
    }
}

/// Result of apply: optional caret hint.
#[derive(Debug, Clone, Default)]
pub struct ApplyResult {
    pub selection: Option<Selection>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use block_markdown::BlockId;

    #[test]
    fn paint_id_prefers_occurrence_identity() {
        let content = BlockId::from("ashlar/block/opaque/n00blk0000000000000000000a");
        let occ_a = BlockId::from("ashlar/child/opaque/n00child0000000000000000a1");
        let occ_b = BlockId::from("ashlar/child/opaque/n00child0000000000000000a2");
        let snap_a = BlockSnapshot {
            id: content.clone(),
            occurrence_id: Some(occ_a.clone()),
            block_type: BlockType::Paragraph,
            indent: 0,
            plain: "hello".into(),
            runs: Vec::new(),
            props: HashMap::new(),
            checked: None,
            number: None,
            language: None,
            url: None,
            form: None,
            width: None,
            table: None,
            read_only: false,
        };
        let mut snap_b = snap_a.clone();
        snap_b.occurrence_id = Some(occ_b.clone());
        assert_eq!(snap_a.id, snap_b.id);
        assert_eq!(snap_a.paint_id(), &occ_a);
        assert_eq!(snap_b.paint_id(), &occ_b);
        assert_ne!(snap_a.paint_id(), snap_b.paint_id());

        let caret = Cursor::new(content, Part::Body, 0).with_occurrence(occ_a);
        assert_eq!(caret.paint_id(), snap_a.paint_id());
        assert_ne!(caret.paint_id(), snap_b.paint_id());
    }
}
