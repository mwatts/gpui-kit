//! Document typography roles (Bezel `markdown::Typography`).
//!
//! Metrics are the Bezel pixel pairs at 1× — not remapped onto gpui-component
//! heading sizes.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use gpui::{App, FontWeight, Global};

/// Size / leading / weight for one role.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub size: f32,
    pub line_height: f32,
    pub weight: FontWeight,
}

impl Metrics {
    #[must_use]
    pub const fn new(size: f32, line_height: f32, weight: FontWeight) -> Self {
        Self {
            size,
            line_height,
            weight,
        }
    }

    #[must_use]
    pub fn size(self) -> f32 {
        self.size
    }

    #[must_use]
    pub fn line_height(self) -> f32 {
        self.line_height
    }
}

/// What a document is set in, role by role.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Typography {
    pub body: Metrics,
    pub h1: Metrics,
    pub h2: Metrics,
    pub h3: Metrics,
    /// Every heading past the third.
    pub h4: Metrics,
    pub code: Metrics,
    pub card: Metrics,
    pub caption: Metrics,
}

impl Typography {
    #[must_use]
    pub fn of(cx: &App) -> Self {
        cx.try_global::<Installed>()
            .map_or_else(Self::default, |installed| installed.0)
    }

    #[must_use]
    pub fn heading(&self, level: u8) -> Metrics {
        match level {
            1 => self.h1,
            2 => self.h2,
            3 => self.h3,
            _ => self.h4,
        }
    }
}

impl Default for Typography {
    /// Body 14/22, H1 19/27 semibold, H2 16/24, H3 15/22, H4+ 14/22,
    /// code 12.5/18, card 12/17, caption 11.5/17.
    fn default() -> Self {
        Self {
            body: Metrics::new(14.0, 22.0, FontWeight::NORMAL),
            h1: Metrics::new(19.0, 27.0, FontWeight::SEMIBOLD),
            h2: Metrics::new(16.0, 24.0, FontWeight::SEMIBOLD),
            h3: Metrics::new(15.0, 22.0, FontWeight::SEMIBOLD),
            h4: Metrics::new(14.0, 22.0, FontWeight::SEMIBOLD),
            code: Metrics::new(12.5, 18.0, FontWeight::NORMAL),
            card: Metrics::new(12.0, 17.0, FontWeight::NORMAL),
            caption: Metrics::new(11.5, 17.0, FontWeight::NORMAL),
        }
    }
}

struct Installed(Typography);

impl Global for Installed {}

/// Install document typography once at boot.
pub fn set_typography(cx: &mut App, typography: Typography) {
    cx.set_global(Installed(typography));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_metrics_match_bezel() {
        let t = Typography::default();
        assert_eq!(t.body.size(), 14.0);
        assert_eq!(t.body.line_height(), 22.0);
        assert_eq!(t.h1.size(), 19.0);
        assert_eq!(t.h1.line_height(), 27.0);
        assert_eq!(t.h1.weight, FontWeight::SEMIBOLD);
        assert_eq!(t.h2.size(), 16.0);
        assert_eq!(t.h3.size(), 15.0);
        assert_eq!(t.code.size(), 12.5);
        assert_eq!(t.caption.size(), 11.5);
    }
}
