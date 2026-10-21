//! Paint-facing document types shared by the block editor and the view.
//!
//! Ported from Bezel `select.rs` / `render.rs` overlays, keyed by stable
//! [`BlockId`] rather than list index.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use std::ops::Range;

use block_markdown::BlockId;
pub use block_model::{Align, BlockSnapshot, Cursor, Part, Selection, TableData};
use gpui::SharedString;

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
    occurrence: Option<BlockId>,
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
            occurrence: None,
            range,
            text: String::new(),
            selection: 0..0,
        }
    }

    /// Bind the preedit to the Child occurrence being edited.
    #[must_use]
    pub fn with_occurrence(mut self, occurrence: impl Into<BlockId>) -> Self {
        self.occurrence = Some(occurrence.into());
        self
    }

    #[must_use]
    pub fn occurrence(&self) -> Option<&BlockId> {
        self.occurrence.as_ref()
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
        let cursor = |offset| {
            let cursor = Cursor::new(self.id.clone(), self.part, offset);
            match &self.occurrence {
                Some(occurrence) => cursor.with_occurrence(occurrence.clone()),
                None => cursor,
            }
        };
        Selection::new(
            cursor(self.range.start + self.selection.start),
            cursor(self.range.start + self.selection.end),
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
    /// Overrides the app-wide [`crate::Typography`] for this render.
    pub typography: Option<crate::Typography>,
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
            typography: None,
        }
    }
}
