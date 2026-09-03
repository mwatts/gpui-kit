//! ` ```chart ` — one `label: number` per line, painted as bars.
//!
//! Ported from Bezel `crates/blocks/src/chart.rs`.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use gpui::{AnyElement, App, Window, div, prelude::*, px, relative};

use crate::paint::EditorPalette;
use crate::types::BASE_RADIUS_PX;

pub const LANGUAGE: &str = "chart";

const LABEL_WIDTH: f32 = 72.0;
const BAR_HEIGHT: f32 = 10.0;
const BAR_RADIUS: f32 = 3.0;
const ROW_GAP: f32 = 4.0;
const PADDING: f32 = 12.0;
const SPACE: f32 = 8.0;

/// Parse chart source into `(label, value)` rows.
#[must_use]
pub fn parse_rows(code: &str) -> Vec<(&str, f32)> {
    code.lines()
        .filter_map(|line| {
            let (label, value) = line.split_once(':')?;
            Some((label.trim(), value.trim().parse().ok()?))
        })
        .collect()
}

/// Fence renderer for [`LANGUAGE`]. Returns `None` when the source is empty /
/// unparseable so ordinary code paint shows instead.
pub fn render(code: &str, _: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let rows = parse_rows(code);
    if rows.is_empty() {
        return None;
    }

    let palette = EditorPalette::from_app(cx);
    let peak = rows.iter().map(|(_, value)| *value).fold(1.0_f32, f32::max);
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(ROW_GAP))
            .p(px(PADDING))
            .rounded(px(BASE_RADIUS_PX))
            .bg(palette.ink(0.02))
            .children(rows.into_iter().map(|(label, value)| {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(SPACE))
                    .child(
                        div()
                            .flex_none()
                            .w(px(LABEL_WIDTH))
                            .text_size(px(11.0))
                            .text_color(palette.text_muted)
                            .child(label.to_string()),
                    )
                    .child(
                        div().flex_1().child(
                            div()
                                .h(px(BAR_HEIGHT))
                                .w(relative(value / peak))
                                .rounded(px(BAR_RADIUS))
                                .bg(palette.accent),
                        ),
                    )
            }))
            .into_any_element(),
    )
}

/// Adapter for [`crate::set_block_renderer`].
pub fn fence_renderer(
    language: &str,
    code: &str,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    if language != LANGUAGE {
        return None;
    }
    render(code, window, cx)
}

/// Install the chart fence renderer (also called from `block_editor::init`).
pub fn install(cx: &mut App) {
    crate::set_block_renderer(cx, fence_renderer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_label_number_rows() {
        let rows = parse_rows("parse: 12\nrender: 47\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "parse");
        assert_eq!(rows[0].1, 12.0);
        assert_eq!(rows[1].0, "render");
        assert_eq!(rows[1].1, 47.0);
    }

    #[test]
    fn empty_source_declines() {
        assert!(parse_rows("").is_empty());
        assert!(parse_rows("not a chart").is_empty());
    }
}
