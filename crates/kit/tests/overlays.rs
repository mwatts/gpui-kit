use gpui_kit::component::{
    Root, WindowExt,
    button::Button,
    dialog::{DialogAction, DialogButtonProps, DialogFooter},
    input::{Input, InputState},
    notification::Notification,
};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AppContext, Context, Entity, Role, TestAppContext, Window, div, prelude::*, px, size,
};
use std::{cell::Cell, rc::Rc, time::Duration};

struct Workspace {
    saved: Entity<InputState>,
    draft: Entity<InputState>,
}
impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialogs = Root::render_dialog_layer(window, cx);
        let sheets = Root::render_sheet_layer(window, cx);
        let notifications = Root::render_notification_layer(window, cx);
        let draft = self.draft.clone();
        let saved = self.saved.clone();
        div()
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(Input::new(&self.saved).id("name"))
            .child(
                Button::new("edit")
                    .label("Edit…")
                    .on_click(move |_, window, cx| {
                        let draft = draft.clone();
                        let saved = saved.clone();
                        window.open_dialog(cx, move |dialog, _, _| {
                            let draft = draft.clone();
                            let saved = saved.clone();
                            dialog
                                .title("Edit profile")
                                .child(Input::new(&draft).id("name"))
                                .footer(DialogFooter::new().child(
                                    DialogAction::new().child(Button::new("ok").label("Save")),
                                ))
                                .on_ok(move |_, window, cx| {
                                    if draft.read(cx).value().is_empty() {
                                        return false;
                                    }
                                    let value = draft.read(cx).value();
                                    saved
                                        .update(cx, |input, cx| input.set_value(value, window, cx));
                                    window.push_notification(
                                        Notification::new()
                                            .message("Profile saved")
                                            .autohide(false),
                                        cx,
                                    );
                                    true
                                })
                        });
                    }),
            )
            .child(
                Button::new("inspect")
                    .label("Inspect…")
                    .on_click(|_, window, cx| {
                        window.open_sheet(cx, |sheet, _, _| {
                            sheet.title("Inspector").child("Details")
                        });
                    }),
            )
            .child(
                Button::new("notify")
                    .label("Notify")
                    .on_click(|_, window, cx| {
                        window.push_notification(
                            Notification::new().message("Updated").autohide(true),
                            cx,
                        );
                    }),
            )
            .children(dialogs)
            .children(sheets)
            .children(notifications)
    }
}

#[derive(Clone, Default)]
struct DialogCounters {
    cancel_attempts: Rc<Cell<usize>>,
    confirmations: Rc<Cell<usize>>,
    closes: Rc<Cell<usize>>,
}

struct DialogContractWorkspace {
    counters: DialogCounters,
}

impl Render for DialogContractWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialogs = Root::render_dialog_layer(window, cx);
        let configured_counters = self.counters.clone();

        div()
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                Button::new("open-configured")
                    .label("Delete note")
                    .on_click(move |_, window, cx| {
                        let counters = configured_counters.clone();
                        window.open_dialog(cx, move |dialog, _, _| {
                            let cancel_attempts = counters.cancel_attempts.clone();
                            let confirmations = counters.confirmations.clone();
                            let closes = counters.closes.clone();
                            dialog
                                .title("Delete note?")
                                .button_props(
                                    DialogButtonProps::default()
                                        .cancel_text("Keep")
                                        .ok_text("Delete")
                                        .show_cancel(true),
                                )
                                .on_cancel(move |_, _, _| {
                                    cancel_attempts.set(cancel_attempts.get() + 1);
                                    false
                                })
                                .on_ok(move |_, _, _| {
                                    confirmations.set(confirmations.get() + 1);
                                    true
                                })
                                .on_close(move |_, _, _| closes.set(closes.get() + 1))
                                .child("This action cannot be undone.")
                        });
                    }),
            )
            .child(
                Button::new("open-passive")
                    .label("Show information")
                    .on_click(|_, window, cx| {
                        window.open_dialog(cx, |dialog, _, _| {
                            dialog
                                .title(String::from("Passive information"))
                                .child("No response is required.")
                        });
                    }),
            )
            .child(
                Button::new("open-custom")
                    .label("Show custom dialog")
                    .on_click(|_, window, cx| {
                        window.open_dialog(cx, |dialog, _, _| {
                            dialog
                                .title(div().child("Custom confirmation"))
                                .accessibility_label("Custom confirmation")
                                .button_props(
                                    DialogButtonProps::default()
                                        .show_cancel(true)
                                        .ok_text("Generated action"),
                                )
                                .footer(
                                    DialogFooter::new()
                                        .child(Button::new("custom-action").label("Custom action")),
                                )
                        });
                    }),
            )
            .child(
                Button::new("open-alert")
                    .label("Show alert")
                    .on_click(|_, window, cx| {
                        window.open_alert_dialog(cx, |alert, _, _| {
                            alert.title("Attention").description("Read this alert.")
                        });
                    }),
            )
            .children(dialogs)
    }
}

#[gpui_kit::test]
async fn dialog_validates_scoped_input_saves_and_dismisses_notification(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let handle = cx.open_window(size(px(800.), px(700.)), |window, cx| {
        let view = cx.new(|cx| Workspace {
            saved: cx.new(|cx| InputState::new(window, cx)),
            draft: cx.new(|cx| InputState::new(window, cx)),
        });
        Root::new(view, window, cx)
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("name", cx);
        window.click("edit", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        let host = window.find(("dialog-host", 0usize));
        assert_eq!(host.role(), Some(Role::Dialog));
        assert_eq!(host.label(), Some("Edit profile"));
        let dialog = window.within("dialog").find(0usize).bounds();
        assert!(dialog.left() >= px(0.) && dialog.right() <= window.viewport_size().width);
        assert!(dialog.top() >= px(0.) && dialog.bottom() <= window.viewport_size().height);
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("notification").is_none());
        assert!(
            window.find("dialog").visible(),
            "empty input must prevent confirmation"
        );
        window.within("dialog").click("name", cx);
        window.within("dialog").input("Ada 中文", cx);
        assert_eq!(
            window.within("dialog").find("name").value(),
            Some("Ada 中文")
        );
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_none() && window.try_find("notification").is_some()
    })
    .await;
    // Toast entrance also uses GPUI's wall-clock Animation (400 ms).
    std::thread::sleep(Duration::from_millis(410));
    cx.update_window(handle.into(), |_, window, cx| {
        assert_eq!(window.find("name").value(), Some("Ada 中文"));
        assert!(window.find("notification").visible());
        window.hover("notification", cx);
        window.within("notification").click("close", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("notification").is_none()
    })
    .await;
}

#[gpui_kit::test]
async fn escape_dismisses_dialog_and_sheet_and_restores_focus(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let handle = cx.open_window(size(px(800.), px(700.)), |window, cx| {
        let view = cx.new(|cx| Workspace {
            saved: cx.new(|cx| InputState::new(window, cx)),
            draft: cx.new(|cx| InputState::new(window, cx)),
        });
        Root::new(view, window, cx)
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("name", cx);
        window.click("edit", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        window.press("escape", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_none() && window.find("name").focused() == Some(true)
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        assert_eq!(window.find("name").focused(), Some(true));
        window.click("inspect", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("sheet-content").is_some()
    })
    .await;
    // GPUI's non-synced Animation uses std::time::Instant, not the test clock.
    // Finish the actual 150 ms entrance before asserting its final geometry.
    std::thread::sleep(Duration::from_millis(160));
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let host = window.find("sheet-host");
        assert_eq!(host.role(), Some(Role::Dialog));
        assert_eq!(host.label(), Some("Inspector"));
        let sheet = window.find("sheet-content").bounds();
        assert!(
            sheet.size.width > px(0.) && sheet.right() <= window.viewport_size().width,
            "{sheet:?}"
        );
        window.press("escape", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("sheet").is_none() && window.try_find("sheet-content").is_none()
    })
    .await;
    cx.update_window(handle.into(), |_, window, _| {
        assert_eq!(window.find("name").focused(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
async fn imperative_dialog_button_props_render_named_actions_and_honor_veto(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_kit::init);
    let counters = DialogCounters::default();
    let view_counters = counters.clone();
    let handle = cx.open_window(size(px(800.), px(700.)), move |window, cx| {
        let view = cx.new(move |_| DialogContractWorkspace {
            counters: view_counters,
        });
        Root::new(view, window, cx)
    });

    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("open-configured", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_some()
    })
    .await;

    cx.update_window(handle.into(), |_, window, cx| {
        let host = window.find(("dialog-host", 0usize));
        assert_eq!(host.role(), Some(Role::Dialog));
        assert_eq!(host.label(), Some("Delete note?"));

        let dialog = window.within("dialog");
        let cancel = dialog.find("cancel");
        assert_eq!(cancel.role(), Some(Role::Button));
        assert_eq!(cancel.label(), Some("Keep"));
        assert_ne!(cancel.disabled(), Some(true));
        let ok = dialog.find("ok");
        assert_eq!(ok.role(), Some(Role::Button));
        assert_eq!(ok.label(), Some("Delete"));
        assert_ne!(ok.disabled(), Some(true));

        window.within("dialog").click("cancel", cx);
        assert!(
            window.find("dialog").visible(),
            "cancel callback vetoes close"
        );
    })
    .unwrap();
    assert_eq!(counters.cancel_attempts.get(), 1);
    assert_eq!(counters.confirmations.get(), 0);
    assert_eq!(counters.closes.get(), 0);

    cx.update_window(handle.into(), |_, window, cx| {
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_none()
    })
    .await;
    assert_eq!(counters.cancel_attempts.get(), 1);
    assert_eq!(counters.confirmations.get(), 1);
    assert_eq!(counters.closes.get(), 1);
}

#[gpui_kit::test]
async fn modal_names_cover_text_custom_and_alert_titles_without_inventing_actions(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_kit::init);
    let handle = cx.open_window(size(px(800.), px(700.)), |window, cx| {
        let view = cx.new(|_| DialogContractWorkspace {
            counters: DialogCounters::default(),
        });
        Root::new(view, window, cx)
    });

    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("open-passive", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        let host = window.find(("dialog-host", 0usize));
        assert_eq!(host.role(), Some(Role::Dialog));
        assert_eq!(host.label(), Some("Passive information"));
        let dialog = window.within("dialog");
        assert!(dialog.try_find("ok").is_none());
        assert!(dialog.try_find("cancel").is_none());
        window.press("escape", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_none()
    })
    .await;

    cx.update_window(handle.into(), |_, window, cx| {
        window.click("open-custom", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        let host = window.find(("dialog-host", 0usize));
        assert_eq!(host.role(), Some(Role::Dialog));
        assert_eq!(host.label(), Some("Custom confirmation"));
        let dialog = window.within("dialog");
        assert_eq!(dialog.find("custom-action").label(), Some("Custom action"));
        assert!(dialog.try_find("ok").is_none());
        assert!(dialog.try_find("cancel").is_none());
        window.press("escape", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_none()
    })
    .await;

    cx.update_window(handle.into(), |_, window, cx| {
        window.click("open-alert", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        let host = window.find(("dialog-host", 0usize));
        assert_eq!(host.role(), Some(Role::AlertDialog));
        assert_eq!(host.label(), Some("Attention"));
        let ok = window.within("dialog").find("ok");
        assert_eq!(ok.role(), Some(Role::Button));
        assert_eq!(ok.label(), Some("OK"));
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("dialog").is_none()
    })
    .await;
}

#[gpui_kit::test]
async fn notification_auto_dismisses_after_its_timer(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let handle = cx.open_window(size(px(800.), px(700.)), |window, cx| {
        let view = cx.new(|cx| Workspace {
            saved: cx.new(|cx| InputState::new(window, cx)),
            draft: cx.new(|cx| InputState::new(window, cx)),
        });
        Root::new(view, window, cx)
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("notify", cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("notification").is_some()
    })
    .await;
    // The default timeout is five seconds; the notification must remain
    // mounted before that deadline, not merely disappear eventually.
    for _ in 0..40 {
        cx.background_executor
            .advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("notification").is_some());
        })
        .unwrap();
    }
    cx.wait_for(handle.into(), Duration::from_secs(10), |window, _| {
        window.try_find("notification").is_none()
    })
    .await;
}
