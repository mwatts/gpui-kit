//! Slash menu query filter (Bezel `slash.rs`): ranking and the active row.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
