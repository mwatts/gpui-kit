//! Perform [`BlockOp`] mutations against a Loro document.

use block_markdown::{
    BlockId, BlockType, CommentState, alt_text, block_map_at, block_type_of, blocks_list,
    ensure_content, find_block, grow_for_code_spans, indent_of, insert_block_map, mark_covers,
    max_rank_in_range, repair_numbers, replay_delta_at, slice_delta_utf8, unmark_utf8,
};
use loro::{LoroDoc, LoroMap, LoroResult, LoroValue};

use crate::types::{ApplyResult, BlockOp, Cursor, LwwValue, Part, Selection};

pub fn perform(doc: &LoroDoc, op: BlockOp) -> LoroResult<ApplyResult> {
    match op {
        BlockOp::InsertText { id, offset, text } => {
            let map = require_block(doc, &id)?;
            let content = text_for_part(&map, Part::Body)?;
            content.insert_utf8(offset, &text)?;
            Ok(ApplyResult {
                selection: Some(Selection::caret(Cursor::new(
                    id,
                    Part::Body,
                    offset + text.len(),
                ))),
            })
        }
        BlockOp::DeleteRange { id, start, end } => {
            let map = require_block(doc, &id)?;
            let content = text_for_part(&map, Part::Body)?;
            if end > start {
                content.delete_utf8(start, end - start)?;
            }
            Ok(ApplyResult {
                selection: Some(Selection::caret(Cursor::new(id, Part::Body, start))),
            })
        }
        BlockOp::SplitBlock { id, offset } => split_block(doc, &id, offset),
        BlockOp::MergeWithPrevious { id } => merge_with_previous(doc, &id),
        BlockOp::DeleteBlock { id } => delete_block(doc, &id),
        BlockOp::DeleteCrossBlock { anchor, focus } => delete_cross_block(doc, &anchor, &focus),
        BlockOp::SetType { id, kind } => {
            let map = require_block(doc, &id)?;
            map.insert("type", kind.as_str())?;
            if kind == BlockType::Ordered && map_missing_number(&map) {
                map.insert("number", 1i64)?;
            }
            if kind == BlockType::Task {
                let _ = map.insert("checked", false);
            }
            let _ = ensure_content(&map);
            repair_numbers(doc);
            Ok(ApplyResult::default())
        }
        BlockOp::ToggleMark {
            id,
            start,
            end,
            mark,
        } => toggle_mark(doc, &id, start, end, mark),
        BlockOp::SetLink {
            id,
            start,
            end,
            url,
        } => {
            let map = require_block(doc, &id)?;
            let content = text_for_part(&map, Part::Body)?;
            content.mark_utf8(start..end, "link", url.as_str())?;
            Ok(ApplyResult::default())
        }
        BlockOp::RemoveLink { id, start, end } => {
            let map = require_block(doc, &id)?;
            let content = text_for_part(&map, Part::Body)?;
            unmark_utf8(&content, start..end, "link")?;
            Ok(ApplyResult::default())
        }
        BlockOp::Indent { id } => {
            let (ix, map) = find_block(doc, &id)
                .ok_or_else(|| loro::LoroError::NotFoundError("block".into()))?;
            if ix == 0 {
                return Ok(ApplyResult::default());
            }
            let list = blocks_list(doc);
            let prev = block_map_at(&list, ix - 1)
                .ok_or_else(|| loro::LoroError::NotFoundError("prev".into()))?;
            let under_list = block_type_of(&prev).is_list_marker();
            if !under_list && indent_of(&map) == 0 {
                // Bezel: deeper only under bullet/number/task — still allow +1 under any
                // when already nested, via repair_indent clamp.
            }
            let indent = indent_of(&map);
            let max = indent_of(&prev) + 1;
            if under_list || indent < max {
                map.insert("indent", (indent + 1).min(max))?;
            }
            // Indent children with parent.
            let end = subtree_end(doc, ix);
            for i in ix + 1..end {
                if let Some(child) = block_map_at(&list, i) {
                    let c = indent_of(&child);
                    let _ = child.insert("indent", c + 1);
                }
            }
            repair_indent(doc);
            Ok(ApplyResult::default())
        }
        BlockOp::Outdent { id } => {
            let map = require_block(doc, &id)?;
            let indent = indent_of(&map);
            if indent > 0 {
                map.insert("indent", indent - 1)?;
            }
            // Outdent children that were deeper.
            outdent_children(doc, &id);
            repair_indent(doc);
            Ok(ApplyResult::default())
        }
        BlockOp::Move { id, to } => {
            let (from, _) = find_block(doc, &id)
                .ok_or_else(|| loro::LoroError::NotFoundError("block".into()))?;
            let list = blocks_list(doc);
            let end = subtree_end(doc, from);
            let len = end - from;
            // MovableList move: move each item.
            if to != from {
                // Move the contiguous subtree.
                let dest = if to > from {
                    to.saturating_sub(len)
                } else {
                    to
                };
                for i in 0..len {
                    list.mov(from + (if to > from { 0 } else { len - 1 - i }), dest + i)?;
                }
            }
            repair_indent(doc);
            repair_numbers(doc);
            Ok(ApplyResult::default())
        }
        BlockOp::ToggleCheck { id } => {
            let map = require_block(doc, &id)?;
            let checked = map
                .get("checked")
                .and_then(|v| match v {
                    loro::ValueOrContainer::Value(LoroValue::Bool(b)) => Some(b),
                    _ => None,
                })
                .unwrap_or(false);
            map.insert("checked", !checked)?;
            Ok(ApplyResult::default())
        }
        BlockOp::SetProp { id, key, value } => {
            let map = require_block(doc, &id)?;
            match value {
                LwwValue::Null => {
                    map.delete(key)?;
                }
                LwwValue::Bool(b) => {
                    map.insert(key, b)?;
                }
                LwwValue::I64(n) => {
                    map.insert(key, n)?;
                }
                LwwValue::String(s) => {
                    map.insert(key, s.as_str())?;
                }
            }
            Ok(ApplyResult::default())
        }
        BlockOp::UnwrapToParagraph { id } => {
            let map = require_block(doc, &id)?;
            map.insert("type", BlockType::Paragraph.as_str())?;
            map.delete("number")?;
            map.delete("checked")?;
            map.delete("language")?;
            let _ = ensure_content(&map);
            repair_numbers(doc);
            Ok(ApplyResult {
                selection: Some(Selection::caret(Cursor::new(id, Part::Body, 0))),
            })
        }
        BlockOp::AddComment { id, range, body } => {
            let comments = block_markdown::comments_map(doc);
            let entry = comments.ensure_mergeable_map(id.as_str())?;
            entry.insert("body", body.as_str())?;
            entry.insert("state", CommentState::Open.as_str())?;
            let block_id = range.focus.id.clone();
            let start = range.anchor.offset.min(range.focus.offset);
            let end = range.anchor.offset.max(range.focus.offset);
            if start < end {
                let map = require_block(doc, &block_id)?;
                let content = text_for_part(&map, range.focus.part)?;
                content.mark_utf8(start..end, "comment", id.as_str())?;
            }
            Ok(ApplyResult::default())
        }
        BlockOp::SetCommentBody { id, body } => {
            let comments = block_markdown::comments_map(doc);
            if let Some(loro::ValueOrContainer::Container(loro::Container::Map(entry))) =
                comments.get(id.as_str())
            {
                entry.insert("body", body.as_str())?;
            }
            Ok(ApplyResult::default())
        }
        BlockOp::SetCommentState { id, state } => {
            let comments = block_markdown::comments_map(doc);
            if let Some(loro::ValueOrContainer::Container(loro::Container::Map(entry))) =
                comments.get(id.as_str())
            {
                entry.insert("state", state.as_str())?;
            }
            Ok(ApplyResult::default())
        }
        BlockOp::DeleteComment { id } => {
            let comments = block_markdown::comments_map(doc);
            comments.delete(id.as_str())?;
            Ok(ApplyResult::default())
        }
        BlockOp::ImeCommit {
            id,
            offset,
            replace_len,
            text,
        } => {
            let map = require_block(doc, &id)?;
            let content = text_for_part(&map, Part::Body)?;
            if replace_len > 0 {
                content.delete_utf8(offset, replace_len)?;
            }
            content.insert_utf8(offset, &text)?;
            Ok(ApplyResult {
                selection: Some(Selection::caret(Cursor::new(
                    id,
                    Part::Body,
                    offset + text.len(),
                ))),
            })
        }
    }
}

fn map_missing_number(map: &LoroMap) -> bool {
    map.get("number").is_none()
}

fn require_block(doc: &LoroDoc, id: &BlockId) -> LoroResult<LoroMap> {
    find_block(doc, id)
        .map(|(_, m)| m)
        .ok_or_else(|| loro::LoroError::NotFoundError(format!("block {}", id).into()))
}

fn text_for_part(map: &LoroMap, part: Part) -> LoroResult<loro::LoroText> {
    match part {
        Part::Caption => alt_text(map)
            .or_else(|| ensure_content(map).ok())
            .ok_or_else(|| loro::LoroError::NotFoundError("alt".into())),
        Part::Body | Part::Code => ensure_content(map),
        Part::Cell { .. } => ensure_content(map),
    }
}

fn split_block(doc: &LoroDoc, id: &BlockId, offset: usize) -> LoroResult<ApplyResult> {
    let (ix, map) =
        find_block(doc, id).ok_or_else(|| loro::LoroError::NotFoundError("block".into()))?;
    let kind = block_type_of(&map);
    let content = ensure_content(&map)?;
    let len = content.len_utf8();
    let offset = offset.min(len);
    let tail = slice_delta_utf8(&content, offset, len)?;
    if offset < len {
        content.delete_utf8(offset, len - offset)?;
    }

    let list = blocks_list(doc);
    let insert_at = subtree_end(doc, ix);
    let new_id = BlockId::new();
    let new_kind = match kind {
        BlockType::Heading { .. } | BlockType::Quote | BlockType::Code => BlockType::Paragraph,
        other => other,
    };
    let indent = indent_of(&map);
    let new_map = insert_block_map(&list, insert_at, &new_id, new_kind, indent)?;
    if new_kind == BlockType::Ordered {
        let number = map
            .get("number")
            .and_then(|v| match v {
                loro::ValueOrContainer::Value(LoroValue::I64(n)) => Some(n),
                _ => None,
            })
            .unwrap_or(1);
        new_map.insert("number", number)?;
    }
    if new_kind == BlockType::Task {
        let checked = map
            .get("checked")
            .and_then(|v| match v {
                loro::ValueOrContainer::Value(LoroValue::Bool(b)) => Some(b),
                _ => None,
            })
            .unwrap_or(false);
        new_map.insert("checked", checked)?;
    }
    let new_text = ensure_content(&new_map)?;
    replay_delta_at(&new_text, 0, &tail)?;
    repair_numbers(doc);
    Ok(ApplyResult {
        selection: Some(Selection::caret(Cursor::new(new_id, Part::Body, 0))),
    })
}

fn merge_with_previous(doc: &LoroDoc, id: &BlockId) -> LoroResult<ApplyResult> {
    let (ix, map) =
        find_block(doc, id).ok_or_else(|| loro::LoroError::NotFoundError("block".into()))?;
    if ix == 0 {
        return Ok(ApplyResult::default());
    }
    let list = blocks_list(doc);
    let prev =
        block_map_at(&list, ix - 1).ok_or_else(|| loro::LoroError::NotFoundError("prev".into()))?;
    let prev_text = ensure_content(&prev)?;
    let caret = prev_text.len_utf8();
    let cur_text = ensure_content(&map)?;
    let delta = slice_delta_utf8(&cur_text, 0, cur_text.len_utf8())?;
    replay_delta_at(&prev_text, caret, &delta)?;
    let prev_id = BlockId(block_markdown::map_string(&prev, "id").unwrap_or_default());
    list.delete(ix, 1)?;
    // Keep at least one block.
    if list.len() == 0 {
        let _ = insert_block_map(&list, 0, &BlockId::new(), BlockType::Paragraph, 0);
    }
    repair_numbers(doc);
    Ok(ApplyResult {
        selection: Some(Selection::caret(Cursor::new(prev_id, Part::Body, caret))),
    })
}

fn delete_block(doc: &LoroDoc, id: &BlockId) -> LoroResult<ApplyResult> {
    let (ix, _) =
        find_block(doc, id).ok_or_else(|| loro::LoroError::NotFoundError("block".into()))?;
    let list = blocks_list(doc);
    if list.len() <= 1 {
        // Clear last block to empty paragraph.
        let map =
            block_map_at(&list, 0).ok_or_else(|| loro::LoroError::NotFoundError("block".into()))?;
        map.insert("type", BlockType::Paragraph.as_str())?;
        map.delete("number")?;
        map.delete("checked")?;
        map.delete("language")?;
        map.delete("url")?;
        if let Ok(text) = ensure_content(&map) {
            if text.len_utf8() > 0 {
                text.delete_utf8(0, text.len_utf8())?;
            }
        }
        return Ok(ApplyResult::default());
    }
    let end = subtree_end(doc, ix);
    list.delete(ix, end - ix)?;
    repair_indent(doc);
    repair_numbers(doc);
    Ok(ApplyResult::default())
}

fn delete_cross_block(doc: &LoroDoc, anchor: &Cursor, focus: &Cursor) -> LoroResult<ApplyResult> {
    // Minimal: delete within single block if same id; else delete intervening.
    if anchor.id == focus.id {
        let start = anchor.offset.min(focus.offset);
        let end = anchor.offset.max(focus.offset);
        return perform(
            doc,
            BlockOp::DeleteRange {
                id: anchor.id.clone(),
                start,
                end,
            },
        );
    }
    Ok(ApplyResult::default())
}

fn toggle_mark(
    doc: &LoroDoc,
    id: &BlockId,
    start: usize,
    end: usize,
    mark: &str,
) -> LoroResult<ApplyResult> {
    if start >= end {
        return Ok(ApplyResult::default());
    }
    let map = require_block(doc, id)?;
    let content = ensure_content(&map)?;
    let mut range = grow_for_code_spans(&content, start, end);

    let is_emphasis = matches!(mark, "bold" | "italic" | "strike");
    if mark_covers(&content, mark, range.start, range.end) {
        unmark_utf8(&content, range.clone(), mark)?;
    } else if is_emphasis {
        let rank = max_rank_in_range(&content, mark, range.start, range.end) + 1;
        // Grow again after potential rank scan (same range).
        range = grow_for_code_spans(&content, range.start, range.end);
        content.mark_utf8(range, mark, rank)?;
    } else if mark == "code" {
        content.mark_utf8(range, mark, true)?;
        // Bezel: applying code grows other marks to cover the code span.
        grow_emphasis_over_code(doc, id)?;
    } else if mark == "comment" {
        // value supplied by AddComment
        content.mark_utf8(range, mark, true)?;
    } else {
        content.mark_utf8(range, mark, true)?;
    }
    Ok(ApplyResult::default())
}

fn grow_emphasis_over_code(doc: &LoroDoc, id: &BlockId) -> LoroResult<()> {
    let map = require_block(doc, id)?;
    let content = ensure_content(&map)?;
    let plain_len = content.len_utf8();
    let codes = block_markdown::code_spans_touching(&content, 0, plain_len);
    if codes.is_empty() {
        return Ok(());
    }
    // Re-read delta; for each non-code mark that half-covers a code span, remake grown.
    // Simpler approach: for each code span, if any emphasis attrs exist on a neighbouring
    // half, expand by re-marking.
    let delta = content.to_delta();
    let (plain, marks) = block_markdown::marks_from_delta(&delta);
    let _ = plain;
    for (range, mark) in marks {
        let key = match &mark {
            block_markdown::RichMark::Bold(_) => "bold",
            block_markdown::RichMark::Italic(_) => "italic",
            block_markdown::RichMark::Strike(_) => "strike",
            block_markdown::RichMark::Link(_) => "link",
            _ => continue,
        };
        for code in &codes {
            let crosses = (range.start > code.start && range.start < code.end)
                || (range.end > code.start && range.end < code.end);
            if crosses {
                let grown = range.start.min(code.start)..range.end.max(code.end);
                match &mark {
                    block_markdown::RichMark::Bold(r) => {
                        content.mark_utf8(grown, key, *r)?;
                    }
                    block_markdown::RichMark::Italic(r) => {
                        content.mark_utf8(grown, key, *r)?;
                    }
                    block_markdown::RichMark::Strike(r) => {
                        content.mark_utf8(grown, key, *r)?;
                    }
                    block_markdown::RichMark::Link(url) => {
                        content.mark_utf8(grown, key, url.as_str())?;
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn subtree_end(doc: &LoroDoc, ix: usize) -> usize {
    let list = blocks_list(doc);
    let Some(base) = block_map_at(&list, ix).map(|m| indent_of(&m)) else {
        return ix;
    };
    let mut end = ix + 1;
    while end < list.len() && block_map_at(&list, end).is_some_and(|m| indent_of(&m) > base) {
        end += 1;
    }
    end
}

fn outdent_children(doc: &LoroDoc, id: &BlockId) {
    let Some((ix, _)) = find_block(doc, id) else {
        return;
    };
    let list = blocks_list(doc);
    let end = subtree_end(doc, ix);
    for i in ix + 1..end {
        if let Some(map) = block_map_at(&list, i) {
            let indent = indent_of(&map);
            if indent > 0 {
                let _ = map.insert("indent", indent - 1);
            }
        }
    }
}

fn repair_indent(doc: &LoroDoc) {
    let list = blocks_list(doc);
    for i in 0..list.len() {
        let Some(map) = block_map_at(&list, i) else {
            continue;
        };
        let max = if i == 0 {
            0
        } else {
            block_map_at(&list, i - 1)
                .map(|m| indent_of(&m) + 1)
                .unwrap_or(0)
        };
        let indent = indent_of(&map).clamp(0, max);
        // Deeper only under list markers.
        let indent = if i > 0
            && indent > 0
            && !block_map_at(&list, i - 1).is_some_and(|m| {
                let t = block_type_of(&m);
                t.is_list_marker() || indent_of(&m) >= indent
            }) {
            // Allow if previous indent is at least indent-1 (nested under any).
            indent.min(
                block_map_at(&list, i - 1)
                    .map(|m| indent_of(&m) + 1)
                    .unwrap_or(0),
            )
        } else {
            indent
        };
        let _ = map.insert("indent", indent);
    }
}
