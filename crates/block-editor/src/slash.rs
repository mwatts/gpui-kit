//! Slash menu catalog and query filter (Bezel `slash.rs`).
//!
//! Pure data — no window. The Editor entity (Task 4) owns scroll / glass paint.

use block_markdown::BlockType;
use gpui_component_block_view::Cursor;

/// Every block the menu offers. Bookmark is absent (needs a URL).
#[must_use]
pub fn items() -> Vec<(&'static str, BlockType)> {
    vec![
        ("Text", BlockType::Paragraph),
        ("Heading 1", BlockType::Heading { level: 1 }),
        ("Heading 2", BlockType::Heading { level: 2 }),
        ("Heading 3", BlockType::Heading { level: 3 }),
        ("Bullet", BlockType::Bullet),
        ("Numbered", BlockType::Ordered),
        ("Task", BlockType::Task),
        ("Quote", BlockType::Quote),
        ("Code", BlockType::Code),
        ("Table", BlockType::Table),
        ("Image", BlockType::Image),
        ("Divider", BlockType::Rule),
    ]
}

/// Match rank of a label against a query: `0` prefix, `1` substring, `None` no
/// match. Case-insensitive; empty query matches everything at rank 1.
#[must_use]
pub fn match_rank(query: &str, label: &str) -> Option<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(1);
    }
    let label = label.to_lowercase();
    if label.starts_with(&query) {
        Some(0)
    } else if label.contains(&query) {
        Some(1)
    } else {
        None
    }
}

/// Filter + rank labels: prefix matches first, then substring, stable within
/// each rank. Returns indices into `labels`.
#[must_use]
pub fn filter_indices(query: &str, labels: &[&str]) -> Vec<usize> {
    let mut ranked: Vec<(usize, usize)> = labels
        .iter()
        .enumerate()
        .filter_map(|(ix, label)| match_rank(query, label).map(|rank| (rank, ix)))
        .collect();
    ranked.sort_by_key(|&(rank, ix)| (rank, ix));
    ranked.into_iter().map(|(_, ix)| ix).collect()
}

/// Step the active row: wraps at both ends; `None` enters at the edge matching
/// the direction. Empty menus stay `None`.
#[must_use]
pub fn menu_step(active: Option<usize>, count: usize, delta: isize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let count_i = count as isize;
    let next = match active {
        None => {
            if delta >= 0 {
                0
            } else {
                count_i - 1
            }
        }
        Some(at) => (at as isize + delta).rem_euclid(count_i),
    };
    Some(next as usize)
}

/// Searchable list state: items, ranked view, active row in the filtered view.
#[derive(Debug, Clone)]
pub struct Filter {
    items: Vec<String>,
    filtered: Vec<usize>,
    active: Option<usize>,
}

impl Filter {
    #[must_use]
    pub fn new(items: Vec<String>) -> Self {
        let filtered: Vec<usize> = (0..items.len()).collect();
        let active = (!filtered.is_empty()).then_some(0);
        Self {
            items,
            filtered,
            active,
        }
    }

    #[must_use]
    pub fn items(&self) -> &[String] {
        &self.items
    }

    #[must_use]
    pub fn filtered(&self) -> &[usize] {
        &self.filtered
    }

    #[must_use]
    pub fn active(&self) -> Option<usize> {
        self.active
    }

    pub fn refilter(&mut self, query: &str) {
        let labels: Vec<&str> = self.items.iter().map(String::as_str).collect();
        self.filtered = filter_indices(query, &labels);
        self.active = (!self.filtered.is_empty()).then_some(0);
    }

    pub fn step(&mut self, delta: isize) {
        self.active = menu_step(self.active, self.filtered.len(), delta);
    }

    pub fn set_active(&mut self, position: usize) {
        if position < self.filtered.len() {
            self.active = Some(position);
        }
    }

    /// Index into [`Self::items`], never into the filtered view.
    #[must_use]
    pub fn active_item(&self) -> Option<usize> {
        self.active
            .and_then(|position| self.filtered.get(position))
            .copied()
    }
}

/// An open slash menu: where the `/` sits, and the ranked list under it.
#[derive(Debug, Clone)]
pub struct Slash {
    /// The `/` itself. Everything between it and the caret is the query.
    pub at: Cursor,
    pub filter: Filter,
}

impl Slash {
    #[must_use]
    pub fn open(at: Cursor) -> Self {
        Self {
            at,
            filter: Filter::new(
                items()
                    .into_iter()
                    .map(|(label, _)| label.to_string())
                    .collect(),
            ),
        }
    }

    pub fn refilter(&mut self, query: &str) {
        self.filter.refilter(query);
    }

    pub fn step(&mut self, delta: isize) {
        self.filter.step(delta);
    }

    /// The block confirming right now would make.
    #[must_use]
    pub fn choice(&self) -> Option<BlockType> {
        let ix = self.filter.active_item()?;
        items().into_iter().nth(ix).map(|(_, kind)| kind)
    }

    /// Text typed since the `/`, or `None` when the caret has left the run
    /// (closes the menu). A space ends the query.
    #[must_use]
    pub fn query(&self, caret: Cursor, text: &str) -> Option<String> {
        if caret.id != self.at.id || caret.part != self.at.part {
            return None;
        }
        let start = self.at.offset + 1;
        if caret.offset < start {
            return None;
        }
        let query = text.get(start..caret.offset)?;
        (!query.contains(char::is_whitespace)).then(|| query.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component_block_view::Part;

    #[test]
    fn items_lock_list_no_bookmark() {
        let labels: Vec<_> = items().into_iter().map(|(l, _)| l).collect();
        assert_eq!(
            labels,
            [
                "Text",
                "Heading 1",
                "Heading 2",
                "Heading 3",
                "Bullet",
                "Numbered",
                "Task",
                "Quote",
                "Code",
                "Table",
                "Image",
                "Divider",
            ]
        );
        assert!(!labels.iter().any(|l| l.eq_ignore_ascii_case("bookmark")));
    }

    #[test]
    fn filter_prefix_before_substring() {
        let labels = ["Heading 1", "Heading 2", "Text"];
        let ranked = filter_indices("hea", &labels);
        assert_eq!(ranked, vec![0, 1]);
        let ranked = filter_indices("ext", &labels);
        assert_eq!(ranked, vec![2]);
        let ranked = filter_indices("", &labels);
        assert_eq!(ranked, vec![0, 1, 2]);
    }

    #[test]
    fn slash_query_and_refilter() {
        let at = Cursor::new("b1".into(), Part::Body, 0);
        let mut slash = Slash::open(at.clone());
        assert_eq!(
            slash.query(Cursor::new("b1".into(), Part::Body, 1), "/"),
            Some(String::new())
        );
        assert_eq!(
            slash.query(Cursor::new("b1".into(), Part::Body, 3), "/he"),
            Some("he".into())
        );
        assert_eq!(
            slash.query(Cursor::new("b1".into(), Part::Body, 4), "/he "),
            None,
            "space closes"
        );
        slash.refilter("hea");
        assert_eq!(slash.choice(), Some(BlockType::Heading { level: 1 }));
        slash.step(1);
        assert_eq!(slash.choice(), Some(BlockType::Heading { level: 2 }));
    }
}
