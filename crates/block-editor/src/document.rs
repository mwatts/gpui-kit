//! [`BlockDocument`]: apply → commit → project, with Loro undo.

use block_markdown::{hydrate, new_empty_doc};
use loro::{ExportMode, LoroDoc, LoroResult, UndoManager};

use crate::apply;
use crate::project;
use crate::types::{ApplyResult, BlockOp};
use gpui_component_block_view::BlockSnapshot;

const MERGE_INTERVAL_MS: i64 = 500;

/// Loro-backed block document. View paints [`Self::snapshots`] only.
pub struct BlockDocument {
    doc: LoroDoc,
    undo: UndoManager,
    snapshots: Vec<BlockSnapshot>,
}

impl BlockDocument {
    /// Empty document: one paragraph.
    #[must_use]
    pub fn new() -> Self {
        let doc = new_empty_doc();
        Self::from_loro(doc)
    }

    /// Wrap an existing Loro doc (styles should already be configured).
    #[must_use]
    pub fn from_loro(doc: LoroDoc) -> Self {
        let mut undo = UndoManager::new(&doc);
        undo.set_max_undo_steps(100);
        undo.set_merge_interval(MERGE_INTERVAL_MS);
        let snapshots = project::project(&doc);
        Self {
            doc,
            undo,
            snapshots,
        }
    }

    /// Hydrate from markdown.
    #[must_use]
    pub fn from_markdown(source: &str) -> Self {
        Self::from_loro(hydrate(source))
    }

    #[must_use]
    pub fn doc(&self) -> &LoroDoc {
        &self.doc
    }

    #[must_use]
    pub fn snapshots(&self) -> &[BlockSnapshot] {
        &self.snapshots
    }

    /// Apply one op, commit, re-project.
    pub fn apply(&mut self, op: BlockOp) -> ApplyResult {
        self.apply_many(std::slice::from_ref(&op))
    }

    /// Perform every op, then **one** commit, then project.
    pub fn apply_many(&mut self, ops: &[BlockOp]) -> ApplyResult {
        if ops.is_empty() {
            return ApplyResult::default();
        }
        let structure = ops.iter().any(BlockOp::is_structure);
        let mut last = ApplyResult::default();
        for op in ops {
            match apply::perform(&self.doc, op.clone()) {
                Ok(result) => last = result,
                Err(err) => {
                    eprintln!("block-editor apply error: {err}");
                }
            }
        }
        self.commit(structure);
        self.snapshots = project::project(&self.doc);
        last
    }

    fn commit(&mut self, structure: bool) {
        if structure {
            // Structure must not merge into the previous typing group.
            self.undo.set_merge_interval(0);
            self.doc.set_next_commit_origin("structure:");
        } else {
            self.doc.set_next_commit_origin("typing");
        }
        self.doc.commit();
        if structure {
            self.undo.set_merge_interval(MERGE_INTERVAL_MS);
        }
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.undo() {
            Ok(did) => {
                if did {
                    self.snapshots = project::project(&self.doc);
                }
                did
            }
            Err(_) => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.undo.redo() {
            Ok(did) => {
                if did {
                    self.snapshots = project::project(&self.doc);
                }
                did
            }
            Err(_) => false,
        }
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    /// Cap the undo stack (default 100).
    pub fn set_undo_limit(&mut self, limit: usize) {
        self.undo.set_max_undo_steps(limit);
    }

    #[must_use]
    pub fn project(&self) -> Vec<BlockSnapshot> {
        project::project(&self.doc)
    }

    pub fn export_snapshot(&self) -> Result<Vec<u8>, loro::LoroEncodeError> {
        self.doc.export(ExportMode::Snapshot)
    }

    /// Import onto a **fresh** document (avoids duplicating the starter paragraph).
    pub fn import_snapshot(bytes: &[u8]) -> LoroResult<Self> {
        let doc = LoroDoc::new();
        block_markdown::configure_text_styles(&doc);
        doc.import(bytes)?;
        Ok(Self::from_loro(doc))
    }

    #[must_use]
    pub fn project_markdown(&self) -> String {
        block_markdown::project_markdown(&self.doc)
    }
}

impl Default for BlockDocument {
    fn default() -> Self {
        Self::new()
    }
}
