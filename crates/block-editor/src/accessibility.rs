//! Accessible text projection for the custom-painted editor.

use block_markdown::{BlockId, BlockType};
use gpui::A11ySubtreeBuilder;
use gpui::accesskit::{Node, NodeId, Role, TextPosition, TextSelection};
use gpui_component_block_view::{Cursor, Part, Selection};

use crate::editor::Editor;

#[derive(Clone, Debug)]
struct AccessibilityRun {
    id: BlockId,
    part: Part,
    text: String,
}

/// Text runs and selection written by the editor's synthetic accessibility tree.
#[derive(Clone, Debug)]
pub(crate) struct AccessibilityText {
    runs: Vec<AccessibilityRun>,
    selection: Selection,
}

impl AccessibilityText {
    pub(crate) fn from_editor(editor: &Editor) -> Self {
        let mut runs = Vec::new();
        for snapshot in editor.snapshots() {
            if let Some(table) = &snapshot.table {
                for (row, cells) in table.rows.iter().enumerate() {
                    for (column, cell) in cells.iter().enumerate() {
                        let part = Part::Cell { row, column };
                        let cursor = Cursor::new(snapshot.id.clone(), part, 0);
                        runs.push(AccessibilityRun {
                            id: snapshot.id.clone(),
                            part,
                            text: editor.project_plain(&cursor, cell),
                        });
                    }
                }
                continue;
            }

            let part = match snapshot.block_type {
                BlockType::Code => Part::Code,
                BlockType::Image => Part::Caption,
                _ => Part::Body,
            };
            let cursor = Cursor::new(snapshot.id.clone(), part, 0);
            runs.push(AccessibilityRun {
                id: snapshot.id.clone(),
                part,
                text: editor.project_plain(&cursor, &snapshot.plain),
            });
        }

        Self {
            runs,
            selection: editor.displayed_selection(),
        }
    }

    pub(crate) fn write(self, builder: &mut A11ySubtreeBuilder<'_>) {
        let ids = self
            .runs
            .iter()
            .map(|run| builder.synthetic_node_id((&run.id.0, run.part)))
            .collect::<Vec<_>>();
        let (nodes, selection) = self.materialize(&ids);
        for (id, node) in nodes {
            builder.push_child(id, node);
        }
        if let Some(selection) = selection {
            builder.parent_node().set_text_selection(selection);
        }
    }

    pub(crate) fn materialize(
        &self,
        ids: &[NodeId],
    ) -> (Vec<(NodeId, Node)>, Option<TextSelection>) {
        let nodes = self
            .runs
            .iter()
            .zip(ids.iter().copied())
            .map(|(run, id)| (id, text_run_node(&run.text)))
            .collect();
        let selection = self.text_selection(ids);
        (nodes, selection)
    }

    fn text_selection(&self, ids: &[NodeId]) -> Option<TextSelection> {
        Some(TextSelection {
            anchor: self.position(&self.selection.anchor, ids)?,
            focus: self.position(&self.selection.focus, ids)?,
        })
    }

    fn position(&self, cursor: &Cursor, ids: &[NodeId]) -> Option<TextPosition> {
        let index = self
            .runs
            .iter()
            .position(|run| run.id == cursor.id && run.part == cursor.part)
            .or_else(|| self.runs.iter().position(|run| run.id == cursor.id))?;
        let text = &self.runs.get(index)?.text;
        let mut byte_offset = cursor.offset.min(text.len());
        while !text.is_char_boundary(byte_offset) {
            byte_offset -= 1;
        }
        Some(TextPosition {
            node: *ids.get(index)?,
            character_index: text[..byte_offset].chars().count(),
        })
    }
}

fn text_run_node(text: &str) -> Node {
    let mut node = Node::new(Role::TextRun);
    node.set_value(text);
    node.set_character_lengths(
        text.chars()
            .map(|character| character.len_utf8() as u8)
            .collect::<Vec<_>>(),
    );
    node
}
