//! Solar SVG glyphs for fence chrome (Bezel's set). Lucide is not used here.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use gpui::{Styled, Svg, px, svg};

const COPY: &[u8] = include_bytes!("../assets/icons/copy.svg");
const CHECK: &[u8] = include_bytes!("../assets/icons/check.svg");

/// Fence "Copy" glyph.
#[must_use]
pub fn copy() -> Svg {
    svg().data(COPY).size(px(13.0))
}

/// Language-row / copied-state check glyph.
#[must_use]
pub fn check() -> Svg {
    svg().data(CHECK).size(px(13.0))
}
