//! Image store hook and URL prompt (Bezel `editor/image.rs`).
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use std::path::Path;
use std::sync::Arc;

use gpui::{App, Context, Entity, Global, SharedString, Window, div, prelude::*, px};
use gpui_component::ActiveTheme;
use gpui_component::input::{Input, InputState};
use gpui_component_block_view::{Cursor, Part, ink};

/// Key context while the image URL prompt holds focus.
pub const PROMPT_CONTEXT: &str = "LimenBlockEditorUrlPrompt";

/// Bytes or a file path the host can turn into a durable image URL.
pub enum Source<'a> {
    Bytes(&'a gpui::Image),
    File(&'a Path),
}

/// Host callback: store screenshot / drop bytes and return a URL the renderer
/// can `img()`. Required for clipboard images; without it, paste falls through.
pub type ImageStore = Arc<dyn Fn(Source<'_>, &mut App) -> Option<String> + Send + Sync>;

struct ImageStoreGlobal(Option<ImageStore>);

impl Global for ImageStoreGlobal {}

/// Install the image store used for clipboard screenshots and file drops.
pub fn set_image_store(cx: &mut App, store: ImageStore) {
    cx.set_global(ImageStoreGlobal(Some(store)));
}

#[must_use]
pub(crate) fn store(cx: &App) -> Option<ImageStore> {
    cx.try_global::<ImageStoreGlobal>()
        .and_then(|g| g.0.clone())
}

/// Open prompt asking for an image URL.
pub struct Prompt {
    pub block_id: block_markdown::BlockId,
    pub input: Entity<InputState>,
}

impl Prompt {
    pub fn new(
        block_id: block_markdown::BlockId,
        window: &mut Window,
        cx: &mut Context<crate::editor::Editor>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("https://…"));
        Self { block_id, input }
    }

    pub(crate) fn paint(
        &self,
        origin: gpui::Point<gpui::Pixels>,
        cx: &mut Context<crate::editor::Editor>,
    ) -> gpui::AnyElement {
        let frost = cx.theme().popover;
        // Opaque popover until Window::paint_backdrop_blur captures on this pin
        // (matches Bezel GLASS_ALPHA = 1.0 / non-glass fallback).
        gpui::deferred(
            gpui::anchored()
                .position(origin)
                .anchor(gpui::Anchor::TopLeft)
                .snap_to_window_with_margin(px(8.0))
                .child(
                    div()
                        .id("image-url-prompt")
                        .debug_selector(|| "image-url-prompt".into())
                        .key_context(PROMPT_CONTEXT)
                        .w(px(280.0))
                        .p(px(10.0))
                        .rounded(px(12.0))
                        .border_1()
                        .border_color(ink(cx, 0.12))
                        .bg(frost)
                        .child(Input::new(&self.input)),
                ),
        )
        .priority(1)
        .into_any_element()
    }

    #[must_use]
    pub fn value(&self, cx: &App) -> SharedString {
        self.input.read(cx).value()
    }
}

/// Empty image target → caret in caption.
#[must_use]
#[allow(dead_code)]
pub fn empty_image_cursor(id: block_markdown::BlockId) -> Cursor {
    Cursor::new(id, Part::Caption, 0)
}
