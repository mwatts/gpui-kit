//! Markdown import/export for the Loro-backed block editor.
//!
//! `hydrate` parses markdown into a fresh [`loro::LoroDoc`]. `project_markdown`
//! projects that document back to markdown for search and embed fields.
//!
//! Ported from Bezel (`crabtalk/bezel`). See `NOTICE` in this crate.

mod hydrate;
mod project;
mod schema;

pub use hydrate::{hydrate, is_image, is_url};
pub use project::project_markdown;
pub use schema::{
    Align, BlockId, BlockType, CommentState, Form, RichMark, alt_text, block_map_at, block_type_of,
    blocks_list, code_spans_touching, comments_map, configure_text_styles, content_text,
    ensure_alt, ensure_content, find_block, grow_for_code_spans, indent_of, insert_block_map,
    map_bool, map_i64, map_string, mark_covers, marks_from_delta, max_rank_in_range, new_empty_doc,
    repair_numbers, replay_delta_at, slice_delta_utf8, unmark_utf8, write_rich_text,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{block_map_at, block_type_of, blocks_list, map_i64};

    #[test]
    fn tests_collect() {
        // Guarantees this suite is not an empty harness.
        assert!(true);
    }

    #[test]
    fn nested_marks_round_trip_spellings() {
        let a = project_markdown(&hydrate("**_x_**"));
        let b = project_markdown(&hydrate("_**x**_"));
        assert_eq!(a, "**_x_**", "bold-outside-italic");
        assert_eq!(b, "_**x**_", "italic-outside-bold");
        assert_eq!(project_markdown(&hydrate(&a)), a);
        assert_eq!(project_markdown(&hydrate(&b)), b);
    }

    #[test]
    fn ordered_start_three() {
        let doc = hydrate("3. a");
        let list = blocks_list(&doc);
        assert_eq!(list.len(), 1);
        let map = block_map_at(&list, 0).unwrap();
        assert_eq!(block_type_of(&map), BlockType::Ordered);
        assert_eq!(map_i64(&map, "number"), Some(3));
        let md = project_markdown(&doc);
        assert!(
            md.starts_with("3."),
            "expected list starting at 3, got {md:?}"
        );
    }

    #[test]
    fn fixed_point_common_blocks() {
        for source in [
            "hello",
            "# Title",
            "## H2",
            "- a\n- b",
            "1. a\n2. b",
            "3. a\n4. b",
            "- [ ] todo",
            "- [x] done",
            "> quote",
            "```\ncode\n```",
            "```rust\nfn main() {}\n```",
            "---",
            "para\n\nnext",
            "**bold** and _italic_",
            "~~strike~~",
            "`code`",
            "![alt](https://example.com/x.png)",
            "| a | b |\n| --- | --- |\n| 1 | 2 |",
        ] {
            let once = project_markdown(&hydrate(source));
            let twice = project_markdown(&hydrate(&once));
            assert_eq!(once, twice, "not a fixed point for {source:?} → {once:?}");
        }
    }

    #[test]
    fn empty_hydrates_to_one_paragraph() {
        let doc = hydrate("");
        let list = blocks_list(&doc);
        assert_eq!(list.len(), 1);
        assert_eq!(
            block_type_of(&block_map_at(&list, 0).unwrap()),
            BlockType::Paragraph
        );
    }
}
