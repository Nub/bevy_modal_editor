//! THE palette engine (M3-C5, spec §7 "one palette engine"): every searchable
//! list — commands, insert kinds, find-object, the materials library — is a list
//! of `PaletteEntry` items ranked by ONE fuzzy matcher. Items are built once per
//! open (`PaletteItems`); ranking re-runs only when the query changes.
//!
//! v1 virtues carried as binding requirements: items are data (label, category,
//! keywords, enabled, suffix); categories group the list; payloads are a typed
//! enum — never stringly-typed dispatch.

use bevy::prelude::*;
use editor_api::prelude::*;

/// What invoking an item does — typed payload, no string dispatch.
#[derive(Clone)]
pub(crate) enum PalettePayload {
    /// Emit this action through the normal dispatch path.
    Action(ActionId),
    /// Select this scene entity (find-object, and later asset pickers).
    Entity(SceneId),
    /// Assign this library material to the selection (C6).
    Material(uuid::Uuid),
}

#[derive(Clone)]
pub(crate) struct PaletteEntry {
    pub label: String,
    /// Section header this item groups under (None = flat).
    pub category: Option<String>,
    /// Extra haystack beyond the label (ids, descriptions).
    pub keywords: String,
    /// Right-aligned hint (key binding, kind, …).
    pub suffix: String,
    pub payload: PalettePayload,
}

/// The open palette's item list — built ONCE at open by the mode's source;
/// ranking filters this, never rebuilds it.
#[derive(Resource, Default)]
pub(crate) struct PaletteItems(pub Vec<PaletteEntry>);

/// Subsequence fuzzy score (fzy-style): all query chars must appear in order;
/// consecutive runs and word-boundary hits rank higher; earlier matches win
/// ties. `None` = no match. Empty query matches everything at score 0.
pub(crate) fn fuzzy_score(query: &str, haystack: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = haystack.chars().collect();
    let hay_lower: Vec<char> = haystack.to_lowercase().chars().collect();
    let query_lower: Vec<char> = query.to_lowercase().chars().collect();
    if hay_lower.len() != hay.len() {
        // Unicode case-mapping changed lengths — fall back to contains.
        return haystack
            .to_lowercase()
            .contains(&query.to_lowercase())
            .then_some(1);
    }

    let mut score = 0i32;
    let mut hay_index = 0usize;
    let mut previous_match: Option<usize> = None;
    for &q in &query_lower {
        let mut found = None;
        while hay_index < hay_lower.len() {
            if hay_lower[hay_index] == q {
                found = Some(hay_index);
                break;
            }
            hay_index += 1;
        }
        let index = found?;
        score += 1;
        if previous_match == Some(index.wrapping_sub(1)) {
            score += 5; // consecutive run
        }
        let boundary = index == 0
            || matches!(hay[index - 1], ' ' | '.' | '-' | '_' | ':' | '/');
        if boundary {
            score += 3;
        }
        // Slight preference for early matches.
        score -= (index / 8) as i32;
        previous_match = Some(index);
        hay_index = index + 1;
    }
    Some(score)
}

/// Rank the open items against a query: matching items sorted by score (desc),
/// stable within category order. Returns indices into `items`.
pub(crate) fn rank(items: &[PaletteEntry], query: &str) -> Vec<usize> {
    let mut scored: Vec<(usize, i32)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let hay = format!("{} {}", item.label, item.keywords);
            fuzzy_score(query, &hay).map(|score| (index, score))
        })
        .collect();
    // Category grouping first (stable input order carries category blocks),
    // score ordering within — for empty queries this preserves source order.
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.into_iter().map(|(index, _)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_prefers_boundaries_and_runs() {
        // "tp" hits both word starts of "Toggle Panel".
        let toggle_panel = fuzzy_score("tp", "Toggle Panel").unwrap();
        let trap = fuzzy_score("tp", "trap").unwrap();
        assert!(toggle_panel > trap, "{toggle_panel} vs {trap}");

        // Consecutive runs beat scattered matches.
        let save = fuzzy_score("save", "Save Scene").unwrap();
        let scattered = fuzzy_score("save", "sand cave").unwrap();
        assert!(save > scattered);

        // In-order requirement: no match when out of order.
        assert!(fuzzy_score("ba", "ab").is_none());
        // Empty query matches all.
        assert_eq!(fuzzy_score("", "anything"), Some(0));
    }

    #[test]
    fn rank_orders_by_score_then_source() {
        let items = vec![
            PaletteEntry {
                label: "Open Scene".into(),
                category: None,
                keywords: "reload".into(),
                suffix: String::new(),
                payload: PalettePayload::Action(ActionId::new_static("a")),
            },
            PaletteEntry {
                label: "Save Scene".into(),
                category: None,
                keywords: String::new(),
                suffix: String::new(),
                payload: PalettePayload::Action(ActionId::new_static("b")),
            },
        ];
        let ranked = rank(&items, "save");
        assert_eq!(ranked, vec![1]);
        let ranked = rank(&items, "load");
        assert_eq!(ranked, vec![0], "keywords are searchable");
        let ranked = rank(&items, "");
        assert_eq!(ranked, vec![0, 1], "empty query keeps source order");
    }
}
