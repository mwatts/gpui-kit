//! `new NavStack(id, () => pages, page => Element | null).path(ids)`.
//! Pages are bounded immutable data objects with unique nonempty string `id`s.
//! `path` is a root-first array of unique page ids (default: first page).
//! Shrinking pops; extending reuses native forward history when possible.
//! `.on_change((path: string[], current: string, cx) => void)` reports changed
//! paths only (including initial mount); empty current means an empty stack.
//! Reflecting that path back does not emit again. Domain routing is app-owned.
use gpui_base::{NavMotion, NavStack, NavStackState, motion::Transition};
use gpui_shell::{
    ArgumentDescriptor as Arg, ArgumentSchema as Schema, ComponentArgument as Argument,
    ComponentCallback, ComponentCallbackArgument as Value, ComponentDataCallback,
    ComponentDataValue, ComponentDescriptor, ComponentElementCallback, ComponentMaterializer,
    ComponentPayload, ComponentRegistry, ConstructorDescriptor, MaterializeRequest,
    MethodDescriptor, RegistryError, anyhow,
    gpui::{
        self, App, AppContext as _, Context, Entity, IntoElement as _, ParentElement as _,
        Refineable as _, Render, RenderOnce, Styled as _, Window,
    },
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

#[derive(Clone)]
struct Payload {
    id: String,
    pages: Argument,
    render: Argument,
}
#[derive(Clone)]
enum Op {
    Path(Vec<String>),
    Change(Argument),
}
struct Page {
    data: ComponentDataValue,
    render: ComponentElementCallback,
}
impl Render for Page {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        #[cfg(test)]
        test_probe::page(self.data.clone());
        match self
            .render
            .build_data_with(&[self.data.clone()], window, cx)
        {
            Ok(Some(element)) => element,
            Ok(None) => gpui::div().into_any_element(),
            Err(error) => gpui::div()
                .child(format!("NavStack page: {error:#}"))
                .into_any_element(),
        }
    }
}
struct Host {
    native: Entity<NavStackState>,
    pages: HashMap<String, Entity<Page>>,
    path: Vec<String>,
    initialized: bool,
}
#[derive(gpui::IntoElement)]
struct Bound {
    id: String,
    pages: ComponentDataCallback,
    render: ComponentElementCallback,
    path: Option<Vec<String>>,
    change: Option<ComponentCallback>,
    style: gpui::StyleRefinement,
}

fn synchronize<T: Render + 'static>(
    native: &Entity<NavStackState>,
    previous: &[String],
    path: &[String],
    pages: &HashMap<String, Entity<T>>,
    cx: &mut App,
) {
    let common = previous
        .iter()
        .zip(path)
        .take_while(|(a, b)| a == b)
        .count();
    native.update(cx, |state, cx| {
        if common == 0 {
            state.clear(cx);
        } else {
            while state.depth() > common {
                state.pop(NavMotion::Animated, cx);
            }
        }
        for id in &path[common..] {
            let page = &pages[id];
            let is_forward = state
                .forward_views()
                .next()
                .is_some_and(|next| next.entity_id() == page.entity_id());
            if is_forward {
                state.forward(NavMotion::Animated, cx);
            } else {
                state.push(page.clone(), NavMotion::Animated, cx);
            }
        }
    });
}
impl Bound {
    fn build(self, window: &mut Window, cx: &mut App) -> anyhow::Result<gpui::AnyElement> {
        let snapshot = self.pages.snapshot_rows_with(&[], window, cx)?;
        let mut rows = Vec::new();
        let mut ids = HashSet::new();
        for index in 0..snapshot.len() {
            let row = snapshot.row(index)?.clone();
            let id = match &row {
                ComponentDataValue::Object(fields) => {
                    fields.iter().find_map(|(key, value)| match value {
                        ComponentDataValue::String(id) if key == "id" && !id.trim().is_empty() => {
                            Some(id.clone())
                        }
                        _ => None,
                    })
                }
                _ => None,
            }
            .ok_or_else(|| anyhow::anyhow!("NavStack page {index} requires a string id"))?;
            anyhow::ensure!(ids.insert(id.clone()), "NavStack duplicate page id `{id}`");
            rows.push((id, row));
        }
        let path = self.path.unwrap_or_else(|| {
            rows.first()
                .map(|(id, _)| vec![id.clone()])
                .unwrap_or_default()
        });
        let mut seen = HashSet::new();
        for id in &path {
            anyhow::ensure!(
                ids.contains(id) && seen.insert(id),
                "NavStack path requires unique known page ids: `{id}`"
            );
        }
        let host: Entity<Host> =
            window.use_keyed_state(format!("shell-nav-stack:{}", self.id), cx, |_, cx| Host {
                native: cx.new(|_| NavStackState::new()),
                pages: HashMap::new(),
                path: Vec::new(),
                initialized: false,
            });
        let (native, changed) = host.update(cx, |host, cx| {
            for (id, data) in rows {
                if let Some(page) = host.pages.get(&id) {
                    page.update(cx, |page, _| {
                        page.data = data;
                        page.render = self.render.clone();
                    });
                } else {
                    host.pages.insert(
                        id,
                        cx.new(|_| Page {
                            data,
                            render: self.render.clone(),
                        }),
                    );
                }
            }
            let changed = !host.initialized || host.path != path;
            if changed {
                synchronize(&host.native, &host.path, &path, &host.pages, cx);
            }
            host.path = path.clone();
            host.initialized = true;
            host.pages.retain(|id, _| ids.contains(id));
            (host.native.clone(), changed)
        });
        #[cfg(test)]
        host.read(cx)
            .pages
            .iter()
            .for_each(|(id, page)| test_probe::identity(id.clone(), page.entity_id()));
        #[cfg(test)]
        test_probe::path(
            path.clone(),
            native.read(cx).current().map(|page| page.entity_id()),
        );
        if changed {
            if let Some(callback) = self.change {
                callback.invoke_and_report_with(
                    "NavStack.on_change",
                    &[
                        Value::Array(path.iter().cloned().map(Value::String).collect()),
                        Value::String(path.last().cloned().unwrap_or_default()),
                    ],
                    window,
                    cx,
                );
            }
        }
        let mut stack =
            NavStack::new(&native).transition(Transition::new(Duration::from_millis(180)));
        stack.style().refine(&self.style);
        Ok(stack.into_any_element())
    }
}
impl RenderOnce for Bound {
    fn render(self, window: &mut Window, cx: &mut App) -> impl gpui::IntoElement {
        self.build(window, cx).unwrap_or_else(|error| {
            gpui::div()
                .child(format!("NavStack: {error:#}"))
                .into_any_element()
        })
    }
}
struct Materializer;
impl ComponentMaterializer for Materializer {
    fn materialize(&self, mut request: MaterializeRequest<'_>) -> anyhow::Result<gpui::AnyElement> {
        let p = request
            .payload()
            .downcast_ref::<Payload>()
            .ok_or_else(|| anyhow::anyhow!("NavStack incompatible payload"))?
            .clone();
        let mut path = None;
        let mut change = None;
        for op in request
            .methods()
            .filter_map(|m| m.payload().downcast_ref::<Op>())
        {
            match op {
                Op::Path(value) => path = Some(value.clone()),
                Op::Change(value) => change = Some(value.clone()),
            }
        }
        anyhow::ensure!(
            request.take_typed_children()?.is_empty(),
            "NavStack uses page callbacks, not children"
        );
        Ok(Bound {
            id: p.id,
            pages: request.resolve_data_callback(&p.pages)?,
            render: request.resolve_element_callback(&p.render)?,
            path,
            change: change.map(|a| request.resolve_callback(&a)).transpose()?,
            style: request.take_style(),
        }
        .into_any_element())
    }
}
pub(super) fn register(registry: &mut ComponentRegistry) -> Result<(), RegistryError> {
    registry.register(ComponentDescriptor::new("NavStack", Arc::new(Materializer)).with_constructors(vec![ConstructorDescriptor::new("NavStack", vec![
        Arg::new("id", Schema::String), Arg::new("pages", Schema::Callback("() => readonly unknown[]")),
        Arg::new("render_page", Schema::Callback("(page: unknown) => Element | null")),
    ], |args| match args {
        [Argument::String(id), pages @ Argument::Callback(_), render @ Argument::Callback(_)] if !id.trim().is_empty() =>
            Ok(ComponentPayload::new(Payload { id: id.clone(), pages: pages.clone(), render: render.clone() })),
        _ => Err("NavStack expects a stable id, pages callback, and page renderer".into()),
    })]).with_methods(vec![
        MethodDescriptor::new("path", vec![Arg::new("ids", Schema::Array(Box::new(Schema::String)))], |args| match args {
            [Argument::Array(ids)] => ids.iter().map(|id| match id { Argument::String(id) => Ok(id.clone()), _ => Err("NavStack.path expects strings".into()) }).collect::<Result<Vec<_>, String>>().map(|ids| ComponentPayload::new(Op::Path(ids))),
            _ => Err("NavStack.path expects an array".into()),
        }).with_documentation("Controls the root-first page path using stable page ids; reuses retained history when possible."),
        MethodDescriptor::new("on_change", vec![Arg::new("callback", Schema::Callback("(path: string[], current: string, cx: Context) => void"))], |args| match args {
            [a @ Argument::Callback(_)] => Ok(ComponentPayload::new(Op::Change(a.clone()))), _ => Err("NavStack.on_change expects a callback".into()),
        }).with_documentation("Reports the changed page path and current page id followed by the script context."),
    ]).with_documentation("Retained native navigation stack. Immutable pages require unique ids; path controls push/back/forward with retained page entities. on_change reports changed path/current only."))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_base::NavStackEvent;
    use std::{cell::RefCell, rc::Rc};
    struct Counter(usize);
    impl Render for Counter {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
            gpui::div()
        }
    }
    #[gpui::test]
    fn controlled_back_and_forward_keep_page_identity_and_deliver_native_events(
        cx: &mut gpui::TestAppContext,
    ) {
        let native = cx.new(|_| NavStackState::new());
        let root = cx.new(|_| Counter(0));
        let detail = cx.new(|_| Counter(42));
        let pages = HashMap::from([("root".into(), root), ("detail".into(), detail.clone())]);
        let events = Rc::new(RefCell::new(Vec::new()));
        cx.update(|cx| {
            let events = events.clone();
            cx.subscribe(&native, move |_, event: &NavStackEvent, _| {
                events.borrow_mut().push(*event)
            })
            .detach();
        });
        let first = vec!["root".into()];
        let second = vec!["root".into(), "detail".into()];
        cx.update(|cx| synchronize(&native, &[], &first, &pages, cx));
        cx.update(|cx| synchronize(&native, &first, &second, &pages, cx));
        detail.update(cx, |page, _| page.0 += 1);
        cx.update(|cx| synchronize(&native, &second, &first, &pages, cx));
        cx.update(|cx| synchronize(&native, &first, &second, &pages, cx));
        native.read_with(cx, |state, _| {
            assert_eq!(state.current().unwrap().entity_id(), detail.entity_id())
        });
        assert_eq!(detail.read_with(cx, |page, _| page.0), 43);
        assert_eq!(
            &*events.borrow(),
            &[
                NavStackEvent::Cleared,
                NavStackEvent::Pushed,
                NavStackEvent::Pushed,
                NavStackEvent::Popped,
                NavStackEvent::Forwarded
            ]
        );
    }
}

#[cfg(test)]
pub(crate) mod test_probe {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default, Clone)]
    pub(crate) struct Snapshot {
        pub identities: HashMap<String, gpui::EntityId>,
        pub path: Vec<String>,
        pub current: Option<gpui::EntityId>,
        pub rendered: Vec<ComponentDataValue>,
    }
    thread_local! { static STATE: RefCell<Snapshot> = RefCell::new(Snapshot::default()); }
    pub(super) fn identity(id: String, entity: gpui::EntityId) {
        STATE.with(|s| {
            s.borrow_mut().identities.insert(id, entity);
        });
    }
    pub(super) fn path(path: Vec<String>, current: Option<gpui::EntityId>) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.path = path;
            s.current = current;
        });
    }
    pub(super) fn page(data: ComponentDataValue) {
        STATE.with(|s| s.borrow_mut().rendered.push(data));
    }
    pub(crate) fn take() -> Snapshot {
        STATE.with(|s| std::mem::take(&mut *s.borrow_mut()))
    }
}
