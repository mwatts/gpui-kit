//! Paint-facing document types shared by the block editor and the view.
//!
//! Ported from Bezel `select.rs` / `render.rs` overlays, keyed by stable
//! [`BlockId`] rather than list index.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use std::collections::HashMap;
use std::ops::Range;

use block_markdown::{BlockId, BlockType, Form};
use gpui::SharedString;
use loro::TextDelta;

use crate::layouts::BlockLayouts;

/// Bullet disc diameter in paint (Bezel).
pub const BULLET_DISC_PX: f32 = 5.0;
/// Task checkbox edge length in paint (Bezel).
pub const TASK_BOX_PX: f32 = 13.0;
/// Quote left-border width (Bezel).
pub const QUOTE_BAR_PX: f32 = 2.0;
/// Caret width (Bezel).
pub const CARET_WIDTH_PX: f32 = 1.5;
/// Base corner radius (Bezel `Theme::BASE_RADIUS`).
pub const BASE_RADIUS_PX: f32 = 8.0;

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

/// Comment wash kind under a selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Annotation {
    #[default]
    Open,
    Resolved,
    Active,
}

/// Whether an image caption paints under the picture.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Caption {
    #[default]
    Shown,
    Hidden,
}

/// An IME composition projected over an unchanged document range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composition {
    id: BlockId,
    part: Part,
    range: Range<usize>,
    text: String,
    selection: Range<usize>,
}

impl Composition {
    /// Creates an empty composition over a UTF-8 document range.
    #[must_use]
    pub fn new(id: BlockId, part: Part, range: Range<usize>) -> Self {
        Self {
            id,
            part,
            range,
            text: String::new(),
            selection: 0..0,
        }
    }

    /// Sets the transient preedit text without changing the document.
    #[must_use]
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    /// Sets the UTF-8 selection relative to the preedit text.
    #[must_use]
    pub fn with_selection(mut self, selection: Range<usize>) -> Self {
        self.selection = selection;
        self
    }

    #[must_use]
    pub fn id(&self) -> &BlockId {
        &self.id
    }

    #[must_use]
    pub const fn part(&self) -> Part {
        self.part
    }

    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }

    /// Returns the preedit range in the projected UTF-8 text.
    #[must_use]
    pub fn projected_range(&self) -> Range<usize> {
        self.range.start..self.range.start + self.text.len()
    }

    /// Returns the preedit selection in projected UTF-8 text coordinates.
    #[must_use]
    pub fn projected_selection(&self) -> Selection {
        Selection::new(
            Cursor::new(
                self.id.clone(),
                self.part,
                self.range.start + self.selection.start,
            ),
            Cursor::new(
                self.id.clone(),
                self.part,
                self.range.start + self.selection.end,
            ),
        )
    }

    /// Replaces the original range in a transient text projection.
    #[must_use]
    pub fn project_text(&self, source: &str) -> String {
        let mut projected = source.to_string();
        projected.replace_range(self.range.clone(), &self.text);
        projected
    }

    /// Maps a range in projected text back to the unchanged document.
    #[must_use]
    pub fn document_range_for_projected(&self, projected: Range<usize>) -> Range<usize> {
        let marked = self.projected_range();
        let replaced_len = self.range.end.saturating_sub(self.range.start);

        let start = if projected.start <= marked.start {
            projected.start
        } else if projected.start < marked.end {
            self.range.start
        } else {
            projected.start - self.text.len() + replaced_len
        };
        let end = if projected.end <= marked.start {
            projected.end
        } else if projected.end <= marked.end {
            self.range.end
        } else {
            projected.end - self.text.len() + replaced_len
        };
        start..end
    }
}

/// What an editor paints over a document.
#[derive(Clone)]
pub struct Editing<'a> {
    pub selections: &'a [Selection],
    pub caret_on: bool,
    pub layouts: Option<&'a BlockLayouts>,
    pub annotations: &'a [(Selection, Annotation)],
    pub placeholder: Option<SharedString>,
    pub caption: Caption,
    pub composition: Option<&'a Composition>,
}

impl Default for Editing<'_> {
    fn default() -> Self {
        Self {
            selections: &[],
            caret_on: true,
            layouts: None,
            annotations: &[],
            placeholder: None,
            caption: Caption::default(),
            composition: None,
        }
    }
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
