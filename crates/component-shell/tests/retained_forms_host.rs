#[path = "../src/shell/input_tokens.rs"]
mod input_tokens;

#[path = "../src/shell/retained_forms/mod.rs"]
mod retained_forms;

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
            "retained-forms-host-{}-{}",
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
    retained_forms::register(&mut registry).unwrap();
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

#[gpui::test]
fn typing_into_input_reports_on_change_and_on_submit_then_unmount_drops_the_subscription(
    cx: &mut TestAppContext,
) {
    let source = r#"
import { View, div } from "gpui-kit";
import { Input, InputState } from "gpui-component";
export default class App extends View {
  init(_props, _cx) {
    this.input = InputState();
    this.changes = [];
    this.submits = [];
    this.mounted = true;
  }
  render() {
    if (!this.mounted) {
      return div().size_full().child(`unmounted changes:${this.changes.join("|")} submits:${this.submits.length}:${this.submits.join("|")}`);
    }
    return div().size_full()
      .child(new Input(this.input).absolute().left(0).top(0).w(400).h(40)
        .on_change((value, cx) => { this.changes.push(value); cx.notify(); })
        .on_submit((value, cx) => { this.submits.push(value); this.mounted = false; cx.notify(); }))
      .child(`changes:${this.changes.join("|")} submits:${this.submits.length}:${this.submits.join("|")}`);
  }
}
"#;
    let (mut context, view, _app) = mount(cx, source);
    retained_forms::test_probe::reset();
    draw(&mut context);
    let initial = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(initial.contains("changes:"), "{initial}");
    assert!(!initial.contains("unmounted"), "{initial}");

    context.simulate_click(point(px(20.), px(16.)), Modifiers::default());
    context.simulate_keystrokes("h i");
    draw(&mut context);

    let typed = context.update(|_, cx| {
        assert_eq!(view.read(cx).build_error(), None);
        view.read(cx).snapshot().unwrap().debug_tree()
    });
    assert!(
        typed.contains("changes:") && typed.contains("hi"),
        "on_change must reach the script with the typed value: {typed}"
    );
    let changes = retained_forms::test_probe::take_changes();
    assert!(
        changes.iter().any(|value| value == "hi"),
        "native InputEvent::Change must report state.value(): {changes:?}"
    );

    context.simulate_keystrokes("enter");
    draw(&mut context);

    let submitted = context.update(|_, cx| {
        assert_eq!(view.read(cx).build_error(), None);
        view.read(cx).snapshot().unwrap().debug_tree()
    });
    assert!(
        submitted.contains("unmounted") && submitted.contains("submits:1:hi"),
        "on_submit must receive the value and unmount the host: {submitted}"
    );
    let submits = retained_forms::test_probe::take_submits();
    assert!(
        submits.iter().any(|value| value == "hi"),
        "native PressEnter must report state.value(): {submits:?}"
    );

    retained_forms::test_probe::take_changes();
    context.simulate_keystrokes("z");
    context.update(|window, cx| retained_forms::test_probe::emit_change(window, cx));
    draw(&mut context);

    assert!(
        retained_forms::test_probe::take_changes().is_empty(),
        "dropping keyed host state must drop the subscription"
    );
    let after = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(
        after.contains("unmounted") && !after.contains("hiz") && !after.contains("changes:hi|"),
        "unmounted input must not report further callbacks: {after}"
    );
}

#[gpui::test]
fn text_submit_reports_enter_modifiers_after_the_context(cx: &mut TestAppContext) {
    let source = r#"
import { View, div } from "gpui-kit";
import { Input, InputState } from "gpui-component";
export default class App extends View {
  init(_props, _cx) { this.input = InputState(); this.seen = []; }
  render() {
    return div().size_full()
      .child(new Input(this.input).absolute().left(0).top(0).w(400).h(40)
        .on_submit((value, cx, modifiers) => {
          this.seen.push(`${value}:${modifiers.shift}:${modifiers.secondary}`);
          cx.notify();
        }))
      .child(`seen:${this.seen.join("|")}`);
  }
}
"#;
    let (mut context, view, _app) = mount(cx, source);
    retained_forms::test_probe::reset();
    draw(&mut context);
    context.simulate_click(point(px(20.), px(16.)), Modifiers::default());
    context.simulate_keystrokes("a enter");
    draw(&mut context);
    context.simulate_keystrokes("shift-enter");
    draw(&mut context);
    context.simulate_keystrokes("secondary-enter");
    draw(&mut context);
    let text = context.update(|_, cx| {
        assert_eq!(view.read(cx).build_error(), None);
        view.read(cx).snapshot().unwrap().debug_tree()
    });
    assert!(
        text.contains("seen:a:false:false|a:true:false|a:false:true"),
        "on_submit must receive the Enter modifiers after cx: {text}"
    );
    assert_eq!(
        retained_forms::test_probe::take_submit_modifiers(),
        [(false, false), (true, false), (false, true)]
    );
    // `(value, cx) => ...` callbacks keep working; the test above uses one.
}

#[gpui::test]
fn time_field_takes_an_initial_and_controlled_value_and_reports_edits(cx: &mut TestAppContext) {
    let source = r#"
import { View, div } from "gpui-kit";
import { TimeField, TimeFieldState } from "gpui-component";
export default class App extends View {
  init(_props, _cx) { this.state = TimeFieldState("09:30"); this.model = null; this.changes = 0; }
  render() {
    let field = new TimeField(this.state).absolute().left(0).top(0).w(200).h(32)
      .on_change((value, cx) => { this.model = value; this.changes += 1; cx.notify(); });
    if (this.model !== null) field = field.value(this.model);
    return div().size_full()
      .child(field)
      .child(div().absolute().left(300).top(0).w(80).h(32).child("noon")
        .on_click((_event, cx) => { this.model = "12:00"; cx.notify(); }))
      .child(`model:${this.model} changes:${this.changes}`);
  }
}
"#;
    let (mut context, view, _app) = mount(cx, source);
    draw(&mut context);
    let state_time = |context: &mut VisualTestContext| {
        context.update(|_, cx| {
            let tree = view.read(cx).snapshot().unwrap().debug_tree();
            assert_eq!(view.read(cx).build_error(), None, "{tree}");
            tree
        })
    };
    assert!(state_time(&mut context).contains("model:null changes:0"));

    // The hour segment is selected first; Up steps it and reports "HH:MM".
    context.simulate_click(point(px(12.), px(16.)), Modifiers::default());
    context.simulate_keystrokes("up");
    draw(&mut context);
    let text = state_time(&mut context);
    assert!(text.contains("model:10:30 changes:1"), "{text}");

    // A controlled value replaces the time without reporting a change.
    context.simulate_click(point(px(320.), px(16.)), Modifiers::default());
    draw(&mut context);
    let text = state_time(&mut context);
    assert!(text.contains("model:12:00 changes:1"), "{text}");
    context.simulate_click(point(px(12.), px(16.)), Modifiers::default());
    context.simulate_keystrokes("up");
    draw(&mut context);
    let text = state_time(&mut context);
    assert!(text.contains("model:13:00 changes:2"), "{text}");
}
