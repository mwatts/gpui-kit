use super::{SearchableListDelegate, SearchableListItem, SearchableListState};
use crate::{
    input::InputEvent,
    list::{List, ListEvent},
};
use gpui::{Context, EventEmitter, IntoElement, Render, Subscription, Window};

/// Events from the inline search field and native keyboard/pointer list.
pub enum InlineSearchableListEvent<V> {
    Query(String),
    Select(V),
    Confirm(V),
}

/// Inline facade over the same query input and adapter used by searchable popovers.
pub struct InlineSearchableList<D: SearchableListDelegate> {
    state: SearchableListState<D>,
    subscriptions: Vec<Subscription>,
}
impl<D: SearchableListDelegate>
    EventEmitter<InlineSearchableListEvent<<D::Item as SearchableListItem>::Value>>
    for InlineSearchableList<D>
{
}
impl<D: SearchableListDelegate> InlineSearchableList<D> {
    pub fn new(delegate: D, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = SearchableListState::new(
            delegate,
            vec![],
            |_, _, _, _| {},
            |_, _, _| {},
            |_, _| gpui::div().into_any_element(),
            |_, _, _| {},
            window,
            cx,
        );
        state
            .list
            .update(cx, |list, cx| list.set_searchable(true, cx));
        let query = state.list.read(cx).query_input.clone();
        let subscriptions = vec![
            cx.subscribe_in(&query, window, |_, input, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.emit(InlineSearchableListEvent::Query(
                        input.read(cx).value().to_string(),
                    ));
                }
            }),
            cx.subscribe_in(&state.list, window, |this, list, event, _, cx| {
                let (path, confirm) = match event {
                    ListEvent::Select(path) => (*path, false),
                    ListEvent::Confirm(path) => (*path, true),
                    _ => return,
                };
                let Some(item) = list.read(cx).delegate().delegate.item(path).cloned() else {
                    return;
                };
                this.state.selection = vec![(path, item.clone())];
                this.state.sync_snapshot(cx);
                cx.emit(if confirm {
                    InlineSearchableListEvent::Confirm(item.value().clone())
                } else {
                    InlineSearchableListEvent::Select(item.value().clone())
                });
                cx.notify();
            }),
        ];
        Self {
            state,
            subscriptions,
        }
    }
    pub fn query(&self, cx: &gpui::App) -> String {
        self.state
            .list
            .read(cx)
            .query_input
            .read(cx)
            .value()
            .to_string()
    }
    pub fn update_delegate(&mut self, update: impl FnOnce(&mut D), cx: &mut Context<Self>) {
        self.state
            .list
            .update(cx, |list, _| update(&mut list.delegate_mut().delegate));
    }
    pub fn set_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.query(cx) != query {
            self.state
                .list
                .update(cx, |list, cx| list.set_query(query, window, cx));
        }
    }
    pub fn set_selected_value(
        &mut self,
        value: Option<&<D::Item as SearchableListItem>::Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path =
            value.and_then(|value| self.state.list.read(cx).delegate().delegate.position(value));
        self.state.set_selected_indices(path, cx);
        self.state.sync_snapshot(cx);
        self.state.list.update(cx, |list, cx| {
            if list.selected_index() != path {
                list.set_selected_index(path, window, cx);
            }
        });
    }
}
impl<D: SearchableListDelegate> Render for InlineSearchableList<D> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _ = &self.subscriptions;
        List::new(&self.state.list)
    }
}
