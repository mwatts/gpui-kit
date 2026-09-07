#[path = "../src/shell/support.rs"]
mod support;

#[path = "../src/shell/data_table/mod.rs"]
mod data_table;

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
            "gpui-component-shell-data-table-{}-{}",
            std::process::id(),
            NEXT_APP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
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
    cx.update(|cx| {
        gpui_component_shell::init(cx);
    });
    let mut registry = gpui_shell::ComponentRegistry::new(
        gpui_shell::COMPONENT_REGISTRY_API_VERSION,
        gpui_shell::DEFAULT_COMPONENT_MODULE,
    )
    .unwrap();
    data_table::register(&mut registry).unwrap();
    let runtime =
        gpui_shell::ShellRuntime::new_isolated_with_components(registry.freeze().unwrap()).unwrap();
    let app = TempApp::new(source);
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
fn retained_data_table_renders_lazy_cells_from_plain_rows(cx: &mut TestAppContext) {
    data_table::test_probe::reset();
    let source = r#"
import { View, div } from "gpui-kit";
import { DataTableState, DataTable } from "gpui-component";
export default class App extends View { render() { return new DataTable(
  DataTableState(["name", "status"]),
  () => [{name: "Ada", status: "Ready"}, {name: "Lin", status: "Busy"}],
  (row, column) => div().child(row[column])
).stripe(true).bordered(false).row_selectable(true).cell_selectable(true); } }
"#;
    let (mut context, view, _app) = mount(cx, source);
    draw(&mut context);
    context.update(|_, cx| assert_eq!(view.read(cx).build_error(), None));
    assert!(data_table::test_probe::cell_builds() >= 4);
}

#[gpui::test]
fn data_table_rejects_non_array_snapshot_without_panicking(cx: &mut TestAppContext) {
    data_table::test_probe::reset();
    let (mut context, view, _app) = mount(
        cx,
        r#"
import { View, div } from "gpui-kit"; import { DataTableState, DataTable } from "gpui-component";
export default class App extends View { render() { return new DataTable(DataTableState(["name"]), () => ({name:"Ada"}), () => div()); } }
"#,
    );
    draw(&mut context);
    context.update(|_, cx| assert_eq!(view.read(cx).build_error(), None));
    assert_eq!(data_table::test_probe::cell_builds(), 0);
    assert!(data_table::test_probe::errors() >= 1);
}

#[gpui::test]
fn sortable_header_click_emits_on_sort_and_script_reorder_paints_new_row_order(
    cx: &mut TestAppContext,
) {
    data_table::test_probe::reset();
    let source = r#"
import { View, div } from "gpui-kit";
import { DataTableState, DataTable } from "gpui-component";
export default class App extends View {
  init() {
    this.rows = [
      {id: "ada", name: "Ada", status: "Ready"},
      {id: "lin", name: "Lin", status: "Busy"}
    ];
    this.sortKey = "";
    this.sortDir = "default";
  }
  render() {
    let table = new DataTable(
      DataTableState(["name", "status"]),
      () => this.rows,
      (row, column) => div().child(row[column])
    ).stripe(true).column_movable(false).sortable(true)
      .on_sort((key, direction, cx) => {
        this.sortKey = key;
        this.sortDir = direction;
        const sign = direction === "ascending" ? 1 : direction === "descending" ? -1 : 0;
        this.rows = [...this.rows].sort((a, b) => sign * String(a[key]).localeCompare(String(b[key])));
        cx.notify();
      });
    if (this.sortKey) table = table.sorted(this.sortKey, this.sortDir);
    return div().size_full()
      .child(`sort:${this.sortKey}:${this.sortDir}`)
      .child(`order:${this.rows.map(row => row.name).join(",")}`)
      .child(table.absolute().left(0).top(0).size_full());
  }
}
"#;
    let (mut context, view, _app) = mount(cx, source);
    draw(&mut context);
    context.update(|_, cx| assert_eq!(view.read(cx).build_error(), None));
    data_table::test_probe::take_cells();
    data_table::test_probe::take_sorts();

    context.simulate_click(point(px(88.), px(14.)), Modifiers::default());
    draw(&mut context);
    context.update(|_, cx| assert_eq!(view.read(cx).build_error(), None));
    let first = data_table::test_probe::take_sorts();
    assert_eq!(first, [("name".into(), "descending".into())], "{first:?}");
    draw(&mut context);
    let descending_tree = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(
        descending_tree.contains("sort:name:descending")
            && descending_tree.contains("order:Lin,Ada"),
        "on_sort must reach the script and reorder the snapshot: {descending_tree}"
    );
    let descending = data_table::test_probe::take_cells();
    let first_name = descending
        .iter()
        .rev()
        .find(|(row, column, _)| *row == 0 && *column == 0)
        .map(|(_, _, value)| value.as_str());
    assert_eq!(first_name, Some("Lin"), "{descending:?}");

    context.simulate_click(point(px(88.), px(14.)), Modifiers::default());
    draw(&mut context);
    let second = data_table::test_probe::take_sorts();
    assert_eq!(second, [("name".into(), "ascending".into())], "{second:?}");
    draw(&mut context);
    let ascending_tree = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(
        ascending_tree.contains("sort:name:ascending") && ascending_tree.contains("order:Ada,Lin"),
        "second click must toggle direction: {ascending_tree}"
    );
    let ascending = data_table::test_probe::take_cells();
    let first_name = ascending
        .iter()
        .rev()
        .find(|(row, column, _)| *row == 0 && *column == 0)
        .map(|(_, _, value)| value.as_str());
    assert_eq!(first_name, Some("Ada"), "{ascending:?}");
}

#[gpui::test]
fn sortable_without_on_sort_is_a_materialize_error(cx: &mut TestAppContext) {
    let (mut context, view, _app) = mount(
        cx,
        r#"
import { View, div } from "gpui-kit";
import { DataTableState, DataTable } from "gpui-component";
export default class App extends View {
  render() {
    return new DataTable(DataTableState(["name"]), () => [{name: "Ada"}], (row, column) => div().child(row[column])).sortable(true);
  }
}
"#,
    );
    draw(&mut context);
    let error = context.update(|_, cx| {
        view.read(cx)
            .build_error()
            .expect("sortable without on_sort must fail materialize")
            .to_owned()
    });
    assert!(error.contains("on_sort"), "{error}");
}
