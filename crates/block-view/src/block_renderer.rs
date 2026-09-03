//! Custom fence painters via `set_block_renderer` (Bezel `block.rs`).
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use gpui::{AnyElement, App, Global, Window};

/// Paints the block a fence's info string names, or `None` for ordinary code.
pub type BlockRenderer =
    fn(language: &str, code: &str, &mut Window, &mut App) -> Option<AnyElement>;

struct Installed(BlockRenderer);

impl Global for Installed {}

/// Install a custom fence renderer once at boot (`chart` from [`crate::chart`]).
pub fn set_block_renderer(cx: &mut App, renderer: BlockRenderer) {
    cx.set_global(Installed(renderer));
}

pub(crate) fn render(
    language: Option<&str>,
    code: &str,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let renderer = cx.try_global::<Installed>()?.0;
    renderer(language?, code, window, cx)
}
