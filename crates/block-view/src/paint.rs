//! Appearance-aware ink / hairline / wash recipes (Bezel `theme::paint`).
//!
//! Do not collapse these onto `muted_foreground` — contrast differs by design.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use gpui::{App, Hsla, hsla};
use gpui_component::ActiveTheme;

/// Light-mode alpha multiplier for fills (Bezel).
pub const INK_FILL_SCALE: f32 = 1.0;
/// Light-mode alpha multiplier for hairlines (Bezel).
pub const INK_HAIRLINE_SCALE: f32 = 1.35;

/// Soft fill ink: white on dark, black on light.
#[must_use]
pub fn ink(cx: &App, alpha: f32) -> Hsla {
    ink_for(cx.theme().is_dark(), alpha)
}

/// Soft fill ink for a known appearance.
#[must_use]
pub fn ink_for(dark: bool, alpha: f32) -> Hsla {
    if dark {
        hsla(0.0, 0.0, 1.0, alpha)
    } else {
        hsla(0.0, 0.0, 0.0, alpha * INK_FILL_SCALE)
    }
}

/// Hairline ink for borders and dividers.
#[must_use]
pub fn hairline(cx: &App, alpha: f32) -> Hsla {
    hairline_for(cx.theme().is_dark(), alpha)
}

#[must_use]
pub fn hairline_for(dark: bool, alpha: f32) -> Hsla {
    if dark {
        hsla(0.0, 0.0, 1.0, alpha)
    } else {
        hsla(0.0, 0.0, 0.0, (alpha * INK_HAIRLINE_SCALE).min(0.5))
    }
}

/// Softened interactive wash.
#[must_use]
pub fn wash(cx: &App, alpha: f32) -> Hsla {
    wash_for(cx.theme().is_dark(), alpha)
}

#[must_use]
pub fn wash_for(dark: bool, alpha: f32) -> Hsla {
    if dark {
        hsla(0.0, 0.0, 0.92, alpha)
    } else {
        hsla(0.0, 0.0, 0.10, alpha * INK_FILL_SCALE)
    }
}

/// Resolved colors for one paint pass (theme pairings + Bezel recipes).
#[derive(Clone)]
pub struct EditorPalette {
    pub dark: bool,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_faint: Hsla,
    pub caret: Hsla,
    pub selection: Hsla,
    pub accent: Hsla,
    pub warning: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub solid: Hsla,
    pub on_solid: Hsla,
    pub code_wash: Hsla,
    pub code_text: Hsla,
    pub element_hover: Hsla,
    pub font_sans: gpui::SharedString,
    pub font_mono: gpui::SharedString,
}

impl EditorPalette {
    #[must_use]
    pub fn from_app(cx: &App) -> Self {
        let theme = cx.theme();
        let dark = theme.is_dark();
        Self {
            dark,
            text: theme.foreground,
            // Bezel text_muted / text_faint — not a flat muted_foreground remap.
            text_muted: theme.muted_foreground,
            text_faint: ink_for(dark, if dark { 0.35 } else { 0.35 }),
            caret: theme.caret,
            selection: theme.selection,
            accent: theme.accent,
            warning: theme.warning,
            border: hairline_for(dark, 0.12),
            border_strong: hairline_for(dark, 0.22),
            solid: theme.primary,
            on_solid: theme.primary_foreground,
            code_wash: ink_for(dark, 0.06),
            code_text: theme.foreground,
            element_hover: ink_for(dark, 0.08),
            font_sans: theme.font_family.clone(),
            font_mono: theme.mono_font_family.clone(),
        }
    }

    #[must_use]
    pub fn ink(&self, alpha: f32) -> Hsla {
        ink_for(self.dark, alpha)
    }

    #[must_use]
    pub fn hairline(&self, alpha: f32) -> Hsla {
        hairline_for(self.dark, alpha)
    }

    #[must_use]
    pub fn wash(&self, alpha: f32) -> Hsla {
        wash_for(self.dark, alpha)
    }

    #[must_use]
    pub fn annotation_wash(&self, kind: crate::types::Annotation) -> Hsla {
        use crate::types::Annotation;
        match kind {
            Annotation::Open => {
                let mut c = self.warning;
                c.a = 0.20;
                c
            }
            Annotation::Resolved => {
                let mut c = self.warning;
                c.a = 0.08;
                c
            }
            Annotation::Active => {
                let mut c = self.warning;
                c.a = 0.38;
                c
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Annotation;

    #[test]
    fn annotation_washes_differ_by_kind() {
        let dark = EditorPalette {
            dark: true,
            text: hsla(0., 0., 1., 1.),
            text_muted: hsla(0., 0., 0.7, 1.),
            text_faint: ink_for(true, 0.35),
            caret: hsla(0., 0., 1., 1.),
            selection: hsla(0.6, 0.5, 0.5, 0.3),
            accent: hsla(0.6, 0.8, 0.5, 1.),
            warning: hsla(0.1, 0.8, 0.5, 1.),
            border: hairline_for(true, 0.12),
            border_strong: hairline_for(true, 0.22),
            solid: hsla(0.6, 0.8, 0.5, 1.),
            on_solid: hsla(0., 0., 1., 1.),
            code_wash: ink_for(true, 0.06),
            code_text: hsla(0., 0., 1., 1.),
            element_hover: ink_for(true, 0.08),
            font_sans: "Sans".into(),
            font_mono: "Mono".into(),
        };
        let open = dark.annotation_wash(Annotation::Open).a;
        let resolved = dark.annotation_wash(Annotation::Resolved).a;
        let active = dark.annotation_wash(Annotation::Active).a;
        assert!(active > open && open > resolved);
    }
}
