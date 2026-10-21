//! Slash menu catalog and query filter (Bezel `slash.rs`).
//!
//! Pure data — no window. The Editor entity (Task 4) owns scroll / glass paint.

use block_model::Cursor;
pub use block_model::slash::{Filter, filter_indices};
use gpui::App;

use crate::registry::{SlashAction, SlashCommand, SlashRegistry};

/// Built-in `/` rows as `(label, action)`.
#[must_use]
pub fn items() -> Vec<(String, SlashAction)> {
    SlashRegistry::default_commands()
        .into_iter()
        .map(|command| (command.label.to_string(), command.action))
        .collect()
}

/// An open slash menu: where the `/` sits, and the ranked list under it.
#[derive(Debug, Clone)]
pub struct Slash {
    /// The `/` itself. Everything between it and the caret is the query.
    pub at: Cursor,
    pub filter: Filter,
    commands: Vec<SlashCommand>,
}

impl Slash {
    #[must_use]
    pub fn open(at: Cursor) -> Self {
        Self::open_with(at, SlashRegistry::default_commands())
    }

    #[must_use]
    pub fn open_in(at: Cursor, cx: &App) -> Self {
        Self::open_with(at, SlashRegistry::commands_in(cx))
    }

    #[must_use]
    pub fn open_with(at: Cursor, commands: Vec<SlashCommand>) -> Self {
        let labels = commands
            .iter()
            .map(|command| command.label.to_string())
            .collect();
        Self {
            at,
            filter: Filter::new(labels),
            commands,
        }
    }

    pub fn refilter(&mut self, query: &str) {
        self.filter.refilter(query);
    }

    pub fn step(&mut self, delta: isize) {
        self.filter.step(delta);
    }

    /// The command confirming right now would run.
    #[must_use]
    pub fn choice(&self) -> Option<SlashAction> {
        let ix = self.filter.active_item()?;
        self.commands.get(ix).map(|command| command.action.clone())
    }

    #[must_use]
    pub fn commands(&self) -> &[SlashCommand] {
        &self.commands
    }

    /// Text typed since the `/`, or `None` when the caret has left the run
    /// (closes the menu). A space ends the query.
    #[must_use]
    pub fn query(&self, caret: Cursor, text: &str) -> Option<String> {
        if caret.id != self.at.id || caret.part != self.at.part {
            return None;
        }
        let start = self.at.offset + 1;
        if caret.offset < start {
            return None;
        }
        let query = text.get(start..caret.offset)?;
        (!query.contains(char::is_whitespace)).then(|| query.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use block_model::Part;

    #[test]
    fn items_lock_list_no_bookmark() {
        let labels: Vec<_> = items().into_iter().map(|(l, _)| l).collect();
        assert_eq!(
            labels,
            [
                "Text",
                "Heading 1",
                "Heading 2",
                "Heading 3",
                "Bullet",
                "Numbered",
                "Task",
                "Quote",
                "Code",
                "Markdown",
                "Table",
                "Image",
                "Divider",
            ]
        );
        assert!(!labels.iter().any(|l| l.eq_ignore_ascii_case("bookmark")));
    }

    #[test]
    fn slash_query_and_refilter() {
        let at = Cursor::new("b1".into(), Part::Body, 0);
        let mut slash = Slash::open(at.clone());
        assert_eq!(
            slash.query(Cursor::new("b1".into(), Part::Body, 1), "/"),
            Some(String::new())
        );
        assert_eq!(
            slash.query(Cursor::new("b1".into(), Part::Body, 3), "/he"),
            Some("he".into())
        );
        assert_eq!(
            slash.query(Cursor::new("b1".into(), Part::Body, 4), "/he "),
            None,
            "space closes"
        );
        slash.refilter("hea");
        assert_eq!(
            slash.choice(),
            Some(SlashAction::Insert(block_markdown::BlockType::Heading {
                level: 1
            }))
        );
        slash.step(1);
        assert_eq!(
            slash.choice(),
            Some(SlashAction::Insert(block_markdown::BlockType::Heading {
                level: 2
            }))
        );
    }
}
