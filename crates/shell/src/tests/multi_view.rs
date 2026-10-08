//! Several views of one loaded application, each in its own window.
//!
//! The embedder this serves runs one object graph and shows it on several
//! screens, so what must hold is that the views share module state, that one
//! view failing to start never costs the others, and that retiring the
//! application silences all of them.

use std::{ops::Deref as _, rc::Rc};

use gpui::{
    Entity, Modifiers, ParentElement as _, Styled as _, TestAppContext, VisualTestContext, point,
    px,
};

use crate::{HostSource, LoadedApplication, ScriptView, ShellRuntime};

/// Counts clicks in module scope and in a global, and shows the module count.
/// `FAIL_AFTER` views initialise normally; later ones throw in `init`.
fn source(fail_after: u32) -> HostSource {
    let main = format!(
        r#"
import {{ View, div }} from "gpui-kit";
let clicks = 0;
let started = 0;
export default class Panel extends View {{
  init() {{
    started += 1;
    if (started > {fail_after}) throw new Error("extra init failed");
  }}
  render() {{
    return div().size_full().child("clicks=" + clicks).on_click(() => {{
      clicks += 1;
      globalThis.__clicks = (globalThis.__clicks || 0) + 1;
    }});
  }}
}}
"#
    );
    HostSource::new("main.js", [("main.js", main.as_bytes())]).expect("seal source")
}

/// A window root that shows whichever view the test attaches.
struct Host(Option<Entity<ScriptView>>);

impl gpui::Render for Host {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div().size_full().children(self.0.clone())
    }
}

struct Fixture {
    runtime: Rc<ShellRuntime>,
    loaded: LoadedApplication,
}

fn fixture(cx: &mut TestAppContext, fail_after: u32) -> Fixture {
    cx.update(crate::init);
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    cx.update(|cx| runtime.set_global(cx));
    let loaded = runtime
        .load_host_source(&source(fail_after))
        .expect("load application");
    Fixture { runtime, loaded }
}

fn new_window(cx: &mut TestAppContext) -> (Entity<Host>, VisualTestContext) {
    let window = cx.add_window(|_, _| Host(None));
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    let host = window.root(&mut context).expect("window root");
    (host, context)
}

/// Shows `view` in the window and lays it out, so clicks can land on it.
fn draw(host: &Entity<Host>, context: &mut VisualTestContext, view: &Entity<ScriptView>) {
    context.update(|window, cx| {
        host.update(cx, |host, cx| {
            host.0 = Some(view.clone());
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
}

fn tree(context: &mut VisualTestContext, view: &Entity<ScriptView>) -> String {
    context.update(|_, cx| {
        view.read(cx)
            .snapshot()
            .map(crate::RenderSnapshot::debug_tree)
            .unwrap_or_default()
    })
}

fn click(context: &mut VisualTestContext) {
    context.simulate_click(point(px(4.), px(4.)), Modifiers::default());
    context.run_until_parked();
}

fn mount_first(fx: &Fixture, context: &mut VisualTestContext) -> Entity<ScriptView> {
    context
        .update(|window, cx| fx.runtime.mount_application(&fx.loaded, window, cx))
        .expect("mount application")
}

fn mount_extra(
    fx: &Fixture,
    context: &mut VisualTestContext,
) -> anyhow::Result<Entity<ScriptView>> {
    context.update(|window, cx| fx.runtime.mount_additional_view(&fx.loaded, window, cx))
}

#[gpui::test]
fn views_share_module_state_and_a_sibling_refresh_shows_it(cx: &mut TestAppContext) {
    let fx = fixture(cx, 2);
    let ((a_host, mut a_cx), (b_host, mut b_cx)) = (new_window(cx), new_window(cx));
    let a = mount_first(&fx, &mut a_cx);
    let b = mount_extra(&fx, &mut b_cx).expect("extra view");
    draw(&a_host, &mut a_cx, &a);
    draw(&b_host, &mut b_cx, &b);
    assert!(tree(&mut b_cx, &b).contains("clicks=0"));

    click(&mut a_cx);
    assert_eq!(fx.runtime.test_global_i32("__clicks").unwrap(), 1);
    // Nothing told B: a host that wants B current must say so.
    draw(&b_host, &mut b_cx, &b);
    assert!(
        tree(&mut b_cx, &b).contains("clicks=0"),
        "B rebuilt unprompted"
    );

    b_cx.update(|_, cx| b.update(cx, |view, cx| view.refresh(cx)));
    draw(&b_host, &mut b_cx, &b);
    assert!(
        tree(&mut b_cx, &b).contains("clicks=1"),
        "B must see the counter A's handler changed: {}",
        tree(&mut b_cx, &b)
    );
}

#[gpui::test]
fn a_failed_extra_init_leaves_the_first_view_working(cx: &mut TestAppContext) {
    let fx = fixture(cx, 1);
    let ((a_host, mut a_cx), (_, mut b_cx)) = (new_window(cx), new_window(cx));
    let a = mount_first(&fx, &mut a_cx);
    draw(&a_host, &mut a_cx, &a);

    let error = mount_extra(&fx, &mut b_cx).expect_err("extra init throws");
    assert!(
        format!("{error:#}").contains("extra init failed"),
        "{error:#}"
    );
    assert!(
        fx.loaded.test_generation().is_active(),
        "an extra view's failure must not retire the application"
    );

    click(&mut a_cx);
    assert_eq!(
        fx.runtime.test_global_i32("__clicks").unwrap(),
        1,
        "the first view's callbacks must survive"
    );
    a_cx.update(|_, cx| a.update(cx, |view, cx| view.refresh(cx)));
    draw(&a_host, &mut a_cx, &a);
    assert!(tree(&mut a_cx, &a).contains("clicks=1"));
}

#[gpui::test]
fn retiring_the_generation_silences_every_view(cx: &mut TestAppContext) {
    let fx = fixture(cx, 2);
    let ((a_host, mut a_cx), (b_host, mut b_cx)) = (new_window(cx), new_window(cx));
    let a = mount_first(&fx, &mut a_cx);
    let b = mount_extra(&fx, &mut b_cx).expect("extra view");
    draw(&a_host, &mut a_cx, &a);
    draw(&b_host, &mut b_cx, &b);
    click(&mut a_cx);
    click(&mut b_cx);
    assert_eq!(
        fx.runtime.test_global_i32("__clicks").unwrap(),
        2,
        "both views' handlers must be live before the release"
    );

    let generation = fx.loaded.test_generation();
    a_cx.update(|_, cx| fx.runtime.release_application_generation(&generation, cx));
    click(&mut a_cx);
    click(&mut b_cx);
    assert_eq!(fx.runtime.test_global_i32("__clicks").unwrap(), 2);
    assert!(
        mount_extra(&fx, &mut b_cx).is_err(),
        "a retired application takes no more views"
    );
}

#[gpui::test]
fn a_second_mount_of_the_same_handle_is_still_refused(cx: &mut TestAppContext) {
    let fx = fixture(cx, 2);
    let (_host, mut context) = new_window(cx);
    let _first = mount_first(&fx, &mut context);
    let error = context
        .update(|window, cx| fx.runtime.mount_application(&fx.loaded, window, cx))
        .expect_err("second mount");
    assert!(
        error.to_string().contains("already been mounted"),
        "{error:#}"
    );
}
