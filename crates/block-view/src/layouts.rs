//! Where each block's text landed, recorded as it painted (Bezel `BlockLayouts`).
//!
//! Hit tests return [`Cursor`] with [`BlockId`], not a list index.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use std::{cell::RefCell, ops::Range, rc::Rc};

use block_markdown::BlockId;
use gpui::{Bounds, Pixels, Point, TextLayout, point};

use crate::types::{Cursor, Part};

/// Where each block's text landed, recorded as it painted.
#[derive(Clone, Default)]
pub struct BlockLayouts(Rc<RefCell<Frames>>);

#[derive(Default)]
struct Frames {
    texts: Vec<Painted>,
    blocks: Vec<(BlockId, Bounds<Pixels>)>,
    languages: Vec<(BlockId, Bounds<Pixels>)>,
    pictures: Vec<(BlockId, Bounds<Pixels>)>,
}

struct Painted {
    id: BlockId,
    part: Part,
    range: Range<usize>,
    layout: TextLayout,
}

impl BlockLayouts {
    /// The position under `point`.
    pub fn hit(&self, point: Point<Pixels>) -> Option<Cursor> {
        let entries = &self.0.borrow().texts;
        let cursor = |painted: &Painted| {
            let (Ok(offset) | Err(offset)) = painted.layout.index_for_position(point);
            Cursor::new(
                painted.id.clone(),
                painted.part,
                painted.range.start + offset.min(painted.range.len()),
            )
        };
        if let Some(painted) = entries
            .iter()
            .find(|painted| painted.layout.bounds().contains(&point))
        {
            return Some(cursor(painted));
        }
        entries
            .iter()
            .min_by_key(|painted| {
                let bounds = painted.layout.bounds();
                let above = (bounds.origin.y - point.y).abs();
                let below = (bounds.origin.y + bounds.size.height - point.y).abs();
                f32::from(above.min(below)) as i64
            })
            .map(cursor)
    }

    /// Where a position painted last frame, and how tall its line is.
    pub fn position(&self, at: &Cursor) -> Option<(Point<Pixels>, Pixels)> {
        let entries = &self.0.borrow().texts;
        let painted = entries.iter().find(|painted| {
            painted.id == at.id
                && painted.part == at.part
                && painted.range.start <= at.offset
                && at.offset <= painted.range.end
        })?;
        let point = painted
            .layout
            .position_for_index(at.offset - painted.range.start)?;
        Some((point, painted.layout.line_height()))
    }

    /// One painted row above or below `at`.
    pub fn step_row(
        &self,
        at: &Cursor,
        from: Point<Pixels>,
        down: bool,
    ) -> Option<(Cursor, Pixels)> {
        let entries = &self.0.borrow().texts;
        let ix = entries.iter().position(|painted| {
            painted.id == at.id
                && painted.part == at.part
                && painted.range.start <= at.offset
                && at.offset <= painted.range.end
        })?;
        let here = &entries[ix];
        let line = here.layout.line_height();
        let index_at = |painted: &Painted, y: Pixels| {
            let (Ok(offset) | Err(offset)) = painted.layout.index_for_position(point(from.x, y));
            (
                Cursor::new(
                    painted.id.clone(),
                    painted.part,
                    painted.range.start + offset.min(painted.range.len()),
                ),
                y,
            )
        };

        let bounds = here.layout.bounds();
        let target = if down { from.y + line } else { from.y - line };
        if target >= bounds.origin.y && target < bounds.origin.y + bounds.size.height {
            return Some(index_at(here, target));
        }

        let next = match down {
            true => entries.get(ix + 1)?,
            false => entries.get(ix.checked_sub(1)?)?,
        };
        let bounds = next.layout.bounds();
        let row = match down {
            true => bounds.origin.y,
            false => bounds.origin.y + bounds.size.height - next.layout.line_height(),
        };
        Some(index_at(next, row))
    }

    /// Whether `point` is inside painted text.
    pub fn over_text(&self, point: Point<Pixels>) -> bool {
        self.0
            .borrow()
            .texts
            .iter()
            .any(|painted| painted.layout.bounds().contains(&point))
    }

    /// The block under `point`.
    pub fn block_at(&self, point: Point<Pixels>) -> Option<BlockId> {
        let blocks = &self.0.borrow().blocks;
        blocks
            .iter()
            .find(|(_, bounds)| bounds.contains(&point))
            .or_else(|| {
                blocks.iter().min_by_key(|(_, bounds)| {
                    let above = (bounds.origin.y - point.y).abs();
                    let below = (bounds.origin.y + bounds.size.height - point.y).abs();
                    f32::from(above.min(below)) as i64
                })
            })
            .map(|(id, _)| id.clone())
    }

    /// Where a block's first painted row sits, and how tall that row is.
    pub fn first_row(&self, id: &BlockId) -> Option<(Pixels, Pixels)> {
        let texts = &self.0.borrow().texts;
        let painted = texts.iter().find(|painted| &painted.id == id)?;
        Some((
            painted.layout.bounds().origin.y,
            painted.layout.line_height(),
        ))
    }

    /// Where a block painted last frame.
    pub fn block_bounds(&self, id: &BlockId) -> Option<Bounds<Pixels>> {
        self.0
            .borrow()
            .blocks
            .iter()
            .find(|(block, _)| block == id)
            .map(|(_, bounds)| *bounds)
    }

    /// Where a fenced block's language label painted.
    pub fn language_bounds(&self, id: &BlockId) -> Option<Bounds<Pixels>> {
        self.0
            .borrow()
            .languages
            .iter()
            .find(|(block, _)| block == id)
            .map(|(_, bounds)| *bounds)
    }

    /// Where an image block's picture painted.
    pub fn picture_bounds(&self, id: &BlockId) -> Option<Bounds<Pixels>> {
        self.0
            .borrow()
            .pictures
            .iter()
            .find(|(block, _)| block == id)
            .map(|(_, bounds)| *bounds)
    }

    pub(crate) fn record(&self, id: BlockId, part: Part, range: Range<usize>, layout: TextLayout) {
        self.0.borrow_mut().texts.push(Painted {
            id,
            part,
            range,
            layout,
        });
    }

    pub(crate) fn record_block(&self, id: BlockId, bounds: Bounds<Pixels>) {
        self.0.borrow_mut().blocks.push((id, bounds));
    }

    pub(crate) fn record_language(&self, id: BlockId, bounds: Bounds<Pixels>) {
        self.0.borrow_mut().languages.push((id, bounds));
    }

    pub(crate) fn record_picture(&self, id: BlockId, bounds: Bounds<Pixels>) {
        self.0.borrow_mut().pictures.push((id, bounds));
    }

    pub(crate) fn clear(&self) {
        let mut frames = self.0.borrow_mut();
        frames.texts.clear();
        frames.blocks.clear();
        frames.languages.clear();
        frames.pictures.clear();
    }
}
