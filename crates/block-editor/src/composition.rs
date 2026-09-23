//! Per-object composition bridge.
//!
//! The stock editor stores one page in one [`LoroDoc`]. Ashlar compositions
//! are independent content objects plus Child occurrences. This module
//! assembles those objects for paint and decomposes [`BlockOp`] edits into
//! draft-shaped change sets. The assembled view is not an authority document.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use block_markdown::{
    BlockId, BlockType, blocks_list, configure_text_styles, ensure_content, find_block,
    insert_block_map, replay_delta_at, slice_delta_utf8,
};
use gpui_component_block_view::{BlockSnapshot, Cursor, Part, Selection};
use loro::{LoroDoc, TextDelta};

use crate::document::BlockDocument;
use crate::types::{ApplyResult, BlockOp};

/// Relation type name for ordered Child occurrences.
pub const CHILD_TYPE: &str = "ashlar.Child";

/// Stable object identity. [`BlockId`] is a semantic subtype of this string.
pub type ObjectId = String;

/// Opaque persisted revision of one object.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectVersion(pub String);

/// Which commands the bridge will carry out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorGate {
    /// Paragraph and heading only. Other kinds keep their bytes and stay read-only.
    #[default]
    Notes,
    /// Built-in text kinds, marks, language, and nested Child placement.
    /// Image, unknown custom, mentions, and media stay read-only.
    Editor,
}

impl EditorGate {
    #[must_use]
    pub fn content_writable(self, kind: &BlockType) -> bool {
        match self {
            Self::Notes => matches!(kind, BlockType::Paragraph | BlockType::Heading { .. }),
            Self::Editor => editor_kind_writable(kind),
        }
    }

    #[must_use]
    pub fn allows(self, op: &BlockOp) -> bool {
        match self {
            Self::Notes => matches!(
                op,
                BlockOp::InsertText { .. }
                    | BlockOp::DeleteRange { .. }
                    | BlockOp::ImeCommit { .. }
                    | BlockOp::SplitBlock { .. }
                    | BlockOp::MergeWithPrevious { .. }
                    | BlockOp::DeleteBlock { .. }
                    | BlockOp::Move { .. }
                    | BlockOp::SetType { .. }
                    | BlockOp::UnwrapToParagraph { .. }
            ),
            Self::Editor => match op {
                BlockOp::InsertText { .. }
                | BlockOp::DeleteRange { .. }
                | BlockOp::ImeCommit { .. }
                | BlockOp::SplitBlock { .. }
                | BlockOp::MergeWithPrevious { .. }
                | BlockOp::DeleteBlock { .. }
                | BlockOp::Move { .. }
                | BlockOp::SetType { .. }
                | BlockOp::UnwrapToParagraph { .. }
                | BlockOp::ToggleMark { .. }
                | BlockOp::Indent { .. }
                | BlockOp::Outdent { .. }
                | BlockOp::SetProp { .. }
                | BlockOp::ToggleCheck { .. } => true,
                BlockOp::SetLink { .. }
                | BlockOp::RemoveLink { .. }
                | BlockOp::AddComment { .. }
                | BlockOp::SetCommentBody { .. }
                | BlockOp::SetCommentState { .. }
                | BlockOp::DeleteComment { .. }
                | BlockOp::DeleteCrossBlock { .. } => false,
            },
        }
    }
}

fn editor_kind_writable(kind: &BlockType) -> bool {
    match kind {
        BlockType::Image => false,
        BlockType::Custom(name) => matches!(name.as_str(), "toggle" | "callout"),
        BlockType::Paragraph
        | BlockType::Heading { .. }
        | BlockType::Bullet
        | BlockType::Ordered
        | BlockType::Task
        | BlockType::Quote
        | BlockType::Code
        | BlockType::Bookmark
        | BlockType::Table
        | BlockType::Rule => true,
    }
}

/// One Child placement of a content object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildOccurrence {
    pub relation_id: ObjectId,
    pub parent: ObjectId,
    pub child: ObjectId,
    pub slot: String,
    pub position: String,
    pub child_missing: bool,
    pub cycle_unresolved: bool,
}

/// One independently stored content object supplied by the host.
#[derive(Debug, Clone)]
pub struct CompositionObject {
    pub id: ObjectId,
    pub version: ObjectVersion,
    /// Lossless per-object Loro snapshot. Empty means a fresh object.
    pub bytes: Vec<u8>,
    pub kind_hint: Option<BlockType>,
}

/// Host-supplied composition to assemble.
#[derive(Debug, Clone)]
pub struct CompositionRead {
    pub root: ObjectId,
    pub root_type: String,
    pub version_tip: String,
    pub objects: BTreeMap<ObjectId, CompositionObject>,
    pub children: Vec<ChildOccurrence>,
    pub collection_versions: BTreeMap<(ObjectId, String), ObjectVersion>,
}

/// Where to put a Child occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceIntent {
    Append {
        parent: ObjectId,
        slot: String,
    },
    PlaceBefore {
        parent: ObjectId,
        slot: String,
        before: ObjectId,
    },
    PlaceAfter {
        parent: ObjectId,
        slot: String,
        after: ObjectId,
    },
}

/// Create or tombstone a relation object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationMutation {
    pub id: ObjectId,
    pub type_name: String,
    pub fields: BTreeMap<String, String>,
    pub tombstone: bool,
}

/// Expected object and collection versions for a conditional write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MutationReadSet {
    pub objects: BTreeMap<ObjectId, ObjectVersion>,
    pub collections: BTreeMap<(ObjectId, String), ObjectVersion>,
}

/// Changed per-object bytes and structural intents. Not a page snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionDraft {
    pub root: ObjectId,
    pub read_set: MutationReadSet,
    pub content: BTreeMap<ObjectId, Vec<u8>>,
    pub relations: Vec<RelationMutation>,
    pub placements: Vec<(ObjectId, PlaceIntent)>,
    pub new_images: BTreeMap<String, Vec<u8>>,
}

/// How new Block and Child identities are allocated.
#[derive(Debug, Clone, Default)]
pub enum IdSource {
    #[default]
    Uuid,
    /// Test / host-supplied identities. Falls back to UUID when a queue is empty.
    Queue {
        blocks: Vec<ObjectId>,
        children: Vec<ObjectId>,
    },
}

impl IdSource {
    fn alloc_block(&mut self) -> ObjectId {
        match self {
            Self::Uuid => uuid::Uuid::new_v4().to_string(),
            Self::Queue { blocks, .. } => {
                if blocks.is_empty() {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    blocks.remove(0)
                }
            }
        }
    }

    fn alloc_child(&mut self) -> ObjectId {
        match self {
            Self::Uuid => uuid::Uuid::new_v4().to_string(),
            Self::Queue { children, .. } => {
                if children.is_empty() {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    children.remove(0)
                }
            }
        }
    }
}

struct ContentSession {
    object_id: ObjectId,
    internal_id: BlockId,
    document: Option<BlockDocument>,
    intact_bytes: Vec<u8>,
    baseline: Vec<u8>,
    version: ObjectVersion,
    writable: bool,
    created: bool,
}

impl ContentSession {
    fn export_bytes(&self) -> Vec<u8> {
        if let Some(document) = &self.document {
            document
                .export_snapshot()
                .unwrap_or_else(|_| self.intact_bytes.clone())
        } else {
            self.intact_bytes.clone()
        }
    }

    fn dirty(&self) -> bool {
        self.created || self.export_bytes() != self.baseline
    }

    fn kind(&self) -> BlockType {
        self.document
            .as_ref()
            .and_then(|doc| doc.snapshots().first().map(|snap| snap.block_type.clone()))
            .unwrap_or(BlockType::Custom("unknown".into()))
    }

    fn snapshot(&self) -> BlockSnapshot {
        if let Some(document) = &self.document {
            if let Some(mut snap) = document.snapshots().first().cloned() {
                snap.id = BlockId(self.object_id.clone());
                snap.read_only = !self.writable;
                return snap;
            }
        }
        BlockSnapshot {
            id: BlockId(self.object_id.clone()),
            occurrence_id: None,
            block_type: BlockType::Custom("unknown".into()),
            indent: 0,
            plain: String::new(),
            runs: Vec::new(),
            props: HashMap::new(),
            checked: None,
            number: None,
            language: None,
            url: None,
            form: None,
            width: None,
            table: None,
            read_only: true,
        }
    }
}

#[derive(Clone)]
enum UndoEntry {
    Content(ObjectId),
    Structure(StructureUndo),
}

#[derive(Clone)]
enum StructureUndo {
    Split {
        prefix: ObjectId,
        _suffix: ObjectId,
        occurrence: ChildOccurrence,
        index: usize,
    },
    Join {
        survivor: ObjectId,
        restored: ChildOccurrence,
        index: usize,
    },
    Move {
        from: usize,
        to: usize,
    },
    Create {
        _content: ObjectId,
        occurrence: ChildOccurrence,
        index: usize,
    },
    Unlink {
        occurrence: ChildOccurrence,
        index: usize,
    },
    Reparent {
        old: ChildOccurrence,
        new: ChildOccurrence,
        old_index: usize,
        new_index: usize,
        subtree_len: usize,
    },
}

struct Resolved {
    content_id: ObjectId,
    occurrence_index: usize,
    relation_id: ObjectId,
}

/// Assembled composition: one content session per ObjectId, occurrence mapping
/// for paint and structure, per-object undo that survives a save flush.
pub struct CompositionSession {
    gate: EditorGate,
    root: ObjectId,
    slot: String,
    version_tip: String,
    objects: HashMap<ObjectId, ContentSession>,
    occurrences: Vec<ChildOccurrence>,
    collection_versions: BTreeMap<(ObjectId, String), ObjectVersion>,
    pending_relations: Vec<RelationMutation>,
    pending_placements: Vec<(ObjectId, PlaceIntent)>,
    undo_log: Vec<UndoEntry>,
    redo_log: Vec<UndoEntry>,
    snapshots: Vec<BlockSnapshot>,
    ids: IdSource,
}

impl CompositionSession {
    /// Assemble a composition. ObjectIds are session keys; Loro block ids inside
    /// imported bytes are mapped, not replaced.
    #[must_use]
    pub fn open(read: CompositionRead, gate: EditorGate, ids: IdSource) -> Self {
        let slot = read
            .children
            .first()
            .map(|child| child.slot.clone())
            .unwrap_or_else(|| "body".into());
        let mut objects = HashMap::new();
        for (id, object) in read.objects {
            objects.insert(id.clone(), load_object(object, gate));
        }
        for child in &read.children {
            objects.entry(child.child.clone()).or_insert_with(|| {
                let mut session = load_object(
                    CompositionObject {
                        id: child.child.clone(),
                        version: ObjectVersion(String::new()),
                        bytes: Vec::new(),
                        kind_hint: None,
                    },
                    gate,
                );
                session.writable = false;
                session
            });
        }
        let mut session = Self {
            gate,
            root: read.root,
            slot,
            version_tip: read.version_tip,
            objects,
            occurrences: read.children,
            collection_versions: read.collection_versions,
            pending_relations: Vec::new(),
            pending_placements: Vec::new(),
            undo_log: Vec::new(),
            redo_log: Vec::new(),
            snapshots: Vec::new(),
            ids,
        };
        session.reproject();
        session
    }

    #[must_use]
    pub fn gate(&self) -> EditorGate {
        self.gate
    }

    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    #[must_use]
    pub fn snapshots(&self) -> &[BlockSnapshot] {
        &self.snapshots
    }

    #[must_use]
    pub fn occurrences(&self) -> &[ChildOccurrence] {
        &self.occurrences
    }

    #[must_use]
    pub fn version_tip(&self) -> &str {
        &self.version_tip
    }

    #[must_use]
    pub fn allows(&self, op: &BlockOp) -> bool {
        self.gate.allows(op)
    }

    /// Apply a host [`BlockOp`]. `occurrence` disambiguates repeated placements.
    pub fn apply(&mut self, op: BlockOp, occurrence: Option<&BlockId>) -> ApplyResult {
        if !self.gate.allows(&op) {
            return ApplyResult::default();
        }
        match &op {
            BlockOp::SplitBlock { id, offset } => {
                let Some(resolved) = self.resolve(id, occurrence, true) else {
                    return ApplyResult::default();
                };
                self.split(resolved, *offset)
            }
            BlockOp::MergeWithPrevious { id } => {
                let Some(resolved) = self.resolve(id, occurrence, true) else {
                    return ApplyResult::default();
                };
                self.join(resolved)
            }
            BlockOp::DeleteBlock { id } => {
                let Some(resolved) = self.resolve(id, occurrence, true) else {
                    return ApplyResult::default();
                };
                self.unlink(resolved)
            }
            BlockOp::Move { id, to } => {
                let Some(resolved) = self.resolve(id, occurrence, true) else {
                    return ApplyResult::default();
                };
                self.reorder(resolved, *to)
            }
            BlockOp::SetType { id, kind } => {
                let Some(resolved) = self.resolve(id, occurrence, false) else {
                    return ApplyResult::default();
                };
                self.set_kind(resolved, kind.clone())
            }
            BlockOp::UnwrapToParagraph { id } => {
                let Some(resolved) = self.resolve(id, occurrence, false) else {
                    return ApplyResult::default();
                };
                self.set_kind(resolved, BlockType::Paragraph)
            }
            BlockOp::InsertText { .. }
            | BlockOp::DeleteRange { .. }
            | BlockOp::ImeCommit { .. }
            | BlockOp::ToggleMark { .. }
            | BlockOp::SetProp { .. }
            | BlockOp::ToggleCheck { .. } => {
                let Some(id) = op.target_id().cloned() else {
                    return ApplyResult::default();
                };
                let Some(resolved) = self.resolve(&id, occurrence, false) else {
                    return ApplyResult::default();
                };
                self.apply_text(resolved, op)
            }
            BlockOp::Indent { id } => {
                let Some(resolved) = self.resolve(id, occurrence, true) else {
                    return ApplyResult::default();
                };
                self.indent(resolved)
            }
            BlockOp::Outdent { id } => {
                let Some(resolved) = self.resolve(id, occurrence, true) else {
                    return ApplyResult::default();
                };
                self.outdent(resolved)
            }
            _ => ApplyResult::default(),
        }
    }

    /// Allocate a writable block and a Child occurrence.
    pub fn create(&mut self, kind: BlockType, after: Option<&str>) -> ApplyResult {
        if !self.gate.content_writable(&kind) {
            return ApplyResult::default();
        }
        let content_id = self.ids.alloc_block();
        let relation_id = self.ids.alloc_child();
        let document = new_block_document(&content_id, kind, &[]);
        let bytes = document.export_snapshot().unwrap_or_default();
        self.objects.insert(
            content_id.clone(),
            ContentSession {
                object_id: content_id.clone(),
                internal_id: BlockId(content_id.clone()),
                document: Some(document),
                intact_bytes: bytes.clone(),
                baseline: Vec::new(),
                version: ObjectVersion(String::new()),
                writable: true,
                created: true,
            },
        );
        let (parent, slot, index) = match after {
            Some(rel) => self
                .occurrences
                .iter()
                .position(|occ| occ.relation_id == rel)
                .map(|ix| {
                    let occ = &self.occurrences[ix];
                    (occ.parent.clone(), occ.slot.clone(), ix + 1)
                })
                .unwrap_or_else(|| (self.root.clone(), self.slot.clone(), self.occurrences.len())),
            None => (self.root.clone(), self.slot.clone(), self.occurrences.len()),
        };
        let occurrence = ChildOccurrence {
            relation_id: relation_id.clone(),
            parent,
            child: content_id.clone(),
            slot,
            position: String::new(),
            child_missing: false,
            cycle_unresolved: false,
        };
        self.occurrences.insert(index, occurrence.clone());
        let place = self.place_for_inserted(&occurrence, index);
        self.pending_relations
            .push(child_relation(&occurrence, false));
        self.pending_placements.push((relation_id.clone(), place));
        self.push_undo(UndoEntry::Structure(StructureUndo::Create {
            _content: content_id.clone(),
            occurrence,
            index,
        }));
        self.reproject();
        ApplyResult {
            selection: Some(Selection::caret(
                Cursor::new(BlockId(content_id), Part::Body, 0)
                    .with_occurrence(BlockId(relation_id)),
            )),
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo_log.pop() else {
            return false;
        };
        let did = match &entry {
            UndoEntry::Content(id) => self
                .objects
                .get_mut(id)
                .and_then(|session| session.document.as_mut())
                .is_some_and(BlockDocument::undo),
            UndoEntry::Structure(change) => {
                self.reverse_structure(change, false);
                true
            }
        };
        if did {
            self.redo_log.push(entry);
            self.reproject();
        }
        did
    }

    pub fn redo(&mut self) -> bool {
        let Some(entry) = self.redo_log.pop() else {
            return false;
        };
        let did = match &entry {
            UndoEntry::Content(id) => self
                .objects
                .get_mut(id)
                .and_then(|session| session.document.as_mut())
                .is_some_and(BlockDocument::redo),
            UndoEntry::Structure(change) => {
                self.reverse_structure(change, true);
                true
            }
        };
        if did {
            self.undo_log.push(entry);
            self.reproject();
        }
        did
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo_log.is_empty()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo_log.is_empty()
    }

    /// Whether a content object is still retained (including unlinked sources).
    #[must_use]
    pub fn has_object(&self, id: &str) -> bool {
        self.objects.contains_key(id)
    }

    /// Current lossless bytes for one content object.
    #[must_use]
    pub fn object_bytes(&self, id: &str) -> Option<Vec<u8>> {
        self.objects.get(id).map(ContentSession::export_bytes)
    }

    /// Changed per-object bytes and structural intents only.
    #[must_use]
    pub fn draft(&self) -> CompositionDraft {
        let mut content = BTreeMap::new();
        let mut read_objects = BTreeMap::new();
        for (id, session) in &self.objects {
            if session.dirty() {
                content.insert(id.clone(), session.export_bytes());
                if !session.created {
                    read_objects.insert(id.clone(), session.version.clone());
                }
            }
        }
        let mut collections = BTreeMap::new();
        if !self.pending_relations.is_empty() || !self.pending_placements.is_empty() {
            for key in self.touched_collections() {
                if let Some(version) = self.collection_versions.get(&key) {
                    collections.insert(key, version.clone());
                } else {
                    collections.insert(key, ObjectVersion(self.version_tip.clone()));
                }
            }
        }
        CompositionDraft {
            root: self.root.clone(),
            read_set: MutationReadSet {
                objects: read_objects,
                collections,
            },
            content,
            relations: self.pending_relations.clone(),
            placements: self.pending_placements.clone(),
            new_images: BTreeMap::new(),
        }
    }

    /// Mark the current draft as saved without clearing per-object undo.
    pub fn acknowledge_save(&mut self, versions: BTreeMap<ObjectId, ObjectVersion>) {
        if versions.is_empty() {
            for session in self.objects.values_mut() {
                session.baseline = session.export_bytes();
                session.created = false;
            }
        } else {
            for (id, version) in versions {
                if let Some(session) = self.objects.get_mut(&id) {
                    session.version = version;
                    session.baseline = session.export_bytes();
                    session.created = false;
                }
            }
        }
        self.pending_relations.clear();
        self.pending_placements.clear();
    }

    fn resolve(
        &self,
        op_id: &BlockId,
        occurrence: Option<&BlockId>,
        require_occurrence: bool,
    ) -> Option<Resolved> {
        if let Some(occ) = occurrence {
            if let Some(ix) = self
                .occurrences
                .iter()
                .position(|row| row.relation_id == occ.0)
            {
                let row = &self.occurrences[ix];
                return Some(Resolved {
                    content_id: row.child.clone(),
                    occurrence_index: ix,
                    relation_id: row.relation_id.clone(),
                });
            }
        }
        if let Some(ix) = self
            .occurrences
            .iter()
            .position(|row| row.relation_id == op_id.0)
        {
            let row = &self.occurrences[ix];
            return Some(Resolved {
                content_id: row.child.clone(),
                occurrence_index: ix,
                relation_id: row.relation_id.clone(),
            });
        }
        let matches: Vec<usize> = self
            .occurrences
            .iter()
            .enumerate()
            .filter(|(_, row)| row.child == op_id.0)
            .map(|(ix, _)| ix)
            .collect();
        match matches.as_slice() {
            [ix] => {
                let row = &self.occurrences[*ix];
                Some(Resolved {
                    content_id: row.child.clone(),
                    occurrence_index: *ix,
                    relation_id: row.relation_id.clone(),
                })
            }
            [] => None,
            _ if require_occurrence => None,
            [ix, ..] => {
                let row = &self.occurrences[*ix];
                Some(Resolved {
                    content_id: row.child.clone(),
                    occurrence_index: *ix,
                    relation_id: row.relation_id.clone(),
                })
            }
        }
    }

    fn apply_text(&mut self, resolved: Resolved, op: BlockOp) -> ApplyResult {
        let result = {
            let Some(session) = self.objects.get_mut(&resolved.content_id) else {
                return ApplyResult::default();
            };
            if !session.writable {
                return ApplyResult::default();
            }
            let Some(document) = session.document.as_mut() else {
                return ApplyResult::default();
            };
            let mapped = remap_op(op, &session.internal_id);
            document.apply(mapped)
        };
        let content_id = resolved.content_id.clone();
        let relation_id = resolved.relation_id.clone();
        self.push_undo(UndoEntry::Content(content_id.clone()));
        self.reproject();
        map_selection(result, &content_id, Some(&relation_id))
    }

    fn set_kind(&mut self, resolved: Resolved, kind: BlockType) -> ApplyResult {
        if !self.gate.content_writable(&kind) {
            return ApplyResult::default();
        }
        let result = {
            let Some(session) = self.objects.get_mut(&resolved.content_id) else {
                return ApplyResult::default();
            };
            if !session.writable {
                return ApplyResult::default();
            }
            let Some(document) = session.document.as_mut() else {
                return ApplyResult::default();
            };
            document.apply(BlockOp::SetType {
                id: session.internal_id.clone(),
                kind,
            })
        };
        let content_id = resolved.content_id.clone();
        let relation_id = resolved.relation_id.clone();
        self.push_undo(UndoEntry::Content(content_id.clone()));
        self.reproject();
        map_selection(result, &content_id, Some(&relation_id))
    }

    fn split(&mut self, resolved: Resolved, offset: usize) -> ApplyResult {
        let (writable, kind, internal) = {
            let Some(session) = self.objects.get(&resolved.content_id) else {
                return ApplyResult::default();
            };
            (
                session.writable,
                session.kind(),
                session.internal_id.clone(),
            )
        };
        if !writable || !self.gate.content_writable(&kind) {
            return ApplyResult::default();
        }
        let suffix_kind = match kind {
            BlockType::Heading { .. } => BlockType::Paragraph,
            other => other,
        };
        let Some(document) = self
            .objects
            .get_mut(&resolved.content_id)
            .and_then(|session| session.document.as_mut())
        else {
            return ApplyResult::default();
        };
        let Some((_, map)) = find_block(document.doc(), &internal) else {
            return ApplyResult::default();
        };
        let Ok(content) = ensure_content(&map) else {
            return ApplyResult::default();
        };
        let len = content.len_utf8();
        let offset = offset.min(len);
        let Ok(tail) = slice_delta_utf8(&content, offset, len) else {
            return ApplyResult::default();
        };
        if offset < len {
            let _ = content.delete_utf8(offset, len - offset);
        }
        document.finish_direct(true);

        let suffix_id = self.ids.alloc_block();
        let relation_id = self.ids.alloc_child();
        let suffix_doc = new_block_document(&suffix_id, suffix_kind, &tail);
        let bytes = suffix_doc.export_snapshot().unwrap_or_default();
        self.objects.insert(
            suffix_id.clone(),
            ContentSession {
                object_id: suffix_id.clone(),
                internal_id: BlockId(suffix_id.clone()),
                document: Some(suffix_doc),
                intact_bytes: bytes.clone(),
                baseline: Vec::new(),
                version: ObjectVersion(String::new()),
                writable: true,
                created: true,
            },
        );
        let index = resolved.occurrence_index + 1;
        let parent = self.occurrences[resolved.occurrence_index].parent.clone();
        let slot = self.occurrences[resolved.occurrence_index].slot.clone();
        let occurrence = ChildOccurrence {
            relation_id: relation_id.clone(),
            parent,
            child: suffix_id.clone(),
            slot,
            position: String::new(),
            child_missing: false,
            cycle_unresolved: false,
        };
        let place = PlaceIntent::PlaceAfter {
            parent: occurrence.parent.clone(),
            slot: occurrence.slot.clone(),
            after: resolved.relation_id.clone(),
        };
        self.occurrences.insert(index, occurrence.clone());
        self.pending_relations
            .push(child_relation(&occurrence, false));
        self.pending_placements.push((relation_id.clone(), place));
        self.push_undo(UndoEntry::Structure(StructureUndo::Split {
            prefix: resolved.content_id,
            _suffix: suffix_id.clone(),
            occurrence,
            index,
        }));
        self.reproject();
        ApplyResult {
            selection: Some(Selection::caret(
                Cursor::new(BlockId(suffix_id), Part::Body, 0)
                    .with_occurrence(BlockId(relation_id)),
            )),
        }
    }

    fn join(&mut self, resolved: Resolved) -> ApplyResult {
        if resolved.occurrence_index == 0 {
            return ApplyResult::default();
        }
        let source = self.occurrences[resolved.occurrence_index].clone();
        let survivor_occ = self.occurrences[resolved.occurrence_index - 1].clone();
        if source.child == survivor_occ.child {
            return self.unlink(resolved);
        }
        let source_writable = self
            .objects
            .get(&source.child)
            .is_some_and(|session| session.writable && session.document.is_some());
        let survivor_writable = self
            .objects
            .get(&survivor_occ.child)
            .is_some_and(|session| session.writable && session.document.is_some());
        if !source_writable || !survivor_writable {
            return ApplyResult::default();
        }
        let delta = {
            let source_session = self.objects.get(&source.child).expect("source");
            let source_doc = source_session.document.as_ref().expect("source doc");
            let Some((_, source_map)) = find_block(source_doc.doc(), &source_session.internal_id)
            else {
                return ApplyResult::default();
            };
            let Ok(source_text) = ensure_content(&source_map) else {
                return ApplyResult::default();
            };
            match slice_delta_utf8(&source_text, 0, source_text.len_utf8()) {
                Ok(delta) => delta,
                Err(_) => return ApplyResult::default(),
            }
        };
        let survivor_internal = self
            .objects
            .get(&survivor_occ.child)
            .expect("survivor")
            .internal_id
            .clone();
        let survivor_id = survivor_occ.child.clone();
        let Some(survivor_doc) = self
            .objects
            .get_mut(&survivor_id)
            .and_then(|session| session.document.as_mut())
        else {
            return ApplyResult::default();
        };
        let Some((_, survivor_map)) = find_block(survivor_doc.doc(), &survivor_internal) else {
            return ApplyResult::default();
        };
        let Ok(survivor_text) = ensure_content(&survivor_map) else {
            return ApplyResult::default();
        };
        let caret = survivor_text.len_utf8();
        let _ = replay_delta_at(&survivor_text, caret, &delta);
        survivor_doc.finish_direct(true);

        let index = resolved.occurrence_index;
        self.occurrences.remove(index);
        self.pending_relations.push(child_relation(&source, true));
        self.push_undo(UndoEntry::Structure(StructureUndo::Join {
            survivor: survivor_id.clone(),
            restored: source,
            index,
        }));
        self.reproject();
        ApplyResult {
            selection: Some(Selection::caret(
                Cursor::new(BlockId(survivor_id), Part::Body, caret)
                    .with_occurrence(BlockId(survivor_occ.relation_id)),
            )),
        }
    }

    fn unlink(&mut self, resolved: Resolved) -> ApplyResult {
        let index = resolved.occurrence_index;
        let occurrence = self.occurrences.remove(index);
        self.pending_relations
            .push(child_relation(&occurrence, true));
        self.push_undo(UndoEntry::Structure(StructureUndo::Unlink {
            occurrence,
            index,
        }));
        self.reproject();
        let selection = self
            .snapshots
            .get(index.min(self.snapshots.len().saturating_sub(1)))
            .map(|snap| {
                let mut cursor = Cursor::new(snap.id.clone(), Part::Body, 0);
                if let Some(occurrence) = &snap.occurrence_id {
                    cursor = cursor.with_occurrence(occurrence.clone());
                }
                Selection::caret(cursor)
            });
        ApplyResult { selection }
    }

    fn indent(&mut self, resolved: Resolved) -> ApplyResult {
        let from = resolved.occurrence_index;
        let Some(prev_ix) = self.previous_sibling_index(from) else {
            return ApplyResult::default();
        };
        let old = self.occurrences[from].clone();
        let prev = self.occurrences[prev_ix].clone();
        if prev.child == old.child || self.would_cycle(&prev.child, &old.child) {
            return ApplyResult::default();
        }
        let subtree_len = self.subtree_len(from);
        let new_rel = self.ids.alloc_child();
        let new = ChildOccurrence {
            relation_id: new_rel,
            parent: prev.child.clone(),
            child: old.child.clone(),
            slot: "body".into(),
            position: String::new(),
            child_missing: false,
            cycle_unresolved: false,
        };
        let insert_at = prev_ix + self.subtree_len(prev_ix);
        self.reparent_subtree(from, subtree_len, insert_at, new.clone(), &old)
    }

    fn outdent(&mut self, resolved: Resolved) -> ApplyResult {
        let from = resolved.occurrence_index;
        let old = self.occurrences[from].clone();
        if old.parent == self.root {
            return ApplyResult::default();
        }
        let Some(parent_ix) = self.nearest_parent_index(from) else {
            return ApplyResult::default();
        };
        let parent_occ = self.occurrences[parent_ix].clone();
        let subtree_len = self.subtree_len(from);
        let new_rel = self.ids.alloc_child();
        let new = ChildOccurrence {
            relation_id: new_rel,
            parent: parent_occ.parent.clone(),
            child: old.child.clone(),
            slot: parent_occ.slot.clone(),
            position: String::new(),
            child_missing: false,
            cycle_unresolved: false,
        };
        let insert_at = parent_ix + self.subtree_len(parent_ix);
        self.reparent_subtree(from, subtree_len, insert_at, new, &old)
    }

    fn reparent_subtree(
        &mut self,
        from: usize,
        subtree_len: usize,
        mut insert_at: usize,
        new: ChildOccurrence,
        old: &ChildOccurrence,
    ) -> ApplyResult {
        let mut subtree: Vec<ChildOccurrence> =
            self.occurrences.drain(from..from + subtree_len).collect();
        subtree[0] = new.clone();
        if insert_at > from {
            insert_at -= subtree_len;
        }
        insert_at = insert_at.min(self.occurrences.len());
        for (offset, occ) in subtree.iter().enumerate() {
            self.occurrences.insert(insert_at + offset, occ.clone());
        }
        self.pending_relations.push(child_relation(old, true));
        self.pending_relations.push(child_relation(&new, false));
        let place = self.place_for_inserted(&new, insert_at);
        self.pending_placements
            .retain(|(id, _)| id != &old.relation_id && id != &new.relation_id);
        self.pending_placements
            .push((new.relation_id.clone(), place));
        self.push_undo(UndoEntry::Structure(StructureUndo::Reparent {
            old: old.clone(),
            new: new.clone(),
            old_index: from,
            new_index: insert_at,
            subtree_len,
        }));
        self.reproject();
        ApplyResult {
            selection: Some(Selection::caret(
                Cursor::new(BlockId(new.child.clone()), Part::Body, 0)
                    .with_occurrence(BlockId(new.relation_id)),
            )),
        }
    }

    fn reorder(&mut self, resolved: Resolved, to: usize) -> ApplyResult {
        let from = resolved.occurrence_index;
        if from >= self.occurrences.len() {
            return ApplyResult::default();
        }
        let to = to.min(self.occurrences.len().saturating_sub(1));
        if from == to {
            return ApplyResult::default();
        }
        let occurrence = self.occurrences[from].clone();
        if self.occurrences[to].parent != occurrence.parent
            || self.occurrences[to].slot != occurrence.slot
        {
            return ApplyResult::default();
        }
        let occurrence = self.occurrences.remove(from);
        let insert_at = to.min(self.occurrences.len());
        self.occurrences.insert(insert_at, occurrence.clone());
        let place = self.place_for_inserted(&occurrence, insert_at);
        self.pending_placements
            .retain(|(id, _)| id != &occurrence.relation_id);
        self.pending_placements
            .push((occurrence.relation_id.clone(), place));
        self.push_undo(UndoEntry::Structure(StructureUndo::Move {
            from,
            to: insert_at,
        }));
        self.reproject();
        ApplyResult {
            selection: Some(Selection::caret(
                Cursor::new(BlockId(occurrence.child), Part::Body, 0)
                    .with_occurrence(BlockId(occurrence.relation_id)),
            )),
        }
    }

    fn reverse_structure(&mut self, change: &StructureUndo, redo: bool) {
        match change {
            StructureUndo::Split {
                prefix,
                _suffix: _,
                occurrence,
                index,
            } => {
                if redo {
                    self.occurrences.insert(*index, occurrence.clone());
                    self.pending_relations
                        .push(child_relation(occurrence, false));
                    self.pending_placements.push((
                        occurrence.relation_id.clone(),
                        PlaceIntent::PlaceAfter {
                            parent: occurrence.parent.clone(),
                            slot: occurrence.slot.clone(),
                            after: self
                                .occurrences
                                .get(index.saturating_sub(1))
                                .map(|row| row.relation_id.clone())
                                .unwrap_or_else(|| occurrence.relation_id.clone()),
                        },
                    ));
                    if let Some(session) = self.objects.get_mut(prefix) {
                        if let Some(document) = session.document.as_mut() {
                            document.redo();
                        }
                    }
                } else {
                    if *index < self.occurrences.len()
                        && self.occurrences[*index].relation_id == occurrence.relation_id
                    {
                        self.occurrences.remove(*index);
                    }
                    self.pending_relations
                        .push(child_relation(occurrence, true));
                    if let Some(session) = self.objects.get_mut(prefix) {
                        if let Some(document) = session.document.as_mut() {
                            document.undo();
                        }
                    }
                }
            }
            StructureUndo::Join {
                survivor,
                restored,
                index,
            } => {
                if redo {
                    if *index < self.occurrences.len()
                        && self.occurrences[*index].relation_id == restored.relation_id
                    {
                        self.occurrences.remove(*index);
                    } else if let Some(ix) = self
                        .occurrences
                        .iter()
                        .position(|row| row.relation_id == restored.relation_id)
                    {
                        self.occurrences.remove(ix);
                    }
                    self.pending_relations.push(child_relation(restored, true));
                    if let Some(session) = self.objects.get_mut(survivor) {
                        if let Some(document) = session.document.as_mut() {
                            document.redo();
                        }
                    }
                } else {
                    self.occurrences.insert(*index, restored.clone());
                    self.pending_relations.push(child_relation(restored, false));
                    if let Some(session) = self.objects.get_mut(survivor) {
                        if let Some(document) = session.document.as_mut() {
                            document.undo();
                        }
                    }
                }
            }
            StructureUndo::Move { from, to } => {
                let (src, dest) = if redo { (*from, *to) } else { (*to, *from) };
                if src < self.occurrences.len() {
                    let occurrence = self.occurrences.remove(src);
                    let dest = dest.min(self.occurrences.len());
                    self.occurrences.insert(dest, occurrence.clone());
                    let place = self.place_for_inserted(&occurrence, dest);
                    self.pending_placements
                        .retain(|(id, _)| id != &occurrence.relation_id);
                    self.pending_placements
                        .push((occurrence.relation_id, place));
                }
            }
            StructureUndo::Create {
                _content: _,
                occurrence,
                index,
            } => {
                if redo {
                    self.occurrences.insert(*index, occurrence.clone());
                    self.pending_relations
                        .push(child_relation(occurrence, false));
                } else if *index < self.occurrences.len()
                    && self.occurrences[*index].relation_id == occurrence.relation_id
                {
                    self.occurrences.remove(*index);
                    self.pending_relations
                        .push(child_relation(occurrence, true));
                }
            }
            StructureUndo::Unlink { occurrence, index } => {
                if redo {
                    if *index < self.occurrences.len()
                        && self.occurrences[*index].relation_id == occurrence.relation_id
                    {
                        self.occurrences.remove(*index);
                    }
                    self.pending_relations
                        .push(child_relation(occurrence, true));
                } else {
                    self.occurrences.insert(*index, occurrence.clone());
                    self.pending_relations
                        .push(child_relation(occurrence, false));
                }
            }
            StructureUndo::Reparent {
                old,
                new,
                old_index,
                new_index,
                subtree_len,
            } => {
                if redo {
                    self.restore_reparent(old, new, *old_index, *new_index, *subtree_len);
                } else {
                    self.restore_reparent(new, old, *new_index, *old_index, *subtree_len);
                }
            }
        }
    }

    fn restore_reparent(
        &mut self,
        remove: &ChildOccurrence,
        insert: &ChildOccurrence,
        from: usize,
        mut insert_at: usize,
        subtree_len: usize,
    ) {
        if from >= self.occurrences.len() {
            return;
        }
        let end = (from + subtree_len).min(self.occurrences.len());
        let mut subtree: Vec<ChildOccurrence> = self.occurrences.drain(from..end).collect();
        if subtree.is_empty() {
            return;
        }
        subtree[0] = insert.clone();
        if insert_at > from {
            insert_at -= subtree.len();
        }
        insert_at = insert_at.min(self.occurrences.len());
        for (offset, occ) in subtree.iter().enumerate() {
            self.occurrences.insert(insert_at + offset, occ.clone());
        }
        self.pending_relations.push(child_relation(remove, true));
        self.pending_relations.push(child_relation(insert, false));
        let place = self.place_for_inserted(insert, insert_at);
        self.pending_placements
            .retain(|(id, _)| id != &remove.relation_id && id != &insert.relation_id);
        self.pending_placements
            .push((insert.relation_id.clone(), place));
    }

    fn place_for_inserted(&self, occurrence: &ChildOccurrence, index: usize) -> PlaceIntent {
        let parent = occurrence.parent.clone();
        let slot = occurrence.slot.clone();
        let sibs: Vec<usize> = self
            .occurrences
            .iter()
            .enumerate()
            .filter(|(_, row)| row.parent == parent && row.slot == slot)
            .map(|(ix, _)| ix)
            .collect();
        match sibs.iter().position(|&ix| ix == index) {
            Some(0) if sibs.len() == 1 => PlaceIntent::Append { parent, slot },
            Some(0) => PlaceIntent::PlaceBefore {
                parent,
                slot,
                before: self.occurrences[sibs[1]].relation_id.clone(),
            },
            Some(pos) => PlaceIntent::PlaceAfter {
                parent,
                slot,
                after: self.occurrences[sibs[pos - 1]].relation_id.clone(),
            },
            None => PlaceIntent::Append { parent, slot },
        }
    }

    fn previous_sibling_index(&self, index: usize) -> Option<usize> {
        let occ = self.occurrences.get(index)?;
        self.occurrences[..index]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, row)| row.parent == occ.parent && row.slot == occ.slot)
            .map(|(ix, _)| ix)
    }

    fn nearest_parent_index(&self, index: usize) -> Option<usize> {
        let parent = &self.occurrences.get(index)?.parent;
        self.occurrences[..index]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, row)| row.child == *parent)
            .map(|(ix, _)| ix)
    }

    fn subtree_len(&self, index: usize) -> usize {
        let depth = self.occurrence_depth(index);
        let mut end = index + 1;
        while end < self.occurrences.len() && self.occurrence_depth(end) > depth {
            end += 1;
        }
        end - index
    }

    fn occurrence_depth(&self, index: usize) -> i64 {
        let mut depth = 0i64;
        let mut parent = self.occurrences[index].parent.clone();
        for _ in 0..self.occurrences.len() {
            if parent == self.root {
                break;
            }
            match self
                .occurrences
                .iter()
                .find(|row| row.child == parent)
                .map(|row| row.parent.clone())
            {
                Some(next) => {
                    depth += 1;
                    parent = next;
                }
                None => break,
            }
        }
        depth
    }

    fn would_cycle(&self, new_parent: &str, moving: &str) -> bool {
        if new_parent == moving {
            return true;
        }
        let mut current = new_parent.to_string();
        for _ in 0..self.occurrences.len() {
            if current == self.root {
                return false;
            }
            let Some(row) = self.occurrences.iter().find(|occ| occ.child == current) else {
                return false;
            };
            if row.parent == moving {
                return true;
            }
            current = row.parent.clone();
        }
        false
    }

    fn touched_collections(&self) -> BTreeSet<(ObjectId, String)> {
        let mut keys = BTreeSet::new();
        keys.insert((self.root.clone(), self.slot.clone()));
        for occ in &self.occurrences {
            keys.insert((occ.parent.clone(), occ.slot.clone()));
        }
        for rel in &self.pending_relations {
            if let (Some(parent), Some(slot)) = (rel.fields.get("parent"), rel.fields.get("slot")) {
                keys.insert((parent.clone(), slot.clone()));
            }
        }
        for (_, place) in &self.pending_placements {
            match place {
                PlaceIntent::Append { parent, slot }
                | PlaceIntent::PlaceBefore { parent, slot, .. }
                | PlaceIntent::PlaceAfter { parent, slot, .. } => {
                    keys.insert((parent.clone(), slot.clone()));
                }
            }
        }
        keys
    }

    fn push_undo(&mut self, entry: UndoEntry) {
        self.undo_log.push(entry);
        self.redo_log.clear();
    }

    fn reproject(&mut self) {
        self.snapshots = self
            .occurrences
            .iter()
            .enumerate()
            .map(|(ix, occurrence)| {
                let mut snap = self
                    .objects
                    .get(&occurrence.child)
                    .map(ContentSession::snapshot)
                    .unwrap_or_else(|| missing_snapshot(&occurrence.child));
                snap.id = BlockId(occurrence.child.clone());
                snap.occurrence_id = Some(BlockId(occurrence.relation_id.clone()));
                snap.indent = self.occurrence_depth(ix);
                if occurrence.child_missing || occurrence.cycle_unresolved {
                    snap.read_only = true;
                }
                snap
            })
            .collect();
    }
}

/// Encode one paragraph or heading as a single-block Loro snapshot whose
/// internal block id is the ObjectId.
#[must_use]
pub fn encode_block(id: &str, kind: BlockType, text: &str) -> Vec<u8> {
    let delta = if text.is_empty() {
        Vec::new()
    } else {
        vec![TextDelta::Insert {
            insert: text.into(),
            attributes: None,
        }]
    };
    new_block_document(id, kind, &delta)
        .export_snapshot()
        .unwrap_or_default()
}

fn new_block_document(id: &str, kind: BlockType, delta: &[TextDelta]) -> BlockDocument {
    let doc = LoroDoc::new();
    configure_text_styles(&doc);
    let _ = doc.get_map("comments");
    let list = blocks_list(&doc);
    let map = insert_block_map(&list, 0, &BlockId(id.to_string()), kind, 0)
        .expect("insert per-object block");
    if !delta.is_empty() {
        if let Ok(text) = ensure_content(&map) {
            let _ = replay_delta_at(&text, 0, delta);
        }
    }
    doc.commit();
    BlockDocument::from_loro(doc)
}

fn load_object(object: CompositionObject, gate: EditorGate) -> ContentSession {
    if object.bytes.is_empty() {
        let kind = object.kind_hint.unwrap_or(BlockType::Paragraph);
        let document = new_block_document(&object.id, kind.clone(), &[]);
        let bytes = document.export_snapshot().unwrap_or_default();
        return ContentSession {
            object_id: object.id.clone(),
            internal_id: BlockId(object.id.clone()),
            document: Some(document),
            intact_bytes: bytes.clone(),
            baseline: bytes,
            version: object.version,
            writable: gate.content_writable(&kind),
            created: false,
        };
    }
    match BlockDocument::import_snapshot(&object.bytes) {
        Ok(document) => {
            let internal_id = document
                .snapshots()
                .first()
                .map(|snap| snap.id.clone())
                .unwrap_or_else(|| BlockId(object.id.clone()));
            let kind = document
                .snapshots()
                .first()
                .map(|snap| snap.block_type.clone())
                .or(object.kind_hint)
                .unwrap_or(BlockType::Custom("unknown".into()));
            let writable = gate.content_writable(&kind);
            ContentSession {
                object_id: object.id,
                internal_id,
                document: Some(document),
                intact_bytes: object.bytes.clone(),
                baseline: object.bytes,
                version: object.version,
                writable,
                created: false,
            }
        }
        Err(_) => ContentSession {
            object_id: object.id,
            internal_id: BlockId("unimported".into()),
            document: None,
            intact_bytes: object.bytes.clone(),
            baseline: object.bytes,
            version: object.version,
            writable: false,
            created: false,
        },
    }
}

fn missing_snapshot(id: &str) -> BlockSnapshot {
    BlockSnapshot {
        id: BlockId(id.to_string()),
        occurrence_id: None,
        block_type: BlockType::Custom("missing".into()),
        indent: 0,
        plain: String::new(),
        runs: Vec::new(),
        props: HashMap::new(),
        checked: None,
        number: None,
        language: None,
        url: None,
        form: None,
        width: None,
        table: None,
        read_only: true,
    }
}

fn child_relation(occurrence: &ChildOccurrence, tombstone: bool) -> RelationMutation {
    let mut fields = BTreeMap::new();
    fields.insert("parent".into(), occurrence.parent.clone());
    fields.insert("child".into(), occurrence.child.clone());
    fields.insert("slot".into(), occurrence.slot.clone());
    RelationMutation {
        id: occurrence.relation_id.clone(),
        type_name: CHILD_TYPE.into(),
        fields,
        tombstone,
    }
}

fn remap_op(op: BlockOp, internal: &BlockId) -> BlockOp {
    match op {
        BlockOp::InsertText { offset, text, .. } => BlockOp::InsertText {
            id: internal.clone(),
            offset,
            text,
        },
        BlockOp::DeleteRange { start, end, .. } => BlockOp::DeleteRange {
            id: internal.clone(),
            start,
            end,
        },
        BlockOp::ImeCommit {
            part,
            offset,
            replace_len,
            text,
            ..
        } => BlockOp::ImeCommit {
            id: internal.clone(),
            part,
            offset,
            replace_len,
            text,
        },
        BlockOp::SetType { kind, .. } => BlockOp::SetType {
            id: internal.clone(),
            kind,
        },
        BlockOp::ToggleMark {
            start, end, mark, ..
        } => BlockOp::ToggleMark {
            id: internal.clone(),
            start,
            end,
            mark,
        },
        BlockOp::SetProp { key, value, .. } => BlockOp::SetProp {
            id: internal.clone(),
            key,
            value,
        },
        BlockOp::ToggleCheck { .. } => BlockOp::ToggleCheck {
            id: internal.clone(),
        },
        other => other,
    }
}

fn map_selection(result: ApplyResult, content: &str, occurrence: Option<&str>) -> ApplyResult {
    ApplyResult {
        selection: result.selection.map(|selection| {
            let map_cursor = |cursor: Cursor| {
                let mut mapped =
                    Cursor::new(BlockId(content.to_string()), cursor.part, cursor.offset);
                if let Some(occurrence) = occurrence {
                    mapped = mapped.with_occurrence(BlockId(occurrence.to_string()));
                }
                mapped
            };
            Selection::new(map_cursor(selection.anchor), map_cursor(selection.focus))
        }),
    }
}

#[cfg(test)]
mod tests {
    include!("composition_tests.rs");
}
