//! Subscription owner for retained form controls.
//!
//! Native events live on the script-created `*State` entity. The keyed `Host`
//! holds the GPUI `Subscription`; later renders only replace the callback cell.
//! Dropping the keyed state drops the subscription. This is the BoundSelect
//! pattern — not Checkbox `on_click`.

use gpui_component::{
    calendar::{CalendarEvent, CalendarState, Date},
    color_picker::{ColorPickerEvent, ColorPickerState},
    date_picker::{DatePickerEvent, DatePickerState},
    input::{InputEvent, InputState, NumberInputEvent, OtpEvent, OtpState},
    slider::{SliderEvent, SliderState, SliderValue},
};
use gpui_shell::{
    ComponentCallback, ComponentCallbackArgument,
    gpui::{self, App, Entity, Subscription, Window},
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
};

#[derive(Clone, Default)]
pub(super) struct FormCallbacks {
    pub on_change: Option<ComponentCallback>,
    pub on_submit: Option<ComponentCallback>,
    pub on_complete: Option<ComponentCallback>,
}

/// One native subscription per script `*State` entity.
///
/// `use_keyed_state` includes the element-id stack in its identity, so a single
/// Input can be keyed twice in one frame (prepaint vs paint). BoundSelect creates
/// its state inside that keyed init, so a second path is a second widget. These
/// controls subscribe to a script-owned state, and a second path would double-fire
/// `on_change` / `on_submit`. The intern keeps one `Subscription`; keyed `Host`
/// values only hold the interned cell so dropping the last path drops it.
struct Interned {
    callback: Rc<RefCell<FormCallbacks>>,
    _subscription: Vec<Subscription>,
}

thread_local! {
    static HOSTS: RefCell<HashMap<String, Weak<Interned>>> = RefCell::new(HashMap::new());
}

pub(super) struct Host {
    _interned: Rc<Interned>,
}

pub(super) fn install<T: 'static>(
    key: String,
    state: Entity<T>,
    callbacks: FormCallbacks,
    subscribe: impl FnOnce(
        Entity<T>,
        Rc<RefCell<FormCallbacks>>,
        &mut Window,
        &mut App,
    ) -> Vec<Subscription>
    + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Entity<T> {
    let interned = HOSTS.with(|hosts| hosts.borrow().get(&key).and_then(Weak::upgrade));
    let interned = if let Some(interned) = interned {
        interned
    } else {
        let callback = Rc::new(RefCell::new(callbacks.clone()));
        let subscription = subscribe(state.clone(), callback.clone(), window, cx);
        let interned = Rc::new(Interned {
            callback,
            _subscription: subscription,
        });
        HOSTS.with(|hosts| {
            let mut hosts = hosts.borrow_mut();
            hosts.retain(|_, weak| weak.strong_count() > 0);
            hosts.insert(key.clone(), Rc::downgrade(&interned));
        });
        interned
    };
    let retained = interned.clone();
    let _: Entity<Host> = window.use_keyed_state(key, cx, move |_, _| Host {
        _interned: retained,
    });
    *interned.callback.borrow_mut() = callbacks;
    state
}

pub(super) fn invoke(
    cell: &Rc<RefCell<FormCallbacks>>,
    pick: impl Fn(&FormCallbacks) -> Option<ComponentCallback>,
    context: &str,
    argument: ComponentCallbackArgument,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(callback) = pick(&cell.borrow()) else {
        return;
    };
    callback.invoke_and_report_with(context, &[argument], window, cx);
}

fn invoke_change(
    cell: &Rc<RefCell<FormCallbacks>>,
    context: &str,
    argument: ComponentCallbackArgument,
    window: &mut Window,
    cx: &mut App,
) {
    invoke(
        cell,
        |callbacks| callbacks.on_change.clone(),
        context,
        argument,
        window,
        cx,
    );
}

fn invoke_submit(
    cell: &Rc<RefCell<FormCallbacks>>,
    context: &str,
    argument: ComponentCallbackArgument,
    window: &mut Window,
    cx: &mut App,
) {
    invoke(
        cell,
        |callbacks| callbacks.on_submit.clone(),
        context,
        argument,
        window,
        cx,
    );
}

fn invoke_complete(
    cell: &Rc<RefCell<FormCallbacks>>,
    context: &str,
    argument: ComponentCallbackArgument,
    window: &mut Window,
    cx: &mut App,
) {
    invoke(
        cell,
        |callbacks| callbacks.on_complete.clone(),
        context,
        argument,
        window,
        cx,
    );
}

pub(super) fn subscribe_input(
    owner: &'static str,
) -> impl FnOnce(
    Entity<InputState>,
    Rc<RefCell<FormCallbacks>>,
    &mut Window,
    &mut App,
) -> Vec<Subscription> {
    move |state, cell, window, cx| {
        #[cfg(test)]
        if owner == "Input" {
            super::test_probe::watch_input(state.clone());
        }
        let watched = state.clone();
        let change_cell = cell.clone();
        let enter_armed = Rc::new(Cell::new(true));
        vec![window.subscribe(
            &state,
            cx,
            move |_, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let value = watched.read(cx).value().to_string();
                    #[cfg(test)]
                    if owner == "Input" {
                        super::test_probe::change(value.clone());
                    }
                    invoke_change(
                        &change_cell,
                        &format!("{owner}.on_change"),
                        ComponentCallbackArgument::String(value),
                        window,
                        cx,
                    );
                }
                InputEvent::PressEnter { .. } => {
                    if !enter_armed.get() {
                        return;
                    }
                    enter_armed.set(false);
                    let value = watched.read(cx).value().to_string();
                    #[cfg(test)]
                    if owner == "Input" {
                        super::test_probe::submit(value.clone());
                    }
                    invoke_submit(
                        &change_cell,
                        &format!("{owner}.on_submit"),
                        ComponentCallbackArgument::String(value),
                        window,
                        cx,
                    );
                    let enter_armed = enter_armed.clone();
                    window.defer(cx, move |_, _| enter_armed.set(true));
                }
                _ => {}
            },
        )]
    }
}

pub(super) fn subscribe_number_input(
    state: Entity<InputState>,
    cell: Rc<RefCell<FormCallbacks>>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<Subscription> {
    let mut subscriptions = subscribe_input("NumberInput")(state.clone(), cell.clone(), window, cx);
    let watched = state.clone();
    subscriptions.push(window.subscribe(
        &state,
        cx,
        move |_, event: &NumberInputEvent, window, cx| {
            if matches!(event, NumberInputEvent::Step(_)) {
                let value = watched.read(cx).value().to_string();
                invoke_change(
                    &cell,
                    "NumberInput.on_change",
                    ComponentCallbackArgument::String(value),
                    window,
                    cx,
                );
            }
        },
    ));
    subscriptions
}

pub(super) fn subscribe_otp(
    state: Entity<OtpState>,
    cell: Rc<RefCell<FormCallbacks>>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<Subscription> {
    let watched = state.clone();
    vec![
        window.subscribe(&state, cx, move |_, event: &OtpEvent, window, cx| {
            let value = watched.read(cx).value().to_string();
            match event {
                OtpEvent::Change => invoke_change(
                    &cell,
                    "OtpInput.on_change",
                    ComponentCallbackArgument::String(value),
                    window,
                    cx,
                ),
                OtpEvent::Complete => invoke_complete(
                    &cell,
                    "OtpInput.on_complete",
                    ComponentCallbackArgument::String(value),
                    window,
                    cx,
                ),
                _ => {}
            }
        }),
    ]
}

pub(super) fn subscribe_slider(
    state: Entity<SliderState>,
    cell: Rc<RefCell<FormCallbacks>>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<Subscription> {
    vec![
        window.subscribe(&state, cx, move |_, event: &SliderEvent, window, cx| {
            let (submit, value) = match event {
                SliderEvent::Change(value) => (false, *value),
                SliderEvent::Release(value) => (true, *value),
            };
            let argument = slider_argument(value);
            if submit {
                invoke_submit(&cell, "Slider.on_submit", argument, window, cx);
            } else {
                invoke_change(&cell, "Slider.on_change", argument, window, cx);
            }
        }),
    ]
}

fn slider_argument(value: SliderValue) -> ComponentCallbackArgument {
    match value {
        SliderValue::Single(value) => ComponentCallbackArgument::Number(f64::from(value)),
        SliderValue::Range(start, end) => {
            ComponentCallbackArgument::String(format!("{start}..{end}"))
        }
    }
}

pub(super) fn subscribe_color(
    state: Entity<ColorPickerState>,
    cell: Rc<RefCell<FormCallbacks>>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<Subscription> {
    vec![window.subscribe(
        &state,
        cx,
        move |_, event: &ColorPickerEvent, window, cx| {
            let ColorPickerEvent::Change(color) = event;
            let argument = match color {
                Some(color) => ComponentCallbackArgument::String(hex_color(*color)),
                None => ComponentCallbackArgument::Array(Vec::new()),
            };
            invoke_change(&cell, "ColorPicker.on_change", argument, window, cx);
        },
    )]
}

fn hex_color(color: gpui::Hsla) -> String {
    let rgba = gpui::Rgba::from(color);
    let channel = |value: f32| (value * 255.) as u32;
    if rgba.a < 1. {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            channel(rgba.r),
            channel(rgba.g),
            channel(rgba.b),
            channel(rgba.a)
        )
    } else {
        format!(
            "#{:02X}{:02X}{:02X}",
            channel(rgba.r),
            channel(rgba.g),
            channel(rgba.b)
        )
    }
}

pub(super) fn subscribe_date(
    state: Entity<DatePickerState>,
    cell: Rc<RefCell<FormCallbacks>>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<Subscription> {
    vec![
        window.subscribe(&state, cx, move |_, event: &DatePickerEvent, window, cx| {
            let DatePickerEvent::Change(date) = event;
            invoke_change(
                &cell,
                "DatePicker.on_change",
                date_argument(*date),
                window,
                cx,
            );
        }),
    ]
}

pub(super) fn subscribe_calendar(
    state: Entity<CalendarState>,
    cell: Rc<RefCell<FormCallbacks>>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<Subscription> {
    vec![
        window.subscribe(&state, cx, move |_, event: &CalendarEvent, window, cx| {
            let CalendarEvent::Selected(date) = event;
            invoke_change(
                &cell,
                "Calendar.on_change",
                date_argument(*date),
                window,
                cx,
            );
        }),
    ]
}

fn date_argument(date: Date) -> ComponentCallbackArgument {
    match date {
        Date::Single(None) | Date::Range(None, None) => {
            ComponentCallbackArgument::Array(Vec::new())
        }
        Date::Single(Some(day)) => ComponentCallbackArgument::String(day.to_string()),
        other => ComponentCallbackArgument::String(other.to_string()),
    }
}
