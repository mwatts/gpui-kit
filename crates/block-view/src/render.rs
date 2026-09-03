//! [`BlockSnapshot`] → GPUI elements (Bezel `markdown/render.rs` against snapshots).
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use std::ops::Range;

use block_markdown::BlockType;
use gpui::{
    AnyElement, App, BorderStyle, Bounds, CursorStyle, ElementId, FontStyle, FontWeight, Hsla,
    InteractiveText, ObjectFit, Pixels, Point, SharedString, StrikethroughStyle, StyledImage as _,
    StyledText, TextLayout, TextRun, UnderlineStyle, Window, canvas, div, font, img, point,
    prelude::*, px, quad, size,
};
use loro::{LoroValue, TextDelta};

use crate::block_renderer;
use crate::layouts::BlockLayouts;
use crate::paint::EditorPalette;
use crate::preview;
use crate::types::{
    Align, Annotation, BULLET_DISC_PX, BlockSnapshot, CARET_WIDTH_PX, Caption, Cursor, Editing,
    Part, QUOTE_BAR_PX, Selection, TASK_BOX_PX, TableData,
};
use crate::typography::Typography;

// Fix the typo - I used BLOCK_DISC_PX incorrectly. Should only be BULLET_DISC_PX.
const BLOCK_GAP: f32 = 12.0;
const LIST_GAP: f32 = 4.0;
const INDENT_WIDTH: f32 = 22.0;
const MARKER_WIDTH: f32 = 18.0;
const MARKER_GAP: f32 = 8.0;
const CODE_PADDING_X: f32 = 12.0;
const CODE_PADDING_Y: f32 = 10.0;
pub const PLAIN_LANGUAGE: &str = "Plain";
const INLINE_CODE_RADIUS: f32 = 4.5;
const INLINE_CODE_PAD_X: f32 = 2.0;
const INLINE_CODE_INSET_Y: f32 = 2.0;
const CHIP_PAD_X: f32 = 4.0;
const CHIP_INSET_Y: f32 = 1.0;
const CHIP_BLOCK_PAD_X: f32 = 8.0;
const CHIP_BLOCK_PAD_Y: f32 = 3.0;
const CHIP_ICON: f32 = 15.0;
const CARD_HEIGHT: f32 = 116.0;
const CARD_IMAGE_WIDTH: f32 = 180.0;
const CARD_COVER_HEIGHT: f32 = 200.0;
const CARD_PADDING: f32 = 14.0;
const CARD_BORDER: f32 = 1.0;
const CARD_ICON: f32 = 16.0;
const CARD_COVER: f32 = 44.0;
const IMAGE_EMPTY_HEIGHT: f32 = 52.0;
const CAPTION_GAP: f32 = 4.0;
const IMAGE_EMPTY: &str = "Add an image";
const CAPTION_HINT: &str = "Write a caption";
const TABLE_CELL_PADDING: f32 = 12.0;
const TABLE_DIVIDER: f32 = 1.0;
const TABLE_MIN_COLUMN_CONTENT: f32 = 48.0;
const TABLE_MIN_COLUMN_WIDTH: f32 = 96.0;
const CONTROL_RADIUS: f32 = 6.0;
const BUTTON_RADIUS: f32 = 8.0;
const PANEL_RADIUS: f32 = 8.0;

/// Overlay state for one block during paint.
#[derive(Clone, Copy)]
struct Overlay<'a> {
    block_ix: usize,
    block_id: &'a block_markdown::BlockId,
    part: Part,
    selection: Option<&'a Selection>,
    caret_on: bool,
    layouts: Option<&'a BlockLayouts>,
    annotations: &'a [(Selection, Annotation)],
    placeholder: Option<&'a SharedString>,
    caption: Caption,
    /// Document order index lookup for selection clipping.
    order: &'a dyn Fn(&block_markdown::BlockId) -> Option<usize>,
}

impl<'a> Overlay<'a> {
    fn at(self, part: Part) -> Self {
        Self { part, ..self }
    }

    fn caret_painted(&self) -> Option<usize> {
        self.caret_on.then(|| self.caret()).flatten()
    }

    fn caret(&self) -> Option<usize> {
        self.selection
            .map(|s| s.head())
            .filter(|head| &head.id == self.block_id && head.part == self.part)
            .map(|head| head.offset)
    }

    fn selected(&self, len: usize) -> Option<Range<usize>> {
        self.clip(self.selection?, len)
    }

    fn annotated(&self, len: usize, palette: &EditorPalette) -> Vec<(Range<usize>, Hsla)> {
        self.annotations
            .iter()
            .filter_map(|(range, kind)| {
                Some((self.clip(range, len)?, palette.annotation_wash(*kind)))
            })
            .collect()
    }

    fn clip(&self, selection: &Selection, len: usize) -> Option<Range<usize>> {
        if selection.is_collapsed() {
            return None;
        }
        let (start, end) = ordered(selection, self.order)?;
        let here_ix = (self.order)(self.block_id)?;
        let start_ix = (self.order)(&start.id)?;
        let end_ix = (self.order)(&end.id)?;

        let here_key = (here_ix, self.part);
        let first = (start_ix, start.part);
        let last = (end_ix, end.part);
        if here_key < first || here_key > last {
            return None;
        }
        let from = if here_key == first { start.offset } else { 0 };
        let to = if here_key == last { end.offset } else { len };
        (from < to).then_some(from..to.min(len))
    }

    fn covers_block(&self) -> bool {
        let Some(selection) = self.selection.filter(|s| !s.is_collapsed()) else {
            return false;
        };
        let Some((start, end)) = ordered(selection, self.order) else {
            return false;
        };
        let Some(here) = (self.order)(self.block_id) else {
            return false;
        };
        let Some(start_ix) = (self.order)(&start.id) else {
            return false;
        };
        let Some(end_ix) = (self.order)(&end.id) else {
            return false;
        };
        start_ix < here && here < end_ix
    }
}

fn ordered<'a>(
    selection: &'a Selection,
    order: &dyn Fn(&block_markdown::BlockId) -> Option<usize>,
) -> Option<(&'a Cursor, &'a Cursor)> {
    let a = (order)(&selection.anchor.id)?;
    let b = (order)(&selection.focus.id)?;
    let ak = (a, selection.anchor.part, selection.anchor.offset);
    let bk = (b, selection.focus.part, selection.focus.offset);
    if ak <= bk {
        Some((&selection.anchor, &selection.focus))
    } else {
        Some((&selection.focus, &selection.anchor))
    }
}

/// Render snapshots read-only.
pub fn render(snapshots: &[BlockSnapshot], window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(snapshots, Editing::default(), window, cx)
}

/// Render snapshots with an editing overlay.
pub fn render_with(
    snapshots: &[BlockSnapshot],
    editing: Editing<'_>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Editing {
        selection,
        caret_on,
        layouts,
        annotations,
        placeholder,
        caption,
        marked: _,
    } = editing;

    let reset = layouts.map(|layouts| {
        let layouts = layouts.clone();
        canvas(move |_, _, _| layouts.clear(), |_, _, _, _| ())
            .absolute()
            .size(px(0.0))
    });

    let palette = EditorPalette::from_app(cx);
    let typography = Typography::of(cx);
    let order_map: Vec<_> = snapshots.iter().map(|s| s.id.clone()).collect();
    let order = move |id: &block_markdown::BlockId| order_map.iter().position(|x| x == id);

    let mut column = div().flex().flex_col().children(reset);

    for (ix, block) in snapshots.iter().enumerate() {
        let gap = match snapshots.get(ix.wrapping_sub(1)) {
            None => 0.0,
            Some(previous) if tight(previous, block) => LIST_GAP,
            Some(_) => BLOCK_GAP,
        };
        let overlay = Overlay {
            block_ix: ix,
            block_id: &block.id,
            part: Part::Body,
            selection: selection.as_ref(),
            caret_on,
            layouts,
            annotations,
            placeholder: placeholder.as_ref(),
            caption,
            order: &order,
        };
        let frame = layouts.map(|layouts| {
            let layouts = layouts.clone();
            let id = block.id.clone();
            canvas(
                move |bounds, _, _| layouts.record_block(id, bounds),
                |_, _, _, _| (),
            )
            .absolute()
            .size_full()
        });
        column = column.child(
            div()
                .mt(px(gap))
                .pl(px(block.indent as f32 * INDENT_WIDTH))
                .relative()
                .children(frame)
                .when(overlay.covers_block() && is_opaque(block), |el| {
                    el.rounded(px(4.0)).bg(palette.selection)
                })
                .child(block_element(
                    block,
                    overlay,
                    &typography,
                    &palette,
                    window,
                    cx,
                )),
        );
    }

    column.into_any_element()
}

fn tight(previous: &BlockSnapshot, next: &BlockSnapshot) -> bool {
    let marker = |block: &BlockSnapshot| {
        matches!(
            block.block_type,
            BlockType::Bullet | BlockType::Ordered | BlockType::Task
        )
    };
    marker(previous) && (marker(next) || next.indent > previous.indent)
}

fn is_opaque(block: &BlockSnapshot) -> bool {
    matches!(
        block.block_type,
        BlockType::Rule
            | BlockType::Image
            | BlockType::Bookmark
            | BlockType::Code
            | BlockType::Table
    )
}

fn block_element(
    block: &BlockSnapshot,
    overlay: Overlay<'_>,
    typography: &Typography,
    palette: &EditorPalette,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let body = overlay.at(Part::Body);
    match block.block_type {
        BlockType::Paragraph => text_element(
            &block.plain,
            &block.runs,
            typography.body.size(),
            typography.body.line_height(),
            FontWeight::NORMAL,
            body,
            palette,
        ),
        BlockType::Heading { level } => {
            let heading = typography.heading(level);
            text_element(
                &block.plain,
                &block.runs,
                heading.size(),
                heading.line_height(),
                heading.weight,
                body,
                palette,
            )
        }
        BlockType::Bullet => marker_row(
            disc(typography, palette),
            &block.plain,
            &block.runs,
            body,
            typography,
            palette,
        ),
        BlockType::Ordered => {
            let number = block.number.unwrap_or(1);
            marker_row(
                div()
                    .flex_none()
                    .w(px(MARKER_WIDTH))
                    .text_size(px(typography.body.size()))
                    .line_height(px(typography.body.line_height()))
                    .text_color(palette.text_muted)
                    .child(SharedString::from(format!("{number}.")))
                    .into_any_element(),
                &block.plain,
                &block.runs,
                body,
                typography,
                palette,
            )
        }
        BlockType::Task => marker_row(
            checkbox(block.checked.unwrap_or(false), typography, palette),
            &block.plain,
            &block.runs,
            body,
            typography,
            palette,
        ),
        BlockType::Quote => div()
            .border_l(px(QUOTE_BAR_PX))
            .border_color(palette.border_strong)
            .pl(px(12.0))
            .pr(px(10.0))
            .py(px(2.0))
            .text_color(palette.text_muted)
            .child(text_element(
                &block.plain,
                &block.runs,
                typography.body.size(),
                typography.body.line_height(),
                FontWeight::NORMAL,
                body,
                palette,
            ))
            .into_any_element(),
        BlockType::Code => {
            let overlay = overlay.at(Part::Code);
            let language = block.language.as_deref();
            let painted = overlay
                .caret()
                .is_none()
                .then(|| block_renderer::render(language, &block.plain, window, cx))
                .flatten();
            match painted {
                Some(element) => div()
                    .when(overlay.covers_block(), |el| {
                        el.rounded(px(4.0)).bg(palette.selection)
                    })
                    .child(element)
                    .into_any_element(),
                None => code_block(
                    language,
                    &block.plain,
                    overlay,
                    typography,
                    palette,
                    window,
                    cx,
                ),
            }
        }
        BlockType::Image => image_block(block, overlay, typography, palette),
        BlockType::Bookmark => bookmark(
            overlay.block_ix,
            block.url.as_deref().unwrap_or(""),
            block.form.unwrap_or_default(),
            typography,
            palette,
            cx,
        ),
        BlockType::Table => table_block(
            block.table.as_ref().unwrap_or(&TableData::default()),
            overlay,
            typography,
            palette,
            window,
        ),
        BlockType::Rule => div()
            .h(px(1.0))
            .w_full()
            .bg(palette.border)
            .into_any_element(),
    }
}

/// A real 5px disc rather than the "•" glyph.
fn disc(typography: &Typography, palette: &EditorPalette) -> AnyElement {
    div()
        .flex_none()
        .w(px(MARKER_WIDTH))
        .h(px(typography.body.line_height()))
        .flex()
        .items_center()
        .child(
            div()
                .ml(px(1.0))
                .w(px(BULLET_DISC_PX))
                .h(px(BULLET_DISC_PX))
                .rounded_full()
                .bg(palette.text_faint)
                .debug_selector(|| "bullet-disc".into()),
        )
        .into_any_element()
}

fn checkbox(checked: bool, typography: &Typography, palette: &EditorPalette) -> AnyElement {
    let mut box_ = div()
        .w(px(TASK_BOX_PX))
        .h(px(TASK_BOX_PX))
        .rounded(px(3.5))
        .border_1()
        .flex()
        .items_center()
        .justify_center()
        .debug_selector(|| "task-checkbox".into());
    box_ = if checked {
        box_.bg(palette.solid)
            .border_color(palette.solid)
            .text_size(px(10.0))
            .text_color(palette.on_solid)
            .child("✓")
    } else {
        box_.border_color(palette.border_strong)
    };

    div()
        .flex_none()
        .w(px(MARKER_WIDTH))
        .h(px(typography.body.line_height()))
        .flex()
        .items_center()
        .child(box_)
        .into_any_element()
}

fn marker_row(
    marker: AnyElement,
    plain: &str,
    runs: &[TextDelta],
    overlay: Overlay<'_>,
    typography: &Typography,
    palette: &EditorPalette,
) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .gap(px(MARKER_GAP))
        .child(marker)
        .child(div().flex_1().min_w_0().child(text_element(
            plain,
            runs,
            typography.body.size(),
            typography.body.line_height(),
            FontWeight::NORMAL,
            overlay,
            palette,
        )))
        .into_any_element()
}

/// Inline content flattened for shaping.
pub struct Flat {
    pub text: SharedString,
    pub runs: Vec<TextRun>,
    pub links: Vec<(Range<usize>, String)>,
    pub code: Vec<Range<usize>>,
    pub chips: Vec<Range<usize>>,
}

/// Marks from Loro delta → consecutive GPUI runs.
pub fn flatten(
    plain: &str,
    delta: &[TextDelta],
    base_weight: FontWeight,
    palette: &EditorPalette,
) -> Flat {
    // Build mark coverage from delta attributes.
    let mut cuts: Vec<usize> = vec![0, plain.len()];
    let mut pos = 0usize;
    let mut spans: Vec<(Range<usize>, String, LoroValue)> = Vec::new();
    for item in delta {
        let TextDelta::Insert { insert, attributes } = item else {
            continue;
        };
        let start = pos;
        pos += insert.len();
        let end = pos;
        if let Some(attrs) = attributes {
            for (key, value) in attrs {
                if matches!(value, LoroValue::Null) {
                    continue;
                }
                spans.push((start..end, key.clone(), value.clone()));
                cuts.push(start);
                cuts.push(end);
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts.retain(|c| *c <= plain.len());

    let mut runs = Vec::new();
    let mut links: Vec<(Range<usize>, String)> = Vec::new();
    let mut code: Vec<Range<usize>> = Vec::new();
    let mut chips: Vec<Range<usize>> = Vec::new();

    for pair in cuts.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start >= end {
            continue;
        }
        let covering = spans
            .iter()
            .filter(|(range, _, _)| range.start <= start && range.end >= end);

        let (mut bold, mut italic, mut mono, mut strike) = (false, false, false, false);
        let mut chip = false;
        let mut link = None;
        for (_, key, value) in covering {
            match key.as_str() {
                "bold" => bold = true,
                "italic" => italic = true,
                "strike" => strike = true,
                "code" => mono = true,
                "mention" => {
                    chip = true;
                    if let Some(s) = value.as_string() {
                        let url = s.split_once('|').map(|(_, u)| u).unwrap_or(s.as_str());
                        link = Some(url.to_string());
                    }
                }
                "link" | "image" => {
                    if let Some(s) = value.as_string() {
                        link = Some(s.to_string());
                    }
                }
                _ => {}
            }
        }

        if mono {
            match code.last_mut() {
                Some(range) if range.end == start => range.end = end,
                _ => code.push(start..end),
            }
        }
        if chip {
            match chips.last_mut() {
                Some(range) if range.end == start => range.end = end,
                _ => chips.push(start..end),
            }
        }
        if let Some(url) = &link {
            match links.last_mut() {
                Some((range, last)) if range.end == start && last == url => range.end = end,
                _ => links.push((start..end, url.clone())),
            }
        }

        let mut face = font(if mono {
            palette.font_mono.clone()
        } else {
            palette.font_sans.clone()
        });
        face.weight = if bold && base_weight.0 < FontWeight::SEMIBOLD.0 {
            FontWeight::SEMIBOLD
        } else {
            base_weight
        };
        face.style = if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };

        runs.push(TextRun {
            len: end - start,
            font: face,
            color: if mono {
                palette.code_text
            } else {
                palette.text
            },
            background_color: None,
            underline: (link.is_some() && !chip).then_some(UnderlineStyle {
                color: Some(palette.text_muted),
                thickness: px(1.0),
                wavy: false,
            }),
            strikethrough: strike.then_some(StrikethroughStyle {
                thickness: px(1.0),
                color: Some(palette.text_muted),
            }),
        });
    }

    if runs.is_empty() {
        let mut face = font(palette.font_sans.clone());
        face.weight = base_weight;
        runs.push(TextRun {
            len: plain.len(),
            font: face,
            color: palette.text,
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    }

    Flat {
        text: plain.to_string().into(),
        runs,
        links,
        code,
        chips,
    }
}

fn text_element(
    plain: &str,
    delta: &[TextDelta],
    size: f32,
    line_height: f32,
    weight: FontWeight,
    overlay: Overlay<'_>,
    palette: &EditorPalette,
) -> AnyElement {
    let flat = flatten(plain, delta, weight, palette);
    painted_text(flat, plain.len(), size, line_height, overlay, palette)
}

fn painted_text(
    flat: Flat,
    len: usize,
    size: f32,
    line_height: f32,
    overlay: Overlay<'_>,
    palette: &EditorPalette,
) -> AnyElement {
    let (id, part) = (overlay.block_id.clone(), overlay.part);
    let (caret, selected) = (overlay.caret_painted(), overlay.selected(len));
    let span = 0..len;
    let hint = overlay
        .placeholder
        .filter(|_| len == 0 && overlay.caret().is_some())
        .map(|hint| {
            div()
                .absolute()
                .text_color(palette.text_faint)
                .child(hint.clone())
        });
    let styled = StyledText::new(flat.text.clone()).with_runs(flat.runs);
    let layout = styled.layout().clone();

    let painted: AnyElement = if flat.links.is_empty() {
        styled.into_any_element()
    } else {
        let (ranges, urls): (Vec<_>, Vec<_>) = flat.links.into_iter().unzip();
        InteractiveText::new(
            ElementId::Name(format!("md-text-{}", overlay.block_ix).into()),
            styled,
        )
        .on_click(ranges, move |clicked, _window, cx| {
            if let Some(url) = urls.get(clicked) {
                cx.open_url(url);
            }
        })
        .into_any_element()
    };

    let wash = palette.code_wash;
    let code_ranges = flat.code;
    let chip_wash = palette.element_hover;
    let chip_edge = palette.border;
    let chip_ranges = flat.chips;
    let caret_color = palette.caret;
    let selection_color = palette.selection;
    let annotated = overlay.annotated(len, palette);
    let layouts = overlay.layouts.cloned();
    let underlay = canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            if let Some(layouts) = &layouts {
                layouts.record(id.clone(), part, span.clone(), layout.clone());
            }
            for (range, wash) in &annotated {
                for rect in range_rects(&layout, range, 0.0, 0.0) {
                    window.paint_quad(quad(
                        rect,
                        px(2.0),
                        *wash,
                        px(0.0),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
            }
            if let Some(range) = &selected {
                for rect in range_rects(&layout, range, 0.0, 0.0) {
                    window.paint_quad(quad(
                        rect,
                        px(2.0),
                        selection_color,
                        px(0.0),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
            }
            if let Some(offset) = caret
                && let Some(head) = layout.position_for_index(offset)
            {
                window.paint_quad(quad(
                    caret_quad(head, size, layout.line_height()),
                    px(0.0),
                    caret_color,
                    px(0.0),
                    gpui::transparent_black(),
                    BorderStyle::default(),
                ));
            }
            for range in &code_ranges {
                for rect in range_rects(&layout, range, INLINE_CODE_PAD_X, INLINE_CODE_INSET_Y) {
                    window.paint_quad(quad(
                        rect,
                        px(INLINE_CODE_RADIUS),
                        wash,
                        px(0.0),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
            }
            for range in &chip_ranges {
                for rect in range_rects(&layout, range, CHIP_PAD_X, CHIP_INSET_Y) {
                    window.paint_quad(quad(
                        rect,
                        px(CONTROL_RADIUS),
                        chip_wash,
                        px(1.0),
                        chip_edge,
                        BorderStyle::Solid,
                    ));
                }
            }
        },
    )
    .absolute()
    .size_full();

    div()
        .text_size(px(size))
        .line_height(px(line_height))
        .relative()
        .child(underlay)
        .children(hint)
        .child(painted)
        .into_any_element()
}

fn caret_quad(head: Point<Pixels>, size: f32, line_height: Pixels) -> Bounds<Pixels> {
    let inset = (line_height - px(size)) / 2.0;
    Bounds::new(
        head + point(px(0.0), inset),
        gpui::size(px(CARET_WIDTH_PX), px(size)),
    )
}

fn range_rects(
    layout: &TextLayout,
    range: &Range<usize>,
    pad_x: f32,
    inset_y: f32,
) -> Vec<Bounds<Pixels>> {
    let mut rects = Vec::new();
    let line_height = layout.line_height();
    let mut cursor = range.start;
    let mut guard = 0;
    while cursor < range.end && guard < 256 {
        guard += 1;
        let Some(head) = layout.position_for_index(cursor) else {
            break;
        };
        let (row_end, next) = match layout.position_for_index(range.end) {
            Some(tail) if tail.y == head.y => (range.end, range.end),
            _ => {
                let (mut low, mut high) = (cursor, range.end);
                while high - low > 1 {
                    let mid = low + (high - low) / 2;
                    match layout.position_for_index(mid) {
                        Some(probe) if probe.y == head.y => low = mid,
                        _ => high = mid,
                    }
                }
                (low, high)
            }
        };
        if let Some(tail) = layout.position_for_index(row_end)
            && tail.x > head.x
        {
            rects.push(Bounds::new(
                point(head.x - px(pad_x), head.y + px(inset_y)),
                size(
                    tail.x - head.x + px(2.0 * pad_x),
                    line_height - px(2.0 * inset_y),
                ),
            ));
        }
        cursor = next.max(cursor + 1);
    }
    rects
}

fn code_block(
    language: Option<&str>,
    code: &str,
    overlay: Overlay<'_>,
    typography: &Typography,
    palette: &EditorPalette,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let id = overlay.block_id.clone();
    let ix = overlay.block_ix;
    let spans = crate::highlight::spans(cx, language, code);
    let mono = font(palette.font_mono.clone());
    let run = |len: usize, color: Hsla| TextRun {
        len,
        font: mono.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut rows: Vec<(Range<usize>, TextLayout)> = Vec::new();
    let mut offset = 0usize;
    let lines: Vec<AnyElement> = code
        .split('\n')
        .map(|line| {
            let start = offset;
            offset += line.len() + 1;
            let mut runs = Vec::new();
            let mut pos = 0usize;
            if let Some(spans) = &spans {
                let end = start + line.len();
                for (range, color) in spans.iter().filter(|(r, _)| r.end > start && r.start < end) {
                    let s = range.start.clamp(start, end) - start;
                    let e = range.end.min(end) - start;
                    if s > pos {
                        runs.push(run(s - pos, palette.text));
                    }
                    runs.push(run(e - s, *color));
                    pos = e;
                }
            }
            if pos < line.len() {
                runs.push(run(line.len() - pos, palette.text));
            }
            if runs.is_empty() {
                runs.push(run(0, palette.text));
            }
            let styled = StyledText::new(SharedString::from(line.to_string())).with_runs(runs);
            rows.push((start..start + line.len(), styled.layout().clone()));
            styled.into_any_element()
        })
        .collect();

    let caret = overlay.caret_painted();
    let selected = overlay.selected(code.len());
    let sink = overlay.layouts.cloned();
    let code_size = typography.code.size();
    let annotated = overlay.annotated(code.len(), palette);
    let (caret_color, selection_color) = (palette.caret, palette.selection);
    let record_id = id.clone();
    let underlay = canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            for (span, layout) in &rows {
                if let Some(sink) = &sink {
                    sink.record(record_id.clone(), Part::Code, span.clone(), layout.clone());
                }
                for (range, wash) in &annotated {
                    let (from, to) = (range.start.max(span.start), range.end.min(span.end));
                    if from < to {
                        for rect in
                            range_rects(layout, &(from - span.start..to - span.start), 0.0, 0.0)
                        {
                            window.paint_quad(quad(
                                rect,
                                px(2.0),
                                *wash,
                                px(0.0),
                                gpui::transparent_black(),
                                BorderStyle::default(),
                            ));
                        }
                    }
                }
                if let Some(range) = &selected {
                    let (from, to) = (range.start.max(span.start), range.end.min(span.end));
                    if from < to {
                        for rect in
                            range_rects(layout, &(from - span.start..to - span.start), 0.0, 0.0)
                        {
                            window.paint_quad(quad(
                                rect,
                                px(2.0),
                                selection_color,
                                px(0.0),
                                gpui::transparent_black(),
                                BorderStyle::default(),
                            ));
                        }
                    }
                }
                if let Some(offset) = caret.filter(|at| span.contains(at) || *at == span.end)
                    && let Some(head) = layout.position_for_index(offset - span.start)
                {
                    window.paint_quad(quad(
                        caret_quad(head, code_size, layout.line_height()),
                        px(0.0),
                        caret_color,
                        px(0.0),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
            }
        },
    )
    .absolute()
    .size_full();

    let lang_id = id.clone();
    div()
        .rounded(px(PANEL_RADIUS))
        .bg(palette.ink(0.035))
        .border_1()
        .border_color(palette.border)
        .overflow_hidden()
        .relative()
        .child(
            div()
                .relative()
                .flex()
                .flex_row()
                .items_center()
                .px(px(CODE_PADDING_X))
                .py(px(5.0))
                .border_b_1()
                .border_color(palette.border)
                .bg(palette.ink(0.02))
                .text_size(px(11.5))
                .text_color(match language {
                    Some(_) => palette.text_muted,
                    None => palette.text_faint,
                })
                .child(
                    div()
                        .relative()
                        .children(overlay.layouts.map(|layouts| {
                            let layouts = layouts.clone();
                            canvas(
                                move |bounds, _, _| layouts.record_language(lang_id, bounds),
                                |_, _, _, _| (),
                            )
                            .absolute()
                            .size_full()
                        }))
                        .child(SharedString::from(
                            language.unwrap_or(PLAIN_LANGUAGE).to_string(),
                        )),
                ),
        )
        .child(
            div()
                .id(ElementId::Name(format!("md-code-{ix}").into()))
                .overflow_x_scroll()
                .restrict_scroll_to_axis()
                .relative()
                .px(px(CODE_PADDING_X))
                .py(px(CODE_PADDING_Y))
                .text_size(px(typography.code.size()))
                .line_height(px(typography.code.line_height()))
                .whitespace_nowrap()
                .child(underlay)
                .children(lines),
        )
        .child(copy_button(code, ix, palette, window, cx))
        .into_any_element()
}

fn copy_button(
    code: &str,
    ix: usize,
    palette: &EditorPalette,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let copied = window.use_keyed_state(
        ElementId::Name(format!("md-copied-{ix}").into()),
        cx,
        |_, _| false,
    );
    let showing = *copied.read(cx);
    let text: SharedString = code.to_string().into();

    div()
        .id(ElementId::Name(format!("md-copy-{ix}").into()))
        .absolute()
        .top(px(3.0))
        .right(px(5.0))
        .h(px(20.0))
        .px(px(6.0))
        .rounded(px(5.0))
        .flex()
        .items_center()
        .cursor_pointer()
        .text_size(px(11.0))
        .text_color(palette.text_muted)
        .hover(|el| el.bg(palette.ink(0.08)))
        .child(if showing {
            crate::icons::check().text_color(palette.text_muted)
        } else {
            crate::icons::copy().text_color(palette.text_muted)
        })
        .on_click({
            let copied = copied.clone();
            move |_, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
                copied.update(cx, |state, cx| {
                    *state = true;
                    cx.notify();
                });
            }
        })
        .on_hover(move |hovering, _, cx| {
            if !*hovering && *copied.read(cx) {
                copied.update(cx, |state, cx| {
                    *state = false;
                    cx.notify();
                });
            }
        })
        .into_any_element()
}

fn image_block(
    block: &BlockSnapshot,
    overlay: Overlay<'_>,
    typography: &Typography,
    palette: &EditorPalette,
) -> AnyElement {
    let hint = SharedString::new_static(CAPTION_HINT);
    let overlay = Overlay {
        placeholder: Some(&hint),
        ..overlay.at(Part::Caption)
    };
    let url = block.url.as_deref().unwrap_or("");
    let picture = if url.is_empty() {
        div()
            .h(px(IMAGE_EMPTY_HEIGHT))
            .flex()
            .items_center()
            .px(px(CARD_PADDING))
            .rounded(px(BUTTON_RADIUS))
            .border_1()
            .border_dashed()
            .border_color(palette.border)
            .text_size(px(typography.body.size()))
            .text_color(palette.text_muted)
            .debug_selector(|| "image-empty".into())
            .child(IMAGE_EMPTY)
    } else {
        let picture = match url.contains("://") {
            true => img(SharedString::from(url.to_string())),
            false => img(std::path::PathBuf::from(url)),
        };
        let box_ = div()
            .relative()
            .rounded(px(BUTTON_RADIUS))
            .overflow_hidden()
            .border_1()
            .border_color(palette.border)
            .children(overlay.layouts.map(|layouts| {
                let layouts = layouts.clone();
                let id = overlay.block_id.clone();
                canvas(
                    move |bounds, _, _| layouts.record_picture(id, bounds),
                    |_, _, _, _| (),
                )
                .absolute()
                .size_full()
            }));
        match block.width {
            Some(width) => box_
                .self_start()
                .max_w_full()
                .w(px(width as f32))
                .child(picture.w(px(width as f32)).max_w_full()),
            None => box_.child(picture.max_w_full()),
        }
    };
    div()
        .flex()
        .flex_col()
        .gap(px(CAPTION_GAP))
        .child(picture)
        .when(
            overlay.caption == Caption::Shown
                && (!block.plain.is_empty() || overlay.caret().is_some()),
            |el| {
                el.child(text_element(
                    &block.plain,
                    &block.runs,
                    typography.caption.size(),
                    typography.caption.line_height(),
                    FontWeight::NORMAL,
                    overlay,
                    palette,
                ))
            },
        )
        .into_any_element()
}

fn bookmark(
    ix: usize,
    url: &str,
    form: block_markdown::Form,
    typography: &Typography,
    palette: &EditorPalette,
    cx: &App,
) -> AnyElement {
    use block_markdown::Form;
    let preview = preview::of(cx, url).unwrap_or_default();
    let host = SharedString::from(preview::host(url).to_string());
    let label = preview.label.clone().unwrap_or_else(|| host.clone());
    let title = preview
        .title
        .clone()
        .unwrap_or_else(|| SharedString::from(url.to_string()));

    let (icon, muted, wash) = (
        preview.icon.clone(),
        palette.text_muted,
        palette.element_hover,
    );
    let site = host.clone();
    let mark = move |size: f32| {
        let host = site.clone();
        match icon.clone() {
            Some(icon) => img(icon)
                .size(px(size))
                .rounded(px(size / 4.0))
                .with_fallback(move || initial(&host, size, muted, wash))
                .into_any_element(),
            None => initial(&host, size, muted, wash),
        }
    };

    if form == Form::Chip {
        let open = url.to_string();
        let pill = div()
            .id(ElementId::Name(format!("md-chip-{ix}").into()))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .px(px(CHIP_BLOCK_PAD_X))
            .py(px(CHIP_BLOCK_PAD_Y))
            .rounded(px(CONTROL_RADIUS))
            .border_1()
            .border_color(palette.border)
            .bg(palette.element_hover)
            .text_size(px(typography.body.size()))
            .line_height(px(typography.body.line_height()))
            .text_color(palette.text)
            .cursor(CursorStyle::PointingHand)
            .hover(|el| el.bg(palette.ink(0.12)))
            .on_click(move |_, _, cx| cx.open_url(&open))
            .child(mark(CHIP_ICON))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .child(preview.title.unwrap_or(label)),
            );
        return div().flex().flex_row().child(pill).into_any_element();
    }

    let words = div()
        .flex()
        .flex_col()
        .min_w_0()
        .px(px(CARD_PADDING))
        .py(px(CARD_PADDING - 2.0))
        .child(
            div()
                .truncate()
                .text_size(px(typography.body.size()))
                .line_height(px(typography.body.line_height()))
                .text_color(palette.text)
                .child(title),
        )
        .children(preview.description.map(|blurb| {
            div()
                .line_clamp(2)
                .text_size(px(typography.card.size()))
                .line_height(px(typography.card.line_height()))
                .text_color(palette.text_muted)
                .child(blurb)
        }))
        .child(
            div()
                .mt_auto()
                .pt(px(6.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(px(typography.card.size()))
                .text_color(palette.text_muted)
                .child(mark(CARD_ICON))
                .child(div().truncate().child(label)),
        );

    let picture = corners(div(), form)
        .bg(palette.ink(0.03))
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .child(match preview.image {
            Some(image) => corners(img(image).size_full().object_fit(ObjectFit::Cover), form)
                .with_fallback(move || mark(CARD_COVER))
                .into_any_element(),
            None => mark(CARD_COVER),
        });

    let open = url.to_string();
    let card = div()
        .id(ElementId::Name(format!("md-bookmark-{ix}").into()))
        .flex()
        .w_full()
        .overflow_hidden()
        .rounded(px(BUTTON_RADIUS))
        .border(px(CARD_BORDER))
        .border_color(palette.border)
        .bg(palette.ink(0.02))
        .cursor(CursorStyle::PointingHand)
        .hover(|el| el.bg(palette.element_hover))
        .on_click(move |_, _, cx| cx.open_url(&open));

    if form == Form::Embed {
        card.flex_col()
            .child(picture.w_full().h(px(CARD_COVER_HEIGHT)))
            .child(words.w_full())
            .into_any_element()
    } else {
        card.h(px(CARD_HEIGHT))
            .child(words.flex_1())
            .child(picture.flex_none().w(px(CARD_IMAGE_WIDTH)).h_full())
            .into_any_element()
    }
}

fn corners<T: Styled>(element: T, form: block_markdown::Form) -> T {
    use block_markdown::Form;
    let corner = px(BUTTON_RADIUS - CARD_BORDER);
    match form {
        Form::Embed => element.rounded_t(corner),
        _ => element.rounded_r(corner),
    }
}

fn initial(host: &str, size: f32, color: Hsla, wash: Hsla) -> AnyElement {
    div()
        .flex_none()
        .size(px(size))
        .rounded(px(size / 4.0))
        .bg(wash)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size * 0.55))
        .text_color(color)
        .child(SharedString::from(
            host.chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string(),
        ))
        .into_any_element()
}

fn table_block(
    table: &TableData,
    overlay: Overlay<'_>,
    typography: &Typography,
    palette: &EditorPalette,
    window: &mut Window,
) -> AnyElement {
    let ix = overlay.block_ix;
    let mut rows = table.rows.clone();
    if rows.is_empty() {
        // Empty table still paints a frameless hairline grid so the block is visible.
        rows = vec![vec![String::new(); 2], vec![String::new(); 2]];
    }
    let columns = rows.iter().map(|r| r.len()).max().unwrap_or(0).max(1);
    let has_header = !table.rows.is_empty();

    let text_system = window.text_system();
    let mut flats: Vec<Vec<Option<Flat>>> = Vec::with_capacity(rows.len());
    let mut content = vec![0.0f32; columns];
    for (r, row) in rows.iter().enumerate() {
        let weight = if has_header && r == 0 {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        };
        let mut out = Vec::with_capacity(columns);
        for (c, natural) in content.iter_mut().enumerate() {
            let Some(cell) = row.get(c) else {
                out.push(None);
                continue;
            };
            let flat = flatten(cell, &[], weight, palette);
            if !flat.text.is_empty() {
                let width = f32::from(
                    text_system
                        .shape_line(
                            flat.text.clone(),
                            px(typography.body.size()),
                            &flat.runs,
                            None,
                        )
                        .width(),
                );
                *natural = natural.max(width);
            }
            out.push(Some(flat));
        }
        flats.push(out);
    }

    let naturals: Vec<f32> = content
        .iter()
        .map(|width| width.max(TABLE_MIN_COLUMN_CONTENT) + 2.0 * TABLE_CELL_PADDING)
        .collect();
    let minimums: Vec<f32> = naturals
        .iter()
        .map(|natural| (*natural).min(TABLE_MIN_COLUMN_WIDTH))
        .collect();
    let hairline = palette.hairline(0.10);

    let mut inner = div()
        .flex()
        .flex_col()
        .w_full()
        .min_w(px(minimums.iter().sum::<f32>()));
    for (r, row) in flats.into_iter().enumerate() {
        if r > 0 {
            inner = inner.child(div().flex_none().h(px(TABLE_DIVIDER)).w_full().bg(hairline));
        }
        let mut row_el = div().flex().flex_row();
        for (c, cell) in row.into_iter().enumerate() {
            let mut cell_el = div()
                .flex_grow(naturals.get(c).copied().unwrap_or(1.0))
                .flex_shrink(naturals.get(c).copied().unwrap_or(1.0))
                .flex_basis(px(0.0))
                .min_w(px(minimums
                    .get(c)
                    .copied()
                    .unwrap_or(TABLE_MIN_COLUMN_WIDTH)))
                .p(px(TABLE_CELL_PADDING))
                .text_size(px(typography.body.size()))
                .line_height(px(typography.body.line_height()));
            cell_el = match table.align.get(c).copied().unwrap_or_default() {
                Align::Left => cell_el,
                Align::Center => cell_el.text_center(),
                Align::Right => cell_el.text_right(),
            };
            if let Some(flat) = cell {
                let row = if has_header { r } else { r + 1 };
                let len = flat.text.len();
                cell_el = cell_el.child(painted_text(
                    flat,
                    len,
                    typography.body.size(),
                    typography.body.line_height(),
                    overlay.at(Part::Cell { row, column: c }),
                    palette,
                ));
            }
            row_el = row_el.child(cell_el);
        }
        inner = inner.child(row_el);
    }

    div()
        .id(ElementId::Name(format!("md-table-{ix}").into()))
        .w_full()
        .overflow_x_scroll()
        .restrict_scroll_to_axis()
        .debug_selector(|| "table-hairlines".into())
        .child(inner)
        .into_any_element()
}
