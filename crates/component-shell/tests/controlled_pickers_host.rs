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

fn picker_source(control: &str, initial: &str, replacement: &str) -> String {
    format!(
        r#"
import {{ View, div }} from "gpui-kit";
import {{ {control}, {control}State }} from "gpui-component";
export default class App extends View {{
 init() {{ this.state = {control}State({initial}); this.model = {initial}; this.count = 0; this.disabled = false; this.seen = ""; }}
 render() {{
  return div().size_full()
   .child(new {control}(this.state).value(this.model).disabled(this.disabled).absolute().left(0).top(0).w(224)
    .on_change((value, cx) => {{ this.model = value; this.seen = typeof value + ":" + value; this.count++; cx.notify(); }}))
   .child(div().absolute().left(450).top(0).w(100).h(40).child("replace")
    .on_click((_, cx) => {{ this.model = {replacement}; cx.notify(); }}))
   .child(div().absolute().left(450).top(50).w(100).h(40).child("clear")
    .on_click((_, cx) => {{ this.model = null; cx.notify(); }}))
   .child(div().absolute().left(450).top(100).w(100).h(40).child("disable")
    .on_click((_, cx) => {{ this.disabled = !this.disabled; cx.notify(); }}))
   .child(`model:${{this.model}} count:${{this.count}} seen:${{this.seen}} disabled:${{this.disabled}}`);
 }}
}}
"#
    )
}

fn click(context: &mut VisualTestContext, x: f32, y: f32) {
    context.simulate_click(point(px(x), px(y)), Modifiers::default());
    draw(context);
}

#[gpui::test]
fn calendar_script_roundtrip_and_disabled_pointer_interaction(cx: &mut TestAppContext) {
    let source = picker_source("Calendar", "\"2026-09-18\"", "\"2027-01-02\"");
    let (mut context, view, _app) = mount(cx, &source);
    draw(&mut context);
    click(&mut context, 470., 16.);
    assert!(snapshot(&mut context, &view).contains("model:2027-01-02 count:0"));
    click(&mut context, 470., 116.);
    // Friday of the first January row; header and weekday each occupy 32px.
    click(&mut context, 176., 80.);
    // Also attempt navigation while disabled. The next enabled selection below
    // must still come from January, proving navigation was blocked too.
    click(&mut context, 208., 16.);
    let text = snapshot(&mut context, &view);
    assert!(
        text.contains("model:2027-01-02 count:0") && text.contains("disabled:true"),
        "{text}"
    );
    click(&mut context, 470., 116.);
    click(&mut context, 176., 80.);
    let text = snapshot(&mut context, &view);
    assert!(
        text.contains("model:2027-01-01 count:1 seen:string:2027-01-01"),
        "native selection must roundtrip through the current callback: {text}"
    );
    click(&mut context, 470., 66.);
    assert!(snapshot(&mut context, &view).contains("model:null count:1"));
    click(&mut context, 176., 80.);
    assert!(
        snapshot(&mut context, &view).contains("model:2027-01-01 count:2 seen:string:2027-01-01")
    );
}

#[gpui::test]
fn date_picker_popup_selection_roundtrips_after_external_update_and_clear(cx: &mut TestAppContext) {
    let source = picker_source("DatePicker", "\"2026-09-18\"", "\"2027-01-02\"");
    let (mut context, view, _app) = mount(cx, &source);
    draw(&mut context);
    click(&mut context, 470., 16.);
    assert!(snapshot(&mut context, &view).contains("model:2027-01-02 count:0"));
    click(&mut context, 20., 16.);
    // Popup: input height 32, offset 6, padding 12, then calendar header,
    // weekday row, and day cell. The point is inside Friday across its 8px animation.
    click(&mut context, 188., 130.);
    let text = snapshot(&mut context, &view);
    assert!(
        text.contains("model:2027-01-01 count:1 seen:string:2027-01-01"),
        "date picker native popup selection must reach script: {text}"
    );
    click(&mut context, 470., 66.);
    assert!(snapshot(&mut context, &view).contains("model:null count:1"));
    click(&mut context, 20., 16.);
    click(&mut context, 188., 130.);
    let text = snapshot(&mut context, &view);
    assert!(
        text.contains("model:2027-01-01 count:2 seen:string:2027-01-01"),
        "clearing must retain calendar navigation and callback ownership: {text}"
    );
}

#[gpui::test]
fn color_picker_palette_callback_reports_hex_after_model_update_and_clear(cx: &mut TestAppContext) {
    use gpui_component::ActiveTheme as _;
    let source = picker_source("ColorPicker", "\"#112233\"", "\"#445566\"");
    let (mut context, view, _app) = mount(cx, &source);
    draw(&mut context);
    let expected = context.update(|_, cx| {
        let rgba = gpui::Rgba::from(cx.theme().red);
        format!(
            "#{:02X}{:02X}{:02X}",
            (rgba.r * 255.) as u32,
            (rgba.g * 255.) as u32,
            (rgba.b * 255.) as u32
        )
    });
    click(&mut context, 470., 16.);
    assert!(snapshot(&mut context, &view).contains("model:#445566 count:0"));
    click(&mut context, 470., 66.);
    assert!(snapshot(&mut context, &view).contains("model:null count:0"));
    click(&mut context, 16., 16.);
    // First featured color, below the segmented tab bar in the popover.
    click(&mut context, 24., 100.);
    let text = snapshot(&mut context, &view);
    assert!(
        text.contains(&format!("model:{expected} count:1 seen:string:{expected}")),
        "native palette choice must cross shell as hex: {text}"
    );
}
