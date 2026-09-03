//! Who colors a fenced block (Bezel `highlight.rs`).
//!
//! Stub-friendly: without an install, fences paint as plain runs.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use std::ops::Range;
use std::sync::LazyLock;

use gpui::{App, Global, Hsla, SharedString};

/// Spans over `code`, in bytes. `None` for a language the caller cannot color.
pub type Highlighter = fn(language: &str, code: &str) -> Option<Vec<(Range<usize>, Hsla)>>;

struct Installed {
    highlighter: Highlighter,
    languages: Vec<SharedString>,
}

impl Global for Installed {}

/// Fence languages offered when no highlighter has installed its own list.
pub const DEFAULT_LANGUAGES: &[&str] = &[
    "bash",
    "c",
    "cpp",
    "css",
    "go",
    "html",
    "java",
    "javascript",
    "json",
    "markdown",
    "python",
    "ruby",
    "rust",
    "shell",
    "sql",
    "swift",
    "toml",
    "typescript",
    "yaml",
    "chart",
];

static DEFAULT_LANGUAGE_NAMES: LazyLock<Vec<SharedString>> = LazyLock::new(|| {
    DEFAULT_LANGUAGES
        .iter()
        .copied()
        .map(SharedString::from)
        .collect()
});

/// Install a highlighter and the language names a picker should offer.
pub fn set_highlighter(
    cx: &mut App,
    highlighter: Highlighter,
    languages: impl IntoIterator<Item = impl Into<SharedString>>,
) {
    cx.set_global(Installed {
        highlighter,
        languages: languages.into_iter().map(Into::into).collect(),
    });
}

/// Languages the installed highlighter can color.
#[must_use]
pub fn languages(cx: &App) -> &[SharedString] {
    cx.try_global::<Installed>()
        .map_or(&[], |installed| &installed.languages)
}

/// Languages a fence picker should list: the highlighter's list, else
/// [`DEFAULT_LANGUAGES`].
#[must_use]
pub fn menu_languages(cx: &App) -> &[SharedString] {
    let installed = languages(cx);
    if installed.is_empty() {
        DEFAULT_LANGUAGE_NAMES.as_slice()
    } else {
        installed
    }
}

pub(crate) fn spans(
    cx: &App,
    language: Option<&str>,
    code: &str,
) -> Option<Vec<(Range<usize>, Hsla)>> {
    (cx.try_global::<Installed>()?.highlighter)(language?, code)
}
