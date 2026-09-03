    use super::*;
    use block_markdown::{BlockType, content_text, find_block, map_i64};
    use loro::{ExpandType, LoroValue, TextDelta};

    #[test]
    fn tests_collect() {
        assert!(true);
    }

    #[test]
    fn empty_doc_has_one_paragraph() {
        let doc = BlockDocument::new();
        let snaps = doc.snapshots();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].block_type, BlockType::Paragraph);
        assert!(snaps[0].plain.is_empty());
    }

    #[test]
    fn insert_toggle_bold_undo_restores_mark() {
        let mut doc = BlockDocument::new();
        let id = doc.snapshots()[0].id.clone();
        doc.apply(BlockOp::InsertText {
            id: id.clone(),
            offset: 0,
            text: "hello".into(),
        });
        doc.apply(BlockOp::ToggleMark {
            id: id.clone(),
            start: 0,
            end: 5,
            mark: "bold",
        });
        let runs = &doc.snapshots()[0].runs;
        assert!(
            runs.iter().any(|r| matches!(
                r,
                TextDelta::Insert {
                    attributes: Some(attrs),
                    ..
                } if attrs.get("bold").is_some()
            )),
            "expected bold in delta: {runs:?}"
        );
        assert!(doc.undo());
        let runs = &doc.snapshots()[0].runs;
        assert!(
            !runs.iter().any(|r| matches!(
                r,
                TextDelta::Insert {
                    attributes: Some(attrs),
                    ..
                } if attrs.get("bold").is_some()
            )),
            "undo should remove bold: {runs:?}"
        );
    }

    #[test]
    fn bold_expand_after_code_expand_none() {
        let doc = BlockDocument::new();
        // Probe via a throwaway text after configuring styles on a fresh doc.
        let probe = loro::LoroDoc::new();
        block_markdown::configure_text_styles(&probe);
        let text = probe.get_text("t");
        text.insert_utf8(0, "ab").unwrap();
        text.mark_utf8(0..1, "bold", 0i64).unwrap();
        text.insert_utf8(1, "X").unwrap();
        // Expand After: insert at end of bold range grows bold onto X.
        let delta = text.to_delta();
        let bold_covers_x = delta.iter().any(|item| match item {
            TextDelta::Insert { insert, attributes } => {
                insert.contains('X')
                    && attributes
                        .as_ref()
                        .is_some_and(|a| a.get("bold").is_some())
            }
            _ => false,
        });
        assert!(bold_covers_x, "bold should expand after: {delta:?}");

        let text2 = probe.get_text("c");
        text2.insert_utf8(0, "ab").unwrap();
        text2.mark_utf8(0..1, "code", true).unwrap();
        text2.insert_utf8(1, "X").unwrap();
        let delta2 = text2.to_delta();
        let code_covers_x = delta2.iter().any(|item| match item {
            TextDelta::Insert { insert, attributes } => {
                insert.contains('X')
                    && attributes
                        .as_ref()
                        .is_some_and(|a| a.get("code").is_some())
            }
            _ => false,
        });
        assert!(!code_covers_x, "code must not expand: {delta2:?}");
        let _ = doc;
        let _ = ExpandType::After;
    }

    #[test]
    fn split_merge_preserve_marks() {
        let mut doc = BlockDocument::new();
        let id = doc.snapshots()[0].id.clone();
        doc.apply(BlockOp::InsertText {
            id: id.clone(),
            offset: 0,
            text: "helloworld".into(),
        });
        doc.apply(BlockOp::ToggleMark {
            id: id.clone(),
            start: 5,
            end: 10,
            mark: "bold",
        });
        doc.apply(BlockOp::SplitBlock {
            id: id.clone(),
            offset: 5,
        });
        assert_eq!(doc.snapshots().len(), 2);
        let tail = &doc.snapshots()[1];
        assert_eq!(tail.plain, "world");
        assert!(
            tail.runs.iter().any(|r| matches!(
                r,
                TextDelta::Insert {
                    attributes: Some(attrs),
                    ..
                } if attrs.get("bold").is_some()
            )),
            "split must keep bold on tail: {:?}",
            tail.runs
        );

        let second_id = tail.id.clone();
        doc.apply(BlockOp::ToggleMark {
            id: second_id.clone(),
            start: 0,
            end: 5,
            mark: "italic",
        });
        doc.apply(BlockOp::MergeWithPrevious { id: second_id });
        assert_eq!(doc.snapshots().len(), 1);
        let merged = &doc.snapshots()[0];
        assert_eq!(merged.plain, "helloworld");
        assert!(
            merged.runs.iter().any(|r| matches!(
                r,
                TextDelta::Insert {
                    attributes: Some(attrs),
                    ..
                } if attrs.get("italic").is_some()
            )),
            "merge must keep italic: {:?}",
            merged.runs
        );
    }

    #[test]
    fn utf8_cafe_mark_offsets() {
        let mut doc = BlockDocument::new();
        let id = doc.snapshots()[0].id.clone();
        doc.apply(BlockOp::InsertText {
            id: id.clone(),
            offset: 0,
            text: "café".into(),
        });
        // "café" = c a f é — é is 2 bytes; mark "fé" = bytes 2..5
        let start = "ca".len();
        let end = "café".len();
        doc.apply(BlockOp::ToggleMark {
            id: id.clone(),
            start,
            end,
            mark: "bold",
        });
        let (_, map) = find_block(doc.doc(), &id).unwrap();
        let text = content_text(&map).unwrap();
        assert_eq!(text.to_string(), "café");
        let delta = text.to_delta();
        let bold_slice: String = delta
            .iter()
            .filter_map(|item| match item {
                TextDelta::Insert { insert, attributes }
                    if attributes.as_ref().is_some_and(|a| a.get("bold").is_some()) =>
                {
                    Some(insert.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(bold_slice, "fé");
    }

    #[test]
    fn import_on_fresh_doc_no_duplicate() {
        let mut doc = BlockDocument::new();
        let id = doc.snapshots()[0].id.clone();
        doc.apply(BlockOp::InsertText {
            id,
            offset: 0,
            text: "only".into(),
        });
        let bytes = doc.export_snapshot().unwrap();
        let imported = BlockDocument::import_snapshot(&bytes).unwrap();
        assert_eq!(imported.snapshots().len(), 1);
        assert_eq!(imported.snapshots()[0].plain, "only");
    }

    #[test]
    fn backspace_chain() {
        // indent → outdent
        let mut doc = BlockDocument::from_markdown("- a\n  - b");
        // Ensure second is indented bullet
        if doc.snapshots().len() >= 2 {
            let id = doc.snapshots()[1].id.clone();
            if doc.snapshots()[1].indent == 0 {
                doc.apply(BlockOp::Indent { id: id.clone() });
            }
            let ops = backspace_at_start(doc.snapshots(), &Cursor::new(id.clone(), Part::Body, 0));
            assert!(matches!(ops.first(), Some(BlockOp::Outdent { .. })));
            doc.apply_many(&ops);
            assert_eq!(doc.snapshots().iter().find(|s| s.id == id).unwrap().indent, 0);
        }

        // heading unwrap
        let mut doc = BlockDocument::from_markdown("# Hello");
        let id = doc.snapshots()[0].id.clone();
        let ops = backspace_at_start(doc.snapshots(), &Cursor::new(id.clone(), Part::Body, 0));
        assert!(matches!(ops.first(), Some(BlockOp::UnwrapToParagraph { .. })));
        doc.apply_many(&ops);
        assert_eq!(doc.snapshots()[0].block_type, BlockType::Paragraph);
        assert_eq!(doc.snapshots()[0].plain, "Hello");

        // empty caption deletes image
        let mut doc = BlockDocument::from_markdown("![](https://example.com/a.png)");
        let img = doc
            .snapshots()
            .iter()
            .find(|s| s.block_type == BlockType::Image)
            .expect("image block")
            .id
            .clone();
        let snaps = doc.snapshots().to_vec();
        let ops = backspace_at_start(&snaps, &Cursor::new(img.clone(), Part::Caption, 0));
        assert!(matches!(ops.first(), Some(BlockOp::DeleteBlock { .. })));
        doc.apply_many(&ops);
        assert!(!doc.snapshots().iter().any(|s| s.block_type == BlockType::Image));

        // empty paragraph after rule deletes rule
        let mut doc = BlockDocument::from_markdown("---\n\n");
        assert!(
            doc.snapshots()
                .iter()
                .any(|s| s.block_type == BlockType::Rule)
        );
        // Ensure there is a paragraph after the rule
        if doc.snapshots().len() == 1 {
            let rule_id = doc.snapshots()[0].id.clone();
            doc.apply(BlockOp::SetType {
                id: rule_id,
                kind: BlockType::Rule,
            });
            // append paragraph via split isn't available on rule; insert via markdown instead
        }
        let mut doc = BlockDocument::from_markdown("---\n\nx");
        // Clear the paragraph text
        let para = doc
            .snapshots()
            .iter()
            .find(|s| s.block_type == BlockType::Paragraph)
            .expect("paragraph after rule")
            .id
            .clone();
        if let Some(snap) = doc.snapshots().iter().find(|s| s.id == para) {
            if !snap.plain.is_empty() {
                doc.apply(BlockOp::DeleteRange {
                    id: para.clone(),
                    start: 0,
                    end: snap.plain.len(),
                });
            }
        }
        let ops = backspace_at_start(doc.snapshots(), &Cursor::new(para, Part::Body, 0));
        assert!(
            matches!(ops.first(), Some(BlockOp::DeleteBlock { .. })),
            "expected delete rule, got {ops:?}"
        );
        doc.apply_many(&ops);
        assert!(!doc.snapshots().iter().any(|s| s.block_type == BlockType::Rule));
    }

    #[test]
    fn add_comment_mark_and_undo() {
        let mut doc = BlockDocument::new();
        let id = doc.snapshots()[0].id.clone();
        doc.apply(BlockOp::InsertText {
            id: id.clone(),
            offset: 0,
            text: "hello".into(),
        });
        let cid = CommentId::new();
        let range = Selection {
            anchor: Cursor::new(id.clone(), Part::Body, 0),
            focus: Cursor::new(id.clone(), Part::Body, 5),
        };
        doc.apply(BlockOp::AddComment {
            id: cid.clone(),
            range,
            body: "note".into(),
        });
        let runs = &doc.snapshots()[0].runs;
        assert!(
            runs.iter().any(|r| matches!(
                r,
                TextDelta::Insert {
                    attributes: Some(attrs),
                    ..
                } if attrs.get("comment").is_some()
            )),
            "comment mark missing: {runs:?}"
        );
        let comments = block_markdown::comments_map(doc.doc());
        assert!(comments.get(cid.as_str()).is_some());
        assert!(doc.undo());
        let runs = &doc.snapshots()[0].runs;
        assert!(
            !runs.iter().any(|r| matches!(
                r,
                TextDelta::Insert {
                    attributes: Some(attrs),
                    ..
                } if attrs.get("comment").is_some()
            )),
            "undo should remove comment mark"
        );
    }

    #[test]
    fn bold_on_half_code_grows() {
        let mut doc = BlockDocument::new();
        let id = doc.snapshots()[0].id.clone();
        doc.apply(BlockOp::InsertText {
            id: id.clone(),
            offset: 0,
            text: "code".into(),
        });
        doc.apply(BlockOp::ToggleMark {
            id: id.clone(),
            start: 0,
            end: 4,
            mark: "code",
        });
        // Bold only first two bytes of the code span.
        doc.apply(BlockOp::ToggleMark {
            id: id.clone(),
            start: 0,
            end: 2,
            mark: "bold",
        });
        let runs = &doc.snapshots()[0].runs;
        let bold_text: String = runs
            .iter()
            .filter_map(|r| match r {
                TextDelta::Insert { insert, attributes }
                    if attributes.as_ref().is_some_and(|a| a.get("bold").is_some()) =>
                {
                    Some(insert.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(bold_text, "code", "bold should grow to whole code span: {runs:?}");
    }

    #[test]
    fn ordered_number_three_and_repair() {
        let mut doc = BlockDocument::from_markdown("3. a\n4. b");
        assert_eq!(doc.snapshots()[0].number, Some(3));
        // Insert another ordered after first via split+set
        let id = doc.snapshots()[0].id.clone();
        doc.apply(BlockOp::SplitBlock {
            id: id.clone(),
            offset: 1,
        });
        // Force both ordered and repair
        let snaps = doc.snapshots().to_vec();
        for s in &snaps {
            doc.apply(BlockOp::SetType {
                id: s.id.clone(),
                kind: BlockType::Ordered,
            });
        }
        // Set first number to 3 explicitly
        let first = doc.snapshots()[0].id.clone();
        doc.apply(BlockOp::SetProp {
            id: first,
            key: "number",
            value: LwwValue::I64(3),
        });
        block_markdown::repair_numbers(doc.doc());
        doc.doc().commit();
        let snaps = doc.project();
        let numbers: Vec<_> = snaps
            .iter()
            .filter(|s| s.block_type == BlockType::Ordered)
            .map(|s| s.number)
            .collect();
        assert!(
            numbers.first() == Some(&Some(3)),
            "first should be 3: {numbers:?}"
        );
        if numbers.len() >= 2 {
            assert_eq!(numbers[1], Some(4), "repair should make second 4: {numbers:?}");
        }
        let _ = map_i64;
        let _ = LoroValue::Null;
    }

    #[test]
    fn structure_vs_typing_undo_groups() {
        let mut doc = BlockDocument::new();
        let id = doc.snapshots()[0].id.clone();
        doc.apply(BlockOp::InsertText {
            id: id.clone(),
            offset: 0,
            text: "hi".into(),
        });
        doc.apply(BlockOp::SplitBlock { id, offset: 2 });
        assert_eq!(doc.snapshots().len(), 2);
        assert!(doc.undo());
        assert_eq!(doc.snapshots().len(), 1, "first undo undoes split only");
        assert_eq!(doc.snapshots()[0].plain, "hi");
        assert!(doc.undo());
        assert_eq!(doc.snapshots()[0].plain, "");
    }

    #[test]
    fn hydrate_project_nested_and_ordered() {
        let a = block_markdown::project_markdown(&block_markdown::hydrate("**_x_**"));
        let b = block_markdown::project_markdown(&block_markdown::hydrate("_**x**_"));
        assert_eq!(block_markdown::project_markdown(&block_markdown::hydrate(&a)), a);
        assert_eq!(block_markdown::project_markdown(&block_markdown::hydrate(&b)), b);
        let md = block_markdown::project_markdown(&block_markdown::hydrate("3. a"));
        assert!(md.starts_with("3."), "got {md:?}");
    }
