use super::*;
use gpui::StatefulInteractiveElement as _;
use gpui_component::{
    chart::{CandlestickChart, SankeyChart},
    plot::shape::SankeyLink,
};
use std::collections::HashMap;

#[derive(Clone)]
enum OptionValue {
    Grid(bool),
    Axis(bool),
    Tick(usize),
    Width(f32),
    Padding(f32),
    Ratio(f32),
    Label(String),
    Id(String),
}

fn object<'a>(
    value: &'a ComponentDataValue,
    keys: &[&str],
) -> anyhow::Result<&'a [(String, ComponentDataValue)]> {
    let ComponentDataValue::Object(fields) = value else {
        anyhow::bail!("expected a plain object")
    };
    anyhow::ensure!(
        fields.len() == keys.len() && fields.iter().all(|(key, _)| keys.contains(&key.as_str())),
        "unexpected or missing chart fields"
    );
    Ok(fields)
}
fn string(fields: &[(String, ComponentDataValue)], key: &str) -> anyhow::Result<String> {
    match field(fields, key) {
        Some(ComponentDataValue::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => anyhow::bail!("{key} must be non-empty text"),
    }
}
fn number(fields: &[(String, ComponentDataValue)], key: &str) -> anyhow::Result<f64> {
    match field(fields, key) {
        Some(ComponentDataValue::Number(n)) if n.is_finite() => Ok(*n),
        _ => anyhow::bail!("{key} must be finite"),
    }
}
fn array(value: &ComponentDataValue) -> anyhow::Result<&[ComponentDataValue]> {
    match value {
        ComponentDataValue::Array(rows) => Ok(rows),
        _ => anyhow::bail!("expected an array"),
    }
}
struct Candle {
    label: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}
fn candles(value: &ComponentDataValue) -> anyhow::Result<Vec<Candle>> {
    array(value)?
        .iter()
        .map(|value| {
            let fields = object(value, &["label", "open", "high", "low", "close"])?;
            let row = Candle {
                label: string(fields, "label")?,
                open: number(fields, "open")?,
                high: number(fields, "high")?,
                low: number(fields, "low")?,
                close: number(fields, "close")?,
            };
            anyhow::ensure!(
                row.low <= row.open
                    && row.open <= row.high
                    && row.low <= row.close
                    && row.close <= row.high,
                "OHLC requires low <= open/close <= high"
            );
            Ok(row)
        })
        .collect()
}
fn sankey(value: &ComponentDataValue) -> anyhow::Result<(Vec<String>, Vec<SankeyLink>)> {
    let graph = object(value, &["nodes", "links"])?;
    let mut ids = HashMap::new();
    let mut labels = Vec::new();
    for node in array(field(graph, "nodes").ok_or_else(|| anyhow::anyhow!("missing nodes"))?)? {
        let fields = object(node, &["id", "label"])?;
        anyhow::ensure!(
            ids.insert(string(fields, "id")?, labels.len()).is_none(),
            "duplicate Sankey node id"
        );
        labels.push(string(fields, "label")?);
    }
    let mut links = Vec::new();
    let mut incoming = vec![0usize; labels.len()];
    let mut outgoing = vec![Vec::new(); labels.len()];
    for link in array(field(graph, "links").ok_or_else(|| anyhow::anyhow!("missing links"))?)? {
        let fields = object(link, &["source", "target", "value"])?;
        let source = *ids
            .get(&string(fields, "source")?)
            .ok_or_else(|| anyhow::anyhow!("unknown Sankey source"))?;
        let target = *ids
            .get(&string(fields, "target")?)
            .ok_or_else(|| anyhow::anyhow!("unknown Sankey target"))?;
        let value = number(fields, "value")?;
        anyhow::ensure!(value >= 0., "Sankey value must be non-negative");
        incoming[target] += 1;
        outgoing[source].push(target);
        links.push(SankeyLink::new(source, target, value));
    }
    // Native Sankey layout requires a DAG; reject cycles before painting.
    let mut queue: Vec<_> = incoming
        .iter()
        .enumerate()
        .filter_map(|(i, n)| (*n == 0).then_some(i))
        .collect();
    let mut visited = 0;
    while let Some(source) = queue.pop() {
        visited += 1;
        for &target in &outgoing[source] {
            incoming[target] -= 1;
            if incoming[target] == 0 {
                queue.push(target);
            }
        }
    }
    anyhow::ensure!(visited == labels.len(), "Sankey graph must be acyclic");
    Ok((labels, links))
}

#[derive(gpui::IntoElement)]
struct Host {
    kind: &'static str,
    callback: gpui_shell::ComponentDataCallback,
    options: Vec<OptionValue>,
}
impl RenderOnce for Host {
    fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl gpui::IntoElement {
        let result = (|| -> anyhow::Result<gpui::AnyElement> {
            let value = self.callback.snapshot_with(&[], window, cx)?;
            let chart = if self.kind == "SankeyChart" {
                let (nodes, links) = sankey(&value)?;
                let mut chart =
                    SankeyChart::new(nodes, links).node_label(|label| label.clone().into());
                for option in &self.options {
                    chart = match option {
                        OptionValue::Width(n) => chart.node_width(*n),
                        OptionValue::Padding(n) => chart.node_padding(*n),
                        _ => chart,
                    };
                }
                chart.into_any_element()
            } else {
                let mut chart = CandlestickChart::new(candles(&value)?)
                    .x(|r| r.label.clone())
                    .open(|r| r.open)
                    .high(|r| r.high)
                    .low(|r| r.low)
                    .close(|r| r.close);
                for option in &self.options {
                    chart = match option {
                        OptionValue::Grid(v) => chart.grid(*v),
                        OptionValue::Axis(v) => chart.x_axis(*v),
                        OptionValue::Tick(v) => chart.tick_margin(*v),
                        OptionValue::Ratio(v) => chart.body_width_ratio(*v),
                        _ => chart,
                    };
                }
                chart.into_any_element()
            };
            #[cfg(test)]
            test_probe::record(self.kind, value);
            Ok(chart)
        })();
        let chart = match result {
            Ok(chart) => chart,
            Err(error) => {
                #[cfg(test)]
                super::test_probe::error(error.to_string());
                return gpui::div()
                    .child(format!("Failed to build {} data: {error:#}", self.kind))
                    .into_any_element();
            }
        };
        let label = self
            .options
            .iter()
            .rev()
            .find_map(|o| {
                if let OptionValue::Label(label) = o {
                    Some(label.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| self.kind.to_owned());
        let id = self.options.iter().rev().find_map(|option| match option {
            OptionValue::Id(id) => Some(id.clone()),
            _ => None,
        });
        let Some(id) = id else {
            return chart;
        };
        gpui::div()
            .id(gpui::SharedString::from(id))
            .size_full()
            .role(gpui::Role::Image)
            .aria_label(label)
            .child(chart)
            .into_any_element()
    }
}
struct Materializer(&'static str);
impl ComponentMaterializer for Materializer {
    fn materialize(&self, request: MaterializeRequest<'_>) -> anyhow::Result<gpui::AnyElement> {
        let payload = request
            .payload()
            .downcast_ref::<Payload>()
            .ok_or_else(|| anyhow::anyhow!("incompatible chart payload"))?;
        let callback = request.resolve_data_callback(&payload.0)?;
        let options = request
            .methods()
            .filter_map(|m| m.payload().downcast_ref::<OptionValue>().cloned())
            .collect::<Vec<_>>();
        anyhow::ensure!(
            !options.iter().any(|o| matches!(o, OptionValue::Label(_)))
                || options.iter().any(|o| matches!(o, OptionValue::Id(_))),
            "chart.aria_label requires a stable chart.id"
        );
        wrap(
            request,
            Host {
                kind: self.0,
                callback,
                options,
            },
        )
    }
}
fn numeric(
    name: &'static str,
    max: f64,
    positive: bool,
    integer: bool,
    make: fn(f64) -> OptionValue,
) -> MethodDescriptor {
    MethodDescriptor::new(
        name,
        vec![ArgumentDescriptor::new(name, ArgumentSchema::Number)],
        move |args| match args {
            [ComponentArgument::Number(n)]
                if n.is_finite()
                    && *n >= 0.
                    && (!positive || *n > 0.)
                    && *n <= max
                    && (!integer || n.fract() == 0.) =>
            {
                Ok(ComponentPayload::new(make(*n)))
            }
            _ => Err(format!("invalid {name}")),
        },
    )
    .with_documentation("Configure a native chart option within its valid range.")
}
pub(super) fn register(registry: &mut ComponentRegistry) -> Result<(), RegistryError> {
    for (name, schema, mut methods) in [
        (
            "SankeyChart",
            "(cx: Context) => { nodes: readonly { id: string; label: string }[]; links: readonly { source: string; target: string; value: number }[] }",
            vec![
                numeric("node_width", f32::MAX as f64, true, false, |n| {
                    OptionValue::Width(n as f32)
                }),
                numeric("node_padding", f32::MAX as f64, false, false, |n| {
                    OptionValue::Padding(n as f32)
                }),
            ],
        ),
        (
            "Candlestick",
            "(cx: Context) => readonly { label: string; open: number; high: number; low: number; close: number }[]",
            vec![
                bool_method("Candlestick", "grid", "Show grid.", OptionValue::Grid),
                bool_method("Candlestick", "x_axis", "Show x axis.", OptionValue::Axis),
                numeric("tick_margin", usize::MAX as f64, true, true, |n| {
                    OptionValue::Tick(n as usize)
                }),
                numeric("body_width_ratio", 1., true, false, |n| {
                    OptionValue::Ratio(n as f32)
                }),
            ],
        ),
    ] {
        methods.push(MethodDescriptor::new("id", vec![ArgumentDescriptor::new("id", ArgumentSchema::String)], |args| match args {
            [ComponentArgument::String(id)] if !id.trim().is_empty() => Ok(ComponentPayload::new(OptionValue::Id(id.clone()))),
            _ => Err("chart.id expects a non-empty stable id".into()),
        }).with_documentation("Sets a stable, sibling-unique identity for the accessible chart wrapper. aria_label describes this identified chart; labels are never used as identity."));
        methods.push(
            MethodDescriptor::new(
                "aria_label",
                vec![ArgumentDescriptor::new("label", ArgumentSchema::String)],
                |args| match args {
                    [ComponentArgument::String(label)] if !label.trim().is_empty() => {
                        Ok(ComponentPayload::new(OptionValue::Label(label.clone())))
                    }
                    _ => Err("aria_label expects non-empty text".into()),
                },
            )
            .with_documentation("Accessible chart description; requires an explicit stable id."),
        );
        registry.register(ComponentDescriptor::new(name,Arc::new(Materializer(name))).with_constructors(vec![ConstructorDescriptor::new(name,vec![ArgumentDescriptor::new("data",ArgumentSchema::Callback(schema))],|args| match args { [arg @ ComponentArgument::Callback(_)] => Ok(ComponentPayload::new(Payload(arg.clone()))), _ => Err("chart requires an immutable data callback".into()) })]).with_methods(methods).with_documentation("Native chart from one atomic plain-data snapshot; native defaults apply unless overridden."))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn data(value: serde_json::Value) -> ComponentDataValue {
        match value {
            serde_json::Value::String(s) => ComponentDataValue::String(s),
            serde_json::Value::Number(n) => ComponentDataValue::Number(n.as_f64().unwrap()),
            serde_json::Value::Array(a) => {
                ComponentDataValue::Array(a.into_iter().map(data).collect())
            }
            serde_json::Value::Object(o) => {
                ComponentDataValue::Object(o.into_iter().map(|(k, v)| (k, data(v))).collect())
            }
            _ => panic!("unsupported fixture"),
        }
    }
    #[test]
    fn sankey_rejects_invalid_topology_before_native_layout() {
        let base = serde_json::json!({"nodes":[{"id":"a","label":"A"},{"id":"b","label":"B"}],"links":[{"source":"a","target":"b","value":2}]});
        let (_, links) = sankey(&data(base.clone())).unwrap();
        assert_eq!(links, vec![SankeyLink::new(0, 1, 2.)]);
        for (key, value) in [
            ("target", serde_json::json!("missing")),
            ("target", serde_json::json!("a")),
            ("value", serde_json::json!(-1)),
        ] {
            let mut invalid = base.clone();
            invalid["links"][0][key] = value;
            assert!(sankey(&data(invalid)).is_err());
        }
        let mut duplicate = base;
        duplicate["nodes"][1]["id"] = serde_json::json!("a");
        assert!(sankey(&data(duplicate)).is_err());
    }
    #[test]
    fn ohlc_preserves_prices_and_rejects_impossible_ranges() {
        let row = serde_json::json!([{"label":"Day","open":2,"high":4,"low":1,"close":3}]);
        let parsed = candles(&data(row.clone())).unwrap();
        assert_eq!(
            (
                parsed[0].open,
                parsed[0].high,
                parsed[0].low,
                parsed[0].close
            ),
            (2., 4., 1., 3.)
        );
        for key in ["open", "close", "low"] {
            let mut invalid = row.clone();
            invalid[0][key] = serde_json::json!(5);
            assert!(candles(&data(invalid)).is_err());
        }
        let mut nonfinite = data(row);
        let ComponentDataValue::Array(rows) = &mut nonfinite else {
            unreachable!()
        };
        let ComponentDataValue::Object(fields) = &mut rows[0] else {
            unreachable!()
        };
        fields.iter_mut().find(|(key, _)| key == "high").unwrap().1 =
            ComponentDataValue::Number(f64::INFINITY);
        assert!(candles(&nonfinite).is_err());
    }
}

#[cfg(test)]
pub(crate) mod test_probe {
    use super::*;
    use std::cell::RefCell;
    thread_local! { static DATA: RefCell<Vec<(&'static str, ComponentDataValue)>> = const { RefCell::new(Vec::new()) }; }
    pub(super) fn record(kind: &'static str, value: ComponentDataValue) {
        DATA.with(|data| data.borrow_mut().push((kind, value)));
    }
    pub(crate) fn take() -> Vec<(&'static str, ComponentDataValue)> {
        DATA.with(|data| std::mem::take(&mut *data.borrow_mut()))
    }
}
