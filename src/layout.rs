//! Pure responsive geometry (spec §5.1). No state, no ratatui widgets — just `(area,
//! threshold) -> two rectangles`, so every breakpoint is a unit test.

use ratatui::layout::Rect;

/// The tree's share of the width in the side-by-side layout (spec §5.1: 左樹 40% / 右 diff 60%).
pub const TREE_PCT: u16 = 40;

/// Which way the pane was divided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Wide pane: tree left, diff right.
    SideBySide,
    /// Narrow pane: tree on top, diff below, each the full width.
    Stacked,
}

/// Where the two regions go for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub tree: Rect,
    pub diff: Rect,
    pub orientation: Orientation,
}

/// Divide `area` into the tree and diff regions (spec §5.1).
///
/// At or above `split_threshold_cols` the pane splits left/right with the tree taking
/// [`TREE_PCT`] of the width; below it the pane stacks, the tree taking the ceiling half of the
/// height (the spec's narrow mockup draws more tree than diff).
///
/// The two regions always tile `area` exactly: no gap, no overlap, nothing outside it.
pub fn geometry(area: Rect, split_threshold_cols: u16) -> Geometry {
    if area.width == 0 || area.height == 0 {
        return Geometry {
            tree: Rect::new(area.x, area.y, 0, 0),
            diff: Rect::new(area.x, area.y, 0, 0),
            orientation: Orientation::Stacked,
        };
    }
    if area.width >= split_threshold_cols {
        // Both columns keep at least one cell: `max(1)` on the tree, and `width - tree_w`
        // cannot reach 0 because tree_w is at most 40% of the width for any width >= 2, and at
        // width 1 the pane is below any sane threshold anyway.
        let tree_w = ((u32::from(area.width) * u32::from(TREE_PCT)) / 100).max(1) as u16;
        let tree_w = tree_w.min(area.width.saturating_sub(1)).max(1);
        return Geometry {
            tree: Rect::new(area.x, area.y, tree_w, area.height),
            diff: Rect::new(area.x + tree_w, area.y, area.width - tree_w, area.height),
            orientation: Orientation::SideBySide,
        };
    }
    let tree_h = area.height.div_ceil(2);
    Geometry {
        tree: Rect::new(area.x, area.y, area.width, tree_h),
        diff: Rect::new(area.x, area.y + tree_h, area.width, area.height - tree_h),
        orientation: Orientation::Stacked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(width: u16, height: u16) -> Rect {
        Rect::new(0, 0, width, height)
    }

    #[test]
    fn at_or_above_the_threshold_the_pane_splits_left_and_right() {
        let g = geometry(area(120, 40), 120);
        assert_eq!(g.orientation, Orientation::SideBySide);
        assert_eq!(g.tree.height, 40);
        assert_eq!(g.diff.height, 40);
        assert_eq!(g.tree.y, 0);
        assert_eq!(g.diff.y, 0);
    }

    #[test]
    fn one_column_below_the_threshold_the_pane_stacks() {
        // The boundary itself is the interesting case: >= is inclusive.
        let g = geometry(area(119, 40), 120);
        assert_eq!(g.orientation, Orientation::Stacked);
        assert_eq!(g.tree.width, 119);
        assert_eq!(g.diff.width, 119);
    }

    #[test]
    fn a_side_by_side_tree_takes_forty_percent_of_the_width() {
        let g = geometry(area(200, 10), 120);
        assert_eq!(g.tree.width, 80);
        assert_eq!(g.diff.width, 120);
    }

    #[test]
    fn a_side_by_side_split_covers_the_whole_width_with_no_gap_or_overlap() {
        for width in [120u16, 121, 150, 333, 1000] {
            let g = geometry(area(width, 10), 120);
            assert_eq!(g.tree.x, 0, "w={width}");
            assert_eq!(g.diff.x, g.tree.width, "w={width}");
            assert_eq!(g.tree.width + g.diff.width, width, "w={width}");
        }
    }

    #[test]
    fn a_stacked_split_covers_the_whole_height_with_no_gap_or_overlap() {
        for height in [1u16, 2, 5, 40, 200] {
            let g = geometry(area(80, height), 120);
            assert_eq!(g.tree.y, 0, "h={height}");
            assert_eq!(g.diff.y, g.tree.height, "h={height}");
            assert_eq!(g.tree.height + g.diff.height, height, "h={height}");
        }
    }

    #[test]
    fn a_stacked_tree_gets_the_larger_half_of_an_odd_height() {
        // The spec's narrow mockup draws more tree than diff; ceiling division gives that.
        let g = geometry(area(80, 5), 120);
        assert_eq!(g.tree.height, 3);
        assert_eq!(g.diff.height, 2);
    }

    #[test]
    fn the_geometry_is_placed_inside_the_given_area_not_at_the_origin() {
        let g = geometry(Rect::new(7, 3, 200, 20), 120);
        assert_eq!(g.tree.x, 7);
        assert_eq!(g.tree.y, 3);
        assert_eq!(g.diff.y, 3);
        assert_eq!(g.diff.x, 7 + g.tree.width);
    }

    #[test]
    fn a_zero_sized_area_yields_zero_sized_regions_rather_than_panicking() {
        for a in [area(0, 0), area(0, 10), area(10, 0)] {
            let g = geometry(a, 120);
            assert_eq!(g.tree.width.min(g.tree.height), 0);
            assert_eq!(g.diff.width.min(g.diff.height), 0);
        }
    }

    #[test]
    fn an_extremely_narrow_pane_still_gives_the_tree_at_least_one_column() {
        let g = geometry(area(1, 10), 120);
        assert_eq!(g.orientation, Orientation::Stacked);
        assert_eq!(g.tree.width, 1);
    }

    #[test]
    fn a_wide_but_one_row_pane_gives_the_tree_the_only_row() {
        let g = geometry(area(200, 1), 120);
        assert_eq!(g.orientation, Orientation::SideBySide);
        assert_eq!(g.tree.height, 1);
        assert_eq!(g.diff.height, 1);
    }

    #[test]
    fn a_threshold_of_zero_always_splits_side_by_side() {
        assert_eq!(
            geometry(area(10, 10), 0).orientation,
            Orientation::SideBySide
        );
    }

    #[test]
    fn a_side_by_side_split_never_starves_either_column_of_its_last_column() {
        // 40% of 40 is 16; both sides must stay non-zero at every width at/above the floor.
        for width in 40u16..=200 {
            let g = geometry(area(width, 10), 40);
            assert!(g.tree.width >= 1, "w={width}");
            assert!(g.diff.width >= 1, "w={width}");
        }
    }
}
