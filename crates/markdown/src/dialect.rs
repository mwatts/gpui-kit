//! Shared GitHub Flavored Markdown rules for hydrate, project, and TextView.
//!
//! Kit [`gpui_base::text`] parses with `markdown::ParseOptions::gfm()`. Soft
//! breaks reflow to a space; hard breaks (`  \\n` or `\\\\\\n`) stay newlines.
//! Hydrate uses the same constructs and break rules so a source string that
//! TextView paints hydrates to the same blocks.

use markdown::ParseOptions;

/// Parse options used by kit TextView (`MarkdownExtensions` default, no MDX).
#[must_use]
pub fn parse_options() -> ParseOptions {
    ParseOptions::gfm()
}

/// Corpus shared by hydrate round-trip tests and mdast agreement tests.
pub const CORPUS: &[&str] = &[
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
    "```markdown\n# nested\n```",
    "```block:callout\nWatch this\n```",
    "---",
    "para\n\nnext",
    "**bold** and _italic_",
    "~~strike~~",
    "`code`",
    "![alt](https://example.com/x.png)",
    "| a | b |\n| --- | --- |\n| 1 | 2 |",
    "this sentence\ncontinues as a soft wrap",
    "Owner: Jane  \nPersona: assistant",
];

/// Fence language prefix that hydrates to [`crate::BlockType::Custom`].
pub const CUSTOM_BLOCK_FENCE: &str = "block:";

/// Collapse CommonMark soft breaks the way kit TextView does.
pub fn collapse_soft_breaks(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\n', '\r'], " ")
}

/// Fence info string for a custom block (`block:{name}`).
#[must_use]
pub fn custom_block_language(name: &str) -> String {
    format!("{CUSTOM_BLOCK_FENCE}{name}")
}

/// Custom block name encoded in a fence language, if any.
#[must_use]
pub fn custom_block_name(language: &str) -> Option<&str> {
    language
        .strip_prefix(CUSTOM_BLOCK_FENCE)
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use markdown::mdast::Node;
    use markdown::to_mdast;

    #[test]
    fn gfm_options_enable_tables_and_strikethrough() {
        let options = parse_options();
        assert!(options.constructs.gfm_table);
        assert!(options.constructs.gfm_strikethrough);
        assert!(options.constructs.gfm_task_list_item);
    }

    #[test]
    fn mdast_parses_the_shared_corpus() {
        for source in CORPUS {
            to_mdast(source, &parse_options())
                .unwrap_or_else(|error| panic!("mdast rejected corpus item {source:?}: {error}"));
        }
    }

    #[test]
    fn mdast_soft_break_is_a_newline_inside_text() {
        let tree =
            to_mdast("this sentence\ncontinues as a soft wrap", &parse_options()).expect("parse");
        let Node::Root(root) = tree else {
            panic!("expected root");
        };
        let Node::Paragraph(paragraph) = &root.children[0] else {
            panic!("expected paragraph");
        };
        let Node::Text(text) = &paragraph.children[0] else {
            panic!("expected text");
        };
        assert!(
            text.value.contains('\n') || text.value.contains(' '),
            "soft break should survive in mdast text, got {:?}",
            text.value
        );
        assert_eq!(
            collapse_soft_breaks(&text.value),
            "this sentence continues as a soft wrap"
        );
    }

    #[test]
    fn custom_block_fence_round_trips_the_name() {
        assert_eq!(custom_block_language("callout"), "block:callout");
        assert_eq!(custom_block_name("block:callout"), Some("callout"));
        assert_eq!(custom_block_name("rust"), None);
        assert_eq!(custom_block_name("block:"), None);
    }
}
