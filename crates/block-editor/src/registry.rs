//! App-global mark and slash-command registries.
//!
//! Built-ins match the historical closed catalog. Hosts call
//! [`MarkRegistry::register`] / [`SlashRegistry::register`] after
//! [`crate::init`] (or before creating a document) to add app-specific marks
//! and commands.

use gpui::{App, Global, SharedString};

use block_markdown::{BlockType, MarkExpand, MarkStyle};

/// How a slash confirmation is carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashAction {
    /// Change the current block to a built-in type.
    Insert(BlockType),
    /// Insert a code block and set its fence language (including `markdown`).
    InsertCode { language: SharedString },
    /// Emit [`crate::EditorEvent::SlashCustom`] with this id.
    Custom(SharedString),
}

/// One `/` menu row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommand {
    pub id: SharedString,
    pub label: SharedString,
    pub action: SlashAction,
}

impl SlashCommand {
    #[must_use]
    pub fn insert(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        kind: BlockType,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            action: SlashAction::Insert(kind),
        }
    }

    /// Insert an app-defined block type (`BlockType::Custom`).
    #[must_use]
    pub fn insert_custom(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        name: impl Into<String>,
    ) -> Self {
        Self::insert(id, label, BlockType::Custom(name.into()))
    }
}

/// Slash catalog installed on [`App`].
#[derive(Debug, Clone)]
pub struct SlashRegistry {
    commands: Vec<SlashCommand>,
}

impl Global for SlashRegistry {}

impl SlashRegistry {
    /// Built-in `/` catalog (bookmark is omitted; it needs a URL).
    #[must_use]
    pub fn default_commands() -> Vec<SlashCommand> {
        vec![
            SlashCommand::insert("paragraph", "Text", BlockType::Paragraph),
            SlashCommand::insert("h1", "Heading 1", BlockType::Heading { level: 1 }),
            SlashCommand::insert("h2", "Heading 2", BlockType::Heading { level: 2 }),
            SlashCommand::insert("h3", "Heading 3", BlockType::Heading { level: 3 }),
            SlashCommand::insert("bullet", "Bullet", BlockType::Bullet),
            SlashCommand::insert("ordered", "Numbered", BlockType::Ordered),
            SlashCommand::insert("task", "Task", BlockType::Task),
            SlashCommand::insert("quote", "Quote", BlockType::Quote),
            SlashCommand::insert("code", "Code", BlockType::Code),
            SlashCommand {
                id: "markdown".into(),
                label: "Markdown".into(),
                action: SlashAction::InsertCode {
                    language: "markdown".into(),
                },
            },
            SlashCommand::insert("table", "Table", BlockType::Table),
            SlashCommand::insert("image", "Image", BlockType::Image),
            SlashCommand::insert("rule", "Divider", BlockType::Rule),
        ]
    }

    #[must_use]
    pub fn notes_commands() -> Vec<SlashCommand> {
        vec![
            SlashCommand::insert("paragraph", "Text", BlockType::Paragraph),
            SlashCommand::insert("h1", "Heading 1", BlockType::Heading { level: 1 }),
            SlashCommand::insert("h2", "Heading 2", BlockType::Heading { level: 2 }),
            SlashCommand::insert("h3", "Heading 3", BlockType::Heading { level: 3 }),
        ]
    }

    #[must_use]
    pub fn editor_commands() -> Vec<SlashCommand> {
        vec![
            SlashCommand::insert("paragraph", "Text", BlockType::Paragraph),
            SlashCommand::insert("h1", "Heading 1", BlockType::Heading { level: 1 }),
            SlashCommand::insert("h2", "Heading 2", BlockType::Heading { level: 2 }),
            SlashCommand::insert("h3", "Heading 3", BlockType::Heading { level: 3 }),
            SlashCommand::insert("h4", "Heading 4", BlockType::Heading { level: 4 }),
            SlashCommand::insert("h5", "Heading 5", BlockType::Heading { level: 5 }),
            SlashCommand::insert("h6", "Heading 6", BlockType::Heading { level: 6 }),
            SlashCommand::insert("bullet", "Bullet", BlockType::Bullet),
            SlashCommand::insert("ordered", "Numbered", BlockType::Ordered),
            SlashCommand::insert("task", "Task", BlockType::Task),
            SlashCommand::insert("toggle", "Toggle", BlockType::Custom("toggle".into())),
            SlashCommand::insert("quote", "Quote", BlockType::Quote),
            SlashCommand::insert("code", "Code", BlockType::Code),
            SlashCommand {
                id: "markdown".into(),
                label: "Markdown".into(),
                action: SlashAction::InsertCode {
                    language: "markdown".into(),
                },
            },
            SlashCommand::insert("table", "Table", BlockType::Table),
            SlashCommand::insert("rule", "Divider", BlockType::Rule),
            SlashCommand::insert("bookmark", "Bookmark", BlockType::Bookmark),
            SlashCommand::insert_custom("callout", "Callout", "callout"),
        ]
    }

    /// Replace the catalog (hosts install the enabled subset after [`crate::init`]).
    pub fn set_commands(cx: &mut App, commands: Vec<SlashCommand>) {
        Self::global_mut(cx).commands = commands;
    }

    /// Host entry used by the notes window. G-EDITOR replaces the paragraph/heading
    /// subset with the editor catalog; image and reference slash stay unregistered.
    pub fn install_notes(cx: &mut App) {
        Self::install_editor(cx);
    }

    /// G-EDITOR slash catalog: built-in text kinds without image or references.
    /// The notes window host still calls [`Self::install_notes`]; after G-EDITOR
    /// that entry installs this catalog so later-gate image/mention stay off.
    pub fn install_editor(cx: &mut App) {
        Self::set_commands(cx, Self::editor_commands());
    }

    pub fn init(cx: &mut App) {
        if !cx.has_global::<Self>() {
            cx.set_global(Self {
                commands: Self::default_commands(),
            });
        }
    }

    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    pub fn global_mut(cx: &mut App) -> &mut Self {
        if !cx.has_global::<Self>() {
            Self::init(cx);
        }
        cx.global_mut::<Self>()
    }

    pub fn register(cx: &mut App, command: SlashCommand) {
        let registry = Self::global_mut(cx);
        if let Some(existing) = registry
            .commands
            .iter_mut()
            .find(|item| item.id == command.id)
        {
            *existing = command;
        } else {
            registry.commands.push(command);
        }
    }

    #[must_use]
    pub fn commands(&self) -> &[SlashCommand] {
        &self.commands
    }

    #[must_use]
    pub fn commands_in(cx: &App) -> Vec<SlashCommand> {
        cx.try_global::<Self>()
            .map(|registry| registry.commands.clone())
            .unwrap_or_else(Self::default_commands)
    }
}

/// One Loro text-style the editor will configure and the toolbar may toggle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkSpec {
    pub key: SharedString,
    pub expand: MarkExpand,
    pub label: SharedString,
    pub builtin: bool,
}

impl MarkSpec {
    #[must_use]
    pub fn style(&self) -> MarkStyle {
        MarkStyle {
            key: self.key.to_string(),
            expand: self.expand,
        }
    }
}

/// Mark catalog installed on [`App`].
#[derive(Debug, Clone)]
pub struct MarkRegistry {
    marks: Vec<MarkSpec>,
}

impl Global for MarkRegistry {}

impl MarkRegistry {
    #[must_use]
    pub fn default_marks() -> Vec<MarkSpec> {
        MarkStyle::defaults()
            .into_iter()
            .map(|style| MarkSpec {
                builtin: matches!(style.key.as_str(), "bold" | "italic" | "strike" | "code"),
                label: style.key.clone().into(),
                key: style.key.into(),
                expand: style.expand,
            })
            .collect()
    }

    /// Ordinary styling registered at G-EDITOR. Passage/highlight stays later.
    #[must_use]
    pub fn editor_marks() -> Vec<MarkSpec> {
        Self::default_marks()
            .into_iter()
            .filter(|spec| matches!(spec.key.as_str(), "bold" | "italic" | "strike" | "code"))
            .collect()
    }

    pub fn init(cx: &mut App) {
        if !cx.has_global::<Self>() {
            cx.set_global(Self {
                marks: Self::default_marks(),
            });
        }
    }

    pub fn set_marks(cx: &mut App, marks: Vec<MarkSpec>) {
        Self::global_mut(cx).marks = marks;
    }

    /// Restrict marks to bold, italic, strike, and inline code.
    pub fn install_editor(cx: &mut App) {
        Self::set_marks(cx, Self::editor_marks());
    }

    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    pub fn global_mut(cx: &mut App) -> &mut Self {
        if !cx.has_global::<Self>() {
            Self::init(cx);
        }
        cx.global_mut::<Self>()
    }

    pub fn register(cx: &mut App, spec: MarkSpec) {
        let registry = Self::global_mut(cx);
        if let Some(existing) = registry.marks.iter_mut().find(|item| item.key == spec.key) {
            *existing = spec;
        } else {
            registry.marks.push(spec);
        }
    }

    #[must_use]
    pub fn marks(&self) -> &[MarkSpec] {
        &self.marks
    }

    #[must_use]
    pub fn styles_in(cx: &App) -> Vec<MarkStyle> {
        cx.try_global::<Self>()
            .map(|registry| registry.marks.iter().map(MarkSpec::style).collect())
            .unwrap_or_else(MarkStyle::defaults)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use block_markdown::BlockType;

    #[test]
    fn default_slash_catalog_includes_markdown() {
        let labels: Vec<_> = SlashRegistry::default_commands()
            .into_iter()
            .map(|command| command.label.to_string())
            .collect();
        assert!(labels.contains(&"Markdown".to_string()));
        assert!(labels.contains(&"Text".to_string()));
        assert!(
            !labels
                .iter()
                .any(|label| label.eq_ignore_ascii_case("bookmark"))
        );
    }

    #[test]
    fn default_marks_include_emphasis() {
        let keys: Vec<_> = MarkRegistry::default_marks()
            .into_iter()
            .map(|spec| spec.key.to_string())
            .collect();
        assert!(keys.contains(&"bold".to_string()));
        assert!(keys.contains(&"comment".to_string()));
    }

    #[test]
    fn insert_command_keeps_block_type() {
        let command = SlashCommand::insert("task", "Task", BlockType::Task);
        assert_eq!(command.action, SlashAction::Insert(BlockType::Task));
    }

    #[test]
    fn notes_slash_catalog_is_paragraph_and_heading_only() {
        let ids: Vec<_> = SlashRegistry::notes_commands()
            .into_iter()
            .map(|command| command.id.to_string())
            .collect();
        assert_eq!(ids, ["paragraph", "h1", "h2", "h3"]);
        assert!(SlashRegistry::default_commands().len() > ids.len());
    }

    #[test]
    fn editor_slash_catalog_covers_g_editor_kinds_without_image() {
        let ids: Vec<_> = SlashRegistry::editor_commands()
            .into_iter()
            .map(|command| command.id.to_string())
            .collect();
        assert!(ids.contains(&"code".to_string()));
        assert!(ids.contains(&"quote".to_string()));
        assert!(ids.contains(&"bullet".to_string()));
        assert!(ids.contains(&"ordered".to_string()));
        assert!(ids.contains(&"task".to_string()));
        assert!(ids.contains(&"toggle".to_string()));
        assert!(ids.contains(&"table".to_string()));
        assert!(ids.contains(&"bookmark".to_string()));
        assert!(ids.contains(&"callout".to_string()));
        assert!(ids.contains(&"markdown".to_string()));
        assert!(ids.contains(&"h6".to_string()));
        assert!(!ids.iter().any(|id| id == "image"));
        assert!(!ids.iter().any(|id| id == "mention"));
    }

    #[test]
    fn editor_marks_are_ordinary_styling() {
        let keys: Vec<_> = MarkRegistry::editor_marks()
            .into_iter()
            .map(|spec| spec.key.to_string())
            .collect();
        assert_eq!(keys, ["bold", "italic", "strike", "code"]);
    }

    #[test]
    fn insert_custom_is_a_first_class_block_type() {
        let command = SlashCommand::insert_custom("callout", "Callout", "callout");
        assert_eq!(
            command.action,
            SlashAction::Insert(BlockType::Custom("callout".into()))
        );
    }
}
