//! Retained native DataTable binding with immutable row snapshots and lazy cells.

use super::support::bool_method;

use gpui_component::table::{Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState};
use gpui_shell::{
    ArgumentDescriptor, ArgumentSchema, ComponentArgument, ComponentCallback,
    ComponentCallbackArgument, ComponentDataValue, ComponentDelegateSnapshot, ComponentDescriptor,
    ComponentElementCallback, ComponentMaterializer, ComponentPayload, ComponentRegistry,
    ConstructorDescriptor, MaterializeRequest, MethodDescriptor, RegistryError, StateDescriptor,
    anyhow,
    gpui::{
        self, AppContext as _, Entity, IntoElement as _, ParentElement as _, Refineable as _,
        RenderOnce, StyleRefinement, Styled as _, Subscription, Window,
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    sync::Arc,
};

struct Delegate {
    columns: Vec<Column>,
    rows: ComponentDelegateSnapshot,
    render_cell: Option<ComponentElementCallback>,
    ids: Vec<String>,
    on_sort: Option<ComponentCallback>,
}

impl Delegate {
    fn new(keys: Vec<String>) -> Self {
        Self {
            columns: keys
                .iter()
                .map(|key| Column::new(key.clone(), key.clone()))
                .collect(),
            rows: ComponentDelegateSnapshot::new(Vec::new()),
            render_cell: None,
            ids: Vec::new(),
            on_sort: None,
        }
    }

    fn apply_sort_config(
        &mut self,
        sortable: bool,
        sortable_keys: Option<&[String]>,
        sorted: Option<&(String, ColumnSort)>,
    ) {
        for column in &mut self.columns {
            let named = match sortable_keys {
                Some(keys) => keys.iter().any(|key| key == column.key.as_ref()),
                None => sortable,
            };
            if let Some((key, direction)) = sorted {
                if column.key.as_ref() == key {
                    column.sort = Some(*direction);
                    continue;
                }
            }
            if named {
                column.sort = Some(ColumnSort::Default);
            }
        }
    }
}

impl TableDelegate for Delegate {
    fn columns_count(&self, _: &gpui::App) -> usize {
        self.columns.len()
    }
    fn rows_count(&self, _: &gpui::App) -> usize {
        self.rows.len()
    }
    fn column(&self, col_ix: usize, _: &gpui::App) -> Column {
        self.columns[col_ix].clone()
    }
    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<TableState<Self>>,
    ) -> impl gpui::IntoElement {
        let result = (|| {
            let callback = self
                .render_cell
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("DataTable cell renderer is unavailable"))?;
            let row = self.rows.row(row_ix)?.clone();
            let key = self
                .columns
                .get(col_ix)
                .ok_or_else(|| anyhow::anyhow!("DataTable column index {col_ix} is out of bounds"))?
                .key
                .to_string();
            callback.build_data_with(&[row, ComponentDataValue::String(key)], window, cx)
        })();
        match result {
            Ok(Some(element)) => {
                #[cfg(test)]
                {
                    test_probe::built();
                    test_probe::cell(row_ix, col_ix, self.cell_text(row_ix, col_ix, cx));
                }
                element
            }
            Ok(None) => gpui::div().into_any_element(),
            Err(error) => {
                #[cfg(test)]
                test_probe::error(error.to_string());
                gpui::div()
                    .child(format!("Failed to render DataTable cell: {error:#}"))
                    .into_any_element()
            }
        }
    }
    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &gpui::App) -> String {
        let Some(column) = self.columns.get(col_ix) else {
            return String::new();
        };
        let Ok(ComponentDataValue::Object(fields)) = self.rows.row(row_ix) else {
            return String::new();
        };
        fields
            .iter()
            .find_map(|(key, value)| {
                (key == column.key.as_ref()).then(|| match value {
                    ComponentDataValue::String(value) => value.clone(),
                    ComponentDataValue::Number(value) => value.to_string(),
                    ComponentDataValue::Boolean(value) => value.to_string(),
                    _ => String::new(),
                })
            })
            .unwrap_or_default()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        window: &mut Window,
        cx: &mut gpui::Context<TableState<Self>>,
    ) {
        let Some(column) = self.columns.get(col_ix) else {
            return;
        };
        let key = column.key.to_string();
        let direction = match sort {
            ColumnSort::Ascending => "ascending",
            ColumnSort::Descending => "descending",
            ColumnSort::Default => "default",
        };
        #[cfg(test)]
        test_probe::sort(key.clone(), direction.to_string());
        let Some(callback) = self.on_sort.clone() else {
            return;
        };
        callback.invoke_and_report_with(
            "DataTable.on_sort",
            &[
                ComponentCallbackArgument::String(key),
                ComponentCallbackArgument::String(direction.to_string()),
            ],
            window,
            cx,
        );
    }
}

#[derive(Clone)]
struct Payload {
    state: ComponentArgument,
    rows: ComponentArgument,
    cell: ComponentArgument,
}
#[derive(Clone)]
enum Op {
    Stripe(bool),
    Bordered(bool),
    Scrollbars(bool, bool),
    RowSelectable(bool),
    ColSelectable(bool),
    CellSelectable(bool),
    RowHeader(bool),
    Sortable(bool),
    ColResizable(bool),
    ColMovable(bool),
    OnSort(ComponentArgument),
    OnSelect(ComponentArgument),
    OnActivate(ComponentArgument),
    Selected(String),
    Sorted(String, String),
    SortableColumns(Vec<String>),
}

struct Materializer;
impl ComponentMaterializer for Materializer {
    fn materialize(&self, mut request: MaterializeRequest<'_>) -> anyhow::Result<gpui::AnyElement> {
        let payload = request
            .payload()
            .downcast_ref::<Payload>()
            .ok_or_else(|| anyhow::anyhow!("DataTable received an incompatible payload"))?
            .clone();
        anyhow::ensure!(
            request.children_len() == 0,
            "DataTable does not accept children"
        );
        let ops = request
            .methods()
            .filter_map(|m| m.payload().downcast_ref::<Op>().cloned())
            .collect::<Vec<_>>();
        let sortable = ops.iter().any(|op| matches!(op, Op::Sortable(true)));
        let sortable_columns = ops.iter().find_map(|op| match op {
            Op::SortableColumns(keys) => Some(keys.clone()),
            _ => None,
        });
        let on_sort = ops.iter().find_map(|op| match op {
            Op::OnSort(argument) => Some(argument.clone()),
            _ => None,
        });
        if (sortable || sortable_columns.is_some()) && on_sort.is_none() {
            anyhow::bail!("DataTable.sortable requires an on_sort callback");
        }
        let on_select = ops.iter().find_map(|op| match op {
            Op::OnSelect(argument) => Some(argument.clone()),
            _ => None,
        });
        let on_activate = ops.iter().find_map(|op| match op {
            Op::OnActivate(argument) => Some(argument.clone()),
            _ => None,
        });
        let selected = ops.iter().find_map(|op| match op {
            Op::Selected(id) => Some(id.clone()),
            _ => None,
        });
        let mut sorted = None;
        for op in &ops {
            if let Op::Sorted(key, direction) = op {
                sorted = Some((key.clone(), parse_sort(direction)?));
            }
        }
        let state =
            request.with_state::<Entity<TableState<Delegate>>, _>(&payload.state, Clone::clone)?;
        let rows = request.resolve_data_callback(&payload.rows)?;
        let cell = request.resolve_element_callback(&payload.cell)?;
        let on_sort = on_sort
            .map(|argument| request.resolve_callback(&argument))
            .transpose()?;
        let on_select = on_select
            .map(|argument| request.resolve_callback(&argument))
            .transpose()?;
        let on_activate = on_activate
            .map(|argument| request.resolve_callback(&argument))
            .transpose()?;
        let style = request.take_style();
        Ok(DataTableHost {
            state,
            rows,
            cell,
            ops,
            on_sort,
            on_select,
            on_activate,
            selected,
            sorted,
            sortable_columns,
            style,
        }
        .into_any_element())
    }
}

fn parse_sort(direction: &str) -> anyhow::Result<ColumnSort> {
    match direction {
        "ascending" | "asc" => Ok(ColumnSort::Ascending),
        "descending" | "desc" => Ok(ColumnSort::Descending),
        "default" => Ok(ColumnSort::Default),
        _ => anyhow::bail!("DataTable.sorted direction must be ascending, descending, or default"),
    }
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
            anyhow::bail!(
                "DataTable row {index} requires a unique string `id` when selection is used"
            );
        };
        if !seen.insert(id.to_string()) {
            anyhow::bail!("DataTable duplicate row id `{id}`");
        }
        ids.push(id.to_string());
    }
    Ok(ids)
}

struct TableCallbacks {
    on_select: Option<ComponentCallback>,
    on_activate: Option<ComponentCallback>,
}

struct TableHost {
    #[allow(dead_code)]
    state: Entity<TableState<Delegate>>,
    callback: Rc<RefCell<TableCallbacks>>,
    ids: Rc<RefCell<Vec<String>>>,
    last_id: Rc<RefCell<Option<String>>>,
    suppress: Rc<Cell<bool>>,
    cleared: Rc<Cell<bool>>,
    _subscription: Subscription,
}

#[derive(gpui::IntoElement)]
struct DataTableHost {
    state: Entity<TableState<Delegate>>,
    rows: gpui_shell::ComponentDataCallback,
    cell: ComponentElementCallback,
    ops: Vec<Op>,
    on_sort: Option<ComponentCallback>,
    on_select: Option<ComponentCallback>,
    on_activate: Option<ComponentCallback>,
    selected: Option<String>,
    sorted: Option<(String, ColumnSort)>,
    sortable_columns: Option<Vec<String>>,
    style: StyleRefinement,
}

impl RenderOnce for DataTableHost {
    fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl gpui::IntoElement {
        let snapshot = match self.rows.snapshot_rows_with(&[], window, cx) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let message =
                    format!("DataTable rows callback must return an array of rows: {error:#}");
                #[cfg(test)]
                test_probe::error(message.clone());
                return gpui::div().child(message).into_any_element();
            }
        };
        let selection_used =
            self.on_select.is_some() || self.on_activate.is_some() || self.selected.is_some();
        let ids = if selection_used {
            match unique_ids(&snapshot) {
                Ok(ids) => ids,
                Err(error) => {
                    #[cfg(test)]
                    test_probe::error(error.to_string());
                    return gpui::div().child(error.to_string()).into_any_element();
                }
            }
        } else {
            Vec::new()
        };
        let sortable = self.ops.iter().any(|op| matches!(op, Op::Sortable(true)))
            || self.sortable_columns.is_some()
            || self.sorted.is_some();
        let event_callbacks = TableCallbacks {
            on_select: self.on_select.clone(),
            on_activate: self.on_activate.clone(),
        };
        let host: Entity<TableHost> = window.use_keyed_state(
            format!("shell-table-host:{}", self.state.entity_id()),
            cx,
            {
                let state = self.state.clone();
                move |window, cx| {
                    let callback = Rc::new(RefCell::new(event_callbacks));
                    let ids = Rc::new(RefCell::new(Vec::<String>::new()));
                    let last_id = Rc::new(RefCell::new(None::<String>));
                    let suppress = Rc::new(Cell::new(false));
                    let cleared = Rc::new(Cell::new(false));
                    let event_callback = callback.clone();
                    let event_ids = ids.clone();
                    let event_last = last_id.clone();
                    let event_suppress = suppress.clone();
                    let subscription =
                        window.subscribe(&state, cx, move |_, event: &TableEvent, window, cx| {
                            if event_suppress.get() {
                                return;
                            }
                            match event {
                                TableEvent::SelectRow(index) => {
                                    let ids = event_ids.borrow();
                                    let Some(id) = ids.get(*index).cloned() else {
                                        return;
                                    };
                                    drop(ids);
                                    *event_last.borrow_mut() = Some(id.clone());
                                    event_suppress.set(false);
                                    #[cfg(test)]
                                    test_probe::select(id.clone());
                                    if let Some(callback) =
                                        event_callback.borrow().on_select.clone()
                                    {
                                        callback.invoke_and_report_with(
                                            "DataTable.on_select",
                                            &[ComponentCallbackArgument::String(id)],
                                            window,
                                            cx,
                                        );
                                    }
                                }
                                TableEvent::DoubleClickedRow(index) => {
                                    let ids = event_ids.borrow();
                                    let Some(id) = ids.get(*index).cloned() else {
                                        return;
                                    };
                                    drop(ids);
                                    *event_last.borrow_mut() = Some(id.clone());
                                    #[cfg(test)]
                                    test_probe::select(id.clone());
                                    if let Some(callback) =
                                        event_callback.borrow().on_activate.clone()
                                    {
                                        callback.invoke_and_report_with(
                                            "DataTable.on_activate",
                                            &[ComponentCallbackArgument::String(id)],
                                            window,
                                            cx,
                                        );
                                    }
                                }
                                _ => {}
                            }
                        });
                    TableHost {
                        state,
                        callback,
                        ids,
                        last_id,
                        suppress,
                        cleared,
                        _subscription: subscription,
                    }
                }
            },
        );
        let (ids_cell, last_id, suppress, cleared, callback) = {
            let host = host.read(cx);
            (
                host.ids.clone(),
                host.last_id.clone(),
                host.suppress.clone(),
                host.cleared.clone(),
                host.callback.clone(),
            )
        };
        *callback.borrow_mut() = TableCallbacks {
            on_select: self.on_select,
            on_activate: self.on_activate,
        };
        *ids_cell.borrow_mut() = ids.clone();
        let target = self.selected.clone().or_else(|| last_id.borrow().clone());
        self.state.update(cx, |state, cx| {
            state.delegate_mut().rows = snapshot;
            state.delegate_mut().render_cell = Some(self.cell);
            state.delegate_mut().ids = ids.clone();
            state.delegate_mut().on_sort = self.on_sort;
            state.delegate_mut().apply_sort_config(
                sortable,
                self.sortable_columns.as_deref(),
                self.sorted.as_ref(),
            );
            for op in &self.ops {
                match op {
                    Op::RowSelectable(value) => state.row_selectable = *value,
                    Op::ColSelectable(value) => state.col_selectable = *value,
                    Op::CellSelectable(value) => state.cell_selectable = *value,
                    Op::RowHeader(value) => state.row_header = *value,
                    Op::Sortable(value) => state.sortable = *value,
                    Op::ColResizable(value) => state.col_resizable = *value,
                    Op::ColMovable(value) => state.col_movable = *value,
                    _ => {}
                }
            }
            if sortable {
                state.sortable = true;
            }
            state.refresh(cx);
            if let Some(id) = target {
                match ids.iter().position(|row| row == &id) {
                    Some(index) => {
                        cleared.set(false);
                        if state.selected_row() != Some(index) {
                            suppress.set(true);
                            state.set_selected_row(index, cx);
                            suppress.set(false);
                        }
                        *last_id.borrow_mut() = Some(id);
                    }
                    None => {
                        suppress.set(true);
                        state.clear_selection(cx);
                        suppress.set(false);
                        *last_id.borrow_mut() = None;
                        if !cleared.replace(true) {
                            #[cfg(test)]
                            test_probe::clear();
                            if let Some(callback) = callback.borrow().on_select.clone() {
                                callback.invoke_and_report_with(
                                    "DataTable.on_select",
                                    &[ComponentCallbackArgument::Array(Vec::new())],
                                    window,
                                    cx,
                                );
                            }
                        }
                    }
                }
            }
        });
        let mut table = DataTable::new(&self.state);
        for op in &self.ops {
            table = match op {
                Op::Stripe(value) => table.stripe(*value),
                Op::Bordered(value) => table.bordered(*value),
                Op::Scrollbars(value, h) => table.scrollbar_visible(*value, *h),
                _ => table,
            };
        }
        let mut host = gpui::div().size_full().child(table);
        host.style().refine(&self.style);
        host.into_any_element()
    }
}

pub(super) fn register(registry: &mut ComponentRegistry) -> Result<(), RegistryError> {
    registry.register_state(
        StateDescriptor::new(
            "DataTableState",
            "DataTableState",
            vec![ArgumentDescriptor::new(
                "columns",
                ArgumentSchema::Array(Box::new(ArgumentSchema::String)),
            )],
            |args, window, cx| match args {
                [ComponentArgument::Array(columns)] => {
                    let keys = columns
                        .iter()
                        .map(|column| match column {
                            ComponentArgument::String(key) if !key.trim().is_empty() => {
                                Ok(key.clone())
                            }
                            _ => Err("DataTableState columns must be non-empty strings".into()),
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    if keys.is_empty() {
                        return Err("DataTableState requires at least one column".into());
                    }
                    let mut unique = std::collections::HashSet::new();
                    if !keys.iter().all(|key| unique.insert(key.clone())) {
                        return Err("DataTableState column keys must be unique".into());
                    }
                    Ok(Box::new(cx.new(|cx| {
                        TableState::new(Delegate::new(keys), window, cx)
                    })))
                }
                _ => Err("DataTableState expects a string array".into()),
            },
        )
        .with_documentation(
            "Retained native DataTable focus, selection, scrolling, measurement and column state.",
        ),
    )?;
    registry.register(ComponentDescriptor::new("DataTable", Arc::new(Materializer))
.with_constructors(vec![ConstructorDescriptor::new("DataTable", vec![
            ArgumentDescriptor::new("state", ArgumentSchema::Entity("DataTableState")),
            ArgumentDescriptor::new("rows", ArgumentSchema::Callback("(cx: Context) => readonly unknown[]")),
            ArgumentDescriptor::new("render_cell", ArgumentSchema::Callback("(row: unknown, column: string, cx: Context) => Element")),
        ], |args| match args { [state @ ComponentArgument::Entity { .. }, rows @ ComponentArgument::Callback(_), cell @ ComponentArgument::Callback(_)] => Ok(ComponentPayload::new(Payload { state: state.clone(), rows: rows.clone(), cell: cell.clone() })), _ => Err("DataTable expects DataTableState, rows callback and cell renderer".into()) })])
.with_methods(vec![
            bool_method("DataTable", "stripe", "Sets native DataTable behavior.", Op::Stripe), bool_method("DataTable", "bordered", "Sets native DataTable behavior.", Op::Bordered),
            MethodDescriptor::new("scrollbar_visible", vec![ArgumentDescriptor::new("vertical", ArgumentSchema::Boolean), ArgumentDescriptor::new("horizontal", ArgumentSchema::Boolean)], |args| match args { [ComponentArgument::Boolean(value), ComponentArgument::Boolean(h)] => Ok(ComponentPayload::new(Op::Scrollbars(*value, *h))), _ => Err("DataTable.scrollbar_visible expects two booleans".into()) }).with_documentation("Chooses when the table shows its scrollbars."),
            bool_method("DataTable", "row_selectable", "Sets native DataTable behavior.", Op::RowSelectable), bool_method("DataTable", "column_selectable", "Sets native DataTable behavior.", Op::ColSelectable), bool_method("DataTable", "cell_selectable", "Sets native DataTable behavior.", Op::CellSelectable), bool_method("DataTable", "row_header", "Sets native DataTable behavior.", Op::RowHeader), bool_method("DataTable", "sortable", "Sets native DataTable behavior.", Op::Sortable), bool_method("DataTable", "column_resizable", "Sets native DataTable behavior.", Op::ColResizable), bool_method("DataTable", "column_movable", "Sets native DataTable behavior.", Op::ColMovable),
            MethodDescriptor::new("on_sort", vec![ArgumentDescriptor::new("on_sort", ArgumentSchema::Callback("(key: string, direction: string, cx: Context) => void"))], |args| match args { [argument @ ComponentArgument::Callback(_)] => Ok(ComponentPayload::new(Op::OnSort(argument.clone()))), _ => Err("DataTable.on_sort expects one callback".into()) }).with_documentation("Reports a sort request; the delegate does not reorder rows."),
            MethodDescriptor::new("sortable_columns", vec![ArgumentDescriptor::new("keys", ArgumentSchema::Array(Box::new(ArgumentSchema::String)))], |args| match args { [ComponentArgument::Array(keys)] => {
                let keys = keys.iter().map(|key| match key { ComponentArgument::String(key) if !key.is_empty() => Ok(key.clone()), _ => Err("DataTable.sortable_columns expects string keys") }).collect::<Result<Vec<_>, _>>()?;
                Ok(ComponentPayload::new(Op::SortableColumns(keys)))
            }, _ => Err("DataTable.sortable_columns expects a string array".into()) }).with_documentation("Marks the named columns as sortable. Requires on_sort."),
            MethodDescriptor::new("sorted", vec![ArgumentDescriptor::new("key", ArgumentSchema::String), ArgumentDescriptor::new("direction", ArgumentSchema::String)], |args| match args { [ComponentArgument::String(key), ComponentArgument::String(direction)] => Ok(ComponentPayload::new(Op::Sorted(key.clone(), direction.clone()))), _ => Err("DataTable.sorted expects column key and direction strings".into()) }).with_documentation("Sets the controlled sort indicator to match the script snapshot."),
            MethodDescriptor::new("on_select", vec![ArgumentDescriptor::new("on_select", ArgumentSchema::Callback("(id: string, cx: Context) => void"))], |args| match args { [argument @ ComponentArgument::Callback(_)] => Ok(ComponentPayload::new(Op::OnSelect(argument.clone()))), _ => Err("DataTable.on_select expects one callback".into()) }).with_documentation("Reports the stable row id after a row is selected."),
            MethodDescriptor::new("on_activate", vec![ArgumentDescriptor::new("on_activate", ArgumentSchema::Callback("(id: string, cx: Context) => void"))], |args| match args { [argument @ ComponentArgument::Callback(_)] => Ok(ComponentPayload::new(Op::OnActivate(argument.clone()))), _ => Err("DataTable.on_activate expects one callback".into()) }).with_documentation("Reports the stable row id after a row is activated."),
            MethodDescriptor::new("selected", vec![ArgumentDescriptor::new("id", ArgumentSchema::String)], |args| match args { [ComponentArgument::String(id)] if !id.is_empty() => Ok(ComponentPayload::new(Op::Selected(id.clone()))), _ => Err("DataTable.selected expects a non-empty row id".into()) }).with_documentation("Controls the selected row by stable id."),
        ])
.with_documentation("A real retained native DataTable. Rows are captured as an immutable plain-data snapshot and visible cells are built lazily from (row, column). Style applies to the full-size table host."))?;
    Ok(())
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod test_probe {
    use std::cell::{Cell, RefCell};
    thread_local! {
        static BUILDS: Cell<usize> = const { Cell::new(0) };
        static ERRORS: Cell<usize> = const { Cell::new(0) };
        static CELLS: RefCell<Vec<(usize, usize, String)>> = const { RefCell::new(Vec::new()) };
        static SORTS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
        static SELECTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static CLEARS: Cell<usize> = const { Cell::new(0) };
    }
    pub(super) fn built() {
        BUILDS.with(|value| value.set(value.get() + 1));
    }
    pub(super) fn error(_: String) {
        ERRORS.with(|value| value.set(value.get() + 1));
    }
    pub(super) fn cell(row: usize, column: usize, value: String) {
        CELLS.with(|values| values.borrow_mut().push((row, column, value)));
    }
    pub(super) fn sort(key: String, direction: String) {
        SORTS.with(|values| values.borrow_mut().push((key, direction)));
    }
    pub(super) fn select(id: String) {
        SELECTS.with(|values| values.borrow_mut().push(id));
    }
    pub(super) fn clear() {
        CLEARS.with(|value| value.set(value.get() + 1));
    }
    pub(crate) fn reset() {
        BUILDS.with(|value| value.set(0));
        ERRORS.with(|value| value.set(0));
        CELLS.with(|values| values.borrow_mut().clear());
        SORTS.with(|values| values.borrow_mut().clear());
        SELECTS.with(|values| values.borrow_mut().clear());
        CLEARS.with(|value| value.set(0));
    }
    pub(crate) fn cell_builds() -> usize {
        BUILDS.with(Cell::get)
    }
    pub(crate) fn errors() -> usize {
        ERRORS.with(Cell::get)
    }
    pub(crate) fn take_cells() -> Vec<(usize, usize, String)> {
        CELLS.with(|values| std::mem::take(&mut *values.borrow_mut()))
    }
    pub(crate) fn take_sorts() -> Vec<(String, String)> {
        SORTS.with(|values| std::mem::take(&mut *values.borrow_mut()))
    }
    pub(crate) fn take_selects() -> Vec<String> {
        SELECTS.with(|values| std::mem::take(&mut *values.borrow_mut()))
    }
    pub(crate) fn take_clears() -> usize {
        CLEARS.with(|value| {
            let count = value.get();
            value.set(0);
            count
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_is_retained_data_table_only() {
        let mut registry = ComponentRegistry::new(
            gpui_shell::COMPONENT_REGISTRY_API_VERSION,
            gpui_shell::DEFAULT_COMPONENT_MODULE,
        )
        .unwrap();
        register(&mut registry).unwrap();
        assert_eq!(
            registry
                .freeze()
                .unwrap()
                .descriptors()
                .map(|d| d.name())
                .collect::<Vec<_>>(),
            ["DataTable"]
        );
    }
}
