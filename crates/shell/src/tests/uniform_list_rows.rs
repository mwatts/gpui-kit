//! A `uniform_list` row may hold a `Button`.
//!
//! The list runs its row renderer from inside layout, over a window of a
//! collection that can be hundreds of thousands of rows long. A handler a row
//! registers there must work, and must go away with the frame that painted the
//! row, because nothing outside the visible range may be kept alive for it.
//! These live here rather than in `tests/` because they read the runtime's
//! handler registry, which is crate-private.

use crate::{ScriptView, ShellRuntime};
use gpui::{Modifiers, TestAppContext, VisualTestContext, point, px};
use std::{ops::Deref, rc::Rc};

const ROWS: usize = 500_000;

fn source() -> String {
    format!(
        r#"
import {{ div, View, uniform_list }} from "gpui-kit";
import {{ v_flex, Button }} from "gpui-base";

export default class Rows extends View {{
  init() {{
    this.clicked = -1;
  }}

  render(cx) {{
    return v_flex()
      .w(300)
      .h(400)
      .child(
        v_flex()
          .h(200)
          .child(
            uniform_list("rows", {ROWS}, (index) => String(index), (range) => {{
              const items = [];
              for (let index = range.start; index < range.end; index++) {{
                items.push(
                  Button.new(`b${{index}}`)
                    .h(20)
                    .w_full()
                    .on_click((event, cx) => {{
                      this.clicked = index;
                      cx.notify();
                    }})
                    .child(`row ${{index}}`),
                );
              }}
              return items;
            }}),
          ),
      )
      .child(`clicked ${{this.clicked}}`);
  }}
}}
"#
    )
}

struct Root(gpui::Entity<ScriptView>);

impl gpui::Render for Root {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        self.0.clone()
    }
}

fn mount(
    cx: &mut TestAppContext,
) -> (Rc<ShellRuntime>, gpui::Entity<ScriptView>, VisualTestContext) {
    cx.update(crate::init);
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    cx.update(|cx| runtime.set_global(cx));
    let view_type = runtime.load_source("rows.js", &source()).expect("load");
    let for_view = Rc::clone(&runtime);
    let window = cx.add_window(move |window, cx| {
        Root(
            for_view
                .instantiate_view(&view_type, window, cx)
                .expect("instantiate"),
        )
    });
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    context.update(|window, cx| window.draw(cx).clear(cx));
    let view = window
        .root(&mut context)
        .expect("view")
        .read_with(&context, |root, _| root.0.clone());
    (runtime, view, context)
}

fn redraw_and_read(context: &mut VisualTestContext, view: &gpui::Entity<ScriptView>) -> String {
    context.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.invalidate();
            cx.notify();
        })
    });
    context.update(|window, cx| window.draw(cx).clear(cx));
    context.update(|_, cx| {
        view.read(cx)
            .snapshot()
            .map(crate::RenderSnapshot::debug_tree)
            .unwrap_or_default()
    })
}

fn scroll_by(context: &mut VisualTestContext, dy: f32) {
    context.simulate_event(gpui::ScrollWheelEvent {
        position: point(px(150.), px(100.)),
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(dy))),
        ..Default::default()
    });
    context.update(|window, cx| window.draw(cx).clear(cx));
}

/// The point of a button in a row is that pressing it does something. The
/// third row covers 40..60, and its handler must know it is row 2 of 500,000.
#[gpui::test]
fn a_button_in_a_row_of_a_half_million_row_list_mounts_and_its_click_reaches_script(
    cx: &mut TestAppContext,
) {
    let (_runtime, view, mut context) = mount(cx);

    context.simulate_click(point(px(150.), px(50.)), Modifiers::default());
    context.update(|window, cx| window.draw(cx).clear(cx));

    let tree = redraw_and_read(&mut context, &view);
    assert!(
        tree.contains("clicked 2"),
        "the row's own on_click must have run for row 2: {tree}"
    );
}

/// Rows are rebuilt on every frame. A handler left behind by each pass would
/// grow with scrolling for as long as the view lived; so the registry must hold
/// the visible rows' handlers and no more, however far the list has scrolled.
#[gpui::test]
fn handlers_of_rows_scrolled_out_are_retired(cx: &mut TestAppContext) {
    let (runtime, view, mut context) = mount(cx);
    let _ = redraw_and_read(&mut context, &view);

    let at_rest = runtime.live_callbacks();
    // The list holds its renderer and key resolver; about ten 20px rows fill
    // the 200px box, and each of them holds a click handler.
    assert!(
        at_rest >= 2 + 10,
        "the visible rows must hold their own handlers, found {at_rest} in all"
    );

    // Warm up so the bound is measured once the frame leases have cycled.
    let mut bound = at_rest;
    for _ in 0..10 {
        scroll_by(&mut context, -20.);
        bound = bound.max(runtime.live_callbacks());
    }

    for frame in 0..1000 {
        scroll_by(&mut context, -20.);
        let live = runtime.live_callbacks();
        assert!(
            live <= bound,
            "frame {frame}: {live} live handlers exceeds the steady bound {bound}; \
             rows scrolled out are leaking"
        );
    }

    // Come to rest: the previous frame's lease goes with the next frame.
    for _ in 0..3 {
        context.update(|window, cx| window.draw(cx).clear(cx));
    }
    assert!(
        runtime.live_callbacks() <= bound,
        "after scrolling stops the registry must not exceed the visible rows' share"
    );
}

/// A list built from a row renderer would register its own renderer in a
/// callback generation that is already closed, so it could never be called.
/// Allowing lists in NavStack pages (a phone's root list) must not open that
/// door: the nested list is still refused, and it is reported, not drawn.
#[gpui::test]
fn a_list_built_inside_a_list_item_renderer_is_still_refused(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    let failures = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    struct Sink(Rc<std::cell::RefCell<Vec<String>>>);
    impl crate::DiagnosticSink for Sink {
        fn report(&self, failure: crate::ScriptFailure) {
            self.0.borrow_mut().push(failure.message().to_owned());
        }
    }
    runtime.set_diagnostic_sink(Rc::new(Sink(failures.clone())));
    cx.update(|cx| runtime.set_global(cx));
    let source = r#"
import { View, uniform_list } from "gpui-kit";
import { v_flex } from "gpui-base";

export default class Nested extends View {
  render(cx) {
    return v_flex().w(300).h(200).child(
      uniform_list("outer", 10, (i) => String(i), (range) => {
        const items = [];
        for (let i = range.start; i < range.end; i++) {
          items.push(v_flex().h(20).child(
            uniform_list(`inner${i}`, 3, (j) => String(j), () => []),
          ));
        }
        return items;
      }),
    );
  }
}
"#;
    let view_type = runtime.load_source("nested.js", source).expect("load");
    let for_view = Rc::clone(&runtime);
    let window = cx.add_window(move |window, cx| {
        Root(
            for_view
                .instantiate_view(&view_type, window, cx)
                .expect("instantiate"),
        )
    });
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    context.update(|window, cx| window.draw(cx).clear(cx));
    context.update(|window, cx| window.draw(cx).clear(cx));
    let failures = failures.borrow();
    assert!(
        failures
            .iter()
            .any(|message| message.contains("cannot be built from inside another list")),
        "a nested list must be refused with the nested-list error: {failures:?}"
    );
}
