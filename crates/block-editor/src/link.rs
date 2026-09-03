//! Paste-URL menu: a URL landed, and what it could be instead (Bezel `link.rs`).

use block_markdown::is_image;
use gpui_component_block_view::Cursor;

/// What a row does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Dismiss,
    Chip,
    Bookmark,
    Embed,
    Image,
}

impl Choice {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Dismiss => "Dismiss",
            Self::Chip => "Create chip",
            Self::Bookmark => "Create bookmark",
            Self::Embed => "Create embed",
            Self::Image => "Create image",
        }
    }
}

/// An open paste menu: the link that landed, and the row under the pointer.
#[derive(Debug, Clone)]
pub struct Paste {
    /// The block the URL went into.
    pub at: Cursor,
    pub url: String,
    /// Whether the URL has a block to itself.
    pub alone: bool,
    pub rows: Vec<Choice>,
    pub active: usize,
}

impl Paste {
    #[must_use]
    pub fn open(at: Cursor, url: String, alone: bool) -> Self {
        let mut rows = vec![Choice::Dismiss, Choice::Chip];
        if alone {
            rows.extend([Choice::Bookmark, Choice::Embed]);
            if is_image(&url) {
                rows.push(Choice::Image);
            }
        }
        Self {
            at,
            url,
            alone,
            rows,
            active: 0,
        }
    }

    /// Walk the rows. Does not wrap: past the end is the end.
    pub fn step(&mut self, delta: isize) {
        let last = self.rows.len() as isize - 1;
        self.active = (self.active as isize + delta).clamp(0, last) as usize;
    }

    #[must_use]
    pub fn choice(&self) -> Choice {
        self.rows[self.active]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component_block_view::Part;

    #[test]
    fn choices_not_alone() {
        let paste = Paste::open(
            Cursor::new("a".into(), Part::Body, 0),
            "https://example.com".into(),
            false,
        );
        assert_eq!(paste.rows, [Choice::Dismiss, Choice::Chip]);
    }

    #[test]
    fn choices_alone_non_image() {
        let paste = Paste::open(
            Cursor::new("a".into(), Part::Body, 0),
            "https://example.com/page".into(),
            true,
        );
        assert_eq!(
            paste.rows,
            [
                Choice::Dismiss,
                Choice::Chip,
                Choice::Bookmark,
                Choice::Embed
            ]
        );
    }

    #[test]
    fn choices_alone_image() {
        let paste = Paste::open(
            Cursor::new("a".into(), Part::Body, 0),
            "https://example.com/pic.png".into(),
            true,
        );
        assert_eq!(
            paste.rows,
            [
                Choice::Dismiss,
                Choice::Chip,
                Choice::Bookmark,
                Choice::Embed,
                Choice::Image
            ]
        );
    }

    #[test]
    fn step_clamps() {
        let mut paste = Paste::open(
            Cursor::new("a".into(), Part::Body, 0),
            "https://example.com".into(),
            false,
        );
        paste.step(-1);
        assert_eq!(paste.choice(), Choice::Dismiss);
        paste.step(10);
        assert_eq!(paste.choice(), Choice::Chip);
    }
}
