use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};
use std::{
    fs,
    ops::Deref,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_APP: AtomicU64 = AtomicU64::new(0);

struct TempApp(PathBuf);

impl TempApp {
    fn new(source: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "controlled-pickers-host-{}-{}",
            std::process::id(),
            NEXT_APP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("main.js"), source).unwrap();
        Self(path)
    }
}

impl Drop for TempApp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn mount(
    cx: &mut TestAppContext,
    source: &str,
) -> (VisualTestContext, Entity<gpui_shell::ScriptView>, TempApp) {
    cx.update(gpui_component_shell::init);
    let app = TempApp::new(source);
    let runtime = gpui_component_shell::new_isolated_runtime().unwrap();
    let loaded = runtime.load_application(&app.0, "main.js").unwrap();
    let mounted = std::rc::Rc::new(std::cell::RefCell::new(None));
    let slot = mounted.clone();
    let window = cx.add_window(move |window, cx| {
        let view = runtime.mount_application(&loaded, window, cx).unwrap();
        *slot.borrow_mut() = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let context = VisualTestContext::from_window(*window.deref(), cx);
    let view = mounted.borrow().clone().unwrap();
    (context, view, app)
}

fn draw(context: &mut VisualTestContext) {
    context.run_until_parked();
    context.update(|window, cx| window.draw(cx).clear(cx));
}

fn snapshot(context: &mut VisualTestContext, view: &Entity<gpui_shell::ScriptView>) -> String {
    context.update(|_, cx| {
        assert_eq!(view.read(cx).build_error(), None);
        view.read(cx).snapshot().unwrap().debug_tree()
    })
}

#[gpui::test]
fn otp_external_update_is_silent_and_next_native_edit_uses_that_value(cx: &mut TestAppContext) {
    let source = r#"
import { View, div } from "gpui-kit";
import { OtpInput, OtpState } from "gpui-component";
export default class App extends View {
 init() { this.state = OtpState(6, "12"); this.model = "12"; this.count = 0; }
 render() {
  return div().size_full()
   .child(new OtpInput(this.state).value(this.model).absolute().left(0).top(0)
    .on_change((value, cx) => { this.model = value; this.count++; cx.notify(); }))
   .child(div().absolute().left(450).top(0).w(100).h(40).child("reset")
    .on_click((_, cx) => { this.model = "45"; cx.notify(); }))
   .child(`model:${this.model} count:${this.count}`);
 }
}
"#;
    let (mut context, view, _app) = mount(cx, source);
    draw(&mut context);
    context.simulate_click(point(px(470.), px(16.)), Modifiers::default());
    draw(&mut context);
    let text = snapshot(&mut context, &view);
    assert!(
        text.contains("model:45 count:0"),
        "external values must not emit: {text}"
    );
    context.simulate_click(point(px(20.), px(16.)), Modifiers::default());
    context.simulate_keystrokes("3");
    draw(&mut context);
    let text = snapshot(&mut context, &view);
    assert!(
        text.contains("model:453 count:1"),
        "native edit must extend externally supplied state exactly once: {text}"
    );
}

#[gpui::test]
fn slider_external_update_is_silent_and_pointer_change_reaches_model(cx: &mut TestAppContext) {
    let source = r#"
import { View, div } from "gpui-kit";
import { Slider, SliderState } from "gpui-component";
export default class App extends View {
 init() { this.state = SliderState(10); this.model = 10; this.count = 0; }
 render() {
  return div().size_full()
   .child(new Slider(this.state).value(this.model).absolute().left(0).top(0).w(100).h(24)
    .on_change((value, cx) => { this.model = value; this.count++; cx.notify(); }))
   .child(div().absolute().left(450).top(0).w(100).h(40).child("reset")
    .on_click((_, cx) => { this.model = 20; cx.notify(); }))
   .child(`model:${this.model} count:${this.count} numeric:${typeof this.model === "number"}`);
 }
}
"#;
    let (mut context, view, _app) = mount(cx, source);
    draw(&mut context);
    context.simulate_click(point(px(470.), px(16.)), Modifiers::default());
    draw(&mut context);
    assert!(snapshot(&mut context, &view).contains("model:20 count:0 numeric:true"));
    context.simulate_click(point(px(50.), px(12.)), Modifiers::default());
    draw(&mut context);
    let text = snapshot(&mut context, &view);
    assert!(
        text.contains("count:1 numeric:true") && !text.contains("model:20"),
        "pointer change must report one numeric native value: {text}"
    );
}

#[gpui::test]
fn native_calendar_sync_is_silent_and_selection_after_clear_emits(cx: &mut TestAppContext) {
    use gpui::AppContext as _;
    use gpui_component::calendar::{CalendarEvent, CalendarState, Date};
    use std::{cell::RefCell, rc::Rc};
    cx.update(gpui_component_shell::init);
    let context = cx.add_empty_window();
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    let (state, _subscription) = context.update(|window, cx| {
        let state = cx.new(|cx| CalendarState::new(window, cx));
        let subscription = cx.subscribe(&state, move |_, event: &CalendarEvent, _| {
            let CalendarEvent::Selected(value) = event;
            seen.borrow_mut().push(*value);
        });
        (state, subscription)
    });
    context.update(|window, cx| {
        let external = "2026-09-18".parse().unwrap();
        state.update(cx, |state, cx| {
            state.set_date(Date::Single(Some(external)), window, cx)
        });
        assert_eq!(state.read(cx).date(), Date::Single(Some(external)));
        state.update(cx, |state, cx| {
            state.set_date(Date::Single(None), window, cx)
        });
        assert_eq!(state.read(cx).date(), Date::Single(None));
    });
    assert!(
        events.borrow().is_empty(),
        "external updates and clearing must not emit selection"
    );
    let edited = "2026-09-20".parse().unwrap();
    context.update(|_, cx| state.update(cx, |state, cx| state.activate_date(edited, cx)));
    assert_eq!(*events.borrow(), [Date::Single(Some(edited))]);
}

#[gpui::test]
fn native_color_sync_is_silent_and_user_selection_after_clear_emits(cx: &mut TestAppContext) {
    use gpui::AppContext as _;
    use gpui_component::color_picker::{ColorPickerEvent, ColorPickerState};
    use std::{cell::RefCell, rc::Rc};
    cx.update(gpui_component_shell::init);
    let context = cx.add_empty_window();
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    let (state, _subscription) = context.update(|window, cx| {
        let state = cx.new(|cx| ColorPickerState::new(window, cx));
        let subscription = cx.subscribe(&state, move |_, event: &ColorPickerEvent, _| {
            let ColorPickerEvent::Change(value) = event;
            seen.borrow_mut().push(*value);
        });
        (state, subscription)
    });
    context.update(|window, cx| {
        let external: gpui::Hsla = gpui::rgb(0x112233).into();
        state.update(cx, |state, cx| state.set_value(external, window, cx));
        assert_eq!(state.read(cx).value(), Some(external));
        state.update(cx, |state, cx| state.clear_value(window, cx));
        assert_eq!(state.read(cx).value(), None);
    });
    assert!(
        events.borrow().is_empty(),
        "external updates and clearing must not emit changes"
    );
    let edited: gpui::Hsla = gpui::rgba(0x44556680).into();
    context
        .update(|window, cx| state.update(cx, |state, cx| state.select_color(edited, window, cx)));
    assert_eq!(*events.borrow(), [Some(edited)]);
}
