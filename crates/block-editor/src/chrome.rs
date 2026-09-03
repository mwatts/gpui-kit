//! Glass/frost popover cards, slash menu, format toolbar, handle, drop line,
//! language menu.
//!
//! Bezel chrome recipes (radius 12). [`GLASS_ALPHA`] stays 1.0 on gpui-pre
//! 0.3, which has no `paint_backdrop_blur` yet, so menus paint the opaque
//! popover surface (Bezel's non-macOS fallback) — never a translucent slab.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use gpui::{
    AnyElement, App, Context, CursorStyle, ElementId, MouseButton, Window, div, prelude::*, px,
};
use gpui_component::ActiveTheme;
use gpui_component_block_view::{
    BlockType, EditorPalette, PLAIN_LANGUAGE, hairline, ink, languages, menu_languages,
};

use crate::editor::Editor;
use crate::icons;
use crate::mark::Mark;

/// Interaction-test selectors.
pub const SLASH_MENU: &str = "slash-menu";
pub const BLOCK_HANDLE: &str = "block-handle";
pub const FORMAT_TOOLBAR: &str = "format-toolbar";
pub const PASTE_MENU: &str = "paste-url-menu";
pub const LANGUAGE_MENU: &str = "language-menu";

const SURFACE_RADIUS: f32 = 12.0;
const HANDLE_SIZE: f32 = 18.0;
const SLASH_WIDTH: f32 = 200.0;
const SLASH_MAX_H: f32 = 280.0;
const PASTE_WIDTH: f32 = 180.0;
const LANG_WIDTH: f32 = 150.0;
const CHIP_PAD_X: f32 = 4.0;
const CHIP_PAD_Y: f32 = 2.0;

/// Matches Bezel `Theme::GLASS_ALPHA`. Translucent frost needs compositor
/// backdrop blur, which gpui-pre 0.3 does not expose, so every platform uses
/// the opaque popover tone.
pub const GLASS_ALPHA: f32 = 1.0;

/// Popover card fill: opaque `popover` while glass is off; Bezel
/// `glass_overlay` when glass is on.
fn frost(cx: &App) -> gpui::Hsla {
    if GLASS_ALPHA >= 1.0 {
        return cx.theme().popover;
    }
    if cx.theme().is_dark() {
        // Bezel glass_overlay dark ≈ oklch(0.33 0 0 / 34%)
        gpui::hsla(0.0, 0.0, 0.33, 0.34)
    } else {
        cx.theme().popover.opacity(0.85)
    }
}

fn frost_card(id: impl Into<ElementId>, width: f32, cx: &App) -> gpui::Stateful<gpui::Div> {
    let radius = px(SURFACE_RADIUS);
    let fill = frost(cx);
    div()
        .id(id)
        .relative()
        .w(px(width))
        .max_h(px(SLASH_MAX_H))
        .overflow_y_scroll()
        .p(px(6.0))
        .rounded(radius)
        .border_1()
        .border_color(hairline(cx, 0.12))
        .bg(fill)
        .text_color(cx.theme().foreground)
        .font_family(EditorPalette::from_app(cx).font_sans)
}

impl Editor {
    pub(super) fn slash_menu(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let slash = self.slash.as_ref()?;
        let (point, line) = self.layouts.position(&slash.at)?;
        let origin = gpui::point(point.x, point.y + line + px(4.0));
        let labels: Vec<_> = crate::slash::items()
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        let filtered = slash.filter.filtered().to_vec();
        let active = slash.filter.active();

        let rows = filtered.into_iter().enumerate().map(|(view_ix, item_ix)| {
            let label = labels[item_ix];
            let lit = active == Some(view_ix);
            let kind = crate::slash::items()[item_ix].1;
            div()
                .id(ElementId::Name(format!("slash-row-{view_ix}").into()))
                .w_full()
                .px(px(8.0))
                .py(px(5.0))
                .rounded(px(6.0))
                .when(lit, |el| el.bg(ink(cx, 0.08)))
                .child(label)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.confirm_slash(Some(kind), cx);
                    }),
                )
        });

        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(origin)
                    .anchor(gpui::Anchor::TopLeft)
                    .snap_to_window_with_margin(px(8.0))
                    .child(
                        frost_card(SLASH_MENU, SLASH_WIDTH, cx)
                            .debug_selector(|| SLASH_MENU.into())
                            .occlude()
                            .children(rows),
                    ),
            )
            .priority(1)
            .into_any_element(),
        )
    }

    pub(super) fn paste_menu(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let pasted = self.pasted.as_ref()?;
        let (point, line) = self.layouts.position(&pasted.at)?;
        let origin = gpui::point(point.x, point.y + line + px(4.0));
        let active = pasted.active;
        let rows: Vec<_> = pasted
            .rows
            .iter()
            .enumerate()
            .map(|(ix, choice)| {
                let lit = ix == active;
                let choice = *choice;
                div()
                    .id(ElementId::Name(format!("paste-row-{ix}").into()))
                    .w_full()
                    .px(px(8.0))
                    .py(px(5.0))
                    .rounded(px(6.0))
                    .when(lit, |el| el.bg(ink(cx, 0.08)))
                    .child(choice.label())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.confirm_paste(choice, cx);
                        }),
                    )
            })
            .collect();

        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(origin)
                    .anchor(gpui::Anchor::TopLeft)
                    .snap_to_window_with_margin(px(8.0))
                    .child(
                        frost_card(PASTE_MENU, PASTE_WIDTH, cx)
                            .debug_selector(|| PASTE_MENU.into())
                            .occlude()
                            .children(rows),
                    ),
            )
            .priority(1)
            .into_any_element(),
        )
    }

    /// Floating format toolbar below the selection (`format-toolbar`).
    pub(super) fn format_toolbar(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let bounds = self.selection_bounds()?;
        let origin = gpui::point(
            bounds.origin.x,
            bounds.origin.y + bounds.size.height + px(6.0),
        );
        let selection = self.selection.clone();
        let marks = [
            ("B", Mark::Bold),
            ("I", Mark::Italic),
            ("S", Mark::Strike),
            ("<>", Mark::Code),
        ];
        let buttons = marks.map(|(glyph, mark)| {
            let lit = self.covered_by(selection.clone(), mark.key());
            div()
                .id(ElementId::Name(format!("bubble-{glyph}").into()))
                .px(px(7.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .when(lit, |el| el.bg(ink(cx, 0.10)))
                .child(glyph)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.toggle_mark(mark, cx);
                    }),
                )
        });

        let comment = div()
            .id("bubble-comment")
            .px(px(7.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .child("Comment")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    let _ = this.add_comment(String::new(), cx);
                }),
            );

        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(origin)
                    .anchor(gpui::Anchor::TopLeft)
                    .snap_to_window_with_margin(px(8.0))
                    .child(
                        div()
                            .id(FORMAT_TOOLBAR)
                            .debug_selector(|| FORMAT_TOOLBAR.into())
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(2.0))
                            .p(px(4.0))
                            .rounded(px(SURFACE_RADIUS))
                            .border_1()
                            .border_color(hairline(cx, 0.12))
                            .bg(frost(cx))
                            .occlude()
                            .children(buttons)
                            .child(
                                div()
                                    .w(px(1.0))
                                    .h(px(16.0))
                                    .mx(px(3.0))
                                    .bg(hairline(cx, 0.14)),
                            )
                            .child(comment),
                    ),
            )
            .priority(1)
            .into_any_element(),
        )
    }

    /// Hit target over a fence's language label — opens [`Self::language_menu`].
    pub(super) fn language_chip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = [self.hovered.clone(), Some(self.selection.head().id.clone())]
            .into_iter()
            .flatten()
            .find(|id| {
                self.snapshots()
                    .iter()
                    .any(|s| &s.id == id && s.block_type == BlockType::Code)
            })?;
        let bounds = self.layouts.language_bounds(&id)?;
        let anchor = gpui::point(
            bounds.origin.x - px(CHIP_PAD_X),
            bounds.origin.y + bounds.size.height + px(CHIP_PAD_Y),
        );
        let open_id = id.clone();
        Some(
            div()
                .id("language-chip")
                .absolute()
                .left(bounds.origin.x - self.origin.x - px(CHIP_PAD_X))
                .top(bounds.origin.y - self.origin.y - px(CHIP_PAD_Y))
                .w(bounds.size.width + px(2.0 * CHIP_PAD_X))
                .h(bounds.size.height + px(2.0 * CHIP_PAD_Y))
                .rounded(px(4.0))
                .cursor(CursorStyle::PointingHand)
                .hover(|el| el.bg(ink(cx, 0.06)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.press_claimed = true;
                        this.language_menu = Some((open_id.clone(), anchor));
                        cx.notify();
                    }),
                )
                .into_any_element(),
        )
    }

    /// Languages the highlighter knows (or the shipped default list).
    pub(super) fn language_menu(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (id, at) = self.language_menu.as_ref()?;
        let id = id.clone();
        let at = *at;
        let current = self
            .snapshots()
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.language.clone());
        let _ = languages(cx); // keep highlighter install live
        let names = menu_languages(cx);

        let mut rows: Vec<AnyElement> = Vec::with_capacity(names.len() + 1);
        let plain_lit = current.is_none();
        let plain_id = id.clone();
        rows.push(
            div()
                .id("lang-row-plain")
                .w_full()
                .px(px(8.0))
                .py(px(5.0))
                .rounded(px(6.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .when(plain_lit, |el| el.bg(ink(cx, 0.08)))
                .child(PLAIN_LANGUAGE)
                .when(plain_lit, |el| {
                    el.child(icons::check().text_color(cx.theme().foreground))
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.language_menu = None;
                        this.set_language(plain_id.clone(), None, cx);
                    }),
                )
                .into_any_element(),
        );
        for name in names {
            let lit = current.as_deref() == Some(name.as_ref());
            let tag = name.to_string();
            let set_id = id.clone();
            let label = name.clone();
            rows.push(
                div()
                    .id(ElementId::Name(format!("lang-row-{label}").into()))
                    .w_full()
                    .px(px(8.0))
                    .py(px(5.0))
                    .rounded(px(6.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .when(lit, |el| el.bg(ink(cx, 0.08)))
                    .child(label.clone())
                    .when(lit, |el| {
                        el.child(icons::check().text_color(cx.theme().foreground))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.language_menu = None;
                            this.set_language(set_id.clone(), Some(tag.clone()), cx);
                        }),
                    )
                    .into_any_element(),
            );
        }

        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(at)
                    .anchor(gpui::Anchor::TopLeft)
                    .snap_to_window_with_margin(px(8.0))
                    .child(
                        frost_card(LANGUAGE_MENU, LANG_WIDTH, cx)
                            .debug_selector(|| LANGUAGE_MENU.into())
                            .occlude()
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.language_menu = None;
                                cx.notify();
                            }))
                            .children(rows),
                    ),
            )
            .priority(1)
            .into_any_element(),
        )
    }

    pub(super) fn block_handle(&self, focused: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self
            .lifted
            .as_ref()
            .map(|(from, _)| from.clone())
            .or_else(|| self.hovered.clone())
            .or_else(|| focused.then(|| self.selection.head().id.clone()))?;
        let bounds = self.layouts.block_bounds(&id)?;
        let top = match self.layouts.first_row(&id) {
            Some((row, line)) => row + (line - px(HANDLE_SIZE)) / 2.0,
            None => bounds.origin.y,
        };
        let inset = crate::layout::Layout::of(cx).text_inset;
        let drag_id = id.clone();
        Some(
            div()
                .id(BLOCK_HANDLE)
                .debug_selector(|| BLOCK_HANDLE.into())
                .absolute()
                .left(bounds.origin.x - self.origin.x - px(inset))
                .top(top - self.origin.y)
                .w(px(HANDLE_SIZE))
                .h(px(HANDLE_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .cursor(CursorStyle::OpenHand)
                .text_color(ink(cx, 0.35))
                .hover(|el| el.bg(ink(cx, 0.08)).text_color(ink(cx, 0.55)))
                .child("⠿")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.press_claimed = true;
                        this.lifted = Some((drag_id.clone(), drag_id.clone()));
                        this.block_menu = None;
                        cx.notify();
                    }),
                )
                .into_any_element(),
        )
    }

    pub(super) fn drop_indicator(&self, cx: &App) -> Option<AnyElement> {
        let target = self
            .lifted
            .as_ref()
            .filter(|(from, to)| from != to)
            .map(|(_, to)| to.clone())
            .or_else(|| self.dropping.clone())?;
        let bounds = self.layouts.block_bounds(&target)?;
        let y = bounds.origin.y - self.origin.y;
        Some(
            div()
                .absolute()
                .left(px(0.0))
                .top(y)
                .w_full()
                .h(px(2.0))
                .bg(cx.theme().accent)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::GLASS_ALPHA;

    #[test]
    fn glass_alpha_opaque_until_backdrop_blur_ships() {
        assert!(
            (GLASS_ALPHA - 1.0).abs() < f32::EPSILON,
            "raise GLASS_ALPHA only after Window::paint_backdrop_blur blurs on this pin"
        );
    }
}
