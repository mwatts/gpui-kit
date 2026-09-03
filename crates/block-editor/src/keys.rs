//! The keymap: every action the editor answers to, and the chords bound to it.
//!
//! Ported from Bezel `editor/keys.rs`. Scoped to [`CONTEXT`] so Tab indent does
//! not leak into the rest of the app. Undo/redo are bound here **before** any
//! gpui-base input undo can claim the chord.
//!
//! Copyright (c) Bezel contributors. MIT. See crate `NOTICE`.

use gpui::{App, KeyBinding, actions};

use crate::image;

/// Key context for the block editor surface.
pub const CONTEXT: &str = "LimenBlockEditor";

actions!(
    limen_block_editor,
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
    ]
);

/// Install the editor's key bindings.
pub fn init(cx: &mut App) {
    let ctx = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("left", Left, ctx),
        KeyBinding::new("right", Right, ctx),
        KeyBinding::new("up", Up, ctx),
        KeyBinding::new("down", Down, ctx),
        KeyBinding::new("home", Home, ctx),
        KeyBinding::new("end", End, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("shift-up", SelectUp, ctx),
        KeyBinding::new("shift-down", SelectDown, ctx),
        KeyBinding::new("shift-home", SelectHome, ctx),
        KeyBinding::new("shift-end", SelectEnd, ctx),
        KeyBinding::new("enter", SplitBlock, ctx),
        KeyBinding::new("tab", Indent, ctx),
        KeyBinding::new("shift-tab", Outdent, ctx),
        KeyBinding::new("escape", Dismiss, ctx),
    ]);

    let prompt = Some(image::PROMPT_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmUrl, prompt),
        KeyBinding::new("escape", CancelUrl, prompt),
    ]);

    // MoveBlockUp/Down, DuplicateBlock, RemoveBlock deliberately unbound —
    // reach them from the block menu / host keymap.

    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-a", SelectAll, ctx),
        KeyBinding::new("cmd-c", Copy, ctx),
        KeyBinding::new("cmd-x", Cut, ctx),
        KeyBinding::new("cmd-v", Paste, ctx),
        // Undo/redo before gpui-base input UndoManager can claim these.
        KeyBinding::new("cmd-z", Undo, ctx),
        KeyBinding::new("cmd-shift-z", Redo, ctx),
        KeyBinding::new("cmd-b", ToggleBold, ctx),
        KeyBinding::new("cmd-i", ToggleItalic, ctx),
        KeyBinding::new("cmd-e", ToggleCode, ctx),
        KeyBinding::new("cmd-shift-x", ToggleStrike, ctx),
        KeyBinding::new("cmd-left", Home, ctx),
        KeyBinding::new("cmd-right", End, ctx),
        KeyBinding::new("cmd-shift-left", SelectHome, ctx),
        KeyBinding::new("cmd-shift-right", SelectEnd, ctx),
        KeyBinding::new("alt-left", WordLeft, ctx),
        KeyBinding::new("alt-right", WordRight, ctx),
        KeyBinding::new("alt-shift-left", SelectWordLeft, ctx),
        KeyBinding::new("alt-shift-right", SelectWordRight, ctx),
        KeyBinding::new("ctrl-a", Home, ctx),
        KeyBinding::new("ctrl-e", End, ctx),
        KeyBinding::new("ctrl-b", Left, ctx),
        KeyBinding::new("ctrl-f", Right, ctx),
        KeyBinding::new("ctrl-n", Down, ctx),
        KeyBinding::new("ctrl-p", Up, ctx),
        KeyBinding::new("ctrl-h", Backspace, ctx),
        KeyBinding::new("ctrl-d", Delete, ctx),
        KeyBinding::new("ctrl-k", KillLine, ctx),
        KeyBinding::new("alt-backspace", DeleteWordLeft, ctx),
        KeyBinding::new("alt-delete", DeleteWordRight, ctx),
        KeyBinding::new("cmd-backspace", DeleteToHome, ctx),
    ]);

    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-a", SelectAll, ctx),
        KeyBinding::new("ctrl-c", Copy, ctx),
        KeyBinding::new("ctrl-x", Cut, ctx),
        KeyBinding::new("ctrl-v", Paste, ctx),
        KeyBinding::new("ctrl-z", Undo, ctx),
        KeyBinding::new("ctrl-shift-z", Redo, ctx),
        KeyBinding::new("ctrl-b", ToggleBold, ctx),
        KeyBinding::new("ctrl-i", ToggleItalic, ctx),
        KeyBinding::new("ctrl-e", ToggleCode, ctx),
        KeyBinding::new("ctrl-shift-x", ToggleStrike, ctx),
        KeyBinding::new("ctrl-left", WordLeft, ctx),
        KeyBinding::new("ctrl-right", WordRight, ctx),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, ctx),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, ctx),
        KeyBinding::new("ctrl-backspace", DeleteWordLeft, ctx),
        KeyBinding::new("ctrl-delete", DeleteWordRight, ctx),
    ]);
}
