use gpui_component::{
    IndexPath,
    searchable_list::{
        InlineSearchableList, InlineSearchableListEvent, SearchableListDelegate, SearchableListItem,
    },
};
use gpui_shell::{
    gpui::{
        self, App, AppContext as _, Entity, IntoElement as _, ParentElement as _, Refineable as _,
        RenderOnce, SharedString, Styled as _, Subscription, Window,
    },
    *,
};
use std::{cell::RefCell, collections::HashSet, rc::Rc, sync::Arc};

#[derive(Clone)]
struct Row {
    id: String,
    label: SharedString,
    data: ComponentDataValue,
}
impl SearchableListItem for Row {
    type Value = String;
    fn title(&self) -> SharedString {
        self.label.clone()
    }
    fn value(&self) -> &String {
        &self.id
    }
}
struct Delegate {
    rows: Vec<Row>,
    visible: Vec<Row>,
    renderer: Option<ComponentElementCallback>,
}
impl Delegate {
    fn filter(&mut self, query: &str) {
        self.visible = self
            .rows
            .iter()
            .filter(|row| row.matches(query))
            .cloned()
            .collect();
    }
}
impl SearchableListDelegate for Delegate {
    type Item = Row;
    fn items_count(&self, section: usize) -> usize {
        if section == 0 { self.visible.len() } else { 0 }
    }
    fn item(&self, ix: IndexPath) -> Option<&Row> {
        self.visible.get(ix.row)
    }
    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        Row: SearchableListItem<Value = V>,
        V: PartialEq,
    {
        self.visible
            .iter()
            .position(|row| row.value() == value)
            .map(IndexPath::new)
    }
    fn perform_search(&mut self, query: &str, _: &mut Window, _: &mut App) -> gpui::Task<()> {
        self.filter(query);
        gpui::Task::ready(())
    }
    fn render_item(
        &self,
        _: IndexPath,
        row: &Row,
        _: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<gpui::AnyElement> {
        #[cfg(test)]
        test_probe::ROWS.with(|rows| rows.borrow_mut().push(row.id.clone()));
        self.renderer.as_ref().map(|renderer| {
            match renderer.build_data_with(&[row.data.clone()], window, cx) {
                Ok(Some(element)) => element,
                Ok(None) => gpui::div().into_any_element(),
                Err(error) => gpui::div()
                    .child(format!("SearchableList row: {error:#}"))
                    .into_any_element(),
            }
        })
    }
}
#[derive(Clone)]
struct Payload {
    id: String,
    rows: ComponentArgument,
    renderer: Option<ComponentArgument>,
}
#[derive(Clone)]
enum Op {
    Query(String),
    Selected(String),
    Callback(&'static str, ComponentArgument),
}
#[derive(Default)]
struct Callbacks {
    query: Option<ComponentCallback>,
    select: Option<ComponentCallback>,
    confirm: Option<ComponentCallback>,
}
struct Host {
    state: Entity<InlineSearchableList<Delegate>>,
    callbacks: Rc<RefCell<Callbacks>>,
    selected: Rc<RefCell<Option<String>>>,
    _subscription: Subscription,
}
#[derive(gpui::IntoElement)]
struct Bound {
    id: String,
    rows: ComponentDataCallback,
    renderer: Option<ComponentElementCallback>,
    query: Option<String>,
    selected: Option<String>,
    callbacks: Callbacks,
    style: gpui::StyleRefinement,
}
fn field<'a>(data: &'a ComponentDataValue, name: &str) -> Option<&'a str> {
    if let ComponentDataValue::Object(fields) = data {
        fields.iter().find_map(|(key, value)| {
            if key == name {
                if let ComponentDataValue::String(value) = value {
                    Some(value.as_str())
                } else {
                    None
                }
            } else {
                None
            }
        })
    } else {
        None
    }
}
impl RenderOnce for Bound {
    fn render(self, window: &mut Window, cx: &mut App) -> impl gpui::IntoElement {
        let rows = (|| -> anyhow::Result<Vec<Row>> {
            let snapshot = self.rows.snapshot_rows_with(&[], window, cx)?;
            let mut ids = HashSet::new();
            (0..snapshot.len())
                .map(|ix| {
                    let data = snapshot.row(ix)?.clone();
                    let id = field(&data, "id")
                        .filter(|id| !id.is_empty())
                        .ok_or_else(|| anyhow::anyhow!("SearchableList row requires id"))?
                        .to_string();
                    anyhow::ensure!(ids.insert(id.clone()), "SearchableList duplicate id {id}");
                    let label = field(&data, "label")
                        .ok_or_else(|| anyhow::anyhow!("SearchableList row requires label"))?
                        .to_string()
                        .into();
                    Ok(Row { id, label, data })
                })
                .collect()
        })();
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) => return gpui::div().child(error.to_string()).into_any_element(),
        };
        let initial = rows.clone();
        let renderer = self.renderer.clone();
        let host: Entity<Host> = window.use_keyed_state(
            format!("shell-searchable-list:{}", self.id),
            cx,
            move |window, cx| {
                let state = cx.new(|cx| {
                    InlineSearchableList::new(
                        Delegate {
                            rows: initial.clone(),
                            visible: initial,
                            renderer,
                        },
                        window,
                        cx,
                    )
                });
                let callbacks = Rc::new(RefCell::new(Callbacks::default()));
                let selected = Rc::new(RefCell::new(None));
                let cb = callbacks.clone();
                let selection = selected.clone();
                let subscription = window.subscribe(
                    &state,
                    cx,
                    move |_, event: &InlineSearchableListEvent<String>, window, cx| {
                        let (callback, name, value) = match event {
                            InlineSearchableListEvent::Query(query) => (
                                cb.borrow().query.clone(),
                                "SearchableList.on_query_change",
                                query.clone(),
                            ),
                            InlineSearchableListEvent::Select(id) => {
                                *selection.borrow_mut() = Some(id.clone());
                                (
                                    cb.borrow().select.clone(),
                                    "SearchableList.on_select",
                                    id.clone(),
                                )
                            }
                            InlineSearchableListEvent::Confirm(id) => {
                                *selection.borrow_mut() = Some(id.clone());
                                (
                                    cb.borrow().confirm.clone(),
                                    "SearchableList.on_confirm",
                                    id.clone(),
                                )
                            }
                        };
                        if let Some(callback) = callback {
                            callback.invoke_and_report_with(
                                name,
                                &[ComponentCallbackArgument::String(value)],
                                window,
                                cx,
                            );
                        }
                    },
                );
                Host {
                    state,
                    callbacks,
                    selected,
                    _subscription: subscription,
                }
            },
        );
        let host = host.read(cx);
        let state = host.state.clone();
        *host.callbacks.borrow_mut() = self.callbacks;
        if let Some(selected) = self.selected {
            *host.selected.borrow_mut() = if selected.is_empty() {
                None
            } else {
                Some(selected)
            };
        }
        let selected = host.selected.borrow().clone();
        state.update(cx, |state, cx| {
            let query = self.query.unwrap_or_else(|| state.query(cx));
            state.update_delegate(
                |delegate| {
                    delegate.rows = rows;
                    delegate.renderer = self.renderer;
                    delegate.filter(&query);
                },
                cx,
            );
            state.set_query(&query, window, cx);
            state.set_selected_value(selected.as_ref(), window, cx);
        });
        let mut container = gpui::div().child(state);
        container.style().refine(&self.style);
        container.into_any_element()
    }
}
struct Materializer;
impl ComponentMaterializer for Materializer {
    fn materialize(&self, mut request: MaterializeRequest<'_>) -> anyhow::Result<gpui::AnyElement> {
        let payload = request
            .payload()
            .downcast_ref::<Payload>()
            .ok_or_else(|| anyhow::anyhow!("SearchableList payload"))?
            .clone();
        let rows = request.resolve_data_callback(&payload.rows)?;
        let renderer = payload
            .renderer
            .as_ref()
            .map(|arg| request.resolve_element_callback(arg))
            .transpose()?;
        let ops: Vec<_> = request
            .methods()
            .filter_map(|method| method.payload().downcast_ref::<Op>().cloned())
            .collect();
        let mut query = None;
        let mut selected = None;
        let mut callbacks = Callbacks::default();
        for op in ops {
            match op {
                Op::Query(value) => query = Some(value),
                Op::Selected(value) => selected = Some(value),
                Op::Callback(name, arg) => {
                    let cb = Some(request.resolve_callback(&arg)?);
                    match name {
                        "on_query_change" => callbacks.query = cb,
                        "on_select" => callbacks.select = cb,
                        _ => callbacks.confirm = cb,
                    }
                }
            }
        }
        anyhow::ensure!(
            request.take_typed_children()?.is_empty(),
            "SearchableList rows come from its items callback"
        );
        Ok(Bound {
            id: payload.id,
            rows,
            renderer,
            query,
            selected,
            callbacks,
            style: request.take_style(),
        }
        .into_any_element())
    }
}
pub(super) fn register(registry: &mut ComponentRegistry) -> Result<(), RegistryError> {
    let constructors = vec![ConstructorDescriptor::new(
        "SearchableList",
        vec![
            ArgumentDescriptor::new("id", ArgumentSchema::String),
            ArgumentDescriptor::new(
                "items",
                ArgumentSchema::Callback("() => readonly {id: string; label: string}[]"),
            ),
            ArgumentDescriptor::new(
                "render_row",
                ArgumentSchema::Optional(Box::new(ArgumentSchema::Callback(
                    "(row: unknown) => Element | null",
                ))),
            ),
        ],
        |args| match args {
            [
                ComponentArgument::String(id),
                rows @ ComponentArgument::Callback(_),
                ComponentArgument::Optional(renderer),
            ] if !id.trim().is_empty() => Ok(ComponentPayload::new(Payload {
                id: id.clone(),
                rows: rows.clone(),
                renderer: renderer.as_deref().cloned(),
            })),
            _ => Err("SearchableList expects id, items callback, optional renderer".into()),
        },
    )];
    let mut methods = vec![];
    for name in ["query", "selected_id"] {
        methods.push(MethodDescriptor::new(
            name,
            vec![ArgumentDescriptor::new("value", ArgumentSchema::String)],
            move |args| match args {
                [ComponentArgument::String(value)] => {
                    Ok(ComponentPayload::new(if name == "query" {
                        Op::Query(value.clone())
                    } else {
                        Op::Selected(value.clone())
                    }))
                }
                _ => Err(format!("SearchableList.{name} expects string")),
            },
        ).with_documentation("Controls the query or selected stable row id; an empty selected id clears selection."));
    }
    for name in ["on_query_change", "on_select", "on_confirm"] {
        methods.push(MethodDescriptor::new(
            name,
            vec![ArgumentDescriptor::new(
                "callback",
                ArgumentSchema::Callback("(value: string, cx: Context) => void"),
            )],
            move |args| match args {
                [arg @ ComponentArgument::Callback(_)] => {
                    Ok(ComponentPayload::new(Op::Callback(name, arg.clone())))
                }
                _ => Err(format!("SearchableList.{name} expects callback")),
            },
        ).with_documentation("Reports native user changes as a string followed by the script context; controlled updates do not emit events."));
    }
    registry.register(
        ComponentDescriptor::new("SearchableList", Arc::new(Materializer))
            .with_constructors(constructors)
            .with_methods(methods)
            .with_documentation("Retained native inline searchable list with controlled query, stable row selection, and keyboard confirmation."),
    )?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_probe {
    thread_local! { pub(super) static ROWS: std::cell::RefCell<Vec<String>> = const {std::cell::RefCell::new(Vec::new())}; }
    pub(crate) fn take_rows() -> Vec<String> {
        ROWS.with(|rows| std::mem::take(&mut *rows.borrow_mut()))
    }
}
