#[path = "../src/shell/searchable_list.rs"]
mod searchable_list;
use gpui::{Modifiers, TestAppContext, VisualTestContext, point, px};
use std::{fs, ops::Deref};
fn draw(context: &mut VisualTestContext) {
    context.run_until_parked();
    context.update(|window, cx| window.draw(cx).clear(cx));
}
#[gpui::test]
fn query_filters_native_rows_and_selection_survives_model_rerender(cx: &mut TestAppContext) {
    cx.update(gpui_component_shell::init);
    let root = std::env::temp_dir().join(format!("searchable-list-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("main.js"),r#"
import {View, div} from 'gpui-kit';
import {SearchableList} from 'gpui-component';
export default class App extends View {
 init() {this.query=''; this.selected=''; this.events=0;}
 render() {
  return div().size_full()
   .child(div().child(`selected:${this.selected};query:${this.query};events:${this.events}`))
   .child(new SearchableList('people',()=>[{id:'alpha',label:'Alpha'},{id:'beta',label:'Beta'}],row=>div().h(32).child(row.label))
    .query(this.query).selected_id(this.selected)
    .on_query_change((query,cx)=>{this.query=query;cx.notify();})
    .on_select((id,cx)=>{this.selected=id;this.events++;cx.notify();})
    .on_confirm((id,cx)=>{this.selected=id;this.events++;cx.notify();})
    .absolute().left(0).top(40).w(280).h(240));
 }
}"#).unwrap();
    let mut registry = gpui_shell::ComponentRegistry::new(
        gpui_shell::COMPONENT_REGISTRY_API_VERSION,
        gpui_shell::DEFAULT_COMPONENT_MODULE,
    )
    .unwrap();
    searchable_list::register(&mut registry).unwrap();
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
    draw(&mut context);
    context.simulate_click(point(px(40.), px(58.)), Modifiers::default());
    context.simulate_keystrokes("b e t a");
    draw(&mut context);
    searchable_list::test_probe::take_rows();
    draw(&mut context);
    let rows = searchable_list::test_probe::take_rows();
    assert!(!rows.is_empty(), "native list must render the matching row");
    assert!(
        rows.iter().all(|id| id == "beta"),
        "query must filter actual rows: {rows:?}"
    );
    context.simulate_keystrokes("down enter");
    draw(&mut context);
    let before = context.update(|_, cx| view.read(cx).snapshot().unwrap().debug_tree());
    assert!(
        before.contains("selected:beta;query:beta"),
        "native selection must roundtrip through JS model: {before}"
    );
    context.update(|_, cx| view.update(cx, |view, cx| view.refresh(cx)));
    draw(&mut context);
    let after = context.update(|_, cx| {
        assert_eq!(view.read(cx).build_error(), None);
        view.read(cx).snapshot().unwrap().debug_tree()
    });
    assert_eq!(
        before, after,
        "refresh must retain selection and must not emit callbacks"
    );
    fs::remove_dir_all(root).unwrap();
}
