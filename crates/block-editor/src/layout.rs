//! Editor layout tokens (Bezel `layout.rs`).

use gpui::{App, Global};

/// Gutter / text inset and related chrome metrics.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    /// Pixels left of the text for the block handle (Bezel `text_inset`).
    pub text_inset: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Self { text_inset: 22.0 }
    }
}

impl Global for Layout {}

/// Install or replace the editor layout global.
pub fn set_layout(cx: &mut App, layout: Layout) {
    cx.set_global(layout);
}

impl Layout {
    #[must_use]
    pub fn of(cx: &App) -> Self {
        cx.try_global::<Layout>().copied().unwrap_or_default()
    }
}
