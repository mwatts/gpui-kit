//! Paint a canonical knowledge-image reference without rewriting the block.

use std::path::{Path, PathBuf};

use gpui::{App, Global};

/// Host hook. `None` means the reference is unavailable. The block URL stays as stored.
pub type ImageResolver = fn(reference: &str) -> Option<PathBuf>;

struct Installed(ImageResolver);

impl Global for Installed {}

/// Install the resolver that maps a `bytes/{hash}` reference to a cache path.
pub fn set_image_resolver(cx: &mut App, resolver: ImageResolver) {
    cx.set_global(Installed(resolver));
}

pub(crate) fn installed(cx: &App) -> Option<ImageResolver> {
    cx.try_global::<Installed>().map(|installed| installed.0)
}

/// What the painter should draw. This never replaces the stored URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImagePaint {
    Empty,
    Remote(String),
    Cache(PathBuf),
    /// Legacy path paint when no resolver is installed.
    Path(PathBuf),
    Unavailable,
}

/// A stored reference names archive bytes, not a filesystem path.
#[must_use]
pub fn is_canonical_image_reference(url: &str) -> bool {
    let rest = url.strip_prefix("bytes/").unwrap_or("");
    !rest.is_empty()
        && !rest.contains('/')
        && !rest.contains('\\')
        && rest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Choose a paint source. `resolved` is `None` when no resolver is installed,
/// and `Some(None)` when the installed resolver cannot find the bytes.
#[must_use]
pub fn image_paint(url: &str, resolved: Option<Option<&Path>>) -> ImagePaint {
    if url.is_empty() {
        return ImagePaint::Empty;
    }
    if url.contains("://") {
        return ImagePaint::Remote(url.to_owned());
    }
    match resolved {
        None if is_canonical_image_reference(url) => ImagePaint::Unavailable,
        None => ImagePaint::Path(PathBuf::from(url)),
        Some(Some(path)) => ImagePaint::Cache(path.to_path_buf()),
        Some(None) => ImagePaint::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_maps_a_hash_without_replacing_the_reference() {
        let reference = "bytes/abcdef";
        let stored = reference.to_owned();
        let cache = Path::new("/tmp/ashlar-cache/abcdef");
        assert_eq!(
            image_paint(&stored, Some(Some(cache))),
            ImagePaint::Cache(cache.to_path_buf())
        );
        assert_eq!(stored, reference);
        assert_eq!(image_paint(&stored, Some(None)), ImagePaint::Unavailable);
        assert_eq!(stored, reference);
        assert!(is_canonical_image_reference(reference));
    }

    #[test]
    fn missing_resolver_keeps_legacy_paths_and_hides_canonical_refs() {
        assert_eq!(
            image_paint("/tmp/photo.png", None),
            ImagePaint::Path(PathBuf::from("/tmp/photo.png"))
        );
        assert_eq!(
            image_paint("bytes/ab", None),
            ImagePaint::Unavailable
        );
        assert_eq!(
            image_paint("https://example.org/a.png", Some(None)),
            ImagePaint::Remote("https://example.org/a.png".into())
        );
    }
}
