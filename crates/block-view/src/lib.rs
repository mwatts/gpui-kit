//! Paint [`BlockSnapshot`] projections and record [`BlockLayouts`] for hit-testing.
//!
//! Ported from Bezel `markdown/render.rs` against Loro snapshots (not a Bezel `Doc`).
//! See `NOTICE` in this crate.

mod block_renderer;
mod chart;
mod highlight;
pub mod icons;
mod layouts;
mod paint;
mod preview;
mod render;
mod types;
mod typography;

pub use block_renderer::{BlockRenderer, set_block_renderer};
pub use chart::{
    LANGUAGE as CHART_LANGUAGE, fence_renderer as chart_fence_renderer, install as install_chart,
    parse_rows as parse_chart_rows, render as render_chart,
};
pub use highlight::{DEFAULT_LANGUAGES, Highlighter, languages, menu_languages, set_highlighter};
pub use layouts::BlockLayouts;
pub use paint::{
    EditorPalette, INK_FILL_SCALE, INK_HAIRLINE_SCALE, hairline, hairline_for, ink, ink_for, wash,
    wash_for,
};
pub use preview::{LinkPreview, Preview, host, set_link_preview};
pub use render::{Flat, PLAIN_LANGUAGE, flatten, render, render_with};
pub use types::{
    Align, Annotation, BASE_RADIUS_PX, BULLET_DISC_PX, BlockSnapshot, CARET_WIDTH_PX, Caption,
    Cursor, Editing, MarkedRange, Part, QUOTE_BAR_PX, Selection, TASK_BOX_PX, TableData,
};
pub use typography::{Metrics, Typography, set_typography};

pub use block_markdown::{BlockId, BlockType, Form};

/// Crate version exposed for workspace smoke checks.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
