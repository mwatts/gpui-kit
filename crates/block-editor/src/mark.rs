//! Inline mark vocabulary shared by keys, toolbar, and apply.

/// Marks the format toolbar and cmd-b/i/e/shift-x toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mark {
    Bold,
    Italic,
    Strike,
    Code,
}

impl Mark {
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Strike => "strike",
            Self::Code => "code",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "bold" => Some(Self::Bold),
            "italic" => Some(Self::Italic),
            "strike" => Some(Self::Strike),
            "code" => Some(Self::Code),
            _ => None,
        }
    }
}
