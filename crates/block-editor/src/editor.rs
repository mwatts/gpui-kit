//! The editing surface: one GPUI entity over one [`BlockDocument`].
//!
//! Paint reads [`BlockDocument::snapshots`] only. Mutations go through
//! [`Self::apply`]. Ported from Bezel `editor.rs` against the Loro loop.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use std::time::Duration;

use block_markdown::{BlockId, BlockType, CommentState, content_text, find_block, mark_covers};
use gpui::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, EventEmitter, FocusHandle, Focusable,
    MouseButton, Pixels, Point, Render, ScrollHandle, SharedString, Task, Window, canvas, div,
    prelude::*, px,
};
use gpui_component_block_view::{
    Annotation, BlockLayouts, Caption, Cursor, Editing, MarkedRange, Part, Selection, render_with,
};
use loro::{LoroDoc, LoroError};

use crate::backspace::backspace_at_start;
use crate::document::BlockDocument;
use crate::image::{self, Prompt};
use crate::keys::{
    self, Backspace, CancelUrl, ConfirmUrl, Copy, Cut, Delete, DeleteToHome, DeleteWordLeft,
    DeleteWordRight, Dismiss, Down, DuplicateBlock, End, Home, Indent, KillLine, Left,
    MoveBlockDown, MoveBlockUp, Outdent, Paste, Redo, RemoveBlock, Right, SelectAll, SelectDown,
    SelectEnd, SelectHome, SelectLeft, SelectRight, SelectUp, SelectWordLeft, SelectWordRight,
    SplitBlock, ToggleBold, ToggleCode, ToggleItalic, ToggleStrike, Undo, Up, WordLeft, WordRight,
};
use crate::link::{self, Choice as LinkChoice};
use crate::mark::Mark;
use crate::shortcut::{inline_ops, try_prefix};
use crate::slash::Slash;
use crate::types::{ApplyResult, BlockOp, BlockSnapshot, CommentId, LwwValue};

const PLACEHOLDER: &str = "Type / for commands";
const BLINK: Duration = Duration::from_millis(500);

/// What the editor tells its host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorEvent {
    Changed,
    CommentActivated(CommentId),
}

/// One comment thread projected for the host list.
#[derive(Debug, Clone)]
pub struct CommentThread {
    pub id: CommentId,
    pub body: String,
    pub state: CommentState,
    pub range: Selection,
    pub detached: bool,
}

/// GPUI entity wrapping [`BlockDocument`] + view state.
pub struct Editor {
    pub(crate) document: BlockDocument,
    pub(crate) selection: Selection,
    focus_handle: FocusHandle,
    pub(crate) marked: Option<MarkedRange>,
    /// Composition fragment while IME is active (not yet in Loro).
    pub(crate) composition: String,
    pub(crate) layouts: BlockLayouts,
    caret_on: bool,
    blink: Option<Task<()>>,
    stored: Vec<Mark>,
    pub(crate) slash: Option<Slash>,
    pub(crate) pasted: Option<link::Paste>,
    url_prompt: Option<Prompt>,
    pub(crate) dropping: Option<BlockId>,
    pub(crate) hovered: Option<BlockId>,
    pub(crate) lifted: Option<(BlockId, BlockId)>,
    pub(crate) block_menu: Option<(BlockId, Point<Pixels>)>,
    pub(crate) language_menu: Option<(BlockId, Point<Pixels>)>,
    pub(crate) press_claimed: bool,
    pub(crate) origin: Point<Pixels>,
    width: Pixels,
    dragging: bool,
    over_text: bool,
    scroll: Option<ScrollHandle>,
    reveal: bool,
    goal: Option<Point<Pixels>>,
    active_comment: Option<CommentId>,
    annotations_cache: Vec<(Selection, Annotation)>,
    comments_cache: Vec<CommentThread>,
}

impl Editor {
    /// Empty document (one paragraph).
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::from_document(BlockDocument::new(), cx)
    }

    /// Import a Loro snapshot onto a fresh doc.
    pub fn from_snapshot(bytes: &[u8], cx: &mut Context<Self>) -> Result<Self, LoroError> {
        Ok(Self::from_document(
            BlockDocument::import_snapshot(bytes)?,
            cx,
        ))
    }

    /// Hydrate from markdown.
    pub fn from_markdown(source: &str, cx: &mut Context<Self>) -> Self {
        Self::from_document(BlockDocument::from_markdown(source), cx)
    }

    fn from_document(document: BlockDocument, cx: &mut Context<Self>) -> Self {
        let id = document
            .snapshots()
            .first()
            .map(|s| s.id.clone())
            .unwrap_or_else(|| BlockId(uuid::Uuid::new_v4().to_string()));
        let mut editor = Self {
            document,
            selection: Selection::caret(Cursor::new(id, Part::Body, 0)),
            focus_handle: cx.focus_handle(),
            marked: None,
            composition: String::new(),
            layouts: BlockLayouts::default(),
            caret_on: true,
            blink: None,
            stored: Vec::new(),
            slash: None,
            pasted: None,
            url_prompt: None,
            dropping: None,
            hovered: None,
            lifted: None,
            block_menu: None,
            language_menu: None,
            press_claimed: false,
            origin: Point::default(),
            width: Pixels::ZERO,
            dragging: false,
            over_text: false,
            scroll: None,
            reveal: false,
            goal: None,
            active_comment: None,
            annotations_cache: Vec::new(),
            comments_cache: Vec::new(),
        };
        editor.refresh_annotations();
        editor
    }

    #[must_use]
    pub fn with_undo_limit(mut self, limit: usize) -> Self {
        self.document.set_undo_limit(limit);
        self
    }

    #[must_use]
    pub fn with_scroll(mut self, handle: ScrollHandle) -> Self {
        self.scroll = Some(handle);
        self
    }

    #[must_use]
    pub fn document(&self) -> &LoroDoc {
        self.document.doc()
    }

    #[must_use]
    pub fn snapshots(&self) -> &[BlockSnapshot] {
        self.document.snapshots()
    }

    #[must_use]
    pub fn selection(&self) -> Selection {
        self.selection.clone()
    }

    pub fn select(&mut self, selection: Selection, cx: &mut Context<Self>) {
        self.selection = self.clamp_selection(selection);
        self.reveal = true;
        self.caret_moved();
        cx.notify();
    }

    #[must_use]
    pub fn selection_bounds(&self) -> Option<Bounds<Pixels>> {
        if self.selection.is_collapsed() {
            return None;
        }
        let (point, line_height) = self.layouts.position(self.selection.head())?;
        Some(Bounds::new(point, gpui::size(px(0.0), line_height)))
    }

    pub fn export_snapshot(&self) -> Result<Vec<u8>, loro::LoroEncodeError> {
        self.document.export_snapshot()
    }

    #[must_use]
    pub fn project_markdown(&self) -> String {
        self.document.project_markdown()
    }

    pub fn apply(&mut self, op: BlockOp, cx: &mut Context<Self>) -> ApplyResult {
        // Skip project while composing (IME overlay owns the caret block).
        if self.marked.is_some() && !matches!(op, BlockOp::ImeCommit { .. }) {
            return ApplyResult::default();
        }
        let result = self.document.apply(op);
        if let Some(sel) = &result.selection {
            self.selection = sel.clone();
        }
        self.refresh_annotations();
        cx.emit(EditorEvent::Changed);
        cx.notify();
        result
    }

    fn apply_many(&mut self, ops: &[BlockOp], cx: &mut Context<Self>) -> ApplyResult {
        if ops.is_empty() {
            return ApplyResult::default();
        }
        let result = self.document.apply_many(ops);
        if let Some(sel) = &result.selection {
            self.selection = sel.clone();
        }
        self.refresh_annotations();
        cx.emit(EditorEvent::Changed);
        cx.notify();
        result
    }

    pub fn toggle_mark(&mut self, mark: Mark, cx: &mut Context<Self>) {
        if matches!(mark, Mark::Code) && self.selection.head().part == Part::Code {
            let id = self.selection.head().id.clone();
            self.apply(
                BlockOp::SetType {
                    id,
                    kind: BlockType::Paragraph,
                },
                cx,
            );
            return;
        }
        if self.selection.is_collapsed() {
            match self.stored.iter().position(|m| *m == mark) {
                Some(ix) => {
                    self.stored.remove(ix);
                }
                None => self.stored.push(mark),
            }
            return cx.notify();
        }
        let (start, end) = self.selection.ordered();
        if start.id != end.id {
            // Multi-block code → fence the first block for now.
            if matches!(mark, Mark::Code) {
                self.apply(
                    BlockOp::SetType {
                        id: start.id.clone(),
                        kind: BlockType::Code,
                    },
                    cx,
                );
            }
            return;
        }
        if matches!(mark, Mark::Code) {
            let plain = self
                .snapshots()
                .iter()
                .find(|s| s.id == start.id)
                .map(|s| s.plain.as_str())
                .unwrap_or("");
            if plain.contains('\n') {
                self.apply(
                    BlockOp::SetType {
                        id: start.id.clone(),
                        kind: BlockType::Code,
                    },
                    cx,
                );
                return;
            }
        }
        self.apply(
            BlockOp::ToggleMark {
                id: start.id.clone(),
                start: start.offset.min(end.offset),
                end: start.offset.max(end.offset),
                mark: mark.key(),
            },
            cx,
        );
    }

    pub fn move_block(&mut self, id: BlockId, delta: isize, cx: &mut Context<Self>) {
        let Some((ix, _)) = find_block(self.document.doc(), &id) else {
            return;
        };
        let to = (ix as isize + delta).max(0) as usize;
        self.apply(BlockOp::Move { id, to }, cx);
    }

    pub fn duplicate_block(&mut self, id: BlockId, cx: &mut Context<Self>) {
        // Duplicate = export one block's markdown and insert after — simplified.
        let Some(snap) = self.snapshots().iter().find(|s| s.id == id).cloned() else {
            return;
        };
        let Some((ix, _)) = find_block(self.document.doc(), &id) else {
            return;
        };
        let _ = snap;
        // Insert empty paragraph after, then copy text via ops.
        let plain = self
            .snapshots()
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.plain.clone())
            .unwrap_or_default();
        let kind = self
            .snapshots()
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.block_type)
            .unwrap_or(BlockType::Paragraph);
        self.apply(
            BlockOp::SplitBlock {
                id: id.clone(),
                offset: plain.len(),
            },
            cx,
        );
        if let Some(new_id) = self.snapshots().get(ix + 1).map(|s| s.id.clone()) {
            self.apply(
                BlockOp::SetType {
                    id: new_id.clone(),
                    kind,
                },
                cx,
            );
            if !plain.is_empty() {
                // Split already moved the tail; for duplicate of whole block at end,
                // re-insert plain if the new block is empty.
                let empty = self
                    .snapshots()
                    .iter()
                    .find(|s| s.id == new_id)
                    .is_some_and(|s| s.plain.is_empty());
                if empty {
                    self.apply(
                        BlockOp::InsertText {
                            id: new_id,
                            offset: 0,
                            text: plain,
                        },
                        cx,
                    );
                }
            }
        }
    }

    pub fn remove_block(&mut self, id: BlockId, cx: &mut Context<Self>) {
        self.apply(BlockOp::DeleteBlock { id }, cx);
    }

    pub fn set_language(&mut self, id: BlockId, language: Option<String>, cx: &mut Context<Self>) {
        self.apply(
            BlockOp::SetProp {
                id,
                key: "language",
                value: match language {
                    Some(s) => LwwValue::String(s),
                    None => LwwValue::Null,
                },
            },
            cx,
        );
    }

    pub fn set_block_type(&mut self, id: BlockId, kind: BlockType, cx: &mut Context<Self>) {
        self.apply(BlockOp::SetType { id, kind }, cx);
    }

    #[must_use]
    pub fn covered_by(&self, selection: Selection, mark: &str) -> bool {
        if selection.is_collapsed() {
            return false;
        }
        let (start, end) = selection.ordered();
        if start.id != end.id {
            return false;
        }
        let Some((_, map)) = find_block(self.document.doc(), &start.id) else {
            return false;
        };
        let Some(text) = content_text(&map) else {
            return false;
        };
        mark_covers(
            &text,
            mark,
            start.offset.min(end.offset),
            start.offset.max(end.offset),
        )
    }

    #[must_use]
    pub fn comment_at(&self, at: Point<Pixels>) -> Option<CommentId> {
        let hit = self.layouts.hit(at)?;
        for (sel, _) in &self.annotations_cache {
            let (start, end) = sel.ordered();
            if start.id == hit.id
                && end.id == hit.id
                && start.offset <= hit.offset
                && hit.offset <= end.offset
            {
                // Recover id from delta attributes on that range.
                if let Some(id) = self.comment_id_covering(&hit.id, hit.offset) {
                    return Some(id);
                }
            }
        }
        self.comment_id_covering(&hit.id, hit.offset)
    }

    fn comment_id_covering(&self, block: &BlockId, offset: usize) -> Option<CommentId> {
        let snap = self.snapshots().iter().find(|s| &s.id == block)?;
        let mut pos = 0usize;
        for run in &snap.runs {
            let loro::TextDelta::Insert { insert, attributes } = run else {
                continue;
            };
            let next = pos + insert.len();
            if pos <= offset && offset < next {
                let id = attributes
                    .as_ref()?
                    .get("comment")?
                    .as_string()
                    .map(|s| CommentId(s.to_string()))?;
                return Some(id);
            }
            pos = next;
        }
        None
    }

    #[must_use]
    pub fn comments(&self) -> &[CommentThread] {
        &self.comments_cache
    }

    fn rebuild_comments(&mut self) {
        let comments = block_markdown::comments_map(self.document.doc());
        let mut out = Vec::new();
        // Discover ids from painted marks, then fill from the Loro map.
        let mut seen = std::collections::HashSet::new();
        for snap in self.snapshots() {
            for run in &snap.runs {
                let loro::TextDelta::Insert { attributes, .. } = run else {
                    continue;
                };
                let Some(cid) = attributes
                    .as_ref()
                    .and_then(|a| a.get("comment"))
                    .and_then(|v| v.as_string().map(|s| s.to_string()))
                else {
                    continue;
                };
                if !seen.insert(cid.clone()) {
                    continue;
                }
                let (body, state) = match comments.get(cid.as_str()) {
                    Some(loro::ValueOrContainer::Container(loro::Container::Map(entry))) => {
                        let body = block_markdown::map_string(&entry, "body").unwrap_or_default();
                        let state = block_markdown::map_string(&entry, "state")
                            .and_then(|s| CommentState::parse(&s))
                            .unwrap_or(CommentState::Open);
                        (body, state)
                    }
                    _ => (String::new(), CommentState::Open),
                };
                let range = self.comment_range(&cid).unwrap_or_else(|| {
                    Selection::caret(Cursor::new(snap.id.clone(), Part::Body, 0))
                });
                let detached = range.is_collapsed();
                out.push(CommentThread {
                    id: CommentId(cid),
                    body,
                    state,
                    range,
                    detached,
                });
            }
        }
        // Map entries with no remaining mark → detached.
        if let loro::LoroValue::Map(entries) = comments.get_deep_value() {
            for (cid, value) in entries.iter() {
                let cid = cid.to_string();
                if seen.contains(&cid) {
                    continue;
                }
                let loro::LoroValue::Map(fields) = value else {
                    continue;
                };
                let body = fields
                    .get("body")
                    .and_then(|v| v.as_string().map(|s| s.to_string()))
                    .unwrap_or_default();
                let state = fields
                    .get("state")
                    .and_then(|v| v.as_string())
                    .and_then(|s| CommentState::parse(&s))
                    .unwrap_or(CommentState::Open);
                let id = self
                    .snapshots()
                    .first()
                    .map(|s| s.id.clone())
                    .unwrap_or_else(|| BlockId(uuid::Uuid::new_v4().to_string()));
                out.push(CommentThread {
                    id: CommentId(cid),
                    body,
                    state,
                    range: Selection::caret(Cursor::new(id, Part::Body, 0)),
                    detached: true,
                });
            }
        }
        self.comments_cache = out;
    }

    fn comment_range(&self, cid: &str) -> Option<Selection> {
        for snap in self.snapshots() {
            let mut pos = 0usize;
            let mut start = None;
            let mut end = None;
            for run in &snap.runs {
                let loro::TextDelta::Insert { insert, attributes } = run else {
                    continue;
                };
                let next = pos + insert.len();
                let has = attributes.as_ref().is_some_and(|a| {
                    a.get("comment")
                        .and_then(|v| v.as_string())
                        .is_some_and(|s| s.as_str() == cid)
                });
                if has {
                    if start.is_none() {
                        start = Some(pos);
                    }
                    end = Some(next);
                }
                pos = next;
            }
            if let (Some(s), Some(e)) = (start, end) {
                return Some(Selection::new(
                    Cursor::new(snap.id.clone(), Part::Body, s),
                    Cursor::new(snap.id.clone(), Part::Body, e),
                ));
            }
        }
        None
    }

    pub fn add_comment(&mut self, body: String, cx: &mut Context<Self>) -> Option<CommentId> {
        if self.selection.is_collapsed() {
            return None;
        }
        let id = CommentId::new();
        let range = self.selection.clone();
        self.apply(
            BlockOp::AddComment {
                id: id.clone(),
                range,
                body,
            },
            cx,
        );
        self.active_comment = Some(id.clone());
        Some(id)
    }

    fn refresh_annotations(&mut self) {
        self.rebuild_comments();
        let active = self.active_comment.clone();
        self.annotations_cache = self
            .comments_cache
            .iter()
            .filter(|t| !t.detached)
            .map(|t| {
                let ann = if Some(&t.id) == active.as_ref() {
                    Annotation::Active
                } else if t.state == CommentState::Resolved {
                    Annotation::Resolved
                } else {
                    Annotation::Open
                };
                (t.range.clone(), ann)
            })
            .collect();
    }

    fn clamp_selection(&self, selection: Selection) -> Selection {
        let clamp = |c: Cursor| {
            let snap = self
                .snapshots()
                .iter()
                .find(|s| s.id == c.id)
                .or_else(|| self.snapshots().first());
            let Some(snap) = snap else {
                return c;
            };
            let max = snap.plain.len();
            Cursor::new(snap.id.clone(), c.part, c.offset.min(max))
        };
        Selection::new(clamp(selection.anchor), clamp(selection.focus))
    }

    fn caret_moved(&mut self) {
        self.blink = None;
    }

    fn start_blink(&mut self, cx: &mut Context<Self>) {
        self.caret_on = true;
        self.blink = Some(cx.spawn(async move |editor, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                if editor
                    .update(cx, |editor, cx| {
                        editor.caret_on = !editor.caret_on;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    pub(crate) fn after_edit(&mut self, typed: &str, cx: &mut Context<Self>) {
        self.track_slash(typed);
        self.reveal = true;
        self.caret_moved();
        self.refresh_annotations();
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    pub(crate) fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        // Prefix shortcut: completing character must not be inserted.
        let head = self.selection.head().clone();
        if head.part == Part::Body {
            if let Some(snap) = self.snapshots().iter().find(|s| s.id == head.id) {
                if let Some(ops) = try_prefix(head.id.clone(), &snap.plain, text) {
                    self.apply_many(&ops, cx);
                    self.after_edit("", cx);
                    return;
                }
            }
        }

        if !self.selection.is_collapsed() {
            let (start, end) = self.selection.ordered();
            if start.id == end.id {
                self.apply(
                    BlockOp::DeleteRange {
                        id: start.id.clone(),
                        start: start.offset.min(end.offset),
                        end: start.offset.max(end.offset),
                    },
                    cx,
                );
            }
        }

        let head = self.selection.head().clone();
        let mut offset = head.offset;
        let marks: Vec<Mark> = self.stored.drain(..).collect();
        if !marks.is_empty() {
            self.apply(
                BlockOp::InsertText {
                    id: head.id.clone(),
                    offset,
                    text: text.to_string(),
                },
                cx,
            );
            let end = offset + text.len();
            for mark in marks {
                self.apply(
                    BlockOp::ToggleMark {
                        id: head.id.clone(),
                        start: offset,
                        end,
                        mark: mark.key(),
                    },
                    cx,
                );
            }
            offset = end;
        } else {
            self.apply(
                BlockOp::InsertText {
                    id: head.id.clone(),
                    offset,
                    text: text.to_string(),
                },
                cx,
            );
            offset += text.len();
        }

        // Inline rule after insert (plain now includes the typed close).
        if let Some(snap) = self.snapshots().iter().find(|s| s.id == head.id) {
            if let Some(ops) = inline_ops(head.id.clone(), &snap.plain, offset) {
                self.apply_many(&ops, cx);
                if let Some(last) = self.snapshots().iter().find(|s| s.id == head.id) {
                    offset = last.plain.len().min(offset);
                }
            }
        }

        let len = self
            .snapshots()
            .iter()
            .find(|s| s.id == head.id)
            .map(|s| s.plain.len())
            .unwrap_or(offset);
        self.selection = Selection::caret(Cursor::new(head.id, head.part, offset.min(len)));
        self.after_edit(text, cx);
    }

    fn track_slash(&mut self, typed: &str) {
        let at = self.selection.head().clone();
        let text = self
            .snapshots()
            .iter()
            .find(|s| s.id == at.id)
            .map(|s| s.plain.clone())
            .unwrap_or_default();

        if self.slash.is_none() {
            let opened = at.offset.checked_sub(1).filter(|_| typed == "/");
            let starts_word = opened.is_none_or(|slash| {
                text.get(..slash)
                    .and_then(|s| s.chars().next_back())
                    .is_none_or(char::is_whitespace)
            });
            if let Some(slash) = opened.filter(|_| starts_word && at.part == Part::Body) {
                self.slash = Some(Slash::open(Cursor::new(at.id.clone(), at.part, slash)));
            }
            return;
        }

        let Some(query) = self
            .slash
            .as_ref()
            .and_then(|slash| slash.query(at.clone(), &text))
        else {
            self.slash = None;
            return;
        };
        if let Some(slash) = &mut self.slash {
            slash.refilter(&query);
        }
    }

    pub(crate) fn confirm_slash(
        &mut self,
        kind: Option<BlockType>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(slash) = &self.slash else {
            return false;
        };
        let at = slash.at.clone();
        let kind = kind.or_else(|| slash.choice());
        self.slash = None;
        let Some(kind) = kind else {
            return false;
        };
        let caret = self.selection.head().clone();
        let mut ops = Vec::new();
        if caret.offset > at.offset {
            ops.push(BlockOp::DeleteRange {
                id: at.id.clone(),
                start: at.offset,
                end: caret.offset,
            });
        }
        ops.push(BlockOp::SetType {
            id: at.id.clone(),
            kind,
        });
        self.apply_many(&ops, cx);
        self.selection = Selection::caret(Cursor::new(at.id, Part::Body, at.offset));
        true
    }

    pub(crate) fn confirm_paste(&mut self, choice: LinkChoice, cx: &mut Context<Self>) {
        let Some(pasted) = self.pasted.take() else {
            return;
        };
        let at = pasted.at.clone();
        let url = pasted.url.clone();
        match choice {
            LinkChoice::Dismiss => {}
            LinkChoice::Chip => {
                // Already inserted as text; leave as link mark over the URL run.
                let end = at.offset + url.len();
                self.apply(
                    BlockOp::SetLink {
                        id: at.id.clone(),
                        start: at.offset,
                        end,
                        url,
                    },
                    cx,
                );
            }
            LinkChoice::Bookmark => {
                self.apply(
                    BlockOp::SetType {
                        id: at.id.clone(),
                        kind: BlockType::Bookmark,
                    },
                    cx,
                );
                self.apply(
                    BlockOp::SetProp {
                        id: at.id.clone(),
                        key: "url",
                        value: LwwValue::String(url),
                    },
                    cx,
                );
            }
            LinkChoice::Embed => {
                self.apply(
                    BlockOp::SetType {
                        id: at.id.clone(),
                        kind: BlockType::Bookmark,
                    },
                    cx,
                );
                self.apply(
                    BlockOp::SetProp {
                        id: at.id.clone(),
                        key: "url",
                        value: LwwValue::String(url),
                    },
                    cx,
                );
                self.apply(
                    BlockOp::SetProp {
                        id: at.id,
                        key: "form",
                        value: LwwValue::String("embed".into()),
                    },
                    cx,
                );
            }
            LinkChoice::Image => {
                self.apply(
                    BlockOp::SetType {
                        id: at.id.clone(),
                        kind: BlockType::Image,
                    },
                    cx,
                );
                self.apply(
                    BlockOp::SetProp {
                        id: at.id,
                        key: "url",
                        value: LwwValue::String(url),
                    },
                    cx,
                );
            }
        }
        cx.notify();
    }

    pub(crate) fn paste_url(&mut self, url: String, cx: &mut Context<Self>) {
        let at = self.selection.head().clone();
        let alone = self
            .snapshots()
            .iter()
            .find(|s| s.id == at.id)
            .is_some_and(|s| s.plain.is_empty() || s.plain == url);
        self.insert_text(&url, cx);
        let at = Cursor::new(
            at.id,
            at.part,
            self.selection.head().offset.saturating_sub(url.len()),
        );
        self.pasted = Some(link::Paste::open(at, url, alone));
        cx.notify();
    }

    fn delete_back(&mut self, cx: &mut Context<Self>) {
        if !self.selection.is_collapsed() {
            let (start, end) = self.selection.ordered();
            if start.id == end.id {
                self.apply(
                    BlockOp::DeleteRange {
                        id: start.id.clone(),
                        start: start.offset.min(end.offset),
                        end: start.offset.max(end.offset),
                    },
                    cx,
                );
                self.selection = Selection::caret(Cursor::new(
                    start.id,
                    start.part,
                    start.offset.min(end.offset),
                ));
            }
            self.after_edit("", cx);
            return;
        }
        let at = self.selection.head().clone();
        if at.offset > 0 {
            let plain = self
                .snapshots()
                .iter()
                .find(|s| s.id == at.id)
                .map(|s| s.plain.as_str())
                .unwrap_or("");
            let prev = plain
                .get(..at.offset)
                .and_then(|s| s.char_indices().next_back().map(|(i, _)| i))
                .unwrap_or(0);
            self.apply(
                BlockOp::DeleteRange {
                    id: at.id.clone(),
                    start: prev,
                    end: at.offset,
                },
                cx,
            );
            self.selection = Selection::caret(Cursor::new(at.id, at.part, prev));
            self.track_slash("");
            self.after_edit("", cx);
            return;
        }
        let ops = backspace_at_start(self.snapshots(), &at);
        if ops.is_empty() {
            return;
        }
        let result = self.apply_many(&ops, cx);
        if let Some(sel) = result.selection {
            self.selection = sel;
        }
        self.track_slash("");
        self.after_edit("", cx);
    }

    fn delete_forward(&mut self, cx: &mut Context<Self>) {
        if !self.selection.is_collapsed() {
            return self.delete_back(cx);
        }
        let at = self.selection.head().clone();
        let plain = self
            .snapshots()
            .iter()
            .find(|s| s.id == at.id)
            .map(|s| s.plain.clone())
            .unwrap_or_default();
        if at.offset >= plain.len() {
            return;
        }
        let next = plain[at.offset..]
            .chars()
            .next()
            .map(|c| at.offset + c.len_utf8())
            .unwrap_or(at.offset);
        self.apply(
            BlockOp::DeleteRange {
                id: at.id.clone(),
                start: at.offset,
                end: next,
            },
            cx,
        );
        self.after_edit("", cx);
    }

    fn move_horizontal(&mut self, extend: bool, forward: bool, cx: &mut Context<Self>) {
        let at = self.selection.head().clone();
        let plain = self
            .snapshots()
            .iter()
            .find(|s| s.id == at.id)
            .map(|s| s.plain.as_str())
            .unwrap_or("");
        let next = if forward {
            plain[at.offset..]
                .chars()
                .next()
                .map(|c| at.offset + c.len_utf8())
                .unwrap_or(at.offset)
        } else {
            plain
                .get(..at.offset)
                .and_then(|s| s.char_indices().next_back().map(|(i, _)| i))
                .unwrap_or(0)
        };
        let to = Cursor::new(at.id, at.part, next);
        if extend {
            self.selection = self.selection.extend_to(to);
        } else {
            self.selection = Selection::caret(to);
        }
        self.goal = None;
        self.caret_moved();
        cx.notify();
    }

    /// Alt/Ctrl word motion — Bezel `Cursor::word_left` / `word_right`.
    fn move_word(&mut self, extend: bool, forward: bool, cx: &mut Context<Self>) {
        let at = self.selection.head().clone();
        let plain = self
            .snapshots()
            .iter()
            .find(|s| s.id == at.id)
            .map(|s| s.plain.as_str())
            .unwrap_or("");
        let next = if forward {
            word_right_offset(plain, at.offset)
        } else {
            word_left_offset(plain, at.offset)
        };
        if next == at.offset {
            return self.move_horizontal(extend, forward, cx);
        }
        let to = Cursor::new(at.id, at.part, next);
        if extend {
            self.selection = self.selection.extend_to(to);
        } else {
            self.selection = Selection::caret(to);
        }
        self.goal = None;
        self.caret_moved();
        cx.notify();
    }

    fn delete_word(&mut self, forward: bool, cx: &mut Context<Self>) {
        if !self.selection.is_collapsed() {
            return self.delete_back(cx);
        }
        let at = self.selection.head().clone();
        let plain = self
            .snapshots()
            .iter()
            .find(|s| s.id == at.id)
            .map(|s| s.plain.as_str())
            .unwrap_or("");
        let other = if forward {
            word_right_offset(plain, at.offset)
        } else {
            word_left_offset(plain, at.offset)
        };
        if other == at.offset {
            if forward {
                self.delete_forward(cx);
            } else {
                self.delete_back(cx);
            }
            return;
        }
        let (start, end) = if forward {
            (at.offset, other)
        } else {
            (other, at.offset)
        };
        self.apply(
            BlockOp::DeleteRange {
                id: at.id.clone(),
                start,
                end,
            },
            cx,
        );
        self.selection = Selection::caret(Cursor::new(at.id, at.part, start));
        self.after_edit("", cx);
    }

    fn move_vertical(&mut self, extend: bool, down: bool, cx: &mut Context<Self>) {
        if let Some(slash) = &mut self.slash {
            slash.step(if down { 1 } else { -1 });
            return cx.notify();
        }
        if let Some(pasted) = &mut self.pasted {
            pasted.step(if down { 1 } else { -1 });
            return cx.notify();
        }
        let at = self.selection.head().clone();
        let from = self
            .goal
            .or_else(|| self.layouts.position(&at).map(|(p, _)| p))
            .unwrap_or_default();
        if let Some((to, _)) = self.layouts.step_row(&at, from, down) {
            self.goal = Some(Point {
                x: from.x,
                y: self
                    .layouts
                    .position(&to)
                    .map(|(p, _)| p.y)
                    .unwrap_or(from.y),
            });
            if extend {
                self.selection = self.selection.extend_to(to);
            } else {
                self.selection = Selection::caret(to);
            }
            self.caret_moved();
            cx.notify();
        }
    }

    fn line_edge(&mut self, extend: bool, end: bool, cx: &mut Context<Self>) {
        let at = self.selection.head().clone();
        let plain = self
            .snapshots()
            .iter()
            .find(|s| s.id == at.id)
            .map(|s| s.plain.as_str())
            .unwrap_or("");
        let offset = if end { plain.len() } else { 0 };
        let to = Cursor::new(at.id, at.part, offset);
        if extend {
            self.selection = self.selection.extend_to(to);
        } else {
            self.selection = Selection::caret(to);
        }
        self.goal = None;
        self.caret_moved();
        cx.notify();
    }

    // --- key handlers ---

    fn on_backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.delete_back(cx);
    }

    fn on_delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        self.delete_forward(cx);
    }

    fn on_split(&mut self, _: &SplitBlock, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(choice) = self.pasted.as_ref().map(|p| p.choice()) {
            return self.confirm_paste(choice, cx);
        }
        if self.confirm_slash(None, cx) {
            return;
        }
        let at = self.selection.head().clone();
        match at.part {
            Part::Code => return self.insert_text("\n", cx),
            Part::Cell { .. } => return,
            Part::Caption => {
                if self
                    .snapshots()
                    .iter()
                    .find(|s| s.id == at.id)
                    .is_some_and(|s| {
                        s.block_type == BlockType::Image
                            && s.url.as_deref().is_none_or(|u| u.is_empty())
                    })
                {
                    self.url_prompt = Some(Prompt::new(at.id.clone(), window, cx));
                    return cx.notify();
                }
            }
            Part::Body => {}
        }
        if !self.selection.is_collapsed() {
            self.delete_back(cx);
        }
        let at = self.selection.head().clone();
        let result = self.apply(
            BlockOp::SplitBlock {
                id: at.id,
                offset: at.offset,
            },
            cx,
        );
        if let Some(sel) = result.selection {
            self.selection = sel;
        }
        self.after_edit("", cx);
    }

    fn on_indent(&mut self, _: &Indent, _: &mut Window, cx: &mut Context<Self>) {
        let id = self.selection.head().id.clone();
        self.apply(BlockOp::Indent { id }, cx);
    }

    fn on_outdent(&mut self, _: &Outdent, _: &mut Window, cx: &mut Context<Self>) {
        let id = self.selection.head().id.clone();
        self.apply(BlockOp::Outdent { id }, cx);
    }

    fn on_dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        if self.pasted.take().is_none()
            && self.slash.take().is_none()
            && self.url_prompt.take().is_none()
            && self.language_menu.take().is_none()
        {
            self.selection = Selection::caret(self.selection.head().clone());
        }
        cx.notify();
    }

    fn on_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.document.undo() {
            self.refresh_annotations();
            cx.emit(EditorEvent::Changed);
            cx.notify();
        }
    }

    fn on_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.document.redo() {
            self.refresh_annotations();
            cx.emit(EditorEvent::Changed);
            cx.notify();
        }
    }

    fn on_toggle_bold(&mut self, _: &ToggleBold, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_mark(Mark::Bold, cx);
    }
    fn on_toggle_italic(&mut self, _: &ToggleItalic, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_mark(Mark::Italic, cx);
    }
    fn on_toggle_strike(&mut self, _: &ToggleStrike, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_mark(Mark::Strike, cx);
    }
    fn on_toggle_code(&mut self, _: &ToggleCode, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_mark(Mark::Code, cx);
    }

    fn on_left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.move_horizontal(false, false, cx);
    }
    fn on_right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.move_horizontal(false, true, cx);
    }
    fn on_up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(false, false, cx);
    }
    fn on_down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(false, true, cx);
    }
    fn on_select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_horizontal(true, false, cx);
    }
    fn on_select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_horizontal(true, true, cx);
    }
    fn on_select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(true, false, cx);
    }
    fn on_select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(true, true, cx);
    }
    fn on_home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.line_edge(false, false, cx);
    }
    fn on_end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.line_edge(false, true, cx);
    }
    fn on_select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.line_edge(true, false, cx);
    }
    fn on_select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.line_edge(true, true, cx);
    }
    fn on_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let Some(first) = self.snapshots().first() else {
            return;
        };
        let Some(last) = self.snapshots().last() else {
            return;
        };
        self.selection = Selection::new(
            Cursor::new(first.id.clone(), Part::Body, 0),
            Cursor::new(last.id.clone(), Part::Body, last.plain.len()),
        );
        cx.notify();
    }

    fn on_word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_word(false, false, cx);
    }
    fn on_word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_word(false, true, cx);
    }
    fn on_select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_word(true, false, cx);
    }
    fn on_select_word_right(
        &mut self,
        _: &SelectWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_word(true, true, cx);
    }

    fn on_copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_plain() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn on_cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        self.on_copy(&Copy, window, cx);
        self.delete_back(cx);
    }

    fn on_paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if let Some(entries) = item.entries().first() {
            match entries {
                gpui::ClipboardEntry::String(s) => {
                    let text = s.text().clone();
                    if block_markdown::is_url(text.trim()) {
                        self.paste_url(text.trim().to_string(), cx);
                    } else {
                        self.insert_text(&text, cx);
                    }
                }
                gpui::ClipboardEntry::Image(img) => {
                    let Some(store) = image::store(cx) else {
                        return;
                    };
                    let Some(url) = store(image::Source::Bytes(img), cx) else {
                        return;
                    };
                    let id = self.selection.head().id.clone();
                    self.apply(
                        BlockOp::SetType {
                            id: id.clone(),
                            kind: BlockType::Image,
                        },
                        cx,
                    );
                    self.apply(
                        BlockOp::SetProp {
                            id,
                            key: "url",
                            value: LwwValue::String(url),
                        },
                        cx,
                    );
                }
                gpui::ClipboardEntry::ExternalPaths(_) => {}
            }
        }
    }

    fn selected_plain(&self) -> Option<String> {
        if self.selection.is_collapsed() {
            return None;
        }
        let (start, end) = self.selection.ordered();
        if start.id != end.id {
            return Some(self.project_markdown());
        }
        let plain = self
            .snapshots()
            .iter()
            .find(|s| s.id == start.id)?
            .plain
            .clone();
        let a = start.offset.min(end.offset);
        let b = start.offset.max(end.offset);
        plain.get(a..b).map(str::to_string)
    }

    fn on_kill_line(&mut self, _: &KillLine, _: &mut Window, cx: &mut Context<Self>) {
        let at = self.selection.head().clone();
        let plain = self
            .snapshots()
            .iter()
            .find(|s| s.id == at.id)
            .map(|s| s.plain.len())
            .unwrap_or(0);
        if at.offset < plain {
            self.apply(
                BlockOp::DeleteRange {
                    id: at.id,
                    start: at.offset,
                    end: plain,
                },
                cx,
            );
        }
    }

    fn on_delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.delete_word(false, cx);
    }
    fn on_delete_word_right(
        &mut self,
        _: &DeleteWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_word(true, cx);
    }
    fn on_delete_to_home(&mut self, _: &DeleteToHome, _: &mut Window, cx: &mut Context<Self>) {
        let at = self.selection.head().clone();
        if at.offset > 0 {
            self.apply(
                BlockOp::DeleteRange {
                    id: at.id.clone(),
                    start: 0,
                    end: at.offset,
                },
                cx,
            );
            self.selection = Selection::caret(Cursor::new(at.id, at.part, 0));
        }
    }

    fn on_move_block_up(&mut self, _: &MoveBlockUp, _: &mut Window, cx: &mut Context<Self>) {
        let id = self.selection.head().id.clone();
        self.move_block(id, -1, cx);
    }
    fn on_move_block_down(&mut self, _: &MoveBlockDown, _: &mut Window, cx: &mut Context<Self>) {
        let id = self.selection.head().id.clone();
        self.move_block(id, 1, cx);
    }
    fn on_duplicate_block(&mut self, _: &DuplicateBlock, _: &mut Window, cx: &mut Context<Self>) {
        let id = self.selection.head().id.clone();
        self.duplicate_block(id, cx);
    }
    fn on_remove_block(&mut self, _: &RemoveBlock, _: &mut Window, cx: &mut Context<Self>) {
        let id = self.selection.head().id.clone();
        self.remove_block(id, cx);
    }

    fn on_confirm_url(&mut self, _: &ConfirmUrl, _: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.url_prompt.take() else {
            return;
        };
        let url = prompt.value(cx).to_string();
        self.apply(
            BlockOp::SetProp {
                id: prompt.block_id,
                key: "url",
                value: LwwValue::String(url),
            },
            cx,
        );
    }

    fn on_cancel_url(&mut self, _: &CancelUrl, _: &mut Window, cx: &mut Context<Self>) {
        self.url_prompt = None;
        cx.notify();
    }
}

impl EventEmitter<EditorEvent> for Editor {}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        if focused {
            if self.blink.is_none() {
                self.start_blink(cx);
            }
        } else {
            self.blink = None;
        }

        let handle = self.focus_handle.clone();
        let entity = cx.entity();
        let input = canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                entity.update(cx, |this, _| {
                    this.origin = bounds.origin;
                    this.width = bounds.size.width;
                });
                window.handle_input(
                    &handle,
                    ElementInputHandler::new(bounds, entity.clone()),
                    cx,
                );
            },
        )
        .absolute()
        .size_full();

        let handle = self.focus_handle.clone().tab_stop(true);
        let selection = focused.then(|| self.selection.clone());
        let marked = self.marked.clone();
        let annotations = self.annotations_cache.clone();
        let placeholder: Option<SharedString> = Some(PLACEHOLDER.into());
        let editing = Editing {
            selection,
            caret_on: focused && self.caret_on,
            layouts: Some(&self.layouts),
            annotations: &annotations,
            placeholder,
            caption: Caption::Shown,
            marked: marked.as_ref(),
        };
        let body = render_with(self.snapshots(), editing, window, cx);

        div()
            .id("limen-block-editor")
            .key_context(keys::CONTEXT)
            .track_focus(&handle)
            .size_full()
            .relative()
            .font_family("Geist")
            .on_action(cx.listener(Self::on_backspace))
            .on_action(cx.listener(Self::on_delete))
            .on_action(cx.listener(Self::on_split))
            .on_action(cx.listener(Self::on_indent))
            .on_action(cx.listener(Self::on_outdent))
            .on_action(cx.listener(Self::on_dismiss))
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(Self::on_redo))
            .on_action(cx.listener(Self::on_toggle_bold))
            .on_action(cx.listener(Self::on_toggle_italic))
            .on_action(cx.listener(Self::on_toggle_strike))
            .on_action(cx.listener(Self::on_toggle_code))
            .on_action(cx.listener(Self::on_left))
            .on_action(cx.listener(Self::on_right))
            .on_action(cx.listener(Self::on_up))
            .on_action(cx.listener(Self::on_down))
            .on_action(cx.listener(Self::on_select_left))
            .on_action(cx.listener(Self::on_select_right))
            .on_action(cx.listener(Self::on_select_up))
            .on_action(cx.listener(Self::on_select_down))
            .on_action(cx.listener(Self::on_home))
            .on_action(cx.listener(Self::on_end))
            .on_action(cx.listener(Self::on_select_home))
            .on_action(cx.listener(Self::on_select_end))
            .on_action(cx.listener(Self::on_select_all))
            .on_action(cx.listener(Self::on_word_left))
            .on_action(cx.listener(Self::on_word_right))
            .on_action(cx.listener(Self::on_select_word_left))
            .on_action(cx.listener(Self::on_select_word_right))
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_cut))
            .on_action(cx.listener(Self::on_paste))
            .on_action(cx.listener(Self::on_kill_line))
            .on_action(cx.listener(Self::on_delete_word_left))
            .on_action(cx.listener(Self::on_delete_word_right))
            .on_action(cx.listener(Self::on_delete_to_home))
            .on_action(cx.listener(Self::on_move_block_up))
            .on_action(cx.listener(Self::on_move_block_down))
            .on_action(cx.listener(Self::on_duplicate_block))
            .on_action(cx.listener(Self::on_remove_block))
            .on_action(cx.listener(Self::on_confirm_url))
            .on_action(cx.listener(Self::on_cancel_url))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                    if std::mem::take(&mut this.press_claimed) {
                        return;
                    }
                    this.block_menu = None;
                    this.pasted = None;
                    this.focus_handle.clone().focus(window, cx);
                    let Some(hit) = this.layouts.hit(event.position) else {
                        return cx.notify();
                    };
                    // Empty image target
                    if this
                        .snapshots()
                        .iter()
                        .find(|s| s.id == hit.id)
                        .is_some_and(|s| {
                            s.block_type == BlockType::Image
                                && s.url.as_deref().is_none_or(|u| u.is_empty())
                        })
                        && this
                            .layouts
                            .picture_bounds(&hit.id)
                            .is_some_and(|b| b.contains(&event.position))
                    {
                        this.url_prompt = Some(Prompt::new(hit.id.clone(), window, cx));
                        this.press_claimed = true;
                        return cx.notify();
                    }
                    this.selection = match event.click_count {
                        _ if event.modifiers.shift => this.selection.extend_to(hit),
                        1 => Selection::caret(hit),
                        _ => Selection::caret(hit),
                    };
                    this.dragging = event.click_count == 1 && !event.modifiers.shift;
                    this.caret_moved();
                    if let Some(id) = this.comment_at(event.position) {
                        this.active_comment = Some(id.clone());
                        this.refresh_annotations();
                        cx.emit(EditorEvent::CommentActivated(id));
                    }
                    // Task checkbox
                    if this
                        .snapshots()
                        .iter()
                        .find(|s| s.id == this.selection.head().id)
                        .is_some_and(|s| s.block_type == BlockType::Task)
                    {
                        // Toggle when click is in marker zone — simplified: always allow via marker hit later
                    }
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.dragging = false;
                    if let Some((from, to)) = this.lifted.take() {
                        if from != to {
                            if let Some((ix, _)) = find_block(this.document.doc(), &to) {
                                this.apply(BlockOp::Move { id: from, to: ix }, cx);
                            }
                        } else {
                            // Release without move → block menu (deferred).
                        }
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                let over = this.layouts.over_text(event.position);
                if over != this.over_text {
                    this.over_text = over;
                    cx.notify();
                }
                if this.dragging {
                    if let Some(hit) = this.layouts.hit(event.position) {
                        this.selection = this.selection.extend_to(hit);
                        cx.notify();
                    }
                }
                if this.lifted.is_some() {
                    if let Some(id) = this.layouts.block_at(event.position) {
                        if let Some((_, to)) = this.lifted.as_mut() {
                            *to = id;
                            cx.notify();
                        }
                    }
                } else {
                    let hovered = this.layouts.block_at(event.position);
                    if hovered != this.hovered {
                        this.hovered = hovered;
                        cx.notify();
                    }
                }
            }))
            .child(input)
            .child(body)
            .children(self.block_handle(focused, cx))
            .children(self.language_chip(cx))
            .children(self.drop_indicator(cx))
            .children(self.slash_menu(window, cx))
            .children(self.paste_menu(window, cx))
            .children(self.language_menu(window, cx))
            .children(self.format_toolbar(window, cx))
            .children(self.url_prompt.as_ref().map(|p| {
                let origin = self
                    .layouts
                    .block_bounds(&p.block_id)
                    .map(|b| Point {
                        x: b.origin.x,
                        y: b.origin.y + b.size.height + px(4.0),
                    })
                    .unwrap_or(self.origin);
                p.paint(origin, cx)
            }))
    }
}

/// Start of the word before `offset` — Bezel `Cursor::word_left`.
#[must_use]
fn word_left_offset(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if offset == 0 {
        return 0;
    }
    let head = &text[..offset];
    let trimmed = head.trim_end_matches(|c: char| !c.is_alphanumeric());
    trimmed.trim_end_matches(char::is_alphanumeric).len()
}

/// End of the word at or after `offset` — Bezel `Cursor::word_right`.
#[must_use]
fn word_right_offset(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if offset >= text.len() {
        return text.len();
    }
    let tail = &text[offset..];
    let skipped = tail.len()
        - tail
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .len();
    let rest = &tail[skipped..];
    let word = rest.len() - rest.trim_start_matches(char::is_alphanumeric).len();
    offset + skipped + word
}

#[cfg(test)]
mod word_motion_tests {
    use super::{word_left_offset, word_right_offset};

    #[test]
    fn word_right_skips_punctuation_then_word() {
        assert_eq!(word_right_offset("hello, world", 5), 12);
        assert_eq!(word_right_offset("hello world", 0), 5);
    }

    #[test]
    fn word_left_backs_over_word() {
        assert_eq!(word_left_offset("hello world", 11), 6);
        assert_eq!(word_left_offset("hello world", 5), 0);
        assert_eq!(word_left_offset("hello", 5), 0);
    }
}
