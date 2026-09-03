//! [`LoroDoc`] → markdown (Bezel serialize adapted for Loro).

use std::ops::Range;

use loro::{Container, LoroDoc, LoroMap, LoroValue, ValueOrContainer};

use crate::schema::{
    Align, BlockType, Form, RichMark, alt_text, block_map_at, block_type_of, blocks_list,
    content_text, indent_of, map_bool, map_i64, map_string, marks_from_delta,
};

const INDENT: &str = "    ";

/// Project a Loro document to its markdown wire form.
#[must_use]
pub fn project_markdown(doc: &LoroDoc) -> String {
    let list = blocks_list(doc);
    let mut out = String::new();
    let mut previous: Option<(BlockType, u8)> = None;
    let mut last_was_empty_task = false;

    for i in 0..list.len() {
        let Some(map) = block_map_at(&list, i) else {
            continue;
        };
        let kind = block_type_of(&map);
        let indent = indent_of(&map) as u8;
        let indent = match previous {
            Some((_, prev)) => indent.min(prev + 1),
            None => 0,
        };

        if let Some((prev_kind, prev_indent)) = previous {
            out.push('\n');
            if !tight_after(prev_kind, kind, indent > prev_indent) {
                out.push('\n');
            }
        }

        write_block(&mut out, &map, kind, indent);
        last_was_empty_task =
            kind == BlockType::Task && content_text(&map).is_some_and(|t| t.to_string().is_empty());
        previous = Some((kind, indent));
    }

    if last_was_empty_task {
        out.push(' ');
    }
    out
}

fn marker_kind(kind: BlockType) -> Option<u8> {
    match kind {
        BlockType::Bullet => Some(0),
        BlockType::Ordered => Some(1),
        BlockType::Task => Some(2),
        _ => None,
    }
}

fn is_marker(kind: BlockType) -> bool {
    marker_kind(kind).is_some()
}

fn tight_after(previous: BlockType, next: BlockType, nested: bool) -> bool {
    // Approximate Bezel without the previous map: empty-marker rules use kind only.
    if nested {
        return is_marker(previous) && is_marker(next) && !matches!(next, BlockType::Ordered);
    }
    marker_kind(previous).is_some() && marker_kind(previous) == marker_kind(next)
}

fn write_block(out: &mut String, map: &LoroMap, kind: BlockType, indent: u8) {
    let pad = INDENT.repeat(indent as usize);
    match kind {
        BlockType::Paragraph => {
            let body = inline_of(map);
            write_lines(out, &pad, &pad, &body);
        }
        BlockType::Heading { level } => {
            let hashes = "#".repeat(level.clamp(1, 6) as usize);
            write_lines(out, &format!("{pad}{hashes} "), &pad, &inline_of(map));
        }
        BlockType::Bullet => {
            let text = plain_of(map);
            let marker = if text.is_empty() { "+ " } else { "- " };
            write_marked(out, &pad, marker, &inline_of(map), text.is_empty());
        }
        BlockType::Ordered => {
            let number = map_i64(map, "number").unwrap_or(1).max(1);
            write_marked(
                out,
                &pad,
                &format!("{number}. "),
                &inline_of(map),
                plain_of(map).is_empty(),
            );
        }
        BlockType::Task => {
            let checked = map_bool(map, "checked").unwrap_or(false);
            let marker = if checked { "- [x] " } else { "- [ ] " };
            write_marked(out, &pad, marker, &inline_of(map), plain_of(map).is_empty());
        }
        BlockType::Quote => {
            let prefix = format!("{pad}> ");
            write_lines(out, &prefix, &prefix, &inline_of(map));
        }
        BlockType::Code => {
            let code = plain_of(map);
            let language = map_string(map, "language").unwrap_or_default();
            let fence = "`".repeat(fence_width(&code));
            out.push_str(&pad);
            out.push_str(&fence);
            out.push_str(&language);
            for line in code.split('\n') {
                out.push('\n');
                out.push_str(&pad);
                out.push_str(line);
            }
            out.push('\n');
            out.push_str(&pad);
            out.push_str(&fence);
        }
        BlockType::Image => {
            let url = map_string(map, "url").unwrap_or_default();
            let alt = alt_text(map).map(|t| t.to_string()).unwrap_or_default();
            out.push_str(&pad);
            out.push_str("![");
            escape_inline(out, &alt);
            if let Some(width) = map_i64(map, "width").filter(|w| *w > 0) {
                out.push('|');
                out.push_str(&width.to_string());
            }
            out.push_str("](");
            out.push_str(&url);
            out.push(')');
        }
        BlockType::Bookmark => {
            let url = map_string(map, "url").unwrap_or_default();
            let form = map_string(map, "form")
                .as_deref()
                .and_then(Form::parse)
                .unwrap_or(Form::Auto);
            out.push_str(&pad);
            match form.title() {
                None => {
                    out.push('<');
                    out.push_str(&url);
                    out.push('>');
                }
                Some(title) => {
                    out.push('[');
                    out.push_str(&url);
                    out.push_str("](");
                    out.push_str(&url);
                    out.push_str(&format!(" \"{title}\")"));
                }
            }
        }
        BlockType::Table => write_table(out, &pad, map),
        BlockType::Rule => {
            out.push_str(&pad);
            out.push_str("---");
        }
    }
}

fn plain_of(map: &LoroMap) -> String {
    content_text(map).map(|t| t.to_string()).unwrap_or_default()
}

fn inline_of(map: &LoroMap) -> String {
    let Some(text) = content_text(map) else {
        return String::new();
    };
    let (plain, marks) = marks_from_delta(&text.to_delta());
    inline(&plain, &marks)
}

fn write_marked(out: &mut String, pad: &str, marker: &str, body: &str, empty: bool) {
    let opener = if empty { marker.trim_end() } else { marker };
    let first = format!("{pad}{opener}");
    let rest = format!("{pad}{}", " ".repeat(marker.chars().count()));
    write_lines(out, &first, &rest, body);
}

fn write_lines(out: &mut String, first: &str, rest: &str, body: &str) {
    for (ix, line) in body.split('\n').enumerate() {
        if ix > 0 {
            out.push('\n');
        }
        out.push_str(if ix == 0 { first } else { rest });
        out.push_str(line);
    }
}

fn fence_width(code: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for c in code.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    (longest + 1).max(3)
}

fn write_table(out: &mut String, pad: &str, map: &LoroMap) {
    let align = read_align(map);
    let (header, rows) = read_table_cells(map);
    let columns = align.len().max(header.len());
    let row_of = |cells: &[String]| {
        let mut line = String::from("|");
        for ix in 0..columns {
            line.push(' ');
            if let Some(cell) = cells.get(ix) {
                line.push_str(cell);
            }
            line.push_str(" |");
        }
        line
    };

    out.push_str(pad);
    out.push_str(&row_of(&header));
    out.push('\n');
    out.push_str(pad);
    out.push('|');
    for ix in 0..columns {
        out.push_str(match align.get(ix).copied().unwrap_or_default() {
            Align::Left => " --- |",
            Align::Center => " :-: |",
            Align::Right => " ---: |",
        });
    }
    for row in rows {
        out.push('\n');
        out.push_str(pad);
        out.push_str(&row_of(&row));
    }
}

fn read_align(map: &LoroMap) -> Vec<Align> {
    let Some(ValueOrContainer::Container(Container::List(list))) = map.get("align") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for i in 0..list.len() {
        let s = match list.get(i) {
            Some(ValueOrContainer::Value(LoroValue::String(s))) => Align::parse(&s),
            _ => Align::Left,
        };
        out.push(s);
    }
    out
}

fn read_table_cells(map: &LoroMap) -> (Vec<String>, Vec<Vec<String>>) {
    let Some(ValueOrContainer::Container(Container::MovableList(rows))) = map.get("rows") else {
        return (Vec::new(), Vec::new());
    };
    let mut all = Vec::new();
    for ri in 0..rows.len() {
        let Some(ValueOrContainer::Container(Container::Map(row_map))) = rows.get(ri) else {
            continue;
        };
        let Some(ValueOrContainer::Container(Container::MovableList(cells))) = row_map.get("cells")
        else {
            continue;
        };
        let mut row = Vec::new();
        for ci in 0..cells.len() {
            let Some(ValueOrContainer::Container(Container::Map(cell))) = cells.get(ci) else {
                row.push(String::new());
                continue;
            };
            let inline = content_text(&cell)
                .map(|t| {
                    let (plain, marks) = marks_from_delta(&t.to_delta());
                    inline(&plain, &marks)
                })
                .unwrap_or_default();
            row.push(inline);
        }
        all.push(row);
    }
    if all.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        let header = all.remove(0);
        (header, all)
    }
}

fn inline(text: &str, marks: &[(Range<usize>, RichMark)]) -> String {
    let mut out = String::new();
    let mut open: Vec<usize> = Vec::new();
    let mut started = vec![false; marks.len()];
    let mut delimiters = vec!['_'; marks.len()];
    let mut cursor = 0usize;

    let mut boundaries: Vec<usize> = marks
        .iter()
        .flat_map(|(r, _)| [r.start, r.end])
        .chain([0, text.len()])
        .collect();
    boundaries.sort_unstable();
    boundaries.dedup();

    for point in boundaries {
        if point < cursor {
            continue;
        }
        if point > text.len() {
            break;
        }
        escape_inline(&mut out, &text[cursor..point]);
        cursor = point;

        while let Some(&top) = open.last() {
            if marks[top].0.end <= point {
                close_mark(&mut out, &marks[top].1, delimiters[top]);
                open.pop();
            } else {
                break;
            }
        }

        for (ix, (range, mark)) in marks.iter().enumerate() {
            if started[ix] || range.start != point {
                continue;
            }
            started[ix] = true;
            if matches!(mark, RichMark::Code) {
                let body = &text[range.clone()];
                let ticks = "`".repeat(fence_width_inline(body));
                out.push_str(&ticks);
                out.push_str(body);
                out.push_str(&ticks);
                cursor = cursor.max(range.end);
                continue;
            }
            if let RichMark::Mention { url, form } = mark
                && *form == Form::Auto
                && text.get(range.clone()) == Some(url.as_str())
                && alone(marks, ix)
            {
                out.push('<');
                out.push_str(url);
                out.push('>');
                cursor = cursor.max(range.end);
                continue;
            }
            if let RichMark::Link(url) = mark
                && text.get(range.clone()) == Some(url.as_str())
                && crate::hydrate::is_url(url)
                && alone(marks, ix)
            {
                out.push_str(url);
                cursor = cursor.max(range.end);
                continue;
            }
            let italic = italic_delimiter(&out, text, range);
            delimiters[ix] = italic;
            open_mark(&mut out, mark, italic);
            if range.is_empty() {
                close_mark(&mut out, mark, italic);
            } else {
                open.push(ix);
            }
        }
    }

    escape_inline(&mut out, &text[cursor.min(text.len())..]);
    while let Some(ix) = open.pop() {
        close_mark(&mut out, &marks[ix].1, delimiters[ix]);
    }
    out
}

fn alone(marks: &[(Range<usize>, RichMark)], ix: usize) -> bool {
    let span = &marks[ix].0;
    marks.iter().enumerate().all(|(other, (range, _))| {
        other == ix || range.end <= span.start || range.start >= span.end
    })
}

fn fence_width_inline(body: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for c in body.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest + 1
}

fn italic_delimiter(written: &str, text: &str, range: &Range<usize>) -> char {
    let intraword = written
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric)
        || text[range.end..]
            .chars()
            .next()
            .is_some_and(char::is_alphanumeric);
    if intraword { '*' } else { '_' }
}

fn open_mark(out: &mut String, mark: &RichMark, italic: char) {
    match mark {
        RichMark::Bold(_) => out.push_str("**"),
        RichMark::Italic(_) => out.push(italic),
        RichMark::Strike(_) => out.push_str("~~"),
        RichMark::Link(_) | RichMark::Mention { .. } => out.push('['),
        RichMark::Image(_) => out.push_str("!["),
        RichMark::Code | RichMark::Comment(_) => {}
    }
}

fn close_mark(out: &mut String, mark: &RichMark, italic: char) {
    match mark {
        RichMark::Bold(_) => out.push_str("**"),
        RichMark::Italic(_) => out.push(italic),
        RichMark::Strike(_) => out.push_str("~~"),
        RichMark::Link(url) | RichMark::Image(url) => {
            out.push_str("](");
            out.push_str(url);
            out.push(')');
        }
        RichMark::Mention { url, form } => {
            out.push_str("](");
            out.push_str(url);
            out.push_str(" \"");
            out.push_str(form.title().unwrap_or("chip"));
            out.push_str("\")");
        }
        RichMark::Code | RichMark::Comment(_) => {}
    }
}

fn escape_inline(out: &mut String, s: &str) {
    let mut line_start = out.is_empty() || out.ends_with('\n');
    for (ix, line) in s.split('\n').enumerate() {
        if ix > 0 {
            out.push('\n');
            line_start = true;
        }
        let body = if line_start {
            escape_block_marker(out, line)
        } else {
            line
        };
        escape_span(out, body);
        line_start = false;
    }
}

fn escape_block_marker<'a>(out: &mut String, line: &'a str) -> &'a str {
    let after_space = |rest: &str| rest.starts_with([' ', '\t']) || rest.is_empty();

    let hashes = line.len() - line.trim_start_matches('#').len();
    if hashes > 0 && after_space(&line[hashes..]) {
        out.push('\\');
        out.push_str(&line[..hashes]);
        return &line[hashes..];
    }
    if let Some(rest) = line.strip_prefix('>') {
        out.push_str("\\>");
        return rest;
    }
    if (line.starts_with('-') || line.starts_with('+')) && after_space(&line[1..]) {
        out.push('\\');
        out.push_str(&line[..1]);
        return &line[1..];
    }
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0 {
        let after = &line[digits..];
        if (after.starts_with('.') || after.starts_with(')')) && after_space(&after[1..]) {
            out.push_str(&line[..digits]);
            out.push('\\');
            out.push_str(&after[..1]);
            return &after[1..];
        }
    }
    let trimmed = line.trim_end();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c == '=' || c == '-') {
        out.push('\\');
        out.push_str(&line[..1]);
        return &line[1..];
    }
    line
}

fn escape_span(out: &mut String, s: &str) {
    for (ix, c) in s.char_indices() {
        let rest = &s[ix + c.len_utf8()..];
        match c {
            '\\' | '*' | '`' | '[' | ']' | '~' | '|' => {
                out.push('\\');
                out.push(c);
            }
            '_' => {
                let before = s[..ix].chars().next_back();
                let inside_word = before.is_some_and(char::is_alphanumeric)
                    && rest.chars().next().is_some_and(char::is_alphanumeric);
                if !inside_word {
                    out.push('\\');
                }
                out.push('_');
            }
            '<' if rest
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || matches!(c, '/' | '!' | '?')) =>
            {
                out.push_str("\\<")
            }
            '&' if rest
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '#') =>
            {
                out.push_str("\\&")
            }
            _ => out.push(c),
        }
    }
}
