//! Virtualized-list arithmetic, shared by every long panel list.
//!
//! It lived in `hierarchy.rs` while there was exactly one such list. The asset
//! browser is the second, and two copies of a window calculation is precisely
//! how the two drift — one gets an overscan fix and the other does not, and the
//! symptom is a blank strip at the top of one panel only. Spec §7 asks for
//! shared widget behaviour for the same reason.

/// Fixed row height (logical px) — the virtualization contract: only the rows
/// inside the scroll viewport (plus overscan) exist as UI nodes, so a 10k-entity
/// scene renders ~30 rows, not 10k (C4).
pub(crate) const ROW_HEIGHT: f32 = 25.0;
const OVERSCAN: usize = 4;

/// Which slice of `rows` to materialize for a viewport.
pub(crate) fn visible_window(scroll_y: f32, view_height: f32, total: usize) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }
    let first = ((scroll_y / ROW_HEIGHT).floor() as usize).saturating_sub(OVERSCAN);
    let count = (view_height / ROW_HEIGHT).ceil() as usize + 2 * OVERSCAN;
    let first = first.min(total.saturating_sub(1));
    (first, (first + count).min(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    // C4: window math — 10k rows materialize only a viewport's worth of nodes.
    #[test]
    fn window_is_viewport_sized() {
        let (first, last) = visible_window(0.0, 400.0, 10_000);
        assert_eq!(first, 0);
        assert!(last <= (400.0 / ROW_HEIGHT).ceil() as usize + 8 + 1);

        let (first, last) = visible_window(5_000.0 * ROW_HEIGHT, 400.0, 10_000);
        assert!((5_000 - 8..=5_000).contains(&first));
        assert!(last - first <= (400.0 / ROW_HEIGHT).ceil() as usize + 8 + 1);

        // Tail clamps.
        let (first, last) = visible_window(1e9, 400.0, 10_000);
        assert_eq!(last, 10_000);
        assert!(first < 10_000);

        assert_eq!(visible_window(0.0, 400.0, 0), (0, 0));
    }
}
