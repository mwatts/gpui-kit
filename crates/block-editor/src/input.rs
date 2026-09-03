//! Platform `EntityInputHandler` — UTF-16 at the boundary, UTF-8 for Loro.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use std::ops::Range;

use gpui::{Context, EntityInputHandler, UTF16Selection, Window};
use gpui_component_block_view::{Cursor, Selection};

use crate::editor::Editor;
use crate::types::BlockOp;

impl Editor {
    /// Caret block plain text (UTF-8).
    pub(crate) fn caret_plain(&self) -> Option<&str> {
        let head = self.selection.head();
        self.document
            .snapshots()
            .iter()
            .find(|s| s.id == head.id)
            .map(|s| s.plain.as_str())
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
        text.get(..offset.min(text.len()))
            .map(|s| s.chars().map(char::len_utf16).sum())
            .unwrap_or(0)
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
        let text = self.caret_plain()?.to_string();
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
        let text = self.caret_plain().unwrap_or("");
        let (start, end) = self.selection.ordered();
        let spans_one = start.id == end.id && start.part == end.part;
        let head = self.selection.head();
        let range = if spans_one {
            start.offset..end.offset
        } else {
            head.offset..head.offset
        };
        Some(UTF16Selection {
            reversed: spans_one && self.selection.head() == &start,
            range: Self::range_to_utf16(text, &range),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let text = self.caret_plain()?;
        let marked = self.marked.as_ref()?;
        Some(Self::range_to_utf16(text, &marked.range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
        self.composition.clear();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if range_utf16.is_none() && self.marked.is_none() && block_markdown::is_url(text.trim()) {
            return self.paste_url(text.trim().to_string(), cx);
        }

        let plain = self.caret_plain().unwrap_or("").to_string();
        let at = self.selection.head().clone();

        if let Some(range_utf16) = range_utf16.or_else(|| {
            self.marked
                .as_ref()
                .map(|m| Self::range_to_utf16(&plain, &m.range))
        }) {
            let range = Self::range_from_utf16(&plain, &range_utf16);
            self.selection = Selection::new(
                Cursor {
                    offset: range.start,
                    ..at.clone()
                },
                Cursor {
                    offset: range.end,
                    ..at.clone()
                },
            );
            // Commit composition or platform replace → ImeCommit / insert path.
            let replace_len = range.end.saturating_sub(range.start);
            self.marked = None;
            self.composition.clear();
            if replace_len > 0 || !text.is_empty() {
                let result = self.apply(
                    BlockOp::ImeCommit {
                        id: at.id.clone(),
                        offset: range.start,
                        replace_len,
                        text: text.to_string(),
                    },
                    cx,
                );
                if let Some(sel) = result.selection {
                    self.selection = sel;
                } else {
                    self.selection =
                        Selection::caret(Cursor::new(at.id, at.part, range.start + text.len()));
                }
                self.after_edit(text, cx);
                return;
            }
        }

        self.marked = None;
        self.composition.clear();
        self.insert_text(text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        marked_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Composition: view-state only — skip Loro project until commit.
        let plain = self.caret_plain().unwrap_or("").to_string();
        let at = self.selection.head().clone();
        let range = range_utf16
            .as_ref()
            .map(|r| Self::range_from_utf16(&plain, r))
            .or_else(|| self.marked.as_ref().map(|m| m.range.clone()))
            .unwrap_or(at.offset..at.offset);

        self.composition = text.to_string();
        let start = range.start;
        self.marked = Some(gpui_component_block_view::MarkedRange {
            id: at.id.clone(),
            range: start..start + text.len(),
        });
        let caret_off = marked_utf16
            .map(|r| {
                let local = Self::range_from_utf16(text, &r);
                start + local.end.min(text.len())
            })
            .unwrap_or(start + text.len());
        self.selection = Selection::caret(Cursor::new(at.id, at.part, caret_off));
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: gpui::Bounds<gpui::Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        let text = self.caret_plain()?;
        let range = Self::range_from_utf16(text, &range_utf16);
        let at = self.selection.head().clone();
        let start = Cursor {
            offset: range.start,
            ..at.clone()
        };
        let (origin, line_height) = self.layouts.position(&start)?;
        let end = self
            .layouts
            .position(&Cursor {
                offset: range.end,
                ..at
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
        let at = self.selection.head();
        let text = self.caret_plain()?;
        (hit.id == at.id && hit.part == at.part).then(|| Self::offset_to_utf16(text, hit.offset))
    }
}
