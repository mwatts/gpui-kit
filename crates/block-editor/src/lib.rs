//! Loro-backed block editor entity and Bezel chrome.
//!
//! Ported from Bezel editor chrome and Catena/Mikra Loro patterns. See `NOTICE`.

mod apply;
mod backspace;
mod chrome;
mod document;
mod editor;
mod icons;
mod image;
mod input;
mod keys;
mod layout;
mod link;
mod mark;
mod project;
mod shortcut;
mod slash;
mod types;

pub use backspace::backspace_at_start;
pub use document::BlockDocument;
pub use editor::{CommentThread, Editor, EditorEvent};
pub use image::{ImageStore, Source, set_image_store};
pub use keys::{CONTEXT, init as keys_init};
pub use layout::{Layout, set_layout};
pub use link::{Choice as LinkChoice, Paste as LinkPaste};
pub use mark::Mark;
pub use shortcut::{
    PrefixKind, inline_ops, match_inline, match_prefix, prefix_ops, try_inline, try_prefix,
};
pub use slash::{Filter as SlashFilter, Slash, filter_indices, items as slash_items};
pub use types::{
    Align, ApplyResult, BlockOp, BlockSnapshot, CommentId, Cursor, LwwValue, Part, Selection,
    TableData,
};

pub use block_markdown::{BlockId, BlockType, CommentState, Form};
pub use gpui_component_block_view as block_view;

use std::borrow::Cow;

use gpui::App;
use gpui_component::Theme;

/// Register keys, Geist fonts (when available), chart fence renderer, and
/// editor layout defaults.
pub fn init(cx: &mut App) {
    keys::init(cx);
    set_layout(cx, Layout::default());
    register_geist_fonts(cx);
    gpui_component_block_view::install_chart(cx);
}

fn register_geist_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Medium.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-SemiBold.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Bold.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono.ttf").as_slice()),
    ];
    match cx.text_system().add_fonts(fonts) {
        Ok(()) => {
            if cx.has_global::<Theme>() {
                let theme = Theme::global_mut(cx);
                theme.font_family = "Geist".into();
                theme.mono_font_family = "Geist Mono".into();
                Theme::sync_base(cx);
            }
        }
        Err(err) => {
            eprintln!("gpui-component-block-editor: failed to register Geist fonts: {err}");
        }
    }
}

/// Crate version exposed for workspace smoke checks.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    include!("lib_model_tests.rs");
}

#[cfg(test)]
mod interaction_tests;
