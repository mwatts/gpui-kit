//! In-memory [`HostSource`] loads: no declaration files, leased generations.

use std::{
    collections::BTreeSet,
    fs,
    ops::Deref as _,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{Entity, IntoElement as _, TestAppContext, VisualTestContext, px, size};
use gpui_shell::{HostSource, LoadedApplication, ScriptView, ShellRuntime};

fn two_module_source(label: &str) -> HostSource {
    let main = format!(
        r#"
import {{ View, div }} from "gpui-kit";
import {{ label }} from "./util.js";
if (label !== "{label}") {{
  throw new Error("imported label was " + label);
}}
export default class Panel extends View {{
  render() {{ return div().child(label); }}
}}
"#
    );
    let util = format!("export const label = \"{label}\";\n");
    HostSource::new(
        "main.js",
        [("main.js", main.as_bytes()), ("./util.js", util.as_bytes())],
    )
    .expect("seal two-module host source")
}

fn names(directory: &Path) -> BTreeSet<String> {
    fs::read_dir(directory)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().into_string().ok()?;
                    if name.starts_with('.') {
                        return None;
                    }
                    Some(name)
                })
                .collect()
        })
        .unwrap_or_default()
}

struct RestoreCwd {
    original: PathBuf,
    sandbox: PathBuf,
}

impl Drop for RestoreCwd {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = fs::set_permissions(&self.sandbox, fs::Permissions::from_mode(0o755));
        }
        let _ = fs::remove_dir_all(&self.sandbox);
    }
}

fn enter_read_only_sandbox() -> RestoreCwd {
    let sandbox = std::env::temp_dir().join(format!(
        "gpui-shell-host-ro-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&sandbox).expect("sandbox");
    let original = std::env::current_dir().expect("cwd");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&sandbox, fs::Permissions::from_mode(0o555)).expect("chmod a+rX,a-w");
    }
    std::env::set_current_dir(&sandbox).expect("chdir sandbox");
    RestoreCwd { original, sandbox }
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

fn render_tree(
    cx: &mut TestAppContext,
    runtime: &Rc<ShellRuntime>,
    loaded: &LoadedApplication,
) -> String {
    cx.update(gpui_shell::init);
    let window = cx.add_window(|_, _| Empty);
    let mut context = VisualTestContext::from_window(*window.deref(), cx);
    let view: Entity<ScriptView> = context
        .update(|window, cx| runtime.mount_application(loaded, window, cx))
        .expect("mount host source");
    let view_for_draw = view.clone();
    context.draw(
        gpui::Point::default(),
        size(px(400.), px(300.)),
        move |_, _| view_for_draw.into_any_element(),
    );
    context.update(|window, cx| window.simulate_next_frame(cx));
    context.update(|_, cx| view.read(cx).snapshot().expect("snapshot").debug_tree())
}

#[test]
fn two_module_host_source_evaluates_the_imported_string() {
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    runtime
        .load_host_source(&two_module_source("from-util"))
        .expect("imported util.js must evaluate");
}

#[gpui::test]
fn two_module_host_source_imported_string_is_visible(cx: &mut TestAppContext) {
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    let loaded = runtime
        .load_host_source(&two_module_source("from-util"))
        .expect("load");
    let tree = render_tree(cx, &runtime, &loaded);
    assert!(
        tree.contains("from-util"),
        "imported string must be evaluated: {tree}"
    );
}

#[test]
fn host_source_load_writes_no_declaration_or_module_files() {
    let restore = enter_read_only_sandbox();
    let before = names(&restore.sandbox);
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    runtime
        .load_host_source(&two_module_source("from-util"))
        .expect("load host source in a read-only cwd");
    let after = names(&restore.sandbox);
    assert_eq!(before, after, "host load must not create files in cwd");
    assert!(!restore.sandbox.join("gpui-kit.d.ts").exists());
    assert!(!restore.sandbox.join("jsconfig.json").exists());
    assert!(!restore.sandbox.join("node_modules").exists());
}

#[test]
fn illegal_parent_specifier_fails_seal_and_load() {
    let sealed = HostSource::new(
        "main.js",
        [
            ("main.js", b"export default class Panel {}".as_slice()),
            ("../x.js", b"export const x = 1;".as_slice()),
        ],
    );
    assert!(sealed.is_err(), "constructor must reject `..` specifiers");

    let source = HostSource::new(
        "main.js",
        [(
            "main.js",
            br#"
import { x } from "../x.js";
export default class Panel {}
"#
            .as_slice(),
        )],
    )
    .expect("entry without `..` keys seals");
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    let error = runtime
        .load_host_source(&source)
        .expect_err("relative `..` import must fail");
    let message = error.to_string();
    assert!(
        message.contains("..") || message.contains("outside") || message.contains("cannot resolve"),
        "{message}"
    );
}

#[gpui::test]
fn dropping_loaded_application_releases_generation_and_reload_sees_new_bytes(
    cx: &mut TestAppContext,
) {
    let runtime = ShellRuntime::new_isolated().expect("runtime");
    let first = runtime
        .load_host_source(&two_module_source("first"))
        .expect("first load");
    let first_generation = first.generation_id();
    drop(first);

    let cwd = std::env::temp_dir().join(format!(
        "gpui-shell-host-gen-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&cwd).expect("side-file probe");
    let original = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&cwd).expect("chdir");
    let second = runtime
        .load_host_source(&two_module_source("second"))
        .expect("second load");
    let second_generation = second.generation_id();
    assert_ne!(
        first_generation, second_generation,
        "a new load must allocate a new generation"
    );
    let after = names(&cwd);
    let _ = std::env::set_current_dir(&original);
    let _ = fs::remove_dir_all(&cwd);
    assert!(
        !after.contains("gpui-kit.d.ts")
            && !after.contains("jsconfig.json")
            && !after.contains("node_modules")
            && !after.iter().any(|name| name.contains("cache")),
        "host origin must not construct git/editor side files: {after:?}"
    );

    let tree = render_tree(cx, &runtime, &second);
    assert!(
        tree.contains("second") && !tree.contains("first"),
        "imported change must be visible after drop: {tree}"
    );
}
