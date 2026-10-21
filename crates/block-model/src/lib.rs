//! The block editor's model without GPUI: a Loro document per block,
//! [`BlockOp`]s and their application, compositions of per-object documents,
//! and the paint-free projection painters and other front ends read.
//!
//! Ported from Bezel and Catena/Mikra Loro patterns. See `NOTICE`.

mod apply;
mod backspace;
mod composition;
mod document;
mod link;
mod project;
mod shortcut;
pub mod slash;
mod types;

pub use backspace::backspace_at_start;
pub use composition::{
    CHILD_TYPE, ChildOccurrence, CompositionDraft, CompositionObject, CompositionRead,
    CompositionSession, EditorGate, IdGenerator, IdKind, IdSource, MutationReadSet, ObjectVersion,
    PlaceIntent, RelationMutation, ReloadError, encode_block,
};
pub use document::BlockDocument;
pub use link::{Choice as LinkChoice, Paste as LinkPaste};
pub use shortcut::{
    PrefixKind, inline_ops, match_inline, match_prefix, prefix_ops, try_inline, try_prefix,
};
pub use types::{
    Align, ApplyResult, BlockOp, BlockSnapshot, CommentId, Cursor, LwwValue, Part, Selection,
    TableData,
};

pub use block_markdown::{BlockId, BlockType, CommentState, Form};

#[cfg(test)]
mod tests {
    include!("lib_model_tests.rs");
}
