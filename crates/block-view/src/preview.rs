//! Who describes a link (Bezel `preview.rs`).
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use gpui::{App, Global, SharedString};

/// What a bookmark paints beyond the URL it already has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    pub title: Option<SharedString>,
    pub description: Option<SharedString>,
    pub image: Option<SharedString>,
    pub icon: Option<SharedString>,
    pub label: Option<SharedString>,
}

/// `None` for a URL the caller has nothing for yet.
pub type LinkPreview = fn(url: &str) -> Option<Preview>;

struct Installed(LinkPreview);

impl Global for Installed {}

/// Install a link-preview resolver once at boot.
pub fn set_link_preview(cx: &mut App, preview: LinkPreview) {
    cx.set_global(Installed(preview));
}

pub(crate) fn of(cx: &App, url: &str) -> Option<Preview> {
    (cx.try_global::<Installed>()?.0)(url)
}

/// The host, without its `www.`.
#[must_use]
pub fn host(url: &str) -> &str {
    let after = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = after.split(['/', '?', '#']).next().unwrap_or(after);
    host.strip_prefix("www.").unwrap_or(host)
}
