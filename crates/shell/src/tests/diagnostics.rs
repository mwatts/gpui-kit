//! Structured execution failures at the owning-library boundary.

use std::{
    cell::RefCell,
    ops::Deref,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use gpui::{
    AppContext as _, Entity, IntoElement as _, Modifiers, TestAppContext, VisualTestContext, point,
    px,
};

use crate::{
    ComponentCallback, ComponentDataCallback, ComponentElementCallback, DiagnosticSink,
    FirstRender, ScriptFailure, ScriptFailureCategory, ScriptPhase, ScriptView, ShellRuntime,
};

struct CollectingSink {
    records: Rc<RefCell<Vec<ScriptFailure>>>,
}

impl DiagnosticSink for CollectingSink {
    fn report(&self, failure: ScriptFailure) {
        self.records.borrow_mut().push(failure);
    }
}

fn sink(runtime: &ShellRuntime) -> Rc<RefCell<Vec<ScriptFailure>>> {
    let records = Rc::new(RefCell::new(Vec::new()));
    runtime.set_diagnostic_sink(Rc::new(CollectingSink {
        records: Rc::clone(&records),
    }));
    records
}

fn render_once(context: &mut VisualTestContext, view: &Entity<ScriptView>) {
    let view = view.clone();
    context.draw(
        gpui::Point::default(),
        gpui::size(gpui::px(400.), gpui::px(300.)),
        move |_, _| view.into_any_element(),
    );
    context.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
}

fn script_view(
    cx: &mut TestAppContext,
    source: &str,
) -> (Rc<ShellRuntime>, VisualTestContext, Entity<ScriptView>) {
    cx.update(|cx| crate::init(cx));
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    cx.update(|cx| runtime.set_global(cx));
    let view_type = runtime.load_source("diagnostics.js", source).expect("load");
    let window = cx.add_window(|_, _| Empty);
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    let view = context.update(|window, cx| {
        let object = runtime
            .instantiate(&view_type, window, cx)
            .expect("instantiate");
        cx.new(|_| ScriptView::new(runtime.clone(), object))
    });
    (runtime, context, view)
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

struct Rooted(Entity<ScriptView>);

impl gpui::Render for Rooted {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        self.0.clone()
    }
}

struct TestApplicationDirectory(PathBuf);

impl TestApplicationDirectory {
    fn with_source(source: &str) -> Self {
        static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "gpui-shell-diagnostics-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).expect("create test application directory");
        std::fs::write(directory.join("main.js"), source).expect("write test application");
        Self(directory)
    }
}

impl Drop for TestApplicationDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove test application directory");
    }
}

fn application_view(
    cx: &mut TestAppContext,
    source: &str,
) -> (
    Rc<ShellRuntime>,
    VisualTestContext,
    Entity<ScriptView>,
    TestApplicationDirectory,
) {
    cx.update(crate::init);
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    cx.update(|cx| runtime.set_global(cx));
    let directory = TestApplicationDirectory::with_source(source);
    let application = runtime
        .load_application(&directory.0, "main.js")
        .expect("load application");
    let window = cx.add_window(|_, _| Empty);
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    let view = context.update(|window, cx| {
        runtime
            .mount_application(&application, window, cx)
            .expect("mount application")
    });
    (runtime, context, view, directory)
}

#[gpui::test]
fn load_throw_retains_structured_failure_without_re_evaluating(cx: &mut TestAppContext) {
    let _ = cx;
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    let records = sink(&runtime);
    let source = r#"
        globalThis.__loads = (globalThis.__loads || 0) + 1;
        throw new Error("intentional load failure");
    "#;
    let error = runtime
        .load_source("load-throw.js", source)
        .expect_err("load must fail");
    let failure = ScriptFailure::from_error(&error);
    assert_eq!(failure.phase(), ScriptPhase::Load);
    assert_eq!(failure.category(), ScriptFailureCategory::Exception);
    assert!(
        failure.message().contains("intentional load failure"),
        "{}",
        failure.message()
    );
    assert_eq!(records.borrow().len(), 1);
    assert_eq!(
        records.borrow()[0].correlation_id(),
        failure.correlation_id()
    );
}

#[gpui::test]
fn render_throw_records_a_structured_failure(cx: &mut TestAppContext) {
    let source = r#"
        import { View, div } from "gpui-kit";
        export default class Lens extends View {
          render() { throw new Error("intentional render failure"); }
        }
    "#;
    let (runtime, mut context, view) = script_view(cx, source);
    let records = sink(&runtime);
    render_once(&mut context, &view);
    let failure = context.update(|_, cx| {
        view.read(cx)
            .build_failure()
            .cloned()
            .expect("structured render failure")
    });
    assert_eq!(failure.phase(), ScriptPhase::Render);
    assert_eq!(failure.category(), ScriptFailureCategory::Exception);
    assert!(failure.message().contains("intentional render failure"));
    assert_eq!(records.borrow().len(), 1);
}

#[gpui::test]
fn first_render_event_fires_exactly_once(cx: &mut TestAppContext) {
    let source = r#"
        import { View, div } from "gpui-kit";
        export default class Lens extends View {
          render() { throw new Error("first render failure"); }
        }
    "#;
    let (_runtime, mut context, view) = script_view(cx, source);
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = Rc::clone(&events);
    let _subscription = context.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &FirstRender, _| {
            captured.borrow_mut().push(event.clone());
        })
    });
    render_once(&mut context, &view);
    context.run_until_parked();
    render_once(&mut context, &view);
    context.run_until_parked();
    let events = events.borrow();
    assert_eq!(events.len(), 1, "first render must be emitted once");
    assert!(!events[0].succeeded());
    assert!(
        events[0]
            .failure()
            .is_some_and(|failure| failure.message().contains("first render failure"))
    );
}

#[gpui::test]
fn ordinary_element_callback_throw_reaches_the_sink(cx: &mut TestAppContext) {
    let source = r#"
        import { View, div } from "gpui-kit";
        export default class Lens extends View {
          render() {
            return div().size_full().child("click").on_click(() => {
              throw new Error("ordinary callback failure");
            });
          }
        }
    "#;
    cx.update(|cx| crate::init(cx));
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    cx.update(|cx| runtime.set_global(cx));
    let records = sink(&runtime);
    let view_type = runtime.load_source("ordinary.js", source).expect("load");
    let runtime_for_view = runtime.clone();
    let window = cx.add_window(move |window, cx| {
        let view = runtime_for_view
            .instantiate_view(&view_type, window, cx)
            .expect("instantiate");
        Rooted(view)
    });
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    context.update(|window, cx| window.draw(cx).clear(cx));
    context.simulate_click(point(px(20.), px(20.)), Modifiers::default());
    context.run_until_parked();
    let records = records.borrow();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].phase(), ScriptPhase::Callback);
    assert_eq!(records[0].category(), ScriptFailureCategory::Exception);
    assert!(records[0].message().contains("ordinary callback failure"));
}

#[gpui::test]
fn component_callback_throw_reaches_the_sink_once(cx: &mut TestAppContext) {
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    let records = sink(&runtime);
    let (id, _) = runtime
        .test_script_callback(r#"() => { throw new Error("component callback failure"); }"#)
        .expect("callback");
    let callback = ComponentCallback::from_runtime(&runtime, id);
    let window = cx.add_window(|_, _| Empty);
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    context.update(|window, cx| {
        callback.invoke_and_report_with("component", &[], window, cx);
    });
    let records = records.borrow();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].phase(), ScriptPhase::Callback);
    assert!(records[0].message().contains("component callback failure"));
}

#[gpui::test]
fn retired_callback_does_not_report_a_script_failure(cx: &mut TestAppContext) {
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    let records = sink(&runtime);
    let (id, generation) = runtime
        .test_script_callback(r#"() => { throw new Error("retired callback failure"); }"#)
        .expect("callback");
    runtime.retire_callback_generation(generation);
    let callback = ComponentCallback::from_runtime(&runtime, id);
    let window = cx.add_window(|_, _| Empty);
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    context.update(|window, cx| {
        callback.invoke_and_report_with("retired", &[], window, cx);
    });
    assert!(
        records.borrow().is_empty(),
        "retired callbacks must not emit diagnostics: {:?}",
        records.borrow()
    );
}

#[gpui::test]
fn data_and_element_callback_failures_report_once_and_return_safe_errors(cx: &mut TestAppContext) {
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    let records = sink(&runtime);
    let (data_id, _) = runtime
        .test_script_callback(r#"() => { throw new Error("PRIVATE_DATA_CALLBACK"); }"#)
        .expect("data callback");
    let (element_id, _) = runtime
        .test_script_callback(r#"() => { throw new Error("PRIVATE_ELEMENT_CALLBACK"); }"#)
        .expect("element callback");
    let data = ComponentDataCallback::from_runtime(&runtime, data_id);
    let element = ComponentElementCallback::from_runtime(&runtime, element_id);
    let window = cx.add_window(|_, _| Empty);
    let mut context = VisualTestContext::from_window(*window.deref(), cx);

    let (data_error, element_error) = context.update(|window, cx| {
        (
            data.snapshot_with(&[], window, cx).unwrap_err(),
            match element.build_with(&[], window, cx) {
                Ok(_) => panic!("element callback must fail"),
                Err(error) => error,
            },
        )
    });

    assert!(!data_error.to_string().contains("PRIVATE_DATA_CALLBACK"));
    assert!(
        !element_error
            .to_string()
            .contains("PRIVATE_ELEMENT_CALLBACK")
    );
    let records = records.borrow();
    assert_eq!(records.len(), 2, "each callback failure is reported once");
    assert!(
        records
            .iter()
            .any(|failure| failure.message().contains("PRIVATE_DATA_CALLBACK"))
    );
    assert!(
        records
            .iter()
            .any(|failure| failure.message().contains("PRIVATE_ELEMENT_CALLBACK"))
    );
}

#[gpui::test]
fn retired_element_callback_is_empty_and_retired_data_callback_is_inactive(
    cx: &mut TestAppContext,
) {
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    let records = sink(&runtime);
    let (element_id, element_generation) = runtime
        .test_script_callback(r#"() => { throw new Error("retired element"); }"#)
        .expect("element callback");
    let (data_id, data_generation) = runtime
        .test_script_callback(r#"() => { throw new Error("retired data"); }"#)
        .expect("data callback");
    let element = ComponentElementCallback::from_runtime(&runtime, element_id);
    let data = ComponentDataCallback::from_runtime(&runtime, data_id);
    runtime.retire_callback_generation(element_generation);
    runtime.retire_callback_generation(data_generation);
    let window = cx.add_window(|_, _| Empty);
    let mut context = VisualTestContext::from_window(*window.deref(), cx);

    let (element_result, data_error) = context.update(|window, cx| {
        (
            element.build_with(&[], window, cx),
            data.snapshot_with(&[], window, cx).unwrap_err(),
        )
    });

    assert!(element_result.unwrap().is_none());
    assert!(
        data_error
            .downcast_ref::<crate::InactiveCallback>()
            .is_some()
    );
    assert!(records.borrow().is_empty());
}

#[gpui::test]
fn infinite_render_is_a_budget_failure(cx: &mut TestAppContext) {
    let source = r#"
        import { View, div } from "gpui-kit";
        export default class Lens extends View {
          render() { while (true) {} }
        }
    "#;
    let (runtime, mut context, view) = script_view(cx, source);
    let records = sink(&runtime);
    render_once(&mut context, &view);
    let failure = context.update(|_, cx| {
        view.read(cx)
            .build_failure()
            .cloned()
            .expect("budget failure")
    });
    assert_eq!(failure.phase(), ScriptPhase::Render);
    assert_eq!(failure.category(), ScriptFailureCategory::ExecutionBudget);
    assert_ne!(
        failure.category(),
        ScriptFailureCategory::Exception,
        "budget interruption must not be classified by matching error text"
    );
    assert_eq!(records.borrow().len(), 1);
    assert_eq!(
        records.borrow()[0].category(),
        ScriptFailureCategory::ExecutionBudget
    );
}

#[gpui::test]
fn installed_sink_hides_raw_exception_from_the_overlay(cx: &mut TestAppContext) {
    let source = r#"
        import { View, div } from "gpui-kit";
        globalThis.__diagnosticGetterCalls = 0;
        export default class Lens extends View {
          render() {
            const failure = new Error("ordinary message");
            Object.defineProperties(failure, {
              message: { get() { __diagnosticGetterCalls++; return "PRIVATE_GETTER_MESSAGE résumé 🔒"; } },
              stack: { get() { __diagnosticGetterCalls++; return "PRIVATE_STACK (/private/PRIVATE_STACK_FILE.js:7:9)"; } },
              fileName: { get() { __diagnosticGetterCalls++; return "/private/PRIVATE_LOCATION.js"; } },
              lineNumber: { get() { __diagnosticGetterCalls++; return 7; } },
              columnNumber: { get() { __diagnosticGetterCalls++; return 9; } },
            });
            throw failure;
          }
        }
    "#;
    let (runtime, mut context, view) = script_view(cx, source);
    let _records = sink(&runtime);
    render_once(&mut context, &view);
    context.run_until_parked();
    let presented = context.update(|_, cx| {
        view.read(cx)
            .presented_failure_text()
            .expect("overlay text")
    });
    for private_fragment in ["PRIVATE_", "résumé", "🔒", "/private/"] {
        assert!(!presented.contains(private_fragment), "{presented}");
    }
    assert!(presented.contains("exception"), "{presented}");
    assert_eq!(
        runtime.test_global_i32("__diagnosticGetterCalls").unwrap(),
        0
    );
}

#[gpui::test]
fn nested_initial_render_failure_fails_the_completed_root_frame(cx: &mut TestAppContext) {
    let source = r#"
        import { View, div } from "gpui-kit";
        class Child extends View {
          render() { throw new Error("nested initial render failure"); }
        }
        export default class Parent extends View {
          init(_props, cx) { this.child = cx.new(Child); }
          render() { return div().size_full().child(this.child); }
        }
    "#;
    let (runtime, mut context, view, _directory) = application_view(cx, source);
    let records = sink(&runtime);
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = Rc::clone(&events);
    let _subscription = context.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &FirstRender, _| {
            captured.borrow_mut().push(event.clone());
        })
    });

    render_once(&mut context, &view);
    context.run_until_parked();
    render_once(&mut context, &view);
    context.run_until_parked();

    let events = events.borrow();
    let rendered = context.update(|_, cx| view.read(cx).snapshot().is_some());
    assert_eq!(
        events.len(),
        1,
        "completed root frame must emit once; rendered={rendered}, records={:?}",
        records.borrow()
    );
    assert!(
        !events[0].succeeded(),
        "nested render failed in the first frame"
    );
    assert!(events[0].failure().is_some(), "failure must be retained");
    assert!(
        records
            .borrow()
            .iter()
            .any(|failure| failure.message().contains("nested initial render failure")),
        "{:?}",
        records.borrow()
    );
}

#[gpui::test]
fn visible_virtual_list_failure_fails_the_completed_root_frame(cx: &mut TestAppContext) {
    let source = r#"
        import { View, div } from "gpui-kit";
        import { v_flex, v_virtual_list } from "gpui-base";
        export default class Rows extends View {
          render() {
            return v_flex().w(300).h(200).child(
              v_virtual_list("rows", 40, 20, (index) => String(index), () => {
                throw new Error("visible virtual rows failure");
              })
            );
          }
        }
    "#;
    let (runtime, mut context, view, _directory) = application_view(cx, source);
    let records = sink(&runtime);
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = Rc::clone(&events);
    let _subscription = context.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &FirstRender, _| {
            captured.borrow_mut().push(event.clone());
        })
    });

    render_once(&mut context, &view);
    context.run_until_parked();
    render_once(&mut context, &view);
    context.run_until_parked();

    let events = events.borrow();
    assert_eq!(events.len(), 1, "completed root frame must emit once");
    assert!(
        !events[0].succeeded(),
        "the visible item factory failed in the first frame"
    );
    assert!(events[0].failure().is_some(), "failure must be retained");
    assert!(
        records
            .borrow()
            .iter()
            .any(|failure| failure.message().contains("visible virtual rows failure")),
        "{:?}",
        records.borrow()
    );
}

#[gpui::test]
fn timer_task_throw_reports_a_safe_callback_failure(cx: &mut TestAppContext) {
    let source = r#"
        import { View, div } from "gpui-kit";
        globalThis.__asyncDiagnosticGetterCalls = 0;
        export default class Lens extends View {
          init(_props, cx) {
            cx.timer.after(0, () => {
              const failure = new Error("ordinary task failure");
              Object.defineProperties(failure, {
                message: { get() { __asyncDiagnosticGetterCalls++; return "PRIVATE_TASK_MESSAGE résumé 🔒"; } },
                stack: { get() { __asyncDiagnosticGetterCalls++; return "PRIVATE_TASK_STACK (/private/task.js:7:9)"; } },
              });
              throw failure;
            });
          }
          render() { return div(); }
        }
    "#;
    let (runtime, context, _view, _directory) = application_view(cx, source);
    let records = sink(&runtime);

    context.run_until_parked();

    let records = records.borrow();
    assert_eq!(records.len(), 1, "task throw must reach the sink once");
    assert_eq!(records[0].phase(), ScriptPhase::Callback);
    assert_eq!(records[0].category(), ScriptFailureCategory::Exception);
    assert_eq!(records[0].safe_summary(), "callback exception");
    assert_eq!(
        runtime
            .test_global_i32("__asyncDiagnosticGetterCalls")
            .unwrap(),
        0
    );
}

#[gpui::test]
fn spawned_promise_rejection_reports_a_safe_callback_failure(cx: &mut TestAppContext) {
    let source = r#"
        import { View, div } from "gpui-kit";
        globalThis.__asyncDiagnosticGetterCalls = 0;
        export default class Lens extends View {
          init(_props, cx) {
            cx.spawn(async () => {
              await Promise.resolve();
              const failure = new Error("ordinary rejection");
              Object.defineProperties(failure, {
                message: { get() { __asyncDiagnosticGetterCalls++; return "PRIVATE_REJECTION_MESSAGE résumé 🔒"; } },
                stack: { get() { __asyncDiagnosticGetterCalls++; return "PRIVATE_REJECTION_STACK (/private/rejection.js:8:4)"; } },
              });
              throw failure;
            });
          }
          render() { return div(); }
        }
    "#;
    let (runtime, mut context, view, _directory) = application_view(cx, source);
    let records = sink(&runtime);

    render_once(&mut context, &view);
    context.run_until_parked();

    let records = records.borrow();
    assert_eq!(records.len(), 1, "spawn rejection must reach the sink once");
    assert_eq!(records[0].phase(), ScriptPhase::Callback);
    assert_eq!(records[0].category(), ScriptFailureCategory::Exception);
    assert_eq!(records[0].safe_summary(), "callback exception");
    assert_eq!(
        runtime
            .test_global_i32("__asyncDiagnosticGetterCalls")
            .unwrap(),
        0
    );
}

#[gpui::test]
fn runaway_timer_task_reports_a_callback_budget_failure(cx: &mut TestAppContext) {
    let source = r#"
        import { View, div } from "gpui-kit";
        export default class Lens extends View {
          init(_props, cx) {
            cx.timer.after(0, () => { while (true) {} });
          }
          render() { return div(); }
        }
    "#;
    let (runtime, context, _view, _directory) = application_view(cx, source);
    let records = sink(&runtime);

    context.run_until_parked();

    let records = records.borrow();
    assert_eq!(records.len(), 1, "runaway task must reach the sink once");
    assert_eq!(records[0].phase(), ScriptPhase::Callback);
    assert_eq!(
        records[0].category(),
        ScriptFailureCategory::ExecutionBudget
    );
    assert_eq!(records[0].safe_summary(), "callback execution_budget");
}

#[gpui::test]
fn retired_spawn_rejection_does_not_replace_current_diagnostics(cx: &mut TestAppContext) {
    let source = r#"
        import { View, div } from "gpui-kit";
        export default class Lens extends View {
          init(_props, cx) {
            cx.spawn(async () => {
              await Promise.resolve();
              throw new Error("retired rejection");
            });
          }
          render() { return div(); }
        }
    "#;
    let (runtime, mut context, view, _directory) = application_view(cx, source);
    let records = sink(&runtime);
    render_once(&mut context, &view);
    let retired = context.update(|_, cx| {
        view.read(cx)
            .application_generation()
            .expect("application generation")
    });
    retired.retire();

    let replacement_directory = TestApplicationDirectory::with_source(
        r#"
            import { View, div } from "gpui-kit";
            export default class Replacement extends View {
              render() { return div(); }
            }
        "#,
    );
    let replacement = runtime
        .load_application(&replacement_directory.0, "main.js")
        .expect("load replacement");
    let replacement_view = context.update(|window, cx| {
        runtime
            .mount_application(&replacement, window, cx)
            .expect("mount replacement")
    });
    render_once(&mut context, &replacement_view);
    context.run_until_parked();

    assert!(
        records.borrow().is_empty(),
        "retired task failure reached the current sink: {:?}",
        records.borrow()
    );
}

#[gpui::test]
fn root_materialize_failure_fails_first_render(cx: &mut TestAppContext) {
    use crate::{
        COMPONENT_REGISTRY_API_VERSION, ComponentDescriptor, ComponentMaterializer,
        ComponentPayload, ComponentRegistry, ConstructorDescriptor, DEFAULT_COMPONENT_MODULE,
        MaterializeRequest,
    };
    use gpui::AnyElement;
    use std::sync::Arc;

    struct Boom;
    impl ComponentMaterializer for Boom {
        fn materialize(&self, _: MaterializeRequest<'_>) -> anyhow::Result<AnyElement> {
            anyhow::bail!("intentional materialize failure");
        }
    }

    cx.update(|cx| crate::init(cx));
    let mut registry =
        ComponentRegistry::new(COMPONENT_REGISTRY_API_VERSION, DEFAULT_COMPONENT_MODULE).unwrap();
    registry
        .register(
            ComponentDescriptor::new("Boom", Arc::new(Boom)).with_constructors(vec![
                ConstructorDescriptor::new("Boom", Vec::new(), |_| Ok(ComponentPayload::new(()))),
            ]),
        )
        .unwrap();
    let runtime = ShellRuntime::new_isolated_with_components(registry.freeze().unwrap()).unwrap();
    cx.update(|cx| runtime.set_global(cx));
    let records = sink(&runtime);
    let view_type = runtime
        .load_source(
            "boom.js",
            r#"
                import { View } from "gpui-kit";
                import { Boom } from "gpui-component";
                export default class Lens extends View {
                  render() { return new Boom(); }
                }
            "#,
        )
        .expect("load");
    let window = cx.add_window(|_, _| Empty);
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    let view = context.update(|window, cx| {
        let object = runtime
            .instantiate(&view_type, window, cx)
            .expect("instantiate");
        cx.new(|_| ScriptView::new(runtime.clone(), object))
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = Rc::clone(&events);
    let _subscription = context.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &FirstRender, _| {
            captured.borrow_mut().push(event.clone());
        })
    });
    render_once(&mut context, &view);
    context.run_until_parked();
    let events = events.borrow();
    assert_eq!(events.len(), 1, "first render must fire once");
    assert!(!events[0].succeeded());
    assert!(
        events[0]
            .failure()
            .is_some_and(|failure| failure.category() == ScriptFailureCategory::Engine)
    );
    assert!(
        records.borrow().iter().any(|failure| failure
            .message()
            .contains("intentional materialize failure")),
        "{:?}",
        records.borrow()
    );
}
