use gpui_component::{
    IndexPath,
    list::{List, ListDelegate, ListEvent, ListItem, ListState},
};
use gpui_shell::{
    ArgumentDescriptor, ArgumentSchema, ComponentArgument, ComponentCallback,
    ComponentCallbackArgument, ComponentDataCallback, ComponentDataValue,
    ComponentDelegateSnapshot, ComponentDescriptor, ComponentElementCallback,
    ComponentMaterializer, ComponentPayload, ComponentRegistry, ConstructorDescriptor,
    MaterializeRequest, MethodDescriptor, RegistryError, anyhow,
    gpui::{
        self, App, AppContext as _, Entity, IntoElement as _, ParentElement as _, Refineable as _,
        RenderOnce, SharedString, Styled as _, Subscription, Window,
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    sync::Arc,
};

#[derive(Clone)]
struct Payload {
    id: String,
    rows: ComponentArgument,
    render_row: ComponentArgument,
}

#[derive(Clone)]
enum ListOp {
    OnSelect(ComponentArgument),
    OnActivate(ComponentArgument),
    Selected(String),
}

struct Delegate {
    rows: ComponentDelegateSnapshot,
    render_row: ComponentElementCallback,
    selected: Option<IndexPath>,
    ids: Vec<String>,
}

fn object_string_field<'a>(row: &'a ComponentDataValue, name: &str) -> Option<&'a str> {
    let ComponentDataValue::Object(fields) = row else {
        return None;
    };
    fields.iter().find_map(|(key, value)| match value {
        ComponentDataValue::String(value) if key == name && !value.is_empty() => {
            Some(value.as_str())
        }
        _ => None,
    })
}

fn unique_ids(snapshot: &ComponentDelegateSnapshot) -> anyhow::Result<Vec<String>> {
    let mut ids = Vec::with_capacity(snapshot.len());
    let mut seen = HashSet::new();
    for index in 0..snapshot.len() {
        let row = snapshot.row(index)?;
        let Some(id) = object_string_field(row, "id") else {
            anyhow::bail!("List row {index} requires a unique string `id`");
        };
        if !seen.insert(id.to_string()) {
            anyhow::bail!("List duplicate row id `{id}`");
        }
        ids.push(id.to_string());
    }
    Ok(ids)
}

impl ListDelegate for Delegate {
    type Item = ListItem;

    fn items_count(&self, section: usize, _: &App) -> usize {
        usize::from(section == 0) * self.rows.len()
    }

    fn render_item(
        &mut self,
        path: IndexPath,
        window: &mut Window,
        cx: &mut gpui::Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let row = match self.rows.row(path.row) {
            Ok(row) => row.clone(),
            Err(error) => {
                return Some(
                    ListItem::new(format!("item-{}", path.row))
                        .child(format!("Failed to read List row: {error:#}")),
                );
            }
        };
        let Some(id) = self
            .ids
            .get(path.row)
            .cloned()
            .or_else(|| object_string_field(&row, "id").map(str::to_string))
        else {
            return Some(
                ListItem::new(("missing-id", path.row))
                    .child("List row requires a unique string `id`"),
            );
        };
        let id: SharedString = id.into();
        let child = match self.render_row.build_data_with(&[row], window, cx) {
            Ok(Some(element)) => {
                #[cfg(test)]
                test_probe::row(id.to_string());
                element
            }
            Ok(None) => gpui::div().into_any_element(),
            Err(error) => gpui::div()
                .child(format!("Failed to render List row: {error:#}"))
                .into_any_element(),
        };
        Some(
            ListItem::new(id)
                .selected(self.selected == Some(path))
                .child(child),
        )
    }

    fn set_selected_index(
        &mut self,
        path: Option<IndexPath>,
        _: &mut Window,
        _: &mut gpui::Context<ListState<Self>>,
    ) {
        self.selected = path;
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod test_probe {
    use std::cell::{Cell, RefCell};

    thread_local! {
        static ROWS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static SELECTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static ACTIVATES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static CLEARS: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) fn row(id: String) {
        ROWS.with(|rows| rows.borrow_mut().push(id));
    }

    pub(super) fn select(id: String) {
        SELECTS.with(|values| values.borrow_mut().push(id));
    }

    pub(super) fn activate(id: String) {
        ACTIVATES.with(|values| values.borrow_mut().push(id));
    }

    pub(super) fn clear() {
        CLEARS.with(|value| value.set(value.get() + 1));
    }

    pub(crate) fn take_rows() -> Vec<String> {
        ROWS.with(|rows| std::mem::take(&mut *rows.borrow_mut()))
    }

    pub(crate) fn take_selects() -> Vec<String> {
        SELECTS.with(|values| std::mem::take(&mut *values.borrow_mut()))
    }

    pub(crate) fn take_activates() -> Vec<String> {
        ACTIVATES.with(|values| std::mem::take(&mut *values.borrow_mut()))
    }

    pub(crate) fn take_clears() -> usize {
        CLEARS.with(|value| {
            let count = value.get();
            value.set(0);
            count
        })
    }
}

struct ListCallbacks {
    on_select: Option<ComponentCallback>,
    on_activate: Option<ComponentCallback>,
}

struct Host {
    state: Entity<ListState<Delegate>>,
    callback: Rc<RefCell<ListCallbacks>>,
    last_id: Rc<RefCell<Option<String>>>,
    cleared: Rc<Cell<bool>>,
    _subscription: Subscription,
}

#[derive(gpui::IntoElement)]
struct BoundList {
    id: String,
    rows: ComponentDataCallback,
    render_row: ComponentElementCallback,
    on_select: Option<ComponentCallback>,
    on_activate: Option<ComponentCallback>,
    selected: Option<String>,
    style: gpui::StyleRefinement,
}

impl RenderOnce for BoundList {
    fn render(self, window: &mut Window, cx: &mut App) -> impl gpui::IntoElement {
        let snapshot = match self.rows.snapshot_rows_with(&[], window, cx) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return gpui::div()
                    .child(format!("Failed to snapshot List rows: {error:#}"))
                    .into_any_element();
            }
        };
        let ids = match unique_ids(&snapshot) {
            Ok(ids) => ids,
            Err(error) => {
                return gpui::div().child(error.to_string()).into_any_element();
            }
        };
        let initial_snapshot = snapshot.clone();
        let initial_renderer = self.render_row.clone();
        let initial_ids = ids.clone();
        let initial_callbacks = ListCallbacks {
            on_select: self.on_select.clone(),
            on_activate: self.on_activate.clone(),
        };
        let host: Entity<Host> =
            window.use_keyed_state(format!("shell-list:{}", self.id), cx, move |window, cx| {
                let state = cx.new(|cx| {
                    ListState::new(
                        Delegate {
                            rows: initial_snapshot,
                            render_row: initial_renderer,
                            selected: None,
                            ids: initial_ids,
                        },
                        window,
                        cx,
                    )
                });
                let callback = Rc::new(RefCell::new(initial_callbacks));
                let last_id = Rc::new(RefCell::new(None::<String>));
                let cleared = Rc::new(Cell::new(false));
                let event_callback = callback.clone();
                let event_last = last_id.clone();
                let event_cleared = cleared.clone();
                let watched = state.clone();
                let subscription =
                    window.subscribe(&state, cx, move |_, event: &ListEvent, window, cx| {
                        let ids = watched.read(cx).delegate().ids.clone();
                        match event {
                            ListEvent::Select(path) => {
                                let Some(id) = ids.get(path.row).cloned() else {
                                    return;
                                };
                                *event_last.borrow_mut() = Some(id.clone());
                                event_cleared.set(false);
                                #[cfg(test)]
                                test_probe::select(id.clone());
                                if let Some(callback) = event_callback.borrow().on_select.clone() {
                                    callback.invoke_and_report_with(
                                        "List.on_select",
                                        &[ComponentCallbackArgument::String(id)],
                                        window,
                                        cx,
                                    );
                                }
                            }
                            ListEvent::Confirm(path) => {
                                let Some(id) = ids.get(path.row).cloned() else {
                                    return;
                                };
                                *event_last.borrow_mut() = Some(id.clone());
                                event_cleared.set(false);
                                #[cfg(test)]
                                test_probe::activate(id.clone());
                                if let Some(callback) = event_callback.borrow().on_activate.clone()
                                {
                                    callback.invoke_and_report_with(
                                        "List.on_activate",
                                        &[ComponentCallbackArgument::String(id)],
                                        window,
                                        cx,
                                    );
                                }
                            }
                            ListEvent::Cancel => {
                                *event_last.borrow_mut() = None;
                                if !event_cleared.replace(true) {
                                    #[cfg(test)]
                                    test_probe::clear();
                                    if let Some(callback) =
                                        event_callback.borrow().on_select.clone()
                                    {
                                        callback.invoke_and_report_with(
                                            "List.on_select",
                                            &[ComponentCallbackArgument::Array(Vec::new())],
                                            window,
                                            cx,
                                        );
                                    }
                                }
                            }
                        }
                    });
                Host {
                    state,
                    callback,
                    last_id,
                    cleared,
                    _subscription: subscription,
                }
            });
        let (state, callback, last_id, cleared) = {
            let host = host.read(cx);
            (
                host.state.clone(),
                host.callback.clone(),
                host.last_id.clone(),
                host.cleared.clone(),
            )
        };
        *callback.borrow_mut() = ListCallbacks {
            on_select: self.on_select.clone(),
            on_activate: self.on_activate.clone(),
        };
        let target = self.selected.clone().or_else(|| last_id.borrow().clone());
        state.update(cx, |state, cx| {
            state.delegate_mut().rows = snapshot;
            state.delegate_mut().render_row = self.render_row;
            state.delegate_mut().ids = ids.clone();
            if let Some(id) = target {
                if let Some(index) = ids.iter().position(|row| row == &id) {
                    cleared.set(false);
                    let path = IndexPath::new(index);
                    if state.selected_index() != Some(path) {
                        state.set_selected_index(Some(path), window, cx);
                    }
                    *last_id.borrow_mut() = Some(id);
                } else {
                    state.set_selected_index(None, window, cx);
                    *last_id.borrow_mut() = None;
                    if !cleared.replace(true) {
                        #[cfg(test)]
                        test_probe::clear();
                        if let Some(callback) = self.on_select.clone() {
                            callback.invoke_and_report_with(
                                "List.on_select",
                                &[ComponentCallbackArgument::Array(Vec::new())],
                                window,
                                cx,
                            );
                        }
                    }
                }
            }
        });
        let mut list = List::new(&state);
        list.style().refine(&self.style);
        list.into_any_element()
    }
}

struct Materializer;

impl ComponentMaterializer for Materializer {
    fn materialize(&self, mut request: MaterializeRequest<'_>) -> anyhow::Result<gpui::AnyElement> {
        let payload = request
            .payload()
            .downcast_ref::<Payload>()
            .ok_or_else(|| anyhow::anyhow!("List incompatible payload"))?
            .clone();
        let rows = request.resolve_data_callback(&payload.rows)?;
        let render_row = request.resolve_element_callback(&payload.render_row)?;
        let mut on_select = None;
        let mut on_activate = None;
        let mut selected = None;
        for op in request
            .methods()
            .filter_map(|method| method.payload().downcast_ref::<ListOp>())
        {
            match op {
                ListOp::OnSelect(argument) => on_select = Some(argument.clone()),
                ListOp::OnActivate(argument) => on_activate = Some(argument.clone()),
                ListOp::Selected(id) => selected = Some(id.clone()),
            }
        }
        let on_select = on_select
            .map(|argument| request.resolve_callback(&argument))
            .transpose()?;
        let on_activate = on_activate
            .map(|argument| request.resolve_callback(&argument))
            .transpose()?;
        anyhow::ensure!(
            request.take_typed_children()?.is_empty(),
            "List does not accept children; rows come from its immutable delegate snapshot"
        );
        Ok(BoundList {
            id: payload.id,
            rows,
            render_row,
            on_select,
            on_activate,
            selected,
            style: request.take_style(),
        }
        .into_any_element())
    }
}

pub(super) fn register(registry: &mut ComponentRegistry) -> Result<(), RegistryError> {
    registry.register(ComponentDescriptor::new("List", Arc::new(Materializer))
.with_constructors(vec![ConstructorDescriptor::new(
            "List",
            vec![
                ArgumentDescriptor::new("id", ArgumentSchema::String),
                ArgumentDescriptor::new(
                    "rows",
                    ArgumentSchema::Callback("() => readonly unknown[]"),
                ),
                ArgumentDescriptor::new(
                    "render_row",
                    ArgumentSchema::Callback("(row: unknown) => Element | null"),
                ),
            ],
            |args| match args {
                [ComponentArgument::String(id), rows @ ComponentArgument::Callback(_), render_row @ ComponentArgument::Callback(_)]
                    if !id.trim().is_empty() => Ok(ComponentPayload::new(Payload {
                        id: id.clone(),
                        rows: rows.clone(),
                        render_row: render_row.clone(),
                    })),
                _ => Err("List expects a non-empty id, rows callback, and row renderer".into()),
            },
        )])
.with_methods(vec![
            MethodDescriptor::new("on_select", vec![ArgumentDescriptor::new("on_select", ArgumentSchema::Callback("(id: string, cx: Context) => void"))], |args| match args { [argument @ ComponentArgument::Callback(_)] => Ok(ComponentPayload::new(ListOp::OnSelect(argument.clone()))), _ => Err("List.on_select expects one callback".into()) }).with_documentation("Reports the stable row id after Select, or an empty array after Cancel."),
            MethodDescriptor::new("on_activate", vec![ArgumentDescriptor::new("on_activate", ArgumentSchema::Callback("(id: string, cx: Context) => void"))], |args| match args { [argument @ ComponentArgument::Callback(_)] => Ok(ComponentPayload::new(ListOp::OnActivate(argument.clone()))), _ => Err("List.on_activate expects one callback".into()) }).with_documentation("Reports the stable row id after Confirm."),
            MethodDescriptor::new("selected", vec![ArgumentDescriptor::new("id", ArgumentSchema::String)], |args| match args { [ComponentArgument::String(id)] if !id.is_empty() => Ok(ComponentPayload::new(ListOp::Selected(id.clone()))), _ => Err("List.selected expects a non-empty row id".into()) }).with_documentation("Controls the selected row by stable id."),
        ])
.with_documentation(
            "Native retained List backed by an immutable rows snapshot. Each row is lazily rendered; object rows must provide a unique string `id`. `item-{index}` is only a ListItem element id when a row cannot be read.",
        ))?;
    Ok(())
}
