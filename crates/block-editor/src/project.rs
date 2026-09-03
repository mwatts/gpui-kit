//! Project Loro state into [`BlockSnapshot`]s.

use std::collections::HashMap;

use block_markdown::{
    BlockId, Form, alt_text, block_map_at, block_type_of, blocks_list, content_text, indent_of,
    map_bool, map_i64, map_string,
};
use loro::LoroDoc;

use gpui_component_block_view::BlockSnapshot;

#[must_use]
pub fn project(doc: &LoroDoc) -> Vec<BlockSnapshot> {
    let list = blocks_list(doc);
    let mut out = Vec::with_capacity(list.len());
    for i in 0..list.len() {
        let Some(map) = block_map_at(&list, i) else {
            continue;
        };
        let id = BlockId(map_string(&map, "id").unwrap_or_else(|| format!("missing-{i}")));
        let block_type = block_type_of(&map);
        let indent = indent_of(&map);
        let (plain, runs) = if let Some(text) = content_text(&map) {
            (text.to_string(), text.to_delta())
        } else if let Some(text) = alt_text(&map) {
            (text.to_string(), text.to_delta())
        } else {
            (String::new(), Vec::new())
        };
        let mut props = HashMap::new();
        if let Some(v) = map_string(&map, "language") {
            props.insert("language".into(), v);
        }
        if let Some(v) = map_string(&map, "url") {
            props.insert("url".into(), v);
        }
        if let Some(v) = map_string(&map, "form") {
            props.insert("form".into(), v);
        }
        out.push(BlockSnapshot {
            id,
            block_type,
            indent,
            plain,
            runs,
            props,
            checked: map_bool(&map, "checked"),
            number: map_i64(&map, "number"),
            language: map_string(&map, "language"),
            url: map_string(&map, "url"),
            form: map_string(&map, "form").as_deref().and_then(Form::parse),
            width: map_i64(&map, "width"),
            table: None,
        });
    }
    out
}
