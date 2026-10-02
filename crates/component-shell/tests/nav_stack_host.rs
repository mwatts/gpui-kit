#[path = "../src/shell/navigation/nav_stack.rs"]
mod nav_stack;
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
    nav_stack::register(&mut registry).unwrap();
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
fn javascript_paths_push_back_forward_without_replacing_pages(cx: &mut TestAppContext) {
    let (mut context, view, _app) = mount(
        cx,
        r#"
import { View, div } from "gpui-kit";
import { NavStack } from "gpui-component";
export default class App extends View {
 init() { this.step = 0; this.path = ["root"]; this.label = "First"; this.changes = 0; }
 render() { return div().size_full()
  .child(div().absolute().left(450).top(0).w(100).h(40).child("advance").on_click((_e,cx) => {
   this.step++; this.path = this.step === 2 ? ["root"] : ["root","detail"];
   if (this.step === 3) this.label = "Updated"; cx.notify();
  }))
  .child(new NavStack("nav", () => [{id:"root",label:"Home"},{id:"detail",label:this.label}], page => div().child(page.label))
   .path(this.path).on_change((path,current,cx) => { this.changes++; this.path = path; cx.notify(); }).w(400).h(200))
  .child(`changes:${this.changes}`); }
}
"#,
    );
    nav_stack::test_probe::take();
    draw(&mut context);
    let first = nav_stack::test_probe::take();
    assert_eq!(first.path, ["root"]);
    assert_eq!(first.current, Some(first.identities["root"]));
    let detail = first.identities["detail"];
    for (step, expected) in [(1, "detail"), (2, "root"), (3, "detail")] {
        context.simulate_click(point(px(470.), px(16.)), Modifiers::default());
        draw(&mut context);
        let state = nav_stack::test_probe::take();
        assert_eq!(state.path.last().map(String::as_str), Some(expected));
        assert_eq!(state.current, Some(first.identities[expected]));
        assert_eq!(state.identities["detail"], detail);
        let tree = context.update(|_, cx| {
            assert_eq!(view.read(cx).build_error(), None);
            view.read(cx).snapshot().unwrap().debug_tree()
        });
        assert!(
            tree.contains(&format!("changes:{}", step + 1)),
            "callback feedback: {tree}"
        );
        if step == 3 {
            assert!(
                state
                    .rendered
                    .iter()
                    .any(|row| format!("{row:?}").contains("Updated")),
                "native page must render refreshed JS data"
            );
        }
    }
}

/// A page is a frame-owned subtree: a control it draws takes clicks, and a
/// `size_full` page still fills the stack (the lease wrapper adds no box).
#[gpui::test]
fn page_controls_take_clicks_and_fill_the_stack(cx: &mut TestAppContext) {
    let (mut context, view, _app) = mount(
        cx,
        r#"
import { View, div } from "gpui-kit";
import { NavStack } from "gpui-component";
export default class App extends View {
 init() { this.clicks = 0; }
 render() { return div().size_full()
  .child(new NavStack("nav", () => [{id:"root"}], () => div().size_full().flex().flex_col().justify_end()
    .child(div().h(40).w(100).child("press").on_click((_e,cx) => { this.clicks++; cx.notify(); })))
   .path(["root"]).w(400).h(200))
  .child(`clicks:${this.clicks}`); }
}
"#,
    );
    draw(&mut context);
    draw(&mut context);
    // The control sits at the bottom of a page that fills the 200px stack.
    context.simulate_click(point(px(20.), px(180.)), Modifiers::default());
    draw(&mut context);
    let tree = context.update(|_, cx| {
        assert_eq!(view.read(cx).build_error(), None);
        view.read(cx).snapshot().unwrap().debug_tree()
    });
    assert!(tree.contains("clicks:1"), "page click: {tree}");
}

/// A phone root list (the Inbox) lives in a NavStack page. If the kit refuses
/// a list built there, the page shows an error and the Inbox is empty, so the
/// page's rows must paint and a row's click must reach script.
#[gpui::test]
fn a_half_million_row_list_in_a_page_paints_rows_and_its_click_reaches_script(
    cx: &mut TestAppContext,
) {
    let (mut context, view, _app) = mount(
        cx,
        r#"
import { View, div, uniform_list } from "gpui-kit";
import { NavStack } from "gpui-component";
export default class App extends View {
 init() { this.clicked = -1; }
 render() { return div().size_full()
  .child(new NavStack("nav", () => [{id:"root"}], () => div().size_full()
    .child(uniform_list("rows", 500000, (i) => String(i), (range) => {
      const items = [];
      for (let i = range.start; i < range.end; i++) {
        items.push(div().h(20).w_full().child(`row ${i}`)
          .on_click((_e, cx) => { this.clicked = i; cx.notify(); }));
      }
      return items;
    }).size_full()))
   .path(["root"]).w(400).h(200))
  .child(`clicked:${this.clicked}`); }
}
"#,
    );
    draw(&mut context);
    draw(&mut context);
    // The third 20px row covers 40..60.
    context.simulate_click(point(px(100.), px(50.)), Modifiers::default());
    draw(&mut context);
    draw(&mut context);
    let tree = context.update(|_, cx| {
        assert_eq!(view.read(cx).build_error(), None);
        view.read(cx).snapshot().unwrap().debug_tree()
    });
    assert!(
        tree.contains("clicked:2"),
        "the page's list must paint rows and row 2's click must reach script: {tree}"
    );
}
