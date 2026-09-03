//! Public mutation types for the Loro-backed block document.
//!
//! Paint-facing types live in `gpui-component-block-view` so the dependency
//! arrow stays editor → block-view.

use block_markdown::{BlockId, BlockType, CommentState};

pub use gpui_component_block_view::{Align, BlockSnapshot, Cursor, Part, Selection, TableData};

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
        mark: &'static str,
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
        offset: usize,
        replace_len: usize,
        text: String,
    },
}

impl BlockOp {
    #[must_use]
    pub fn is_structure(&self) -> bool {
        matches!(
            self,
            Self::SplitBlock { .. }
                | Self::SetType { .. }
                | Self::Move { .. }
                | Self::Indent { .. }
                | Self::Outdent { .. }
                | Self::DeleteBlock { .. }
                | Self::MergeWithPrevious { .. }
                | Self::DeleteCrossBlock { .. }
                | Self::UnwrapToParagraph { .. }
                | Self::AddComment { .. }
                | Self::DeleteComment { .. }
        )
    }
}

/// Result of apply: optional caret hint.
#[derive(Debug, Clone, Default)]
pub struct ApplyResult {
    pub selection: Option<Selection>,
}
