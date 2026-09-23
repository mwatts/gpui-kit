use super::*;
use crate::types::{BlockOp, LwwValue};
use block_markdown::{BlockId, BlockType};
use std::collections::BTreeMap;

const NOTE: &str = "ashlar/note/opaque/n00note00000000000000000001";
const BLOCK_A: &str = "ashlar/block/opaque/n00blk0000000000000000000a";
const BLOCK_B: &str = "ashlar/block/opaque/n00blk0000000000000000000b";
const CHILD_A1: &str = "ashlar/child/opaque/n00child0000000000000000a1";
const CHILD_A2: &str = "ashlar/child/opaque/n00child0000000000000000a2";
const CHILD_B1: &str = "ashlar/child/opaque/n00child0000000000000000b1";
const SUFFIX_BLOCK: &str = "ashlar/block/opaque/n00blk00000000000000000sx";
const SUFFIX_CHILD: &str = "ashlar/child/opaque/n00child00000000000000sx";
const NEW_BLOCK: &str = "ashlar/block/opaque/n00blk00000000000000000nw";
const NEW_CHILD: &str = "ashlar/child/opaque/n00child00000000000000nw";

fn object(id: &str, kind: BlockType, text: &str) -> CompositionObject {
    CompositionObject {
        id: id.into(),
        version: ObjectVersion("v1".into()),
        bytes: encode_block(id, kind.clone(), text),
        kind_hint: Some(kind),
    }
}

fn child(relation: &str, target: &str, position: &str) -> ChildOccurrence {
    ChildOccurrence {
        relation_id: relation.into(),
        parent: NOTE.into(),
        child: target.into(),
        slot: "body".into(),
        position: position.into(),
        child_missing: false,
        cycle_unresolved: false,
    }
}

fn fixture() -> CompositionRead {
    let mut objects = BTreeMap::new();
    objects.insert(
        BLOCK_A.into(),
        object(BLOCK_A, BlockType::Paragraph, "alpha"),
    );
    objects.insert(
        BLOCK_B.into(),
        object(BLOCK_B, BlockType::Heading { level: 1 }, "bravo"),
    );
    CompositionRead {
        root: NOTE.into(),
        root_type: "ashlar.Note".into(),
        version_tip: "tip-1".into(),
        objects,
        children: vec![
            child(CHILD_A1, BLOCK_A, "a"),
            child(CHILD_B1, BLOCK_B, "b"),
            child(CHILD_A2, BLOCK_A, "c"),
        ],
        collection_versions: BTreeMap::from([(
            (NOTE.into(), "body".into()),
            ObjectVersion("col-1".into()),
        )]),
    }
}

fn open_fixture() -> CompositionSession {
    CompositionSession::open(fixture(), EditorGate::Notes, IdSource::default())
}

fn ids(blocks: &[&str], children: &[&str]) -> IdSource {
    IdSource::Queue {
        blocks: blocks.iter().map(|id| (*id).to_string()).collect(),
        children: children.iter().map(|id| (*id).to_string()).collect(),
    }
}

fn occ(id: &str) -> BlockId {
    BlockId(id.into())
}

#[test]
fn encode_block_uses_object_id_as_block_id() {
    let bytes = encode_block(BLOCK_A, BlockType::Paragraph, "hello");
    let document = BlockDocument::import_snapshot(&bytes).unwrap();
    assert_eq!(document.snapshots().len(), 1);
    assert_eq!(document.snapshots()[0].id.0, BLOCK_A);
    assert_eq!(document.snapshots()[0].plain, "hello");
}

#[test]
fn repeated_placements_share_content_and_keep_distinct_occurrences() {
    let mut session = open_fixture();
    assert_eq!(session.snapshots().len(), 3);
    assert_eq!(session.snapshots()[0].id.0, BLOCK_A);
    assert_eq!(session.snapshots()[2].id.0, BLOCK_A);
    assert_eq!(
        session.snapshots()[0]
            .occurrence_id
            .as_ref()
            .map(|id| id.0.as_str()),
        Some(CHILD_A1)
    );
    assert_eq!(
        session.snapshots()[2]
            .occurrence_id
            .as_ref()
            .map(|id| id.0.as_str()),
        Some(CHILD_A2)
    );
    assert_ne!(
        session.snapshots()[0].paint_id(),
        session.snapshots()[2].paint_id()
    );

    session.apply(
        BlockOp::InsertText {
            id: BlockId(BLOCK_A.into()),
            offset: 5,
            text: "!".into(),
        },
        Some(&occ(CHILD_A2)),
    );
    assert_eq!(session.snapshots()[0].plain, "alpha!");
    assert_eq!(session.snapshots()[2].plain, "alpha!");
    assert_eq!(session.snapshots()[1].plain, "bravo");
    let draft = session.draft();
    assert_eq!(draft.content.len(), 1);
    assert!(draft.content.contains_key(BLOCK_A));
    assert!(!draft.content.contains_key(BLOCK_B));
}

#[test]
fn split_keeps_prefix_id_and_inserts_suffix_only_at_initiating_occurrence() {
    let mut session = CompositionSession::open(
        fixture(),
        EditorGate::Notes,
        ids(&[SUFFIX_BLOCK], &[SUFFIX_CHILD]),
    );
    session.apply(
        BlockOp::SplitBlock {
            id: BlockId(BLOCK_A.into()),
            offset: 2,
        },
        Some(&occ(CHILD_A1)),
    );
    let snaps = session.snapshots();
    assert_eq!(snaps.len(), 4);
    assert_eq!(snaps[0].id.0, BLOCK_A);
    assert_eq!(snaps[0].plain, "al");
    assert_eq!(snaps[0].occurrence_id.as_ref().unwrap().0, CHILD_A1);
    assert_eq!(snaps[1].id.0, SUFFIX_BLOCK);
    assert_eq!(snaps[1].plain, "pha");
    assert_eq!(snaps[1].occurrence_id.as_ref().unwrap().0, SUFFIX_CHILD);
    assert_eq!(snaps[2].id.0, BLOCK_B);
    assert_eq!(snaps[3].id.0, BLOCK_A);
    assert_eq!(snaps[3].plain, "al");
    assert_eq!(snaps[3].occurrence_id.as_ref().unwrap().0, CHILD_A2);

    let draft = session.draft();
    assert!(draft.content.contains_key(BLOCK_A));
    assert!(draft.content.contains_key(SUFFIX_BLOCK));
    assert_eq!(draft.relations.len(), 1);
    assert_eq!(draft.relations[0].id, SUFFIX_CHILD);
    assert!(!draft.relations[0].tombstone);
    assert_eq!(
        draft.placements,
        vec![(
            SUFFIX_CHILD.into(),
            PlaceIntent::PlaceAfter {
                parent: NOTE.into(),
                slot: "body".into(),
                after: CHILD_A1.into(),
            }
        )]
    );
}

#[test]
fn join_unlinks_occurrence_only_and_preserves_source_object() {
    let mut objects = BTreeMap::new();
    objects.insert(
        BLOCK_A.into(),
        object(BLOCK_A, BlockType::Paragraph, "hello"),
    );
    objects.insert(
        BLOCK_B.into(),
        object(BLOCK_B, BlockType::Paragraph, "world"),
    );
    let read = CompositionRead {
        root: NOTE.into(),
        root_type: "ashlar.Note".into(),
        version_tip: "tip-1".into(),
        objects,
        children: vec![child(CHILD_A1, BLOCK_A, "a"), child(CHILD_B1, BLOCK_B, "b")],
        collection_versions: BTreeMap::new(),
    };
    let mut session = CompositionSession::open(read, EditorGate::Notes, IdSource::default());
    session.apply(
        BlockOp::MergeWithPrevious {
            id: BlockId(BLOCK_B.into()),
        },
        Some(&occ(CHILD_B1)),
    );
    assert_eq!(session.snapshots().len(), 1);
    assert_eq!(session.snapshots()[0].id.0, BLOCK_A);
    assert_eq!(session.snapshots()[0].plain, "helloworld");
    assert_eq!(
        session.snapshots()[0].occurrence_id.as_ref().unwrap().0,
        CHILD_A1
    );
    let draft = session.draft();
    assert!(draft.content.contains_key(BLOCK_A));
    assert!(
        !draft.content.contains_key(BLOCK_B),
        "join must not rewrite the unlinked source object"
    );
    assert_eq!(draft.relations.len(), 1);
    assert_eq!(draft.relations[0].id, CHILD_B1);
    assert!(draft.relations[0].tombstone);
    assert!(session.has_object(BLOCK_B));
}

#[test]
fn join_duplicate_occurrence_unlinks_without_doubling_shared_text() {
    let mut objects = BTreeMap::new();
    objects.insert(
        BLOCK_A.into(),
        object(BLOCK_A, BlockType::Paragraph, "alpha"),
    );
    objects.insert(
        BLOCK_B.into(),
        object(BLOCK_B, BlockType::Heading { level: 1 }, "bravo"),
    );
    let read = CompositionRead {
        root: NOTE.into(),
        root_type: "ashlar.Note".into(),
        version_tip: "tip-1".into(),
        objects,
        children: vec![
            child(CHILD_A1, BLOCK_A, "a"),
            child(CHILD_A2, BLOCK_A, "c"),
            child(CHILD_B1, BLOCK_B, "b"),
        ],
        collection_versions: BTreeMap::new(),
    };
    let mut session = CompositionSession::open(read, EditorGate::Notes, IdSource::default());
    session.apply(
        BlockOp::MergeWithPrevious {
            id: BlockId(BLOCK_A.into()),
        },
        Some(&occ(CHILD_A2)),
    );
    assert_eq!(session.snapshots().len(), 2);
    assert_eq!(session.snapshots()[0].plain, "alpha");
    assert_eq!(session.snapshots()[0].id.0, BLOCK_A);
    assert!(
        session
            .snapshots()
            .iter()
            .all(|snap| snap.occurrence_id.as_ref().unwrap().0 != CHILD_A2)
    );
}

#[test]
fn reorder_changes_occurrence_order_not_content_ids() {
    let mut session = open_fixture();
    session.apply(
        BlockOp::Move {
            id: BlockId(BLOCK_B.into()),
            to: 0,
        },
        Some(&occ(CHILD_B1)),
    );
    let ids: Vec<_> = session
        .snapshots()
        .iter()
        .map(|snap| {
            (
                snap.id.0.clone(),
                snap.occurrence_id.as_ref().unwrap().0.clone(),
            )
        })
        .collect();
    assert_eq!(
        ids,
        vec![
            (BLOCK_B.into(), CHILD_B1.into()),
            (BLOCK_A.into(), CHILD_A1.into()),
            (BLOCK_A.into(), CHILD_A2.into()),
        ]
    );
    let draft = session.draft();
    assert!(draft.content.is_empty());
    assert_eq!(draft.placements.len(), 1);
    assert_eq!(draft.placements[0].0, CHILD_B1);
    assert_eq!(
        draft.placements[0].1,
        PlaceIntent::PlaceBefore {
            parent: NOTE.into(),
            slot: "body".into(),
            before: CHILD_A1.into(),
        }
    );
}

#[test]
fn kind_change_keeps_content_identity() {
    let mut session = open_fixture();
    session.apply(
        BlockOp::SetType {
            id: BlockId(BLOCK_A.into()),
            kind: BlockType::Heading { level: 2 },
        },
        Some(&occ(CHILD_A1)),
    );
    assert_eq!(
        session.snapshots()[0].block_type,
        BlockType::Heading { level: 2 }
    );
    assert_eq!(
        session.snapshots()[2].block_type,
        BlockType::Heading { level: 2 }
    );
    assert_eq!(session.snapshots()[0].id.0, BLOCK_A);
    assert_eq!(session.snapshots()[2].id.0, BLOCK_A);
}

#[test]
fn create_emits_block_and_child_intents() {
    let mut session = CompositionSession::open(
        fixture(),
        EditorGate::Notes,
        ids(&[NEW_BLOCK], &[NEW_CHILD]),
    );
    session.create(BlockType::Paragraph, Some(CHILD_B1));
    assert_eq!(session.snapshots().len(), 4);
    assert_eq!(session.snapshots()[2].id.0, NEW_BLOCK);
    assert_eq!(
        session.snapshots()[2].occurrence_id.as_ref().unwrap().0,
        NEW_CHILD
    );
    let draft = session.draft();
    assert!(draft.content.contains_key(NEW_BLOCK));
    assert_eq!(draft.relations[0].id, NEW_CHILD);
    assert_eq!(draft.relations[0].type_name, CHILD_TYPE);
    assert!(!draft.relations[0].tombstone);
}

#[test]
fn undo_survives_save_flush_per_object() {
    let mut session = open_fixture();
    session.apply(
        BlockOp::InsertText {
            id: BlockId(BLOCK_A.into()),
            offset: 5,
            text: "x".into(),
        },
        Some(&occ(CHILD_A1)),
    );
    session.apply(
        BlockOp::InsertText {
            id: BlockId(BLOCK_B.into()),
            offset: 5,
            text: "y".into(),
        },
        Some(&occ(CHILD_B1)),
    );
    assert_eq!(session.snapshots()[0].plain, "alphax");
    assert_eq!(session.snapshots()[1].plain, "bravoy");
    let draft = session.draft();
    assert_eq!(draft.content.len(), 2);
    session.acknowledge_save(BTreeMap::new());
    assert!(session.draft().content.is_empty());
    assert!(session.can_undo());
    assert!(session.undo());
    assert_eq!(session.snapshots()[1].plain, "bravo");
    assert_eq!(session.snapshots()[0].plain, "alphax");
    assert!(session.undo());
    assert_eq!(session.snapshots()[0].plain, "alpha");
    assert!(session.can_redo());
}

#[test]
fn reopen_preserves_object_ids() {
    let session = open_fixture();
    let mut objects = BTreeMap::new();
    for (id, bytes) in [
        (
            BLOCK_A,
            encode_block(BLOCK_A, BlockType::Paragraph, "alpha"),
        ),
        (
            BLOCK_B,
            encode_block(BLOCK_B, BlockType::Heading { level: 1 }, "bravo"),
        ),
    ] {
        objects.insert(
            id.to_string(),
            CompositionObject {
                id: id.into(),
                version: ObjectVersion("v2".into()),
                bytes,
                kind_hint: None,
            },
        );
    }
    let reopened = CompositionSession::open(
        CompositionRead {
            root: NOTE.into(),
            root_type: "ashlar.Note".into(),
            version_tip: "tip-2".into(),
            objects,
            children: session.occurrences().to_vec(),
            collection_versions: BTreeMap::new(),
        },
        EditorGate::Notes,
        IdSource::default(),
    );
    let ids: Vec<_> = reopened
        .snapshots()
        .iter()
        .map(|snap| {
            (
                snap.id.0.clone(),
                snap.occurrence_id.as_ref().unwrap().0.clone(),
            )
        })
        .collect();
    assert_eq!(
        ids,
        vec![
            (BLOCK_A.into(), CHILD_A1.into()),
            (BLOCK_B.into(), CHILD_B1.into()),
            (BLOCK_A.into(), CHILD_A2.into()),
        ]
    );
}

#[test]
fn unknown_bytes_stay_intact_and_unsafe_actions_are_disabled() {
    let code = encode_block(BLOCK_B, BlockType::Code, "fn main() {}");
    let mut objects = BTreeMap::new();
    objects.insert(BLOCK_A.into(), object(BLOCK_A, BlockType::Paragraph, "ok"));
    objects.insert(
        BLOCK_B.into(),
        CompositionObject {
            id: BLOCK_B.into(),
            version: ObjectVersion("v1".into()),
            bytes: code.clone(),
            kind_hint: Some(BlockType::Code),
        },
    );
    let mut session = CompositionSession::open(
        CompositionRead {
            root: NOTE.into(),
            root_type: "ashlar.Note".into(),
            version_tip: "tip".into(),
            objects,
            children: vec![child(CHILD_A1, BLOCK_A, "a"), child(CHILD_B1, BLOCK_B, "b")],
            collection_versions: BTreeMap::new(),
        },
        EditorGate::Notes,
        IdSource::default(),
    );
    assert!(session.snapshots()[1].read_only);
    assert_eq!(session.snapshots()[1].block_type, BlockType::Code);
    session.apply(
        BlockOp::InsertText {
            id: BlockId(BLOCK_B.into()),
            offset: 0,
            text: "nope".into(),
        },
        Some(&occ(CHILD_B1)),
    );
    session.apply(
        BlockOp::SetType {
            id: BlockId(BLOCK_B.into()),
            kind: BlockType::Paragraph,
        },
        Some(&occ(CHILD_B1)),
    );
    session.apply(
        BlockOp::ToggleMark {
            id: BlockId(BLOCK_A.into()),
            start: 0,
            end: 2,
            mark: "bold".into(),
        },
        Some(&occ(CHILD_A1)),
    );
    session.apply(
        BlockOp::Indent {
            id: BlockId(BLOCK_A.into()),
        },
        Some(&occ(CHILD_A1)),
    );
    assert_eq!(session.snapshots()[1].plain, "fn main() {}");
    assert_eq!(session.snapshots()[1].block_type, BlockType::Code);
    assert_eq!(session.snapshots()[0].plain, "ok");
    let opaque = b"not-a-loro-snapshot".to_vec();
    let mut objects = BTreeMap::new();
    objects.insert(
        BLOCK_A.into(),
        CompositionObject {
            id: BLOCK_A.into(),
            version: ObjectVersion("v1".into()),
            bytes: opaque.clone(),
            kind_hint: Some(BlockType::Custom("legacy".into())),
        },
    );
    let mut session = CompositionSession::open(
        CompositionRead {
            root: NOTE.into(),
            root_type: "ashlar.Note".into(),
            version_tip: "tip".into(),
            objects,
            children: vec![child(CHILD_A1, BLOCK_A, "a")],
            collection_versions: BTreeMap::new(),
        },
        EditorGate::Notes,
        IdSource::default(),
    );
    assert!(session.snapshots()[0].read_only);
    session.apply(
        BlockOp::InsertText {
            id: BlockId(BLOCK_A.into()),
            offset: 0,
            text: "x".into(),
        },
        Some(&occ(CHILD_A1)),
    );
    assert!(session.draft().content.is_empty());
    let exported = session.object_bytes(BLOCK_A).unwrap();
    assert_eq!(exported, opaque);
}

#[test]
fn notes_gate_rejects_heading_to_code() {
    let mut session = open_fixture();
    session.apply(
        BlockOp::SetType {
            id: BlockId(BLOCK_A.into()),
            kind: BlockType::Code,
        },
        Some(&occ(CHILD_A1)),
    );
    assert_eq!(session.snapshots()[0].block_type, BlockType::Paragraph);
    assert!(session.draft().content.is_empty());
}

fn open_editor() -> CompositionSession {
    CompositionSession::open(fixture(), EditorGate::Editor, IdSource::default())
}

const NEST_CHILD: &str = "ashlar/child/opaque/n00child00000000000000in";

#[test]
fn editor_gate_writes_code_marks_and_language() {
    let mut session = open_editor();
    session.apply(
        BlockOp::SetType {
            id: BlockId(BLOCK_A.into()),
            kind: BlockType::Code,
        },
        Some(&occ(CHILD_A1)),
    );
    assert_eq!(session.snapshots()[0].block_type, BlockType::Code);
    session.apply(
        BlockOp::SetProp {
            id: BlockId(BLOCK_A.into()),
            key: "language",
            value: LwwValue::String("rust".into()),
        },
        Some(&occ(CHILD_A1)),
    );
    assert_eq!(
        session.snapshots()[0].language.as_deref(),
        Some("rust")
    );
    session.apply(
        BlockOp::ToggleMark {
            id: BlockId(BLOCK_A.into()),
            start: 0,
            end: 2,
            mark: "bold".into(),
        },
        Some(&occ(CHILD_A1)),
    );
    assert!(
        session.snapshots()[0].runs.iter().any(|run| matches!(
            run,
            loro::TextDelta::Insert {
                attributes: Some(attrs),
                ..
            } if attrs.get("bold").is_some()
        )),
        "bold mark should apply: {:?}",
        session.snapshots()[0].runs
    );
    let draft = session.draft();
    assert!(draft.content.contains_key(BLOCK_A));
    assert!(draft.relations.is_empty());
}

#[test]
fn editor_indent_reparents_child_and_undo_restores_occurrence() {
    let mut session = CompositionSession::open(
        fixture(),
        EditorGate::Editor,
        ids(&[], &[NEST_CHILD]),
    );
    let content = session.snapshots()[1].id.0.clone();
    let original = session.snapshots()[1]
        .occurrence_id
        .as_ref()
        .unwrap()
        .0
        .clone();
    session.apply(
        BlockOp::Indent {
            id: BlockId(BLOCK_B.into()),
        },
        Some(&occ(CHILD_B1)),
    );
    assert_eq!(session.snapshots()[1].id.0, content);
    assert_eq!(session.snapshots()[1].indent, 1);
    let nested = session.snapshots()[1]
        .occurrence_id
        .as_ref()
        .unwrap()
        .0
        .clone();
    assert_ne!(nested, original, "cross-parent indent allocates a new Child");
    assert_eq!(nested, NEST_CHILD);
    let occs = session.occurrences();
    assert_eq!(occs[1].parent, BLOCK_A);
    assert_eq!(occs[1].child, BLOCK_B);
    assert_eq!(occs[1].slot, "body");
    let draft = session.draft();
    assert!(
        draft
            .relations
            .iter()
            .any(|rel| rel.id == original && rel.tombstone)
    );
    assert!(
        draft
            .relations
            .iter()
            .any(|rel| rel.id == nested && !rel.tombstone)
    );
    assert_eq!(
        draft.placements,
        vec![(
            nested.clone(),
            PlaceIntent::Append {
                parent: BLOCK_A.into(),
                slot: "body".into(),
            }
        )]
    );
    assert!(!draft
        .read_set
        .collections
        .contains_key(&(BLOCK_A.into(), "body".into())));

    session.undo();
    assert_eq!(
        session.snapshots()[1]
            .occurrence_id
            .as_ref()
            .unwrap()
            .0,
        original
    );
    assert_eq!(session.snapshots()[1].indent, 0);
    assert_eq!(session.occurrences()[1].parent, NOTE);
    assert_eq!(session.occurrences()[1].child, BLOCK_B);
}

#[test]
fn indent_before_flush_reanchors_following_new_child() {
    let mut session = CompositionSession::open(
        fixture(),
        EditorGate::Editor,
        ids(&[NEW_BLOCK], &[NEW_CHILD, NEST_CHILD]),
    );
    session.create(BlockType::Paragraph, Some(CHILD_B1));
    assert_eq!(
        session
            .draft()
            .placements
            .iter()
            .find(|(id, _)| id == NEW_CHILD)
            .map(|(_, place)| place),
        Some(&PlaceIntent::PlaceAfter {
            parent: NOTE.into(),
            slot: "body".into(),
            after: CHILD_B1.into(),
        })
    );

    session.apply(
        BlockOp::Indent {
            id: BlockId(BLOCK_B.into()),
        },
        Some(&occ(CHILD_B1)),
    );
    let draft = session.draft();
    assert_eq!(
        draft
            .placements
            .iter()
            .find(|(id, _)| id == NEW_CHILD)
            .map(|(_, place)| place),
        Some(&PlaceIntent::PlaceAfter {
            parent: NOTE.into(),
            slot: "body".into(),
            after: CHILD_A1.into(),
        })
    );
    assert!(draft.placements.iter().all(|(_, place)| {
        !matches!(
            place,
            PlaceIntent::PlaceAfter { after, .. } if after == CHILD_B1
        )
    }));
}

#[test]
fn indent_new_child_before_flush_emits_only_live_relation() {
    const PARENT: &str = "ashlar/block/opaque/n00blk00000000000000000pa";
    const CHILD: &str = "ashlar/block/opaque/n00blk00000000000000000ch";
    const ORIGINAL: &str = "ashlar/child/opaque/n00child00000000000000or";
    let mut session = CompositionSession::open(
        fixture(),
        EditorGate::Editor,
        ids(&[PARENT, CHILD], &[NEW_CHILD, ORIGINAL, NEST_CHILD]),
    );
    session.create(BlockType::Bullet, Some(CHILD_A2));
    session.create(BlockType::Bullet, Some(NEW_CHILD));
    session.apply(
        BlockOp::Indent { id: BlockId(CHILD.into()) },
        Some(&occ(ORIGINAL)),
    );

    let draft = session.draft();
    assert!(draft.relations.iter().all(|relation| relation.id != ORIGINAL));
    assert!(draft.relations.iter().any(|relation| {
        relation.id == NEST_CHILD && !relation.tombstone
    }));
    assert!(!draft.read_set.collections.contains_key(&(PARENT.into(), "body".into())));
}

#[test]
fn editor_outdent_returns_to_root_collection() {
    let mut session = CompositionSession::open(
        fixture(),
        EditorGate::Editor,
        ids(&[], &[NEST_CHILD, "ashlar/child/opaque/n00child00000000000000ou"]),
    );
    session.apply(
        BlockOp::Indent {
            id: BlockId(BLOCK_B.into()),
        },
        Some(&occ(CHILD_B1)),
    );
    session.apply(
        BlockOp::Outdent {
            id: BlockId(BLOCK_B.into()),
        },
        Some(&occ(NEST_CHILD)),
    );
    assert_eq!(session.snapshots()[1].indent, 0);
    assert_eq!(session.occurrences()[1].parent, NOTE);
    assert_eq!(session.occurrences()[1].child, BLOCK_B);
    assert_ne!(
        session.occurrences()[1].relation_id,
        NEST_CHILD,
        "outdent is a new occurrence, not the indented id"
    );
}

#[test]
fn editor_image_stays_read_only() {
    let mut objects = BTreeMap::new();
    objects.insert(
        BLOCK_A.into(),
        object(BLOCK_A, BlockType::Image, ""),
    );
    let mut session = CompositionSession::open(
        CompositionRead {
            root: NOTE.into(),
            root_type: "ashlar.Note".into(),
            version_tip: "tip".into(),
            objects,
            children: vec![child(CHILD_A1, BLOCK_A, "a")],
            collection_versions: BTreeMap::new(),
        },
        EditorGate::Editor,
        IdSource::default(),
    );
    assert!(session.snapshots()[0].read_only);
    session.apply(
        BlockOp::InsertText {
            id: BlockId(BLOCK_A.into()),
            offset: 0,
            text: "x".into(),
        },
        Some(&occ(CHILD_A1)),
    );
    assert!(session.draft().content.is_empty());
}

#[test]
fn notes_gate_still_ignores_indent() {
    let mut session = open_fixture();
    session.apply(
        BlockOp::Indent {
            id: BlockId(BLOCK_B.into()),
        },
        Some(&occ(CHILD_B1)),
    );
    assert_eq!(session.occurrences()[1].parent, NOTE);
    assert_eq!(session.snapshots()[1].indent, 0);
    assert!(session.draft().relations.is_empty());
}
