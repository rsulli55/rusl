use itertools::Itertools;
use std::slice::ChunksExact;

use crate::constants::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutInfo {
    pub num_cols: usize,
    pub col_width: Vec<usize>,
}

impl LayoutInfo {
    pub fn new(num_cols: usize, col_width: Vec<usize>) -> Self {
        Self {
            num_cols,
            col_width,
        }
    }
}

impl Default for LayoutInfo {
    fn default() -> Self {
        Self {
            num_cols: 1,
            col_width: vec![0],
        }
    }
}

fn col_widths_by_cols(min_width: usize, num_cols: usize, lens: &[usize]) -> Vec<usize> {
    let num_rows = lens.len() / num_cols;
    let rem = lens.len() % num_cols;
    // the first `rem` cols must accomodate num_rows + 1 rows
    let end = rem * (num_rows + 1);

    // calculate appropriate col width from chunks of columns
    let chunks_to_col_width = |chunks: ChunksExact<_>| {
        chunks
            .into_iter()
            .map(|col| col.iter().fold(min_width, |acc, l| std::cmp::max(acc, *l)))
            .collect_vec()
    };

    // chunk the first rem columns by num_rows + 1 elements
    let chunks = lens[..end].chunks_exact(num_rows + 1);
    debug_assert!(chunks.remainder().is_empty());
    let start_col_widths = chunks_to_col_width(chunks);

    // chunk the rest of the columns by num_rows elements
    let chunks = lens[end..].chunks_exact(num_rows);
    debug_assert!(chunks.remainder().is_empty());
    let fin_col_widths = chunks_to_col_width(chunks);
    [start_col_widths, fin_col_widths].concat()
}

fn col_widths_by_lines(min_width: usize, num_cols: usize, lens: &[usize]) -> Vec<usize> {
    let mut col_width = Vec::with_capacity(num_cols);
    for offset in 0..num_cols {
        let width = lens[offset..]
            .iter()
            .step_by(num_cols)
            .fold(min_width, |acc, l| std::cmp::max(acc, *l));
        dbg!(num_cols, offset, width);
        col_width.push(width);
    }
    dbg!(&col_width);
    col_width
}

/// Determines the layout for displaying a list of strings within the current terminal width with
/// the maximal amount of columns.
///
/// This function calculates possible layouts for the given `paths` based on the available terminal columns (`term_cols`)
/// and the minimum column size. It tries different numbers of columns, computes the required width for each layout,
/// and selects the most space-efficient layout that fits within the terminal.
///
/// # Parameters
/// - `by_lines`: If `true`, strings in `str_paths` are layed out across rows, otherwise down columns.
/// - `term_cols`: The total number of columns available in the terminal.
/// - `paths`: A slice of PathInfo representing the items to be displayed.
///
/// # Returns
/// A `LayoutInfo` struct describing the chosen layout (number of columns and their widths). If no valid layout fits,
/// returns a default `LayoutInfo`.
pub fn determine_layout(by_lines: bool, term_cols: usize, lens: &[usize]) -> LayoutInfo {
    let max_cols = std::cmp::min(term_cols / MIN_COL_SIZE, lens.len());

    let mut valid_layouts = Vec::with_capacity(max_cols);
    for num_cols in 1..=max_cols {
        let col_width = if by_lines {
            col_widths_by_lines(MIN_COL_SIZE, num_cols, lens)
        } else {
            col_widths_by_cols(MIN_COL_SIZE, num_cols, lens)
        };
        dbg!(&col_width);
        let total_width: usize = col_width.iter().sum();
        if total_width <= term_cols {
            valid_layouts.push(LayoutInfo::new(num_cols, col_width));
        }
    }
    dbg!(&valid_layouts);
    valid_layouts.pop().unwrap_or_default()
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_determine_layout1() {
        let term_cols = 10;
        let lens = vec![5, 5, 4, 4];
        let layout_by_cols = LayoutInfo::new(2, vec![5, 4]);
        assert_eq!(determine_layout(false, term_cols, &lens), layout_by_cols);
        let layout_by_lines = LayoutInfo::new(2, vec![5, 5]);
        assert_eq!(determine_layout(true, term_cols, &lens), layout_by_lines);
    }

    #[test]
    fn test_determine_layout2() {
        let term_cols = 13;
        let lens = vec![3, 3, 7, 7, 3];
        // by columns:
        // attempting to layout this in 2 columns fails:
        // the widths are:
        // 3   7
        // 3   3
        // 7
        // but three columns works:
        // 3   7   3
        // 3   7
        let layout_by_cols = LayoutInfo::new(3, vec![3, 7, 3]);
        assert_eq!(determine_layout(false, term_cols, &lens), layout_by_cols);
        // by lines:
        // attempting to layout this in 2 columns fails:
        // the widths are:
        // 3   3
        // 7   7
        // 3
        // three columns doesn't work either:
        // 3   3   7
        // 7   3
        // only 1 column will work
        let layout_by_lines = LayoutInfo::new(1, vec![7]);
        assert_eq!(determine_layout(true, term_cols, &lens), layout_by_lines);
    }
}
