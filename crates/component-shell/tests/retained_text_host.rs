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

fn exercise(cx: &mut TestAppContext, control: &str, constructor: &str) {
    let source = format!(
        r#"
import {{ View, div }} from "gpui-kit";
import {{ {control}, {control_state} }} from "gpui-component";
export default class App extends View {{
  init() {{ this.state = {constructor}; this.model = ""; this.count = 0; this.seen = -1; }}
  render() {{
    const generation = this.count;
    return div().size_full()
      .child(new {control}(this.state).value(this.model).absolute().left(0).top(0).w(400).h(120)
        .on_change((value, cx) => {{ this.model = value; this.count += 1; this.seen = generation; cx.notify(); }}))
      .child(div().absolute().left(450).top(0).w(100).h(40).child("reset")
        .on_click((_event, cx) => {{ this.model = "45"; cx.notify(); }}))
      .child(`model:${{this.model}} count:${{this.count}} seen:${{this.seen}}`);
  }}
}}
"#,
        control_state = if control == "NumberInput" {
            "InputState".to_owned()
        } else {
            format!("{control}State")
        }
    );
    let (mut context, view, _app) = mount(cx, &source);
    draw(&mut context);
    context.simulate_click(point(px(20.), px(16.)), Modifiers::default());
    context.simulate_keystrokes("1");
    draw(&mut context);
    context.simulate_keystrokes("2");
    draw(&mut context);
    let text = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(
        text.contains("model:12 count:2 seen:1"),
        "native edits and latest callback must reach model exactly once: {text}"
    );
    context.simulate_click(point(px(470.), px(16.)), Modifiers::default());
    draw(&mut context);
    let text = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(
        text.contains("model:45 count:2"),
        "external updates must not cause callback feedback: {text}"
    );
    context.simulate_click(point(px(20.), px(16.)), Modifiers::default());
    context.simulate_keystrokes("cmd-right 3");
    draw(&mut context);
    let text = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(
        text.contains("model:453 count:3 seen:2"),
        "editing after external update must use retained native state: {text}"
    );
}

#[gpui::test]
fn input_model_round_trip(cx: &mut TestAppContext) {
    exercise(cx, "Input", "InputState()");
}
#[gpui::test]
fn number_input_model_round_trip(cx: &mut TestAppContext) {
    exercise(cx, "NumberInput", "InputState()");
}
#[gpui::test]
fn textarea_model_round_trip(cx: &mut TestAppContext) {
    exercise(cx, "Textarea", "TextareaState()");
}
#[gpui::test]
fn editor_model_round_trip(cx: &mut TestAppContext) {
    exercise(cx, "Editor", "EditorState(\"\")");
}
