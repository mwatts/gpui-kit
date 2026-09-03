//! Loro document schema shared by the codec and the block editor.

use std::ops::Range;

use loro::{
    Container, ExpandType, LoroDoc, LoroMap, LoroMovableList, LoroResult, LoroText, LoroValue,
    StyleConfig, StyleConfigMap, TextDelta, ValueOrContainer, cursor::PosType,
};
use uuid::Uuid;

/// Stable block identity (UUID string).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub String);

impl BlockId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for BlockId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for BlockId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for BlockId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Closed block vocabulary (Bezel / plan).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlockType {
    #[default]
    Paragraph,
    Heading {
        level: u8,
    },
    Bullet,
    Ordered,
    Task,
    Quote,
    Code,
    Image,
    Bookmark,
    Table,
    Rule,
}

impl BlockType {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paragraph => "paragraph",
            Self::Heading { level: 1 } => "h1",
            Self::Heading { level: 2 } => "h2",
            Self::Heading { level: 3 } => "h3",
            Self::Heading { level: 4 } => "h4",
            Self::Heading { level: 5 } => "h5",
            Self::Heading { level: 6 } => "h6",
            Self::Heading { .. } => "h1",
            Self::Bullet => "bullet",
            Self::Ordered => "number",
            Self::Task => "task",
            Self::Quote => "quote",
            Self::Code => "code",
            Self::Image => "image",
            Self::Bookmark => "bookmark",
            Self::Table => "table",
            Self::Rule => "rule",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "paragraph" => Self::Paragraph,
            "h1" => Self::Heading { level: 1 },
            "h2" => Self::Heading { level: 2 },
            "h3" => Self::Heading { level: 3 },
            "h4" => Self::Heading { level: 4 },
            "h5" => Self::Heading { level: 5 },
            "h6" => Self::Heading { level: 6 },
            "bullet" => Self::Bullet,
            "number" => Self::Ordered,
            "task" => Self::Task,
            "quote" => Self::Quote,
            "code" => Self::Code,
            "image" => Self::Image,
            "bookmark" => Self::Bookmark,
            "table" => Self::Table,
            "rule" => Self::Rule,
            _ => return None,
        })
    }

    #[must_use]
    pub fn is_list_marker(self) -> bool {
        matches!(self, Self::Bullet | Self::Ordered | Self::Task)
    }

    #[must_use]
    pub fn has_body(self) -> bool {
        matches!(
            self,
            Self::Paragraph
                | Self::Heading { .. }
                | Self::Bullet
                | Self::Ordered
                | Self::Task
                | Self::Quote
                | Self::Code
        )
    }
}

/// Bookmark / mention display form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Form {
    #[default]
    Auto,
    Chip,
    Embed,
}

impl Form {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Chip => "chip",
            Self::Embed => "embed",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "auto" => Self::Auto,
            "chip" => Self::Chip,
            "embed" => Self::Embed,
            _ => return None,
        })
    }

    #[must_use]
    pub fn title(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Chip => Some("chip"),
            Self::Embed => Some("embed"),
        }
    }

    #[must_use]
    pub fn from_title(title: &str) -> Option<Self> {
        match title {
            "chip" => Some(Self::Chip),
            "embed" => Some(Self::Embed),
            _ => None,
        }
    }
}

/// GFM table column alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "center" => Self::Center,
            "right" => Self::Right,
            _ => Self::Left,
        }
    }
}

/// Comment thread state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentState {
    Open,
    Resolved,
}

impl CommentState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "open" => Self::Open,
            "resolved" => Self::Resolved,
            _ => return None,
        })
    }
}

/// Configure Loro text-style expand behaviour before any mark.
pub fn configure_text_styles(doc: &LoroDoc) {
    let mut styles = StyleConfigMap::new();
    for (key, expand) in [
        ("bold", ExpandType::After),
        ("italic", ExpandType::After),
        ("strike", ExpandType::After),
        ("code", ExpandType::None),
        ("link", ExpandType::None),
        ("mention", ExpandType::None),
        ("comment", ExpandType::Both),
        ("image", ExpandType::None),
    ] {
        styles.insert(key.into(), StyleConfig { expand });
    }
    doc.config_text_style(styles);
}

/// Fresh document: one empty paragraph and an empty comments map.
#[must_use]
pub fn new_empty_doc() -> LoroDoc {
    let doc = LoroDoc::new();
    configure_text_styles(&doc);
    let _ = doc.get_map("comments");
    let blocks = doc.get_movable_list("blocks");
    let _ = insert_block_map(&blocks, 0, &BlockId::new(), BlockType::Paragraph, 0);
    doc.commit();
    doc
}

/// Root `blocks` movable list.
#[must_use]
pub fn blocks_list(doc: &LoroDoc) -> LoroMovableList {
    doc.get_movable_list("blocks")
}

/// Root `comments` map.
#[must_use]
pub fn comments_map(doc: &LoroDoc) -> LoroMap {
    doc.get_map("comments")
}

/// Insert a new block map at `index` with mergeable `content` text.
pub fn insert_block_map(
    list: &LoroMovableList,
    index: usize,
    id: &BlockId,
    block_type: BlockType,
    indent: i64,
) -> LoroResult<LoroMap> {
    let map = list.insert_container(index, LoroMap::new())?;
    map.insert("id", id.as_str())?;
    map.insert("type", block_type.as_str())?;
    map.insert("indent", indent)?;
    if block_type.has_body() || matches!(block_type, BlockType::Image) {
        if matches!(block_type, BlockType::Image) {
            let _ = map.ensure_mergeable_text("alt")?;
        } else {
            let _ = map.ensure_mergeable_text("content")?;
        }
    }
    if matches!(block_type, BlockType::Code) {
        // Code uses content; already ensured above.
    }
    Ok(map)
}

/// Read the block map at `index`.
pub fn block_map_at(list: &LoroMovableList, index: usize) -> Option<LoroMap> {
    match list.get(index)? {
        ValueOrContainer::Container(Container::Map(map)) => Some(map),
        _ => None,
    }
}

/// Find a block by id; returns (index, map).
pub fn find_block(doc: &LoroDoc, id: &BlockId) -> Option<(usize, LoroMap)> {
    let list = blocks_list(doc);
    for i in 0..list.len() {
        let Some(map) = block_map_at(&list, i) else {
            continue;
        };
        if map_string(&map, "id").as_deref() == Some(id.as_str()) {
            return Some((i, map));
        }
    }
    None
}

/// Body / caption text container for a block map.
pub fn content_text(map: &LoroMap) -> Option<LoroText> {
    match map.get("content")? {
        ValueOrContainer::Container(Container::Text(t)) => Some(t),
        _ => map.ensure_mergeable_text("content").ok(),
    }
}

/// Image caption text.
pub fn alt_text(map: &LoroMap) -> Option<LoroText> {
    match map.get("alt")? {
        ValueOrContainer::Container(Container::Text(t)) => Some(t),
        _ => map.ensure_mergeable_text("alt").ok(),
    }
}

/// Ensure mergeable content and return it.
pub fn ensure_content(map: &LoroMap) -> LoroResult<LoroText> {
    map.ensure_mergeable_text("content")
}

/// Ensure mergeable alt and return it.
pub fn ensure_alt(map: &LoroMap) -> LoroResult<LoroText> {
    map.ensure_mergeable_text("alt")
}

#[must_use]
pub fn map_string(map: &LoroMap, key: &str) -> Option<String> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

#[must_use]
pub fn map_i64(map: &LoroMap, key: &str) -> Option<i64> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::I64(n)) => Some(n),
        _ => None,
    }
}

#[must_use]
pub fn map_bool(map: &LoroMap, key: &str) -> Option<bool> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::Bool(b)) => Some(b),
        _ => None,
    }
}

#[must_use]
pub fn block_type_of(map: &LoroMap) -> BlockType {
    map_string(map, "type")
        .as_deref()
        .and_then(BlockType::parse)
        .unwrap_or(BlockType::Paragraph)
}

#[must_use]
pub fn indent_of(map: &LoroMap) -> i64 {
    map_i64(map, "indent").unwrap_or(0).max(0)
}

/// Bezel-style renumber: first ordered at an indent keeps its start; later
/// siblings in the run become consecutive.
pub fn repair_numbers(doc: &LoroDoc) {
    let list = blocks_list(doc);
    let mut expected: Vec<Option<i64>> = Vec::new();
    for i in 0..list.len() {
        let Some(map) = block_map_at(&list, i) else {
            continue;
        };
        let indent = indent_of(&map) as usize;
        expected.truncate(indent + 1);
        expected.resize(indent + 1, None);

        if block_type_of(&map) == BlockType::Ordered {
            let current = map_i64(&map, "number").unwrap_or(1).max(1);
            let number = if let Some(next) = expected[indent] {
                next
            } else {
                current
            };
            let _ = map.insert("number", number);
            expected[indent] = Some(number.saturating_add(1));
        } else {
            expected[indent] = None;
        }
    }
}

/// Write plain text + ranked/typed marks onto a LoroText (UTF-8).
pub fn write_rich_text(
    text: &LoroText,
    plain: &str,
    marks: &[(Range<usize>, RichMark)],
) -> LoroResult<()> {
    if text.len_utf8() > 0 {
        text.delete_utf8(0, text.len_utf8())?;
    }
    if !plain.is_empty() {
        text.insert_utf8(0, plain)?;
    }
    for (range, mark) in marks {
        if range.start > range.end || range.end > plain.len() {
            continue;
        }
        if range.is_empty() && !matches!(mark, RichMark::Image(_)) {
            continue;
        }
        match mark {
            RichMark::Bold(rank) => text.mark_utf8(range.clone(), "bold", *rank)?,
            RichMark::Italic(rank) => text.mark_utf8(range.clone(), "italic", *rank)?,
            RichMark::Strike(rank) => text.mark_utf8(range.clone(), "strike", *rank)?,
            RichMark::Code => text.mark_utf8(range.clone(), "code", true)?,
            RichMark::Link(url) => text.mark_utf8(range.clone(), "link", url.as_str())?,
            RichMark::Mention { url, form } => {
                let value = format!("{}|{}", form.as_str(), url);
                text.mark_utf8(range.clone(), "mention", value.as_str())?;
            }
            RichMark::Image(url) => text.mark_utf8(range.clone(), "image", url.as_str())?,
            RichMark::Comment(id) => text.mark_utf8(range.clone(), "comment", id.as_str())?,
        }
    }
    Ok(())
}

/// Mark value used when writing rich text into Loro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RichMark {
    Bold(i64),
    Italic(i64),
    Strike(i64),
    Code,
    Link(String),
    Mention { url: String, form: Form },
    Image(String),
    Comment(String),
}

/// Read marks from a text delta into ranked spans (outermost-first for emphasis).
#[must_use]
pub fn marks_from_delta(delta: &[TextDelta]) -> (String, Vec<(Range<usize>, RichMark)>) {
    let mut plain = String::new();
    let mut open: Vec<(String, LoroValue, usize)> = Vec::new();
    let mut closed: Vec<(Range<usize>, RichMark)> = Vec::new();

    for item in delta {
        let TextDelta::Insert { insert, attributes } = item else {
            continue;
        };
        let start = plain.len();
        plain.push_str(insert);
        let end = plain.len();
        let attrs = attributes.clone().unwrap_or_default();

        // Close marks that ended.
        open.retain(|(key, value, mark_start)| {
            let still = attrs.get(key).is_some_and(|v| v == value);
            if !still {
                if let Some(mark) = rich_mark_from(key, value) {
                    closed.push((*mark_start..start, mark));
                }
            }
            still
        });

        for (key, value) in &attrs {
            if open.iter().any(|(k, v, _)| k == key && v == value) {
                continue;
            }
            open.push((key.clone(), value.clone(), start));
        }
        let _ = end;
    }

    for (key, value, mark_start) in open {
        if let Some(mark) = rich_mark_from(&key, &value) {
            closed.push((mark_start..plain.len(), mark));
        }
    }

    // Emphasis: sort by rank so outermost (lower rank) comes first.
    closed.sort_by(|a, b| {
        let rank_a = emphasis_rank(&a.1);
        let rank_b = emphasis_rank(&b.1);
        match (rank_a, rank_b) {
            (Some(ra), Some(rb)) => ra.cmp(&rb).then_with(|| a.0.start.cmp(&b.0.start)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.0.start.cmp(&b.0.start),
        }
    });

    (plain, closed)
}

fn emphasis_rank(mark: &RichMark) -> Option<i64> {
    match mark {
        RichMark::Bold(r) | RichMark::Italic(r) | RichMark::Strike(r) => Some(*r),
        _ => None,
    }
}

fn rich_mark_from(key: &str, value: &LoroValue) -> Option<RichMark> {
    match key {
        "bold" => Some(RichMark::Bold(value_as_i64(value).unwrap_or(0))),
        "italic" => Some(RichMark::Italic(value_as_i64(value).unwrap_or(0))),
        "strike" => Some(RichMark::Strike(value_as_i64(value).unwrap_or(0))),
        "code" => Some(RichMark::Code),
        "link" => value.as_string().map(|s| RichMark::Link(s.to_string())),
        "image" => value.as_string().map(|s| RichMark::Image(s.to_string())),
        "comment" => value.as_string().map(|s| RichMark::Comment(s.to_string())),
        "mention" => {
            let s = value.as_string()?;
            let (form, url) = s.split_once('|').unwrap_or(("auto", s.as_str()));
            Some(RichMark::Mention {
                url: url.to_string(),
                form: Form::parse(form).unwrap_or(Form::Auto),
            })
        }
        _ => None,
    }
}

fn value_as_i64(value: &LoroValue) -> Option<i64> {
    match value {
        LoroValue::I64(n) => Some(*n),
        LoroValue::Bool(true) => Some(0),
        LoroValue::Double(n) => Some(*n as i64),
        _ => None,
    }
}

/// Replay a sliced delta onto `text` at UTF-8 byte offset `at`, preserving marks.
pub fn replay_delta_at(text: &LoroText, at: usize, delta: &[TextDelta]) -> LoroResult<()> {
    let mut pos = at;
    // Insert plain first, then mark — avoids expand:After bleeding across segments.
    let mut pending_marks: Vec<(Range<usize>, String, LoroValue)> = Vec::new();
    for item in delta {
        let TextDelta::Insert { insert, attributes } = item else {
            continue;
        };
        let start = pos;
        if !insert.is_empty() {
            text.insert_utf8(pos, insert)?;
            pos += insert.len();
        }
        if let Some(attrs) = attributes {
            for (key, value) in attrs {
                pending_marks.push((start..pos, key.clone(), value.clone()));
            }
        }
    }
    for (range, key, value) in pending_marks {
        if range.start < range.end {
            text.mark_utf8(range, &key, value)?;
        }
    }
    Ok(())
}

/// Slice the tail of a text (UTF-8), including marks.
pub fn slice_delta_utf8(text: &LoroText, start: usize, end: usize) -> LoroResult<Vec<TextDelta>> {
    text.slice_delta(start, end, PosType::Bytes)
}

/// Unmark using UTF-8 offsets (Loro's public `unmark` is Unicode-only).
pub fn unmark_utf8(text: &LoroText, range: Range<usize>, key: &str) -> LoroResult<()> {
    text.mark_utf8(range, key, LoroValue::Null)
}

/// Max stacking rank for an emphasis key over `[start, end)`.
#[must_use]
pub fn max_rank_in_range(text: &LoroText, key: &str, start: usize, end: usize) -> i64 {
    let mut max = -1i64;
    let mut pos = 0usize;
    for item in text.to_delta() {
        let TextDelta::Insert { insert, attributes } = item else {
            continue;
        };
        let next = pos + insert.len();
        if next > start && pos < end {
            if let Some(attrs) = attributes {
                if let Some(v) = attrs.get(key) {
                    max = max.max(value_as_i64(v).unwrap_or(0));
                }
            }
        }
        pos = next;
    }
    max
}

/// Whether `key` covers the whole `[start, end)` range.
#[must_use]
pub fn mark_covers(text: &LoroText, key: &str, start: usize, end: usize) -> bool {
    if start >= end {
        return false;
    }
    let mut pos = 0usize;
    let mut covered = 0usize;
    for item in text.to_delta() {
        let TextDelta::Insert { insert, attributes } = item else {
            continue;
        };
        let next = pos + insert.len();
        let overlap_start = pos.max(start);
        let overlap_end = next.min(end);
        if overlap_start < overlap_end {
            let has = attributes
                .as_ref()
                .is_some_and(|a| a.get(key).is_some_and(|v| !matches!(v, LoroValue::Null)));
            if has {
                covered += overlap_end - overlap_start;
            } else {
                return false;
            }
        }
        pos = next;
    }
    covered >= end.saturating_sub(start)
}

/// Code-span ranges overlapping `[start, end)`.
#[must_use]
pub fn code_spans_touching(text: &LoroText, start: usize, end: usize) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut pos = 0usize;
    for item in text.to_delta() {
        let TextDelta::Insert { insert, attributes } = item else {
            continue;
        };
        let next = pos + insert.len();
        let is_code = attributes
            .as_ref()
            .is_some_and(|a| a.get("code").is_some_and(|v| !matches!(v, LoroValue::Null)));
        if is_code && next > start && pos < end {
            // Merge contiguous code runs.
            if let Some(last) = out.last_mut() {
                if last.end == pos {
                    last.end = next;
                } else {
                    out.push(pos..next);
                }
            } else {
                out.push(pos..next);
            }
        }
        pos = next;
    }
    out
}

/// Grow `[start, end)` to cover any code span it half-intersects (Bezel rule).
#[must_use]
pub fn grow_for_code_spans(text: &LoroText, start: usize, end: usize) -> Range<usize> {
    let mut range = start..end;
    for code in code_spans_touching(text, start, end) {
        let crosses = (range.start > code.start && range.start < code.end)
            || (range.end > code.start && range.end < code.end);
        if crosses {
            range.start = range.start.min(code.start);
            range.end = range.end.max(code.end);
        }
    }
    range
}
