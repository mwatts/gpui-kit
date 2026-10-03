//! The keymap: every action the editor answers to, and the chords bound to it.
//!
//! Ported from Bezel `editor/keys.rs`. Scoped to [`CONTEXT`] so Tab indent does
//! not leak into the rest of the app. Undo/redo are bound here **before** any
//! gpui-base input undo can claim the chord.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use gpui::{App, KeyBinding, NoAction, actions};
use gpui_component_block_view::EMBED_CONTEXT;

use crate::image;

/// Key context for the block editor surface.
pub const CONTEXT: &str = "BlockEditor";

actions!(
    block_editor,
    [
        Backspace,
        Delete,
        KillLine,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteToHome,
        Left,
        Right,
        Up,
        Down,
        Home,
        End,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectHome,
        SelectEnd,
        SelectAll,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        SplitBlock,
        Indent,
        Outdent,
        Dismiss,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        ToggleBold,
        ToggleItalic,
        ToggleStrike,
        ToggleCode,
        MoveBlockUp,
        MoveBlockDown,
        DuplicateBlock,
        RemoveBlock,
        ConfirmUrl,
        CancelUrl,
        LeaveEmbed,
        EmbedFocusNext,
        EmbedFocusPrev,
    ]
);

/// Bind each chord in the editor, and disable it inside a hosted embed so the
/// embed's own control (a spreadsheet grid, an input) receives it.
macro_rules! editor_keys {
    ($cx:expr, $($chord:literal => $action:expr),* $(,)?) => {{
        $cx.bind_keys([$(KeyBinding::new($chord, $action, Some(CONTEXT))),*]);
        $cx.bind_keys([$(KeyBinding::new($chord, NoAction, Some(EMBED_CONTEXT))),*]);
    }};
}

/// Install the editor's key bindings.
pub fn init(cx: &mut App) {
    editor_keys!(cx,
        "backspace" => Backspace,
        "delete" => Delete,
        "left" => Left,
        "right" => Right,
        "up" => Up,
        "down" => Down,
        "home" => Home,
        "end" => End,
        "shift-left" => SelectLeft,
        "shift-right" => SelectRight,
        "shift-up" => SelectUp,
        "shift-down" => SelectDown,
        "shift-home" => SelectHome,
        "shift-end" => SelectEnd,
        "enter" => SplitBlock,
        "escape" => Dismiss,
    );

    let prompt = Some(image::PROMPT_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmUrl, prompt),
        KeyBinding::new("escape", CancelUrl, prompt),
    ]);

    // MoveBlockUp/Down, DuplicateBlock, RemoveBlock deliberately unbound —
    // reach them from the block menu / host keymap.

    #[cfg(target_os = "macos")]
    editor_keys!(cx,
        "cmd-a" => SelectAll,
        "cmd-c" => Copy,
        "cmd-x" => Cut,
        "cmd-v" => Paste,
        // Undo/redo before gpui-base input UndoManager can claim these.
        "cmd-z" => Undo,
        "cmd-shift-z" => Redo,
        "cmd-b" => ToggleBold,
        "cmd-i" => ToggleItalic,
        "cmd-e" => ToggleCode,
        "cmd-shift-x" => ToggleStrike,
        "cmd-left" => Home,
        "cmd-right" => End,
        "cmd-shift-left" => SelectHome,
        "cmd-shift-right" => SelectEnd,
        "alt-left" => WordLeft,
        "alt-right" => WordRight,
        "alt-shift-left" => SelectWordLeft,
        "alt-shift-right" => SelectWordRight,
        "ctrl-a" => Home,
        "ctrl-e" => End,
        "ctrl-b" => Left,
        "ctrl-f" => Right,
        "ctrl-n" => Down,
        "ctrl-p" => Up,
        "ctrl-h" => Backspace,
        "ctrl-d" => Delete,
        "ctrl-k" => KillLine,
        "alt-backspace" => DeleteWordLeft,
        "alt-delete" => DeleteWordRight,
        "cmd-backspace" => DeleteToHome,
    );

    #[cfg(not(target_os = "macos"))]
    editor_keys!(cx,
        "ctrl-a" => SelectAll,
        "ctrl-c" => Copy,
        "ctrl-x" => Cut,
        "ctrl-v" => Paste,
        "ctrl-z" => Undo,
        "ctrl-shift-z" => Redo,
        "ctrl-b" => ToggleBold,
        "ctrl-i" => ToggleItalic,
        "ctrl-e" => ToggleCode,
        "ctrl-shift-x" => ToggleStrike,
        "ctrl-left" => WordLeft,
        "ctrl-right" => WordRight,
        "ctrl-shift-left" => SelectWordLeft,
        "ctrl-shift-right" => SelectWordRight,
        "ctrl-backspace" => DeleteWordLeft,
        "ctrl-delete" => DeleteWordRight,
    );

    // Inside an embed Tab moves focus on, as it does outside the editor.
    cx.bind_keys([
        KeyBinding::new("tab", Indent, Some(CONTEXT)),
        KeyBinding::new("shift-tab", Outdent, Some(CONTEXT)),
        KeyBinding::new("tab", EmbedFocusNext, Some(EMBED_CONTEXT)),
        KeyBinding::new("shift-tab", EmbedFocusPrev, Some(EMBED_CONTEXT)),
    ]);

    // Last, so it outranks the embed's disabled escape.
    cx.bind_keys([KeyBinding::new("escape", LeaveEmbed, Some(EMBED_CONTEXT))]);
}
