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

    pub fn init(cx: &mut App) {
        if !cx.has_global::<Self>() {
            cx.set_global(Self {
                marks: Self::default_marks(),
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
    fn insert_custom_is_a_first_class_block_type() {
        let command = SlashCommand::insert_custom("callout", "Callout", "callout");
        assert_eq!(
            command.action,
            SlashAction::Insert(BlockType::Custom("callout".into()))
        );
    }
}
