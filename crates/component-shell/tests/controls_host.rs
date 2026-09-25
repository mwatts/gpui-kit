#[path = "../src/shell/support.rs"]
mod support;

#[path = "../src/shell/controls/mod.rs"]
mod controls;

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
            "controls-{}-{}",
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
    let mut registry = gpui_shell::ComponentRegistry::new(
        gpui_shell::COMPONENT_REGISTRY_API_VERSION,
        gpui_shell::DEFAULT_COMPONENT_MODULE,
    )
    .unwrap();
    controls::register(&mut registry).unwrap();
    let runtime =
        gpui_shell::ShellRuntime::new_isolated_with_components(registry.freeze().unwrap()).unwrap();
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

/// A two-state control that cannot report its new state is set-only: the script
/// owns `checked`, so a click changes nothing and the control looks broken
/// while behaving exactly as registered. `on_change` is what closes the loop.
#[gpui::test]
fn clicking_a_two_state_control_reports_its_new_state(cx: &mut TestAppContext) {
    let source = r#"
import { View, div } from "gpui-kit";
import { Checkbox, Switch, Toggle } from "gpui-component";
export default class App extends View {
  init(_props, _cx) { this.checkbox = false; this.switch = false; this.toggle = false; }
  render() {
    return div().size_full()
      .child(new Checkbox("cb").label("Checkbox").checked(this.checkbox).absolute().left(0).top(0).w(200).h(30)
        .on_change((checked, cx) => { this.checkbox = checked; cx.notify(); }))
      .child(new Switch("sw").label("Switch").checked(this.switch).absolute().left(0).top(40).w(200).h(30)
        .on_change((checked, cx) => { this.switch = checked; cx.notify(); }))
      .child(new Toggle("tg").label("Toggle").checked(this.toggle).absolute().left(0).top(80).w(200).h(30)
        .on_change((checked, cx) => { this.toggle = checked; cx.notify(); }))
      .child(`state: ${this.checkbox}|${this.switch}|${this.toggle}`);
  }
}
"#;
    let (mut context, view, _app) = mount(cx, source);
    draw(&mut context);
    let initial = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(initial.contains("state: false|false|false"), "{initial}");

    for (label, position) in [
        ("checkbox", point(px(10.), px(14.))),
        ("switch", point(px(10.), px(54.))),
        ("toggle", point(px(10.), px(94.))),
    ] {
        context.simulate_click(position, Modifiers::default());
        draw(&mut context);
        let _ = label;
    }

    let clicked = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(
        clicked.contains("state: true|true|true"),
        "every control must report its click: {clicked}"
    );
}

/// A `Button` whose materializer honours `MaterializeRequest::on_click` still
/// cannot be clicked unless its descriptor declares the method: the engine
/// refuses a common behavior a registered component has not declared, so the
/// wiring on both ends is live and the call never arrives.
#[gpui::test]
fn clicking_a_button_reaches_the_script(cx: &mut TestAppContext) {
    let source = r#"
import { View, div } from "gpui-kit";
import { Button } from "gpui-component";
export default class App extends View {
  init(_props, _cx) { this.hits = 0; }
  render() {
    return div().size_full()
      .child(new Button("press").primary().label("Press me").accessibility_label("Press the test button").absolute().left(0).top(0).w(200).h(32)
        .on_click((_event, cx) => { this.hits++; cx.notify(); }))
      .child(`hits: ${this.hits}`);
  }
}
"#;
    let (mut context, view, _app) = mount(cx, source);
    draw(&mut context);
    let before = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(before.contains("hits: 0"), "{before}");

    context.simulate_click(point(px(10.), px(16.)), Modifiers::default());
    draw(&mut context);

    let after = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(
        after.contains("hits: 1"),
        "the click must reach the script: {after}"
    );
}

/// GPUI activates a focused clickable on the key's release, so a press is the
/// key going down and coming back up.
fn activate(context: &mut VisualTestContext, key: &str) {
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    context.simulate_event(gpui::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    context.simulate_event(gpui::KeyUpEvent { keystroke });
    draw(context);
}

fn hits(context: &mut VisualTestContext, view: &Entity<gpui_shell::ScriptView>) -> String {
    context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree())
}

/// An enabled `Button` is a tab stop, and Enter and Space each activate it the
/// way the kit's native button does. A disabled one is skipped.
#[gpui::test]
fn tab_reaches_a_button_and_enter_or_space_activates_it(cx: &mut TestAppContext) {
    let source = r#"
import { View, div } from "gpui-kit";
import { Button } from "gpui-component";
export default class App extends View {
  init(_props, cx) { this.hits = 0; this.field = cx.focus_handle(); }
  render() {
    return div().size_full()
      .child(div().id("start").track_focus(this.field).tab_stop(true).w(10).h(10))
      .child(new Button("off").label("Off").disabled(true)
        .on_click((_event, cx) => { this.hits += 100; cx.notify(); }))
      .child(new Button("press").label("Press me")
        .on_click((_event, cx) => { this.hits++; cx.notify(); }))
      .child(`hits: ${this.hits}`);
  }
}
"#;
    let (mut context, view, _app) = mount(cx, source);
    draw(&mut context);
    // Focus the leading tab stop, then Tab once onto the button.
    context.update(|window, cx| window.focus_next(cx));
    draw(&mut context);
    context.simulate_keystrokes("tab");
    draw(&mut context);
    activate(&mut context, "enter");
    activate(&mut context, "space");
    let after = hits(&mut context, &view);
    assert!(
        after.contains("hits: 2"),
        "Enter and Space each click once: {after}"
    );
}

/// `track_focus` on a registered `Button` hands it a script-owned handle, so a
/// script can put the keyboard on the button and Enter then activates it.
#[gpui::test]
fn a_button_tracks_a_script_focus_handle(cx: &mut TestAppContext) {
    let source = r#"
import { View, div } from "gpui-kit";
import { Button } from "gpui-component";
export default class App extends View {
  init(_props, cx) { this.hits = 0; this.focus = cx.focus_handle(); this.focused = false; }
  render() {
    const self = this;
    return div().size_full()
      .child(new Button("other").label("Other")
        .on_click((_event, cx) => { this.hits += 100; cx.notify(); }))
      .child(new Button("press").label("Press me").track_focus(this.focus)
        .on_click((_event, cx) => { this.hits++; cx.notify(); }))
      .child(`hits: ${this.hits}`)
      .when(!this.focused, el => el.on_mouse_down("left", (_event, cx) => {
        self.focused = true; self.focus.focus(); cx.notify();
      }));
  }
}
"#;
    let (mut context, view, _app) = mount(cx, source);
    draw(&mut context);
    context.simulate_click(point(px(400.), px(300.)), Modifiers::default());
    draw(&mut context);
    context.update(|window, cx| assert!(window.focused(cx).is_some(), "the handle took focus"));
    activate(&mut context, "enter");
    let after = hits(&mut context, &view);
    assert!(
        after.contains("hits: 1"),
        "Enter clicks the tracked button: {after}"
    );
}

/// Handing `track_focus` anything but a focus handle is refused at the call.
#[gpui::test]
fn a_button_refuses_a_non_handle_for_track_focus(cx: &mut TestAppContext) {
    let source = r#"
import { View, div } from "gpui-kit";
import { Button } from "gpui-component";
export default class App extends View {
  render() {
    return div().child(new Button("press").label("Press").track_focus({ __handle: 999999 }));
  }
}
"#;
    let (mut context, view, _app) = mount(cx, source);
    draw(&mut context);
    let error = context.update(|_, cx| view.read(cx).build_error().map(|error| error.to_string()));
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("expects a live FocusHandle")),
        "{error:?}"
    );
}
