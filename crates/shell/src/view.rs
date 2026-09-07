//! The single bridge between a script view and GPUI's render loop.
//!
//! Every script-defined view, panel, or dialog body is carried by a `ScriptView`
//! entity. [`ShellRoot`](crate::root::ShellRoot) mounts the entity through
//! GPUI's cached-view path, so a clean window frame reuses its rendered subtree
//! without calling this `render` at all. When the entity or one of its retained
//! descendants is dirty, `render` remains deliberately *not* necessarily a
//! script call:
//!
//! ```text
//! dirty view render ─▶ snapshot still valid? ──yes──▶ materialize   (no VM)
//!                          │
//!                          no
//!                          ▼
//!                  script render() ──▶ publish snapshot ──▶ materialize
//! ```
//!
//! The script runs only when something invalidated its snapshot: `cx.notify()`
//! from an event or a task, a hot reload, or a palette change. Everything else
//! replays the description the script already produced. Clean frames skip both
//! operations through GPUI's subtree cache. That is what keeps script and
//! materialization cost proportional to application activity rather than frame
//! rate.

use std::rc::Rc;

use gpui::{
    Context, EntityId, EventEmitter, IntoElement, ParentElement as _, Render, Styled as _, Window,
    div,
};

use crate::{
    engine::{ShellRuntime, ViewObject},
    error::ScriptFailure,
    materialize::try_materialize,
    policy::Policy,
    runtime::{InitialFrameOutcome, error_banner, error_overlay},
    snapshot::RenderSnapshot,
};

/// Outcome of the first root snapshot-and-materialize attempt of a [`ScriptView`].
///
/// Emitted once, after that attempt, so a host can subscribe before drawing
/// instead of polling `build_error`. Nested [`ScriptView`]s do not emit this
/// event. A first failure is terminal for this view's qualification: a later
/// manual refresh that succeeds does not emit again.
///
/// Deferred component factories (`ComponentElementFactory::build` after the
/// first paint, including overlay slots) report through [`crate::DiagnosticSink`]
/// and do not change this outcome. Those failures are later diagnostics, not a
/// first-render success claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FirstRender {
    succeeded: bool,
    failure: Option<ScriptFailure>,
}

impl FirstRender {
    pub fn succeeded(&self) -> bool {
        self.succeeded
    }

    pub fn failure(&self) -> Option<&ScriptFailure> {
        self.failure.as_ref()
    }
}

pub struct ScriptView {
    /// Declared before `runtime` because fields drop in declaration order, and
    /// a script value released after its engine aborts the process. A view that
    /// happens to hold the last reference to the runtime would otherwise free
    /// the VM first and then release this handle into it. The snapshots below
    /// are in the same position: retiring their callbacks releases script
    /// values, so they must go first too.
    ///
    /// This ordering is the only thing that retires a view's callbacks while
    /// the VM that owns them is still alive.
    object: ViewObject,
    /// The published description. `None` only before the first render.
    current: Option<RenderSnapshot>,
    /// The snapshot this one replaced, held one generation longer.
    ///
    /// GPUI can dispatch an event against the elements of a frame that has
    /// already been superseded — a click landing between a rebuild and the
    /// repaint that follows it. Keeping the previous snapshot alive keeps its
    /// callbacks resolvable for exactly that window; anything older is stale and
    /// is meant to resolve to nothing.
    previous: Option<RenderSnapshot>,
    /// Set when script-visible state may have changed, cleared by the rebuild.
    dirty: bool,
    /// Set when the script-visible handle has been released. GPUI may retain
    /// the entity for an older frame, but it must never rebuild after release.
    retired: bool,
    /// The tokens and appearance the current snapshot resolved against.
    theme: Option<crate::theme_tokens::ThemeSnapshotKey>,
    /// The failure of the most recent build, if it failed.
    ///
    /// Held rather than re-derived so a script that throws is not re-run on
    /// every frame: a broken render is exactly as frame-coupled as a working one
    /// if the failure re-triggers the build.
    error: Option<ScriptFailure>,
    /// Set once the completed first frame has emitted [`FirstRender`].
    emitted_first_render: bool,
    /// Whether the completion callback for the first demanded frame is queued.
    scheduled_first_render: bool,
    /// First failure for test-only roots loaded without an application generation.
    local_first_failure: Option<ScriptFailure>,
    /// Whose authority this view's script runs under.
    ///
    /// Captured when the view is constructed rather than read when it is used:
    /// a callback firing three seconds later must run under the grant its own
    /// script was loaded with, and no swap made in between can change that.
    policy: Rc<Policy>,
    ownership: ViewOwnership,
    runtime: Rc<ShellRuntime>,
}

/// Which cleanup boundary this view owns.
#[derive(Clone, Copy)]
enum ViewOwnership {
    /// The application root owns application-wide retained state and tasks.
    Root,
    /// A nested view owns only work keyed to its exact GPUI entity identity.
    Nested(EntityId),
}

impl ScriptView {
    /// Under the policy in force where the view was constructed.
    #[cfg(test)]
    pub(crate) fn new(runtime: Rc<ShellRuntime>, object: ViewObject) -> Self {
        Self::with_policy(runtime, object, crate::scope::policy())
    }

    pub(crate) fn with_policy(
        runtime: Rc<ShellRuntime>,
        object: ViewObject,
        policy: Rc<Policy>,
    ) -> Self {
        Self::with_ownership(runtime, object, policy, ViewOwnership::Root)
    }

    pub(crate) fn nested(
        runtime: Rc<ShellRuntime>,
        object: ViewObject,
        policy: Rc<Policy>,
        entity_id: EntityId,
    ) -> Self {
        Self::with_ownership(runtime, object, policy, ViewOwnership::Nested(entity_id))
    }

    fn with_ownership(
        runtime: Rc<ShellRuntime>,
        object: ViewObject,
        policy: Rc<Policy>,
        ownership: ViewOwnership,
    ) -> Self {
        Self {
            object,
            current: None,
            previous: None,
            dirty: true,
            retired: false,
            policy,
            theme: None,
            error: None,
            emitted_first_render: false,
            scheduled_first_render: false,
            local_first_failure: None,
            ownership,
            runtime,
        }
    }

    /// Marks the script description as possibly out of date.
    ///
    /// This is what Shell `cx.notify()` means: *my* description may have
    /// changed. Scheduling and coalescing the actual repaint stays with GPUI —
    /// three notifies before the next frame rebuild one snapshot, not three.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Invalidates and notifies: the host changed something the script reads.
    ///
    /// **A host that mutates state a script reads must call this, not
    /// `cx.notify()` alone.** The two are different requests now, and the
    /// difference is the point of this type:
    ///
    /// ```text
    /// cx.notify()  ── draw this view again          (no script runs)
    /// refresh()    ── and the description is stale  (the script runs)
    /// ```
    ///
    /// A bare `notify` is still the right call for a repaint that changes
    /// nothing the script can see. Getting it wrong in the other direction is
    /// visible immediately — the interface simply does not update — which is the
    /// same failure mode as a forgotten `cx.notify()` in GPUI itself, and it is
    /// cheap to find for the same reason.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.invalidate();
        cx.notify();
    }

    /// Replaces the script instance behind this view.
    ///
    /// Hot reload keeps the entity — and therefore the window, the focus and
    /// the element identities — and swaps only what the script produced. The
    /// description that the old instance built is now meaningless, so the view
    /// is invalidated with it.
    pub(crate) fn replace_object(&mut self, object: ViewObject) {
        self.object = object;
        self.dirty = true;
    }

    /// The script state behind this view, for host code that needs to read it.
    pub(crate) fn object(&self) -> &ViewObject {
        &self.object
    }

    /// The published description, if one has been built.
    /// Why the most recent build failed, if it did.
    ///
    /// A view with no snapshot is not the same as a view with nothing to draw:
    /// it means the script threw and the failure was recorded here. A test that
    /// finds `snapshot()` empty should report this rather than the absence,
    /// because the absence is the symptom and this is the cause.
    pub fn build_error(&self) -> Option<&str> {
        self.error.as_ref().map(ScriptFailure::message)
    }

    pub fn build_failure(&self) -> Option<&ScriptFailure> {
        self.error.as_ref()
    }

    pub fn snapshot(&self) -> Option<&RenderSnapshot> {
        self.current.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn presented_failure_text(&self) -> Option<String> {
        self.error.as_ref().map(|failure| {
            if self.runtime.has_diagnostic_sink() {
                failure.safe_summary()
            } else {
                failure.to_string()
            }
        })
    }

    /// Whether the next GPUI render will enter the VM.
    /// The authority this view's script runs under.
    pub fn policy(&self) -> Rc<Policy> {
        self.policy.clone()
    }

    pub(crate) fn runtime(&self) -> Rc<ShellRuntime> {
        self.runtime.clone()
    }

    pub(crate) fn application_generation(
        &self,
    ) -> Option<Rc<crate::runtime::ApplicationGeneration>> {
        self.object.application_generation()
    }

    pub fn is_dirty(&self) -> bool {
        !self.retired && self.dirty
    }

    /// Makes a retained entity inert before its store handle is removed.
    /// A rendered GPUI frame may still retain the entity after script release.
    pub(crate) fn retire(&mut self) {
        self.retired = true;
        self.dirty = false;
        self.error = None;
        self.emitted_first_render = true;
        self.scheduled_first_render = true;
        self.local_first_failure = None;
        self.previous = None;
        self.current = None;
    }

    /// Runs the script and publishes what it produced.
    ///
    /// The build is transactional. A replacement snapshot is assembled beside
    /// the live one and swapped in only after the script returns successfully;
    /// a script that throws half-way leaves the previous description, and the
    /// callbacks that belong to it, exactly as they were.
    fn rebuild(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Cleared before the script runs, not after: draining the task queue at
        // the end of a build can notify this same view, and that notify has to
        // survive into the next frame rather than be wiped by the build that
        // was already in flight.
        self.dirty = false;

        let runtime = self.runtime.clone();
        let object = self.object.clone();
        let policy = self.policy.clone();
        let entity = cx.entity();

        match runtime.build_snapshot(&object, Some(entity), policy, window, cx) {
            Ok(snapshot) => {
                // Measured here rather than anywhere else because this is the
                // only place two consecutive descriptions of one view exist at
                // the same time. Nothing acts on the answer: it counts how often
                // a rebuild produced the shape it replaced, which is what a
                // template cache would have to be able to fill instead of
                // rebuild (§20.7 of `docs/gpui-shell.md`). A first build has no
                // predecessor and is not a data point either way.
                if let Some(current) = self.current.as_ref() {
                    runtime
                        .metrics()
                        .record_structure(current.structure() == snapshot.structure());
                }

                // Assigning through `previous` is what retires the snapshot
                // before last: dropping it releases its callbacks.
                self.previous = self.current.replace(snapshot);
                self.error = None;
            }
            Err(error) => {
                let failure = ScriptFailure::from_error(&error);
                self.record_initial_failure(&failure);
                runtime.report_script_failure(failure.clone());
                self.error = Some(failure);
            }
        }
    }

    /// Rebuilds a dirty snapshot before the host reads diagnostics for this frame.
    pub fn prepare_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.retired {
            return;
        }
        let theme = crate::theme_tokens::sync(cx);
        if self.theme.as_ref() != Some(&theme) {
            self.theme = Some(theme);
            self.dirty = true;
        }
        if self.is_dirty() {
            self.rebuild(window, cx);
        }
    }

    fn present_failure(
        &self,
        failure: &ScriptFailure,
        overlay: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let message = if self.runtime.has_diagnostic_sink() {
            failure.safe_summary()
        } else {
            failure.to_string()
        };
        if overlay {
            error_overlay(&message, window, cx)
        } else {
            error_banner(&message, window, cx)
        }
    }

    fn record_initial_failure(&mut self, failure: &ScriptFailure) {
        if self.emitted_first_render {
            return;
        }
        if let Some(application) = self.object.application_generation() {
            application.record_initial_frame_failure(failure);
        } else if matches!(self.ownership, ViewOwnership::Root)
            && self.local_first_failure.is_none()
        {
            self.local_first_failure = Some(failure.clone());
        }
    }

    fn schedule_first_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.scheduled_first_render || matches!(self.ownership, ViewOwnership::Nested(_)) {
            return;
        }
        self.scheduled_first_render = true;
        let entity = cx.weak_entity();
        window.on_next_frame(move |_, cx| {
            let _ = entity.update(cx, |view, cx| view.finish_first_render(cx));
        });
    }

    fn finish_first_render(&mut self, cx: &mut Context<Self>) {
        if self.retired || self.emitted_first_render {
            return;
        }
        let outcome = if let Some(application) = self.object.application_generation() {
            application.complete_initial_frame()
        } else {
            Some(match self.local_first_failure.take() {
                Some(failure) => InitialFrameOutcome::Failed(failure),
                None => InitialFrameOutcome::Succeeded,
            })
        };
        let Some(outcome) = outcome else {
            return;
        };
        self.emitted_first_render = true;
        let (succeeded, failure) = match outcome {
            InitialFrameOutcome::Succeeded => (true, None),
            InitialFrameOutcome::Failed(failure) => (false, Some(failure)),
        };
        cx.emit(FirstRender { succeeded, failure });
    }
}

impl Drop for ScriptView {
    fn drop(&mut self) {
        match self.ownership {
            ViewOwnership::Root => {
                if let Some(application) = self.object.application_generation() {
                    self.runtime
                        .release_application_generation_without_context(&application);
                }
            }
            ViewOwnership::Nested(entity_id) => {
                // Child-owned retained records are removed by EntityStore in
                // the same operation that removes the child handle. Reaching
                // back into that RefCell here would re-enter its mutable borrow.
                crate::engine::quickjs::cancel_view_tasks(&self.runtime, entity_id);
            }
        }
    }
}

impl Render for ScriptView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.retired {
            return div().into_any_element();
        }
        self.prepare_render(window, cx);

        match (self.error.clone(), self.current.clone()) {
            (None, Some(snapshot)) => match try_materialize(&self.runtime, &snapshot, window, cx) {
                Ok(element) => {
                    self.schedule_first_render(window, cx);
                    element
                }
                Err(error) => {
                    let failure = self.runtime.report_materialize_error(&error);
                    self.record_initial_failure(&failure);
                    self.error = Some(failure.clone());
                    self.schedule_first_render(window, cx);
                    self.present_failure(&failure, true, window, cx)
                }
            },
            // A build that failed left the last good snapshot in place, so the
            // interface is still there to show. Reporting over it beats
            // replacing it: the reader keeps their scroll, their focus and
            // whatever they were reading, and still learns what broke.
            (Some(failure), Some(snapshot)) => {
                self.schedule_first_render(window, cx);
                match try_materialize(&self.runtime, &snapshot, window, cx) {
                    Ok(element) => div()
                        .relative()
                        .size_full()
                        .child(element)
                        .child(self.present_failure(&failure, false, window, cx))
                        .into_any_element(),
                    Err(_) => self.present_failure(&failure, true, window, cx),
                }
            }
            // Nothing to keep: this view has never rendered successfully.
            (Some(failure), None) => {
                self.schedule_first_render(window, cx);
                self.present_failure(&failure, true, window, cx)
            }
            // Unreachable in practice: a build either publishes a snapshot or
            // records an error. An empty element is the honest answer if it ever
            // is reached, rather than a panic in a render.
            (None, None) => {
                self.schedule_first_render(window, cx);
                div().into_any_element()
            }
        }
    }
}

impl EventEmitter<FirstRender> for ScriptView {}
