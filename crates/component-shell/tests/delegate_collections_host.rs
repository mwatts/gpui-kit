#[path = "../src/shell/delegate_collections/mod.rs"]
mod delegate_collections;

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};
use std::{fs, ops::Deref, path::PathBuf};

#[test]
fn delegate_collection_catalog_exposes_retained_list_contract() {
    let mut registry = gpui_shell::ComponentRegistry::new(
        gpui_shell::COMPONENT_REGISTRY_API_VERSION,
        gpui_shell::DEFAULT_COMPONENT_MODULE,
    )
    .unwrap();
    delegate_collections::register(&mut registry).unwrap();
    let registry = registry.freeze().unwrap();

    assert_eq!(
        registry
            .descriptors()
            .map(|item| item.name())
            .collect::<Vec<_>>(),
        ["List"]
    );
    assert_eq!(registry.states().count(), 0);
}

struct ScriptRoot(Entity<gpui_shell::ScriptView>);
impl gpui::Render for ScriptRoot {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        self.0.clone()
    }
}

#[gpui::test]
fn list_uses_a_fresh_immutable_snapshot_and_lazy_row_renderer(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component_shell::init(cx);
    });
    let root = std::env::temp_dir().join(format!("delegate-list-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("main.js"),
        r#"import { View, div } from "gpui-kit";
import { List } from "gpui-component";
export default class App extends View {
  init() { this.updated = false; }
  render() {
    const rows = this.updated ? [{id: "beta", label: "Beta"}] : [{id: "alpha", label: "Alpha"}];
    this.updated = true;
    return new List("people", () => rows, row => div().child(row.label));
  }
}"#,
    )
    .unwrap();
    let mut registry = gpui_shell::ComponentRegistry::new(
        gpui_shell::COMPONENT_REGISTRY_API_VERSION,
        gpui_shell::DEFAULT_COMPONENT_MODULE,
    )
    .unwrap();
    delegate_collections::register(&mut registry).unwrap();
    let runtime =
        gpui_shell::ShellRuntime::new_isolated_with_components(registry.freeze().unwrap()).unwrap();
    let loaded = runtime.load_application(&root, "main.js").unwrap();
    let mounted = std::rc::Rc::new(std::cell::RefCell::new(None));
    let capture = mounted.clone();
    let window = cx.add_window(move |window, cx| {
        let view = runtime.mount_application(&loaded, window, cx).unwrap();
        *capture.borrow_mut() = Some(view.clone());
        ScriptRoot(view)
    });
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    let view = mounted.borrow().clone().unwrap();

    delegate_collections::test_probe::take_rows();
    context.update(|window, cx| window.draw(cx).clear(cx));
    let initial = delegate_collections::test_probe::take_rows();
    assert!(!initial.is_empty());
    assert!(initial.iter().all(|id| id == "alpha"), "{initial:?}");
    context.update(|_, cx| view.update(cx, |view, cx| view.refresh(cx)));
    context.run_until_parked();
    delegate_collections::test_probe::take_rows();
    context.update(|window, cx| window.draw(cx).clear(cx));
    let refreshed = delegate_collections::test_probe::take_rows();
    assert!(!refreshed.is_empty());
    assert!(refreshed.iter().all(|id| id == "beta"), "{refreshed:?}");
    context.update(|_, cx| assert_eq!(view.read(cx).build_error(), None));

    fs::remove_dir_all(PathBuf::from(root)).unwrap();
}

fn draw(context: &mut VisualTestContext) {
    context.run_until_parked();
    context.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn list_selection_is_stable_row_id_across_reorder_and_clears_when_filtered_away(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_component_shell::init);
    let root = std::env::temp_dir().join(format!("delegate-list-select-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("main.js"),
        r#"import { View, div } from "gpui-kit";
import { List } from "gpui-component";
export default class App extends View {
  init() {
    this.rows = [{id: "alpha", label: "Alpha"}, {id: "beta", label: "Beta"}];
    this.selected = "";
  }
  render() {
    let list = new List("people", () => this.rows, row => div().h(32).child(row.label))
      .on_select((id, cx) => {
        this.selected = Array.isArray(id) ? "" : id;
        cx.notify();
      })
      .on_activate((id, cx) => {
        this.selected = id;
        cx.notify();
      });
    if (this.selected) list = list.selected(this.selected);
    return div().size_full()
      .child(div().absolute().left(0).top(0).w(80).h(20).child("reorder").on_click((_e, cx) => {
        this.rows = [{id: "beta", label: "Beta"}, {id: "alpha", label: "Alpha"}];
        cx.notify();
      }))
      .child(div().absolute().left(90).top(0).w(80).h(20).child("filter").on_click((_e, cx) => {
        this.rows = this.rows.filter(row => row.id !== "alpha");
        cx.notify();
      }))
      .child(`selected:${this.selected || "none"}`)
      .child(list.absolute().left(0).top(24).w(280).h(160));
  }
}"#,
    )
    .unwrap();
    let mut registry = gpui_shell::ComponentRegistry::new(
        gpui_shell::COMPONENT_REGISTRY_API_VERSION,
        gpui_shell::DEFAULT_COMPONENT_MODULE,
    )
    .unwrap();
    delegate_collections::register(&mut registry).unwrap();
    let runtime =
        gpui_shell::ShellRuntime::new_isolated_with_components(registry.freeze().unwrap()).unwrap();
    let loaded = runtime.load_application(&root, "main.js").unwrap();
    let mounted = std::rc::Rc::new(std::cell::RefCell::new(None));
    let capture = mounted.clone();
    let window = cx.add_window(move |window, cx| {
        let view = runtime.mount_application(&loaded, window, cx).unwrap();
        *capture.borrow_mut() = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    let view = mounted.borrow().clone().unwrap();

    delegate_collections::test_probe::take_selects();
    delegate_collections::test_probe::take_activates();
    let _ = delegate_collections::test_probe::take_clears();
    draw(&mut context);

    context.simulate_click(point(px(24.), px(48.)), Modifiers::default());
    draw(&mut context);
    context.update(|_, cx| assert_eq!(view.read(cx).build_error(), None));
    let selected = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(
        selected.contains("selected:alpha"),
        "click/activate must select stable id alpha: {selected}"
    );
    let mut native_ids = delegate_collections::test_probe::take_selects();
    native_ids.extend(delegate_collections::test_probe::take_activates());
    assert!(
        native_ids.iter().any(|id| id == "alpha"),
        "native list event must report id alpha: {native_ids:?}"
    );

    context.simulate_click(point(px(20.), px(10.)), Modifiers::default());
    draw(&mut context);
    let reordered = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(
        reordered.contains("selected:alpha"),
        "reorder must keep selected id alpha: {reordered}"
    );
    assert!(
        delegate_collections::test_probe::take_selects().is_empty(),
        "reorder must not emit another on_select"
    );
    assert_eq!(delegate_collections::test_probe::take_clears(), 0);

    context.simulate_click(point(px(110.), px(10.)), Modifiers::default());
    draw(&mut context);
    let filtered = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(
        filtered.contains("selected:none"),
        "filtering alpha away must clear selection: {filtered}"
    );
    assert_eq!(
        delegate_collections::test_probe::take_clears(),
        1,
        "filter-away must emit one cleared selection"
    );

    fs::remove_dir_all(PathBuf::from(root)).unwrap();
}
