#[path = "../src/shell/support.rs"]
mod support;

#[path = "../src/shell/chart/mod.rs"]
mod chart;

use gpui::{Entity, TestAppContext, VisualTestContext};
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
            "gpui-component-shell-chart-{}-{}",
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
struct Empty;
impl gpui::Render for Empty {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div()
    }
}

fn mount(
    cx: &mut TestAppContext,
    source: &str,
) -> (VisualTestContext, Entity<gpui_shell::ScriptView>) {
    cx.update(|cx| {
        gpui_component_shell::init(cx);
    });
    let mut registry = gpui_shell::ComponentRegistry::new(
        gpui_shell::COMPONENT_REGISTRY_API_VERSION,
        gpui_shell::DEFAULT_COMPONENT_MODULE,
    )
    .unwrap();
    chart::register(&mut registry).unwrap();
    let runtime =
        gpui_shell::ShellRuntime::new_isolated_with_components(registry.freeze().unwrap()).unwrap();
    let app = TempApp::new(source);
    let loaded = runtime.load_application(&app.0, "main.js").unwrap();
    let window = cx.add_window(|_, _| Empty);
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    let view = context
        .update(|window, cx| runtime.mount_application(&loaded, window, cx))
        .unwrap();
    (context, view)
}

#[gpui::test]
fn concrete_charts_consume_plain_immutable_rows(cx: &mut TestAppContext) {
    let source = r#"
import { View, div } from "gpui-kit";
import { BarChart, LineChart, AreaChart, PieChart, RadarChart } from "gpui-component";
globalThis.calls = 0;
const rows = () => { globalThis.calls++; return [{label: "Jan", value: 2}, {label: "Feb", value: 5}]; };
export default class App extends View {
  render() { return div()
    .child(new BarChart(rows).grid(false).value_axis(true))
    .child(new LineChart(rows).linear().dot().grid(false))
    .child(new AreaChart(rows).step_after().grid(false))
    .child(new PieChart(rows).inner_radius(8).pad_angle(0.05).labels(true))
    .child(new RadarChart(rows).dot().grid_levels(3)); }
}
"#;
    let (mut context, view) = mount(cx, source);
    context.draw(
        gpui::Point::default(),
        gpui::size(gpui::px(800.), gpui::px(600.)),
        {
            let view = view.clone();
            move |_, _| gpui::IntoElement::into_any_element(view)
        },
    );
    context.update(|_, cx| {
        assert_eq!(view.read(cx).build_error(), None);
        let tree = view.read(cx).snapshot().unwrap().debug_tree();
        for name in [
            "BarChart",
            "LineChart",
            "AreaChart",
            "PieChart",
            "RadarChart",
        ] {
            assert!(tree.contains(name), "missing {name}:\n{tree}");
        }
    });
}

#[gpui::test]
fn chart_rows_reject_missing_fields_without_panicking(cx: &mut TestAppContext) {
    let (mut context, view) = mount(
        cx,
        r#"
import { View } from "gpui-kit";
import { BarChart } from "gpui-component";
export default class App extends View { render() { return new BarChart(() => [{label: "Jan"}]); } }
"#,
    );
    context.draw(
        gpui::Point::default(),
        gpui::size(gpui::px(400.), gpui::px(300.)),
        {
            let view = view.clone();
            move |_, _| gpui::IntoElement::into_any_element(view)
        },
    );
    context.update(|_, cx| assert_eq!(view.read(cx).build_error(), None));
    assert!(
        chart::test_probe::take_error()
            .unwrap_or_default()
            .contains("finite number field `value`")
    );
}

#[gpui::test]
fn native_flow_and_ohlc_charts_mount(cx: &mut TestAppContext) {
    let (mut context, view) = mount(
        cx,
        r#"
import { View, div } from "gpui-kit";
import { SankeyChart, Candlestick } from "gpui-component";
export default class App extends View {
  render() { return div()
    .child(new SankeyChart(() => ({nodes:[{id:"a",label:"Source"},{id:"b",label:"Target"}],links:[{source:"a",target:"b",value:5}]})).node_width(12).id("flow").aria_label("Flow"))
    .child(new Candlestick(() => [{label:"Day",open:2,high:4,low:1,close:3}]).body_width_ratio(0.7).id("prices").aria_label("Prices")); }
}
"#,
    );
    chart::test_probe::take_error();
    context.draw(
        gpui::Point::default(),
        gpui::size(gpui::px(800.), gpui::px(600.)),
        {
            let view = view.clone();
            move |_, _| gpui::IntoElement::into_any_element(view)
        },
    );
    assert_eq!(chart::test_probe::take_error(), None);
    context.update(|_, cx| {
        assert_eq!(view.read(cx).build_error(), None);
        let tree = view.read(cx).snapshot().unwrap().debug_tree();
        assert!(
            tree.contains("SankeyChart") && tree.contains("Candlestick"),
            "{tree}"
        );
    });
}

#[gpui::test]
fn changed_javascript_data_reaches_native_charts_with_distinct_same_label_ids(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_component_shell::init);
    let mut registry = gpui_shell::ComponentRegistry::new(
        gpui_shell::COMPONENT_REGISTRY_API_VERSION,
        gpui_shell::DEFAULT_COMPONENT_MODULE,
    )
    .unwrap();
    chart::register(&mut registry).unwrap();
    let runtime =
        gpui_shell::ShellRuntime::new_isolated_with_components(registry.freeze().unwrap()).unwrap();
    let app = TempApp::new(
        r#"
import { View, div } from "gpui-kit";
import { Candlestick, SankeyChart } from "gpui-component";
export default class App extends View {
 init() { this.value = 3; }
 render() { const prices = () => [{label:"Day",open:2,high:8,low:1,close:this.value}];
 return div().size_full()
 .child(div().absolute().left(450).top(0).w(100).h(40).child("update").on_click((_e,cx) => { this.value = 7; cx.notify(); }))
 .child(new Candlestick(prices).id("prices-a").aria_label("Prices").w(200).h(150))
 .child(new Candlestick(prices).id("prices-b").aria_label("Prices").w(200).h(150))
 .child(new SankeyChart(() => ({nodes:[{id:"a",label:"A"},{id:"b",label:"B"}],links:[{source:"a",target:"b",value:this.value}]})).id("flow").w(200).h(150)); }
}
"#,
    );
    let loaded = runtime.load_application(&app.0, "main.js").unwrap();
    let window = cx.add_window(move |window, cx| {
        let view = runtime.mount_application(&loaded, window, cx).unwrap();
        gpui_component::Root::new(view, window, cx)
    });
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    chart::extended::test_probe::take();
    context.update(|window, cx| window.draw(cx).clear(cx));
    let before = chart::extended::test_probe::take();
    assert!(
        before
            .iter()
            .filter(|(kind, _)| *kind == "Candlestick")
            .count()
            >= 2
    );
    assert!(
        before
            .iter()
            .all(|(_, data)| format!("{data:?}").contains("Number(3.0)"))
    );
    context.simulate_click(
        gpui::point(gpui::px(470.), gpui::px(16.)),
        gpui::Modifiers::default(),
    );
    context.run_until_parked();
    context.update(|window, cx| window.draw(cx).clear(cx));
    let after = chart::extended::test_probe::take();
    for kind in ["Candlestick", "SankeyChart"] {
        assert!(
            after
                .iter()
                .any(|(name, data)| *name == kind && format!("{data:?}").contains("Number(7.0)")),
            "updated {kind} data missing: {after:?}"
        );
    }
    assert_eq!(chart::test_probe::take_error(), None);
}
