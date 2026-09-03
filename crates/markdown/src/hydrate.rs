//! Markdown → [`LoroDoc`] (Bezel parse adapted for Loro marks).

use std::ops::Range;

use loro::LoroDoc;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};

use crate::schema::{
    Align, BlockId, BlockType, Form, RichMark, blocks_list, configure_text_styles, ensure_alt,
    ensure_content, insert_block_map, repair_numbers, write_rich_text,
};

/// Parse markdown into a fresh Loro document.
#[must_use]
pub fn hydrate(source: &str) -> LoroDoc {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut state = ParseState::default();
    for event in Parser::new_ext(source, options) {
        state.event(event);
    }
    state.renumber();

    let doc = LoroDoc::new();
    configure_text_styles(&doc);
    let _ = doc.get_map("comments");
    let list = blocks_list(&doc);

    if state.blocks.is_empty() {
        let _ = insert_block_map(&list, 0, &BlockId::new(), BlockType::Paragraph, 0);
        doc.commit();
        return doc;
    }

    for (ix, block) in state.blocks.iter().enumerate() {
        let id = BlockId::new();
        let map = match insert_block_map(&list, ix, &id, block.block_type, block.indent as i64) {
            Ok(m) => m,
            Err(_) => continue,
        };
        match block.block_type {
            BlockType::Ordered => {
                let _ = map.insert("number", block.number.max(1) as i64);
                if let Ok(text) = ensure_content(&map) {
                    let _ = write_rich_text(&text, &block.text, &block.marks);
                }
            }
            BlockType::Task => {
                let _ = map.insert("checked", block.checked);
                if let Ok(text) = ensure_content(&map) {
                    let _ = write_rich_text(&text, &block.text, &block.marks);
                }
            }
            BlockType::Code => {
                if let Some(lang) = &block.language {
                    let _ = map.insert("language", lang.as_str());
                }
                if let Ok(text) = ensure_content(&map) {
                    let _ = write_rich_text(&text, &block.text, &[]);
                }
            }
            BlockType::Image => {
                let _ = map.insert("url", block.url.as_str());
                if let Some(w) = block.width {
                    let _ = map.insert("width", w as i64);
                }
                if let Ok(text) = ensure_alt(&map) {
                    let _ = write_rich_text(&text, &block.text, &[]);
                }
            }
            BlockType::Bookmark => {
                let _ = map.insert("url", block.url.as_str());
                let _ = map.insert("form", block.form.as_str());
            }
            BlockType::Table => {
                write_table(&map, block);
            }
            BlockType::Rule => {}
            _ => {
                if let Ok(text) = ensure_content(&map) {
                    let _ = write_rich_text(&text, &block.text, &block.marks);
                }
            }
        }
    }
    repair_numbers(&doc);
    doc.commit();
    doc
}

fn write_table(map: &loro::LoroMap, block: &BlockDraft) {
    let align = match map.ensure_mergeable_list("align") {
        Ok(list) => list,
        Err(_) => return,
    };
    for (i, a) in block.align.iter().enumerate() {
        let _ = align.insert(i, a.as_str());
    }
    let rows = match map.ensure_mergeable_movable_list("rows") {
        Ok(list) => list,
        Err(_) => return,
    };
    // Row 0 = header
    let all_rows: Vec<&Vec<(String, Vec<(Range<usize>, RichMark)>)>> =
        std::iter::once(&block.header)
            .chain(block.rows.iter())
            .collect();
    for (ri, row) in all_rows.into_iter().enumerate() {
        let row_map = match rows.insert_container(ri, loro::LoroMap::new()) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let cells = match row_map.ensure_mergeable_movable_list("cells") {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (ci, (plain, marks)) in row.iter().enumerate() {
            let cell = match cells.insert_container(ci, loro::LoroMap::new()) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if let Ok(text) = cell.ensure_mergeable_text("content") {
                let _ = write_rich_text(&text, plain, marks);
            }
        }
    }
}

#[derive(Default)]
struct BlockDraft {
    block_type: BlockType,
    indent: u8,
    text: String,
    marks: Vec<(Range<usize>, RichMark)>,
    number: u64,
    checked: bool,
    language: Option<String>,
    url: String,
    form: Form,
    width: Option<u32>,
    align: Vec<Align>,
    header: Vec<(String, Vec<(Range<usize>, RichMark)>)>,
    rows: Vec<Vec<(String, Vec<(Range<usize>, RichMark)>)>>,
}

#[derive(Default)]
struct TextBuilder {
    text: String,
    marks: Vec<(Range<usize>, RichMark)>,
    open: Vec<usize>,
    next_rank: i64,
}

impl TextBuilder {
    fn open(&mut self, mark: RichMark) {
        let ix = self.marks.len();
        let at = self.text.len();
        self.marks.push((at..at, mark));
        self.open.push(ix);
    }

    fn open_emphasis(&mut self, kind: Emphasis) {
        let rank = self.next_rank;
        self.next_rank += 1;
        let mark = match kind {
            Emphasis::Bold => RichMark::Bold(rank),
            Emphasis::Italic => RichMark::Italic(rank),
            Emphasis::Strike => RichMark::Strike(rank),
        };
        self.open(mark);
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty() && self.marks.is_empty()
    }

    fn close(&mut self) {
        if let Some(ix) = self.open.pop() {
            self.marks[ix].0.end = self.text.len();
        }
    }

    fn wrap(&mut self, mark: RichMark, s: &str) {
        let start = self.text.len();
        self.text.push_str(s);
        self.marks.push((start..self.text.len(), mark));
    }

    fn take(&mut self) -> (String, Vec<(Range<usize>, RichMark)>) {
        self.open.clear();
        self.next_rank = 0;
        let text = std::mem::take(&mut self.text);
        let marks = std::mem::take(&mut self.marks);
        normalize(&text, &marks)
    }
}

#[derive(Clone, Copy)]
enum Emphasis {
    Bold,
    Italic,
    Strike,
}

#[derive(Clone, Copy)]
enum Marker {
    Bullet,
    Ordered(u64),
    Task(bool),
}

impl Marker {
    fn into_draft(self, text: String, marks: Vec<(Range<usize>, RichMark)>) -> BlockDraft {
        match self {
            Self::Bullet => BlockDraft {
                block_type: BlockType::Bullet,
                text,
                marks,
                ..Default::default()
            },
            Self::Ordered(number) => BlockDraft {
                block_type: BlockType::Ordered,
                number,
                text,
                marks,
                ..Default::default()
            },
            Self::Task(checked) => BlockDraft {
                block_type: BlockType::Task,
                checked,
                text,
                marks,
                ..Default::default()
            },
        }
    }
}

#[derive(Default)]
struct TableBuild {
    align: Vec<Align>,
    header: Vec<(String, Vec<(Range<usize>, RichMark)>)>,
    rows: Vec<Vec<(String, Vec<(Range<usize>, RichMark)>)>>,
    row: Vec<(String, Vec<(Range<usize>, RichMark)>)>,
    in_head: bool,
}

#[derive(Default)]
struct ParseState {
    blocks: Vec<BlockDraft>,
    builder: TextBuilder,
    lists: Vec<Option<u64>>,
    quote_depth: u8,
    pending_marker: Option<Marker>,
    heading: Option<u8>,
    code: Option<(Option<String>, String)>,
    table: Option<TableBuild>,
}

impl ParseState {
    fn indent(&self) -> u8 {
        self.lists.len() as u8
    }

    fn push(&mut self, mut block: BlockDraft, indent: u8) {
        let max = self.blocks.last().map_or(0, |b| b.indent + 1);
        block.indent = indent.min(max);
        self.blocks.push(block);
    }

    fn flush_marker(&mut self) {
        let Some(marker) = self.pending_marker.take() else {
            return;
        };
        let indent = self.indent().saturating_sub(1);
        self.push(marker.into_draft(String::new(), Vec::new()), indent);
    }

    fn flush_inline(&mut self) {
        if self.table.is_none() && !self.builder.is_empty() {
            self.finish_paragraph();
        }
    }

    fn finish_paragraph(&mut self) {
        let (text, marks) = self.builder.take();

        if let [(range, RichMark::Image(url))] = marks.as_slice()
            && range.start == 0
            && range.end == text.len()
        {
            let (caption, width) = split_width(&text);
            self.flush_marker();
            let indent = self.indent();
            self.push(
                BlockDraft {
                    block_type: BlockType::Image,
                    url: url.clone(),
                    text: caption.to_string(),
                    width,
                    ..Default::default()
                },
                indent,
            );
            return;
        }

        if let [(range, RichMark::Mention { url, form })] = marks.as_slice()
            && range.start == 0
            && range.end == text.len()
            && text == *url
            && is_url(url)
        {
            self.flush_marker();
            let indent = self.indent();
            self.push(
                BlockDraft {
                    block_type: BlockType::Bookmark,
                    url: url.clone(),
                    form: *form,
                    ..Default::default()
                },
                indent,
            );
            return;
        }

        if self.quote_depth > 0 {
            self.flush_marker();
            let indent = self.indent();
            self.push(
                BlockDraft {
                    block_type: BlockType::Quote,
                    text,
                    marks,
                    ..Default::default()
                },
                indent,
            );
        } else if let Some(marker) = self.pending_marker.take() {
            let indent = self.indent().saturating_sub(1);
            self.push(marker.into_draft(text, marks), indent);
        } else {
            let indent = self.indent();
            self.push(
                BlockDraft {
                    block_type: BlockType::Paragraph,
                    text,
                    marks,
                    ..Default::default()
                },
                indent,
            );
        }
    }

    fn renumber(&mut self) {
        let mut expected: Vec<Option<u64>> = Vec::new();
        for block in &mut self.blocks {
            let indent = block.indent as usize;
            expected.truncate(indent + 1);
            expected.resize(indent + 1, None);
            if block.block_type == BlockType::Ordered {
                if let Some(next) = expected[indent] {
                    block.number = next;
                }
                expected[indent] = Some(block.number.saturating_add(1));
            } else {
                expected[indent] = None;
            }
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => match &mut self.code {
                Some((_, code)) => code.push_str(&t),
                None => self.builder.text.push_str(&t),
            },
            Event::Code(t) => self.builder.wrap(RichMark::Code, &t),
            Event::Html(t) | Event::InlineHtml(t) => self.builder.text.push_str(&t),
            Event::SoftBreak | Event::HardBreak => match &mut self.code {
                Some((_, code)) => code.push('\n'),
                None => self.builder.text.push('\n'),
            },
            Event::Rule => {
                self.flush_inline();
                self.flush_marker();
                let indent = self.indent();
                self.push(
                    BlockDraft {
                        block_type: BlockType::Rule,
                        ..Default::default()
                    },
                    indent,
                );
            }
            Event::TaskListMarker(checked) => {
                self.pending_marker = Some(Marker::Task(checked));
            }
            Event::FootnoteReference(label) => {
                self.builder.text.push_str(&format!("[^{label}]"));
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush_inline();
                self.heading = Some(level as u8);
            }
            Tag::BlockQuote(_) => {
                self.flush_inline();
                self.quote_depth += 1;
            }
            Tag::CodeBlock(kind) => {
                self.flush_inline();
                self.flush_marker();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        let tag = info.split_whitespace().next().unwrap_or("");
                        (!tag.is_empty()).then(|| tag.to_string())
                    }
                    CodeBlockKind::Indented => None,
                };
                self.code = Some((language, String::new()));
            }
            Tag::List(start) => {
                self.flush_inline();
                self.flush_marker();
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush_inline();
                self.pending_marker = Some(match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let n = *number;
                        *number += 1;
                        Marker::Ordered(n)
                    }
                    _ => Marker::Bullet,
                });
            }
            Tag::Table(aligns) => {
                self.flush_inline();
                self.flush_marker();
                self.table = Some(TableBuild {
                    align: aligns.iter().map(align_of).collect(),
                    ..TableBuild::default()
                });
            }
            Tag::TableHead => {
                if let Some(table) = &mut self.table {
                    table.in_head = true;
                }
            }
            Tag::Emphasis => self.builder.open_emphasis(Emphasis::Italic),
            Tag::Strong => self.builder.open_emphasis(Emphasis::Bold),
            Tag::Strikethrough => self.builder.open_emphasis(Emphasis::Strike),
            Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            } => {
                let url = dest_url.into_string();
                let form = match link_type {
                    LinkType::Autolink => Some(Form::Auto),
                    _ => Form::from_title(&title),
                };
                self.builder.open(match form {
                    Some(form) => RichMark::Mention { url, form },
                    None => RichMark::Link(url),
                });
            }
            Tag::Image { dest_url, .. } => {
                self.builder.open(RichMark::Image(dest_url.into_string()));
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::HtmlBlock => self.flush_inline(),
            TagEnd::Heading(_) => {
                self.flush_marker();
                let level = self.heading.take().unwrap_or(1).clamp(1, 6);
                let (mut text, marks) = self.builder.take();
                if text.contains('\n') {
                    text = text.replace('\n', " ");
                }
                let indent = self.indent();
                self.push(
                    BlockDraft {
                        block_type: BlockType::Heading { level },
                        text,
                        marks,
                        ..Default::default()
                    },
                    indent,
                );
            }
            TagEnd::BlockQuote(_) => {
                self.flush_inline();
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                if let Some((language, code)) = self.code.take() {
                    let indent = self.indent();
                    let code = code.strip_suffix('\n').map_or(code.clone(), str::to_string);
                    self.push(
                        BlockDraft {
                            block_type: BlockType::Code,
                            language,
                            text: code,
                            ..Default::default()
                        },
                        indent,
                    );
                }
            }
            TagEnd::List(_) => {
                self.flush_inline();
                self.lists.pop();
            }
            TagEnd::Item => {
                self.flush_inline();
                self.flush_marker();
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    let indent = self.indent();
                    self.push(
                        BlockDraft {
                            block_type: BlockType::Table,
                            align: table.align,
                            header: table.header,
                            rows: table.rows,
                            ..Default::default()
                        },
                        indent,
                    );
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = &mut self.table {
                    table.header = std::mem::take(&mut table.row);
                    table.in_head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(table) = &mut self.table {
                    let row = std::mem::take(&mut table.row);
                    if !table.in_head {
                        table.rows.push(row);
                    }
                }
            }
            TagEnd::TableCell => {
                let (mut text, marks) = self.builder.take();
                if text.contains('\n') {
                    text = text.replace('\n', " ");
                }
                if let Some(table) = &mut self.table {
                    table.row.push((text, marks));
                }
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.builder.close();
            }
            TagEnd::Image => self.builder.close(),
            _ => {}
        }
    }
}

fn align_of(alignment: &Alignment) -> Align {
    match alignment {
        Alignment::Center => Align::Center,
        Alignment::Right => Align::Right,
        Alignment::Left | Alignment::None => Align::Left,
    }
}

fn split_width(alt: &str) -> (&str, Option<u32>) {
    let Some((caption, tail)) = alt.rsplit_once('|') else {
        return (alt, None);
    };
    match tail.parse().ok().filter(|width: &u32| *width > 0) {
        Some(width) => (caption, Some(width)),
        None => (alt, None),
    }
}

const SCHEMES: [&str; 2] = ["https://", "http://"];

pub fn is_url(source: &str) -> bool {
    matches!(urls(source).as_slice(), [only] if *only == (0..source.len()))
}

pub fn is_image(source: &str) -> bool {
    let path = source.split(['?', '#']).next().unwrap_or(source);
    let Some((_, extension)) = path.rsplit_once('.') else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" | "avif"
    )
}

fn urls(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let Some((start, scheme)) = SCHEMES
            .iter()
            .filter_map(|scheme| text[at..].find(scheme).map(|ix| (at + ix, *scheme)))
            .min_by_key(|(ix, _)| *ix)
        else {
            break;
        };
        let stop = text[start..]
            .find(char::is_whitespace)
            .map_or(text.len(), |ix| start + ix);
        let end = start + trim_url(&text[start..stop]);
        let opens = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        if opens && end > start + scheme.len() {
            found.push(start..end);
        }
        at = stop.max(start + 1);
    }
    found
}

fn trim_url(run: &str) -> usize {
    let mut end = run.len();
    while let Some(last) = run[..end].chars().next_back() {
        let keep = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' => false,
            ')' => run[..end].matches('(').count() >= run[..end].matches(')').count(),
            ']' => run[..end].matches('[').count() >= run[..end].matches(']').count(),
            _ => true,
        };
        if keep {
            break;
        }
        end -= last.len_utf8();
    }
    end
}

fn normalize(
    text: &str,
    marks: &[(Range<usize>, RichMark)],
) -> (String, Vec<(Range<usize>, RichMark)>) {
    let bytes = text.as_bytes();
    let mut keep = vec![true; text.len()];
    let mut line_begin = 0;
    for offset in memchr_newlines(text).chain([text.len()]) {
        let line = &text[line_begin..offset];
        let lead = line.len() - line.trim_start_matches([' ', '\t']).len();
        let trail = line.len() - line.trim_end_matches([' ', '\t']).len();
        keep[line_begin..line_begin + lead].fill(false);
        keep[offset - trail..offset].fill(false);
        line_begin = offset + 1;
    }
    let mut head = 0;
    while head < text.len() && (!keep[head] || bytes[head] == b'\n') {
        keep[head] = false;
        head += 1;
    }
    let mut tail = text.len();
    while tail > 0 && (!keep[tail - 1] || bytes[tail - 1] == b'\n') {
        keep[tail - 1] = false;
        tail -= 1;
    }

    let mut out = String::with_capacity(text.len());
    let mut map = vec![0; text.len() + 1];
    for (offset, ch) in text.char_indices() {
        map[offset] = out.len();
        if keep[offset] {
            out.push(ch);
        }
    }
    map[text.len()] = out.len();

    let mut linkified = marks
        .iter()
        .map(|(range, mark)| (map[range.start]..map[range.end], mark.clone()))
        .filter(|(range, mark)| !range.is_empty() || matches!(mark, RichMark::Image(_)))
        .collect::<Vec<_>>();

    // Bare URL linkify
    for range in urls(&out) {
        let overlaps = linkified.iter().any(|(r, mark)| {
            matches!(
                mark,
                RichMark::Link(_) | RichMark::Mention { .. } | RichMark::Image(_) | RichMark::Code
            ) && r.start < range.end
                && range.start < r.end
        });
        if !overlaps {
            let url = out[range.clone()].to_string();
            linkified.push((range, RichMark::Link(url)));
        }
    }

    (out, linkified)
}

fn memchr_newlines(text: &str) -> impl Iterator<Item = usize> + '_ {
    text.bytes()
        .enumerate()
        .filter_map(|(ix, b)| (b == b'\n').then_some(ix))
}
