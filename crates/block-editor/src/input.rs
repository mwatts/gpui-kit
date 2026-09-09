//! Platform `EntityInputHandler` — UTF-16 at the boundary, UTF-8 for Loro.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use std::ops::Range;

use gpui::{Context, EntityInputHandler, UTF16Selection, Window};
use gpui_component_block_view::{Composition, Cursor, Part, Selection};

use crate::editor::Editor;
use crate::types::BlockOp;

impl Editor {
    /// Plain UTF-8 text for one editable block part.
    pub(crate) fn plain_for(&self, cursor: &Cursor) -> Option<&str> {
        let snapshot = self
            .document
            .snapshots()
            .iter()
            .find(|snapshot| snapshot.id == cursor.id)?;
        match cursor.part {
            Part::Cell { row, column } => snapshot
                .table
                .as_ref()?
                .rows
                .get(row)?
                .get(column)
                .map(String::as_str),
            Part::Body | Part::Code | Part::Caption => Some(snapshot.plain.as_str()),
        }
    }

    fn composition_target(&self) -> Cursor {
        self.composition.as_ref().map_or_else(
            || self.selection.head().clone(),
            |composition| {
                Cursor::new(
                    composition.id().clone(),
                    composition.part(),
                    composition.range().start,
                )
            },
        )
    }

    pub(crate) fn projected_text_for(&self, cursor: &Cursor) -> Option<String> {
        let plain = self.plain_for(cursor)?;
        Some(self.project_plain(cursor, plain))
    }

    pub(crate) fn project_plain(&self, cursor: &Cursor, plain: &str) -> String {
        match self.composition.as_ref().filter(|composition| {
            composition.id() == &cursor.id && composition.part() == cursor.part
        }) {
            Some(composition) => composition.project_text(plain),
            None => plain.to_string(),
        }
    }

    pub(crate) fn displayed_selection(&self) -> Selection {
        self.composition
            .as_ref()
            .map(Composition::projected_selection)
            .unwrap_or_else(|| self.selection.clone())
    }

    fn selected_document_range(&self, target: &Cursor) -> Range<usize> {
        let (start, end) = self.selection.ordered();
        if start.id == target.id
            && end.id == target.id
            && start.part == target.part
            && end.part == target.part
        {
            start.offset..end.offset
        } else {
            target.offset..target.offset
        }
    }

    fn offset_from_utf16(text: &str, offset_utf16: usize) -> usize {
        let mut utf16 = 0usize;
        for (byte_ix, ch) in text.char_indices() {
            if utf16 >= offset_utf16 {
                return byte_ix;
            }
            utf16 += ch.len_utf16();
        }
        text.len()
    }

    fn offset_to_utf16(text: &str, offset: usize) -> usize {
        let mut offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        text[..offset].chars().map(char::len_utf16).sum()
    }

    fn range_from_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
        Self::offset_from_utf16(text, range.start)..Self::offset_from_utf16(text, range.end)
    }

    fn range_to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
        Self::offset_to_utf16(text, range.start)..Self::offset_to_utf16(text, range.end)
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let target = self.composition_target();
        let text = self.projected_text_for(&target)?;
        let range = Self::range_from_utf16(&text, &range_utf16);
        let range = range.start.min(text.len())..range.end.min(text.len());
        *adjusted = Some(Self::range_to_utf16(&text, &range));
        Some(text.get(range)?.to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let selection = self.displayed_selection();
        let (start, end) = selection.ordered();
        let spans_one = start.id == end.id && start.part == end.part;
        let head = selection.head();
        let range = if spans_one {
            start.offset..end.offset
        } else {
            head.offset..head.offset
        };
        let text = self.projected_text_for(head)?;
        Some(UTF16Selection {
            reversed: spans_one && !selection.is_collapsed() && selection.head() == &start,
            range: Self::range_to_utf16(&text, &range),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let composition = self.composition.as_ref()?;
        let target = Cursor::new(
            composition.id().clone(),
            composition.part(),
            composition.range().start,
        );
        let text = self.projected_text_for(&target)?;
        Some(Self::range_to_utf16(&text, &composition.projected_range()))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.composition.take().is_some() {
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if range_utf16.is_none()
            && self.composition.is_none()
            && block_markdown::is_url(text.trim())
        {
            return self.paste_url(text.trim().to_string(), cx);
        }

        let target = self.composition_target();
        let Some(plain) = self.plain_for(&target).map(ToOwned::to_owned) else {
            return;
        };

        let range = if let Some(range_utf16) = range_utf16 {
            let projected = self.project_plain(&target, &plain);
            let range = Self::range_from_utf16(&projected, &range_utf16);
            self.composition
                .as_ref()
                .filter(|composition| {
                    composition.id() == &target.id && composition.part() == target.part
                })
                .map_or(range.clone(), |composition| {
                    composition.document_range_for_projected(range)
                })
        } else if let Some(composition) = self.composition.as_ref() {
            composition.range()
        } else {
            self.insert_text(text, cx);
            return;
        };

        if range.end > plain.len()
            || !plain.is_char_boundary(range.start)
            || !plain.is_char_boundary(range.end)
        {
            return;
        }

        let replace_len = range.end.saturating_sub(range.start);
        self.composition = None;
        let result = self.apply(
            BlockOp::ImeCommit {
                id: target.id.clone(),
                part: target.part,
                offset: range.start,
                replace_len,
                text: text.to_string(),
            },
            cx,
        );
        self.selection = result.selection.unwrap_or_else(|| {
            Selection::caret(Cursor::new(
                target.id,
                target.part,
                range.start + text.len(),
            ))
        });
        self.after_edit(text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        marked_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if text.is_empty() {
            if self.composition.take().is_some() {
                cx.notify();
            }
            return;
        }

        let target = self.composition_target();
        let Some(plain) = self.plain_for(&target).map(ToOwned::to_owned) else {
            return;
        };

        let range = if let Some(range_utf16) = range_utf16 {
            let projected = self.project_plain(&target, &plain);
            let range = Self::range_from_utf16(&projected, &range_utf16);
            self.composition
                .as_ref()
                .filter(|composition| {
                    composition.id() == &target.id && composition.part() == target.part
                })
                .map_or(range.clone(), |composition| {
                    composition.document_range_for_projected(range)
                })
        } else {
            self.composition
                .as_ref()
                .map(Composition::range)
                .unwrap_or_else(|| self.selected_document_range(&target))
        };

        if range.end > plain.len()
            || !plain.is_char_boundary(range.start)
            || !plain.is_char_boundary(range.end)
        {
            return;
        }

        let selection = marked_utf16
            .as_ref()
            .map(|range| Self::range_from_utf16(text, range))
            .unwrap_or(text.len()..text.len());
        self.extra_selections.clear();
        self.composition = Some(
            Composition::new(target.id, target.part, range)
                .with_text(text)
                .with_selection(selection),
        );
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: gpui::Bounds<gpui::Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        let target = self.composition_target();
        let text = self.projected_text_for(&target)?;
        let range = Self::range_from_utf16(&text, &range_utf16);
        let start = Cursor {
            offset: range.start,
            ..target.clone()
        };
        let (origin, line_height) = self.layouts.position(&start)?;
        let end = self
            .layouts
            .position(&Cursor {
                offset: range.end,
                ..target
            })
            .map(|(point, _)| point)
            .filter(|point| point.y == origin.y);
        let width = end.map_or(gpui::px(0.0), |point| point.x - origin.x);
        Some(gpui::Bounds::new(origin, gpui::size(width, line_height)))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<gpui::Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let hit = self.layouts.hit(point)?;
        let text = self.projected_text_for(&hit)?;
        Some(Self::offset_to_utf16(&text, hit.offset))
    }
}
