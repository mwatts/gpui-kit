//! Event-triggered native window effects.
//!
//! These registrations deliberately render real buttons. Dialogs, sheets, and
//! notifications are opened only by the button's GPUI click event, never while
//! the script tree is materialized. Custom footer/action IR is intentionally
//! omitted; the public surface is limited to native contracts we can close.

#![allow(clippy::double_ended_iterator_last)] // Script methods are folded in source order; last-call-wins is intentional.

use std::{cell::Cell, rc::Rc, sync::Arc};

use gpui_component::{
    Placement, WindowExt as _,
    button::Button,
    dialog::DialogButtonProps,
    notification::{Notification, NotificationType},
};
use gpui_shell::{
    ArgumentDescriptor, ArgumentSchema, ComponentArgument, ComponentCallback,
    ComponentCallbackArgument, ComponentDescriptor, ComponentMaterializer, ComponentPayload,
    ComponentRegistry, ConstructorDescriptor, MaterializeRequest, MethodDescriptor, RegistryError,
    anyhow,
    gpui::{
        self, AppContext as _, IntoElement as _, ParentElement as _, Refineable as _, Styled as _,
    },
};

#[derive(Clone)]
struct Trigger {
    id: String,
    label: String,
    reporter: ComponentArgument,
}

#[derive(Clone)]
enum Op {
    Title(String),
    Description(String),
    Placement(Placement),
    Kind(NotificationType),
    Autohide(bool),
    ShowCancel(bool),
    OnOk(ComponentArgument),
    OnCancel(ComponentArgument),
    OnClose(ComponentArgument),
    OnClick(ComponentArgument),
}

#[derive(Clone, Copy)]
enum Kind {
    Dialog,
    AlertDialog,
    Sheet,
    Notification,
}

struct Materializer(Kind);

fn invoke(
    callback: &Option<ComponentCallback>,
    label: &'static str,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) {
    if let Some(callback) = callback {
        callback.invoke_and_report_with(label, &[], window, cx);
    }
}

fn take_named_slots(request: &mut MaterializeRequest<'_>, names: &[&str]) -> usize {
    names
        .iter()
        .map(|name| request.take_slot_factories(name).len() + request.take_slots(name).len())
        .sum()
}

fn reject_named_slots(
    request: &mut MaterializeRequest<'_>,
    names: &[&str],
    message: &'static str,
) -> anyhow::Result<()> {
    if take_named_slots(request, names) != 0 {
        #[cfg(test)]
        test_probe::slot_rejection(message);
        anyhow::bail!(message);
    }
    Ok(())
}

fn take_content_factory(
    request: &mut MaterializeRequest<'_>,
) -> anyhow::Result<gpui_shell::ComponentElementFactory> {
    let mut factories = request.take_slot_factories("content");
    if factories.len() != 1 {
        let message = "Dialog and Sheet require exactly one content(element) named slot";
        #[cfg(test)]
        test_probe::slot_rejection(message);
        anyhow::bail!(message);
    }
    Ok(factories.remove(0))
}

fn report_factory_error(
    reporter: &ComponentCallback,
    surface: &str,
    error: &anyhow::Error,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> String {
    let message = format!("Failed to render {surface} content: {error:#}");
    if let Err(reporter_error) = reporter.invoke_with(
        &[ComponentCallbackArgument::String(message.clone())],
        window,
        cx,
    ) {
        let diagnosis = format!("{message}; effect error reporter also failed: {reporter_error:#}");
        eprintln!("{diagnosis}");
        #[cfg(test)]
        test_probe::reporter_failure(diagnosis);
    }
    message
}

/// The newest content of one Dialog or Sheet trigger. Every materialize of
/// the trigger replaces it, so an overlay that is already open draws from the
/// current surface model rather than the one it opened with.
struct LatestContent {
    factory: gpui_shell::ComponentElementFactory,
    title: Option<String>,
}

/// The content entity of each open Dialog or Sheet, by window and trigger id.
/// Entries are weak: an overlay that closed drops its content, and the entry
/// is pruned the next time one is recorded.
#[derive(Default)]
struct OpenContents(
    std::collections::HashMap<(gpui::WindowId, String), gpui::WeakEntity<LatestContent>>,
);

impl gpui::Global for OpenContents {}

/// Records the content of an overlay that `id` is opening in `window`.
fn open_content(
    window: &gpui::Window,
    id: &str,
    factory: gpui_shell::ComponentElementFactory,
    title: Option<String>,
    cx: &mut gpui::App,
) -> gpui::Entity<LatestContent> {
    let latest = cx.new(|_| LatestContent { factory, title });
    let contents = cx.default_global::<OpenContents>();
    contents.0.retain(|_, content| content.upgrade().is_some());
    contents.0.insert(
        (window.window_handle().window_id(), id.to_string()),
        latest.downgrade(),
    );
    latest
}

/// Hands a newly materialized trigger's content to its open overlay, if one
/// is open. Without the render authority (a materialize outside a window
/// render) there is no open overlay to update.
fn refresh_open_content(
    request: &mut MaterializeRequest<'_>,
    id: &str,
    factory: &gpui_shell::ComponentElementFactory,
    title: &Option<String>,
) {
    let _ = request.with_window_app(|window, cx| {
        let key = (window.window_handle().window_id(), id.to_string());
        let open = cx
            .try_global::<OpenContents>()
            .and_then(|contents| contents.0.get(&key))
            .and_then(|content| content.upgrade());
        if let Some(latest) = open {
            latest.update(cx, |latest, _| {
                latest.factory = factory.clone();
                latest.title = title.clone();
            });
        }
        Ok(())
    });
}

/// The body of an open Dialog or Sheet. It builds from the newest factory on
/// every draw; the surface draws first in the frame, so the body follows it.
struct OverlayContent {
    latest: gpui::Entity<LatestContent>,
    reporter: ComponentCallback,
    surface: &'static str,
    reported: Rc<Cell<bool>>,
}

impl gpui::Render for OverlayContent {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let factory = self.latest.read(cx).factory.clone();
        match factory.build(window, cx) {
            Ok(element) => element,
            Err(error) => {
                let message = format!("Failed to render {} content: {error:#}", self.surface);
                if !self.reported.replace(true) {
                    report_factory_error(&self.reporter, self.surface, &error, window, cx);
                }
                gpui::div().child(message).into_any_element()
            }
        }
    }
}

impl ComponentMaterializer for Materializer {
    fn materialize(&self, mut request: MaterializeRequest<'_>) -> anyhow::Result<gpui::AnyElement> {
        let trigger = request
            .payload()
            .downcast_ref::<Trigger>()
            .ok_or_else(|| anyhow::anyhow!("window effect received an incompatible payload"))?
            .clone();
        anyhow::ensure!(
            request.children_len() == 0,
            "window effect triggers do not accept children"
        );
        let reporter = request.resolve_callback(&trigger.reporter)?;
        let effects = reporter.window_effects();
        let factory_reporter = reporter.clone();
        let operations = request
            .methods()
            .filter_map(|method| method.payload().downcast_ref::<Op>().cloned())
            .collect::<Vec<_>>();
        let callback = |pick: fn(&Op) -> Option<&ComponentArgument>| -> anyhow::Result<Option<ComponentCallback>> {
            operations.iter().filter_map(pick).last().map(|value| request.resolve_callback(value)).transpose()
        };
        let on_ok = callback(|op| {
            if let Op::OnOk(value) = op {
                Some(value)
            } else {
                None
            }
        })?;
        let on_cancel = callback(|op| {
            if let Op::OnCancel(value) = op {
                Some(value)
            } else {
                None
            }
        })?;
        let on_close = callback(|op| {
            if let Op::OnClose(value) = op {
                Some(value)
            } else {
                None
            }
        })?;
        let on_click = callback(|op| {
            if let Op::OnClick(value) = op {
                Some(value)
            } else {
                None
            }
        })?;
        let content = match self.0 {
            Kind::Dialog | Kind::Sheet => {
                reject_named_slots(
                    &mut request,
                    &["trigger", "header", "footer"],
                    "Dialog and Sheet accept only the content named slot",
                )?;
                let factory = take_content_factory(&mut request)?;
                let title = operations
                    .iter()
                    .filter_map(|op| {
                        if let Op::Title(value) = op {
                            Some(value.clone())
                        } else {
                            None
                        }
                    })
                    .last();
                refresh_open_content(&mut request, &trigger.id, &factory, &title);
                Some((factory, title))
            }
            Kind::AlertDialog | Kind::Notification => {
                reject_named_slots(
                    &mut request,
                    &["content", "trigger", "header", "footer"],
                    "AlertDialog and Notification do not accept named slots",
                )?;
                None
            }
        };
        let style = request.take_style();
        let kind = self.0;
        let id = trigger.id.clone();
        let mut button =
            Button::new(trigger.id)
                .label(trigger.label)
                .on_click(move |_, window, cx| {
                    let operations = operations.clone();
                    let content = content.clone();
                    let on_ok = on_ok.clone();
                    let on_cancel = on_cancel.clone();
                    let on_close = on_close.clone();
                    let on_click = on_click.clone();
                    let factory_reporter = factory_reporter.clone();
                    let key = format!("window-effect:{id}");
                    let _ = effects.event(window, cx, |event| {
                        event.run_once(key, |window, cx| {
                            let factory_error_reported = Rc::new(Cell::new(false));
                            match kind {
                                Kind::Dialog => {
                                    let (factory, title) =
                                        content.clone().expect("validated dialog content");
                                    let latest = open_content(window, &id, factory, title, cx);
                                    let body = cx.new(|_| OverlayContent {
                                        latest: latest.clone(),
                                        reporter: factory_reporter.clone(),
                                        surface: "Dialog",
                                        reported: factory_error_reported.clone(),
                                    });
                                    window.open_dialog(cx, move |mut dialog, _window, cx| {
                                        if let Some(title) = latest.read(cx).title.clone() {
                                            dialog = dialog.title(title);
                                        }
                                        let body = body.clone();
                                        dialog = dialog.content(move |content, _, _| {
                                            content.child(body.clone())
                                        });
                                        let ok = on_ok.clone();
                                        let cancel = on_cancel.clone();
                                        let close = on_close.clone();
                                        dialog
                                            .on_ok(move |_, window, cx| {
                                                invoke(
                                                    &ok,
                                                    "Dialog.on_ok callback failed",
                                                    window,
                                                    cx,
                                                );
                                                true
                                            })
                                            .on_cancel(move |_, window, cx| {
                                                invoke(
                                                    &cancel,
                                                    "Dialog.on_cancel callback failed",
                                                    window,
                                                    cx,
                                                );
                                                true
                                            })
                                            .on_close(move |_, window, cx| {
                                                invoke(
                                                    &close,
                                                    "Dialog.on_close callback failed",
                                                    window,
                                                    cx,
                                                )
                                            })
                                    });
                                }
                                Kind::AlertDialog => {
                                    let title = operations
                                        .iter()
                                        .filter_map(|op| {
                                            if let Op::Title(value) = op {
                                                Some(value.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .last();
                                    let description = operations
                                        .iter()
                                        .filter_map(|op| {
                                            if let Op::Description(value) = op {
                                                Some(value.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .last();
                                    let show_cancel = operations
                                        .iter()
                                        .filter_map(|op| {
                                            if let Op::ShowCancel(value) = op {
                                                Some(*value)
                                            } else {
                                                None
                                            }
                                        })
                                        .last()
                                        .unwrap_or(false);
                                    window.open_alert_dialog(cx, move |mut alert, _, _| {
                                        if let Some(title) = title.clone() {
                                            alert = alert.title(title);
                                        }
                                        if let Some(description) = description.clone() {
                                            alert = alert.description(description);
                                        }
                                        let ok = on_ok.clone();
                                        let cancel = on_cancel.clone();
                                        let close = on_close.clone();
                                        alert
                                            .button_props(
                                                DialogButtonProps::default()
                                                    .show_cancel(show_cancel)
                                                    .on_ok(move |_, window, cx| {
                                                        invoke(
                                                            &ok,
                                                            "AlertDialog.on_ok callback failed",
                                                            window,
                                                            cx,
                                                        );
                                                        true
                                                    })
                                                    .on_cancel(move |_, window, cx| {
                                                        invoke(
                                                            &cancel,
                                                            "AlertDialog.on_cancel callback failed",
                                                            window,
                                                            cx,
                                                        );
                                                        true
                                                    }),
                                            )
                                            .on_close(move |_, window, cx| {
                                                invoke(
                                                    &close,
                                                    "AlertDialog.on_close callback failed",
                                                    window,
                                                    cx,
                                                )
                                            })
                                    });
                                }
                                Kind::Sheet => {
                                    let (factory, title) =
                                        content.clone().expect("validated sheet content");
                                    let latest = open_content(window, &id, factory, title, cx);
                                    let body = cx.new(|_| OverlayContent {
                                        latest: latest.clone(),
                                        reporter: factory_reporter.clone(),
                                        surface: "Sheet",
                                        reported: factory_error_reported.clone(),
                                    });
                                    let placement = operations
                                        .iter()
                                        .filter_map(|op| {
                                            if let Op::Placement(value) = op {
                                                Some(*value)
                                            } else {
                                                None
                                            }
                                        })
                                        .last()
                                        .unwrap_or(Placement::Right);
                                    window.open_sheet_at(
                                        placement,
                                        cx,
                                        move |mut sheet, _window, cx| {
                                            if let Some(title) = latest.read(cx).title.clone() {
                                                sheet = sheet.title(title);
                                            }
                                            sheet = sheet.child(body.clone());
                                            let close = on_close.clone();
                                            sheet.on_close(move |_, window, cx| {
                                                invoke(
                                                    &close,
                                                    "Sheet.on_close callback failed",
                                                    window,
                                                    cx,
                                                )
                                            })
                                        },
                                    );
                                }
                                Kind::Notification => {
                                    let title = operations
                                        .iter()
                                        .filter_map(|op| {
                                            if let Op::Title(value) = op {
                                                Some(value.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .last();
                                    let message = operations
                                        .iter()
                                        .filter_map(|op| {
                                            if let Op::Description(value) = op {
                                                Some(value.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .last();
                                    let kind = operations
                                        .iter()
                                        .filter_map(|op| {
                                            if let Op::Kind(value) = op {
                                                Some(*value)
                                            } else {
                                                None
                                            }
                                        })
                                        .last()
                                        .unwrap_or_default();
                                    let autohide = operations
                                        .iter()
                                        .filter_map(|op| {
                                            if let Op::Autohide(value) = op {
                                                Some(*value)
                                            } else {
                                                None
                                            }
                                        })
                                        .last()
                                        .unwrap_or(true);
                                    let mut notification = Notification::new()
                                        .id1::<Materializer>(id.clone())
                                        .with_type(kind)
                                        .autohide(autohide);
                                    if let Some(title) = title {
                                        notification = notification.title(title);
                                    }
                                    if let Some(message) = message {
                                        notification = notification.message(message);
                                    }
                                    let click = on_click.clone();
                                    let close = on_close.clone();
                                    notification = notification
                                        .on_click(move |_, window, cx| {
                                            invoke(
                                                &click,
                                                "Notification.on_click callback failed",
                                                window,
                                                cx,
                                            )
                                        })
                                        .on_close(move |window, cx| {
                                            invoke(
                                                &close,
                                                "Notification.on_close callback failed",
                                                window,
                                                cx,
                                            )
                                        });
                                    window.push_notification(notification, cx);
                                }
                            }
                            Ok(())
                        })?;
                        Ok(())
                    });
                });
        button.style().refine(&style);
        Ok(button.into_any_element())
    }
}

fn callback_method(name: &'static str, op: fn(ComponentArgument) -> Op) -> MethodDescriptor {
    MethodDescriptor::new(
        name,
        vec![ArgumentDescriptor::new(
            "callback",
            ArgumentSchema::Callback("(cx: Context) => void"),
        )],
        move |args| match args {
            [value @ ComponentArgument::Callback(_)] => {
                Ok(ComponentPayload::new(op(value.clone())))
            }
            _ => Err(format!("{name}(callback) expects a callback")),
        },
    )
}

fn descriptor(
    name: &'static str,
    kind: Kind,
    methods: Vec<MethodDescriptor>,
) -> ComponentDescriptor {
    // These families share one shape, so the shared sentence is the accurate
    // one. It is applied here, where the family is known, rather than left for
    // the registry to invent on everyone's behalf.
    let methods = methods
        .into_iter()
        .map(|method| match method.documentation() {
            Some(_) => method,
            None => method.with_documentation("Configures this native window effect."),
        })
        .collect();
    ComponentDescriptor::new(name, Arc::new(Materializer(kind)))
.with_constructors(vec![ConstructorDescriptor::new(
            name,
            vec![
                ArgumentDescriptor::new("id", ArgumentSchema::String),
                ArgumentDescriptor::new("label", ArgumentSchema::String),
                ArgumentDescriptor::new(
                    "on_effect_error",
                    ArgumentSchema::Callback("(message: string, cx: Context) => void"),
                ),
            ],
            move |args| match args {
                [
                    ComponentArgument::String(id),
                    ComponentArgument::String(label),
                    reporter @ ComponentArgument::Callback(_),
                ] if !id.trim().is_empty() && !label.trim().is_empty() => {
                    Ok(ComponentPayload::new(Trigger {
                        id: id.clone(),
                        label: label.clone(),
                        reporter: reporter.clone(),
                    }))
                }
                _ => Err(format!(
                    "{name}(id, label, on_effect_error) expects two non-empty strings and a callback"
                )),
            },
        )])
.with_methods(methods)
.with_documentation(
            "A real button-triggered native window effect; on_effect_error receives asynchronous effect failures.",
        )
}

fn text(name: &'static str, op: fn(String) -> Op) -> MethodDescriptor {
    MethodDescriptor::new(
        name,
        vec![ArgumentDescriptor::new("text", ArgumentSchema::String)],
        move |args| match args {
            [ComponentArgument::String(value)] if !value.trim().is_empty() => {
                Ok(ComponentPayload::new(op(value.clone())))
            }
            _ => Err(format!("{name}(text) expects non-empty text")),
        },
    )
}

pub(super) fn register(registry: &mut ComponentRegistry) -> Result<(), RegistryError> {
    registry.register(descriptor(
        "Dialog",
        Kind::Dialog,
        vec![
            text("title", Op::Title),
            callback_method("on_ok", Op::OnOk),
            callback_method("on_cancel", Op::OnCancel),
            callback_method("on_close", Op::OnClose),
        ],
    ))?;
    registry.register(descriptor(
        "AlertDialog",
        Kind::AlertDialog,
        vec![
            text("title", Op::Title),
            text("description", Op::Description),
            MethodDescriptor::new(
                "show_cancel",
                vec![ArgumentDescriptor::new("value", ArgumentSchema::Boolean)],
                |args| match args {
                    [ComponentArgument::Boolean(value)] => {
                        Ok(ComponentPayload::new(Op::ShowCancel(*value)))
                    }
                    _ => Err("show_cancel(value) expects boolean".into()),
                },
            ),
            callback_method("on_ok", Op::OnOk),
            callback_method("on_cancel", Op::OnCancel),
            callback_method("on_close", Op::OnClose),
        ],
    ))?;
    registry.register(descriptor(
        "Sheet",
        Kind::Sheet,
        vec![
            text("title", Op::Title),
            MethodDescriptor::new(
                "placement",
                vec![ArgumentDescriptor::new(
                    "placement",
                    ArgumentSchema::Enum(&["top", "right", "bottom", "left"]),
                )],
                |args| match args {
                    [ComponentArgument::Enum(value)] => match value.as_str() {
                        "top" => Ok(Placement::Top),
                        "right" => Ok(Placement::Right),
                        "bottom" => Ok(Placement::Bottom),
                        "left" => Ok(Placement::Left),
                        _ => Err("unsupported placement".into()),
                    }
                    .map(Op::Placement)
                    .map(ComponentPayload::new),
                    _ => Err("placement expects top, right, bottom, or left".into()),
                },
            ),
            callback_method("on_close", Op::OnClose),
        ],
    ))?;
    registry.register(descriptor(
        "Notification",
        Kind::Notification,
        vec![
            text("title", Op::Title),
            text("message", Op::Description),
            MethodDescriptor::new(
                "type",
                vec![ArgumentDescriptor::new(
                    "type",
                    ArgumentSchema::Enum(&["info", "success", "warning", "error"]),
                )],
                |args| match args {
                    [ComponentArgument::Enum(value)] => match value.as_str() {
                        "info" => Ok(NotificationType::Info),
                        "success" => Ok(NotificationType::Success),
                        "warning" => Ok(NotificationType::Warning),
                        "error" => Ok(NotificationType::Error),
                        _ => Err("unsupported notification type".into()),
                    }
                    .map(Op::Kind)
                    .map(ComponentPayload::new),
                    _ => Err("type expects info, success, warning, or error".into()),
                },
            ),
            callback_method("on_click", Op::OnClick),
            callback_method("on_close", Op::OnClose),
            MethodDescriptor::new(
                "autohide",
                vec![ArgumentDescriptor::new("value", ArgumentSchema::Boolean)],
                |args| match args {
                    [ComponentArgument::Boolean(value)] => {
                        Ok(ComponentPayload::new(Op::Autohide(*value)))
                    }
                    _ => Err("autohide(value) expects boolean".into()),
                },
            ),
        ],
    ))?;
    Ok(())
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod test_probe {
    use std::cell::RefCell;

    thread_local! {
        static REPORTER_FAILURES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static SLOT_REJECTIONS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn reporter_failure(error: String) {
        REPORTER_FAILURES.with(|errors| errors.borrow_mut().push(error));
    }

    pub(crate) fn take_reporter_failures() -> Vec<String> {
        REPORTER_FAILURES.with(|errors| std::mem::take(&mut *errors.borrow_mut()))
    }

    pub(super) fn slot_rejection(error: &str) {
        SLOT_REJECTIONS.with(|errors| errors.borrow_mut().push(error.to_owned()));
    }

    pub(crate) fn take_slot_rejections() -> Vec<String> {
        SLOT_REJECTIONS.with(|errors| std::mem::take(&mut *errors.borrow_mut()))
    }
}
