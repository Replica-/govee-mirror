//! From the screen grid to the colors of each target.
//!
//! An assignment gives one cell to one segment of one target. A segment that
//! several cells feed takes their average. A segment that no cell feeds stays
//! black.

use std::collections::HashMap;

use crate::settings::{Assignment, Cell, Grid};

/// The cells that feed each segment of one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sources {
    pub zones: usize,
    /// One list per segment, of cell indexes, row by row.
    pub cells: Vec<Vec<usize>>,
}

/// Group the cells by the segment they feed.
///
/// An assignment to a target that is not in `zones`, to a segment past the
/// count, or to a cell outside the grid is left out, and stays in the
/// settings.
pub fn plan<'a>(
    assignments: impl IntoIterator<Item = (Cell, &'a Assignment)>,
    zones: &HashMap<String, usize>,
    grid: Grid,
) -> HashMap<String, Sources> {
    let mut targets: HashMap<String, Sources> = HashMap::new();
    for (cell, assignment) in assignments {
        let Some(&count) = zones.get(&assignment.target) else {
            continue;
        };
        if assignment.segment >= count || cell.row >= grid.rows || cell.col >= grid.cols {
            continue;
        }
        let sources = targets
            .entry(assignment.target.clone())
            .or_insert_with(|| Sources {
                zones: count,
                cells: vec![Vec::new(); count],
            });
        // A run past the last segment stops there.
        for segment in assignment.segments().take_while(|s| *s < count) {
            sources.cells[segment].push(cell.row * grid.cols + cell.col);
        }
    }
    targets
}

/// The colors of one frame, per target.
///
/// `cells` holds one color per cell, row by row. `smoothing` is the share of
/// the previous frame that stays, from 0 to 1. `state` carries that previous
/// frame unrounded: rounded bytes stop short of the value they move to.
/// `saturation` is applied to the average before the smoothing; see
/// [`saturate`].
pub fn render(
    targets: &HashMap<String, Sources>,
    cells: &[[u8; 3]],
    state: &mut HashMap<String, Vec<[f32; 3]>>,
    smoothing: f32,
    saturation: f32,
) -> HashMap<String, Vec<[u8; 3]>> {
    let mut frames = HashMap::with_capacity(targets.len());
    for (target, sources) in targets {
        let exact = state
            .entry(target.clone())
            .or_insert_with(|| vec![[f32::NAN; 3]; sources.zones]);
        exact.resize(sources.zones, [f32::NAN; 3]);
        let mut frame = vec![[0u8; 3]; sources.zones];
        for (zone, list) in sources.cells.iter().enumerate() {
            if list.is_empty() {
                continue;
            }
            let mut sum = [0u32; 3];
            for &cell in list {
                let Some(color) = cells.get(cell) else {
                    continue;
                };
                for channel in 0..3 {
                    sum[channel] += u32::from(color[channel]);
                }
            }
            #[allow(clippy::cast_precision_loss)]
            let average = sum.map(|s| s as f32 / list.len() as f32);
            let average = saturate(average, saturation);
            for channel in 0..3 {
                let value = average[channel];
                let last = exact[zone][channel];
                let next = if last.is_nan() {
                    value
                } else {
                    last * smoothing + value * (1.0 - smoothing)
                };
                exact[zone][channel] = next;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    frame[zone][channel] = next.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        frames.insert(target.clone(), frame);
    }
    frames
}

/// Move a color away from grey by `factor`, keeping its brightest channel.
/// 1 changes nothing, and 2 doubles the distance of each channel from the
/// brightest one.
pub fn saturate(color: [f32; 3], factor: f32) -> [f32; 3] {
    let top = color[0].max(color[1]).max(color[2]);
    color.map(|c| (top + (c - top) * factor).clamp(0.0, 255.0))
}

/// A rectangle of cells, both corners included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub top: Cell,
    pub bottom: Cell,
}

impl Rect {
    /// The rectangle with `a` and `b` at two opposite corners.
    pub fn between(a: Cell, b: Cell) -> Self {
        Self {
            top: Cell {
                row: a.row.min(b.row),
                col: a.col.min(b.col),
            },
            bottom: Cell {
                row: a.row.max(b.row),
                col: a.col.max(b.col),
            },
        }
    }

    pub fn rows(self) -> usize {
        self.bottom.row - self.top.row + 1
    }

    pub fn cols(self) -> usize {
        self.bottom.col - self.top.col + 1
    }

    pub fn cells(self) -> impl Iterator<Item = Cell> {
        (self.top.row..=self.bottom.row)
            .flat_map(move |row| (self.top.col..=self.bottom.col).map(move |col| Cell { row, col }))
    }
}

/// Spread the segments `from` to `to` over a rectangle, along its longer
/// side: left to right where it is at least as wide as it is tall, else top
/// to bottom. Every cell of one column, or of one row, takes the same run.
/// `to` below `from` runs the segments backwards.
///
/// Answers each cell with the first segment of its run and the run's length.
/// With more segments than columns or rows, the runs cover the range with no
/// gap. With fewer, each one is a single segment, the ends take `from` and
/// `to`, and the ones between take an even step.
pub fn fill(rect: Rect, from: usize, to: usize) -> Vec<(Cell, usize, usize)> {
    let across = rect.cols() >= rect.rows();
    let lines = if across { rect.cols() } else { rect.rows() };
    let span = from.abs_diff(to) + 1;
    rect.cells()
        .map(|cell| {
            let index = if across {
                cell.col - rect.top.col
            } else {
                cell.row - rect.top.row
            };
            // Positions along the range, from `from` toward `to`.
            let (low, high) = if span > lines {
                (index * span / lines, (index + 1) * span / lines - 1)
            } else if lines == 1 {
                (0, 0)
            } else {
                #[allow(
                    clippy::cast_precision_loss,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss
                )]
                let at = (index as f64 * (span - 1) as f64 / (lines - 1) as f64).round() as usize;
                (at, at)
            };
            let (first, last) = if to >= from {
                (from + low, from + high)
            } else {
                (from - high, from - low)
            };
            (cell, first, last - first + 1)
        })
        .collect()
}

/// The row count that makes the cells of `cols` columns square on a screen
/// of `ratio`, width over height. Square to a pixel where the height does not
/// divide.
pub fn square_rows(cols: usize, ratio: f32) -> usize {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let rows = (cols as f32 / ratio).round() as usize;
    rows.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: Grid = Grid { cols: 2, rows: 2 };

    fn at(row: usize, col: usize, target: &str, segment: usize) -> (Cell, Assignment) {
        (Cell { row, col }, Assignment::one(target, segment))
    }

    fn zones(target: &str, count: usize) -> HashMap<String, usize> {
        HashMap::from([(target.to_owned(), count)])
    }

    #[test]
    fn a_segment_takes_the_average_of_its_cells_and_a_segment_with_no_cell_stays_black() {
        let assignments = [at(0, 0, "a", 0), at(0, 1, "a", 0)];
        let targets = plan(
            assignments.iter().map(|(c, a)| (*c, a)),
            &zones("a", 2),
            GRID,
        );
        let cells = [[200, 0, 0], [0, 100, 0], [9, 9, 9], [9, 9, 9]];
        let frames = render(&targets, &cells, &mut HashMap::new(), 0.0, 1.0);
        assert_eq!(frames["a"], vec![[100, 50, 0], [0, 0, 0]]);
    }

    #[test]
    fn one_cell_feeds_a_segment_of_each_light_that_paints_it() {
        let assignments = [at(0, 0, "a", 1), at(0, 0, "b", 0)];
        let zones = HashMap::from([("a".to_owned(), 2), ("b".to_owned(), 1)]);
        let targets = plan(assignments.iter().map(|(c, a)| (*c, a)), &zones, GRID);
        let cells = [[90, 0, 0], [0; 3], [0; 3], [0; 3]];
        let frames = render(&targets, &cells, &mut HashMap::new(), 0.0, 1.0);
        assert_eq!(frames["a"], vec![[0, 0, 0], [90, 0, 0]]);
        assert_eq!(frames["b"], vec![[90, 0, 0]]);
    }

    #[test]
    fn an_assignment_outside_the_grid_past_the_count_or_to_an_absent_target_is_left_out() {
        let assignments = [at(2, 0, "a", 0), at(0, 0, "a", 5), at(0, 1, "b", 0)];
        assert!(
            plan(
                assignments.iter().map(|(c, a)| (*c, a)),
                &zones("a", 2),
                GRID
            )
            .is_empty()
        );
    }

    #[test]
    fn smoothing_reaches_the_value_it_moves_to() {
        let assignments = [at(0, 0, "a", 0)];
        let targets = plan(
            assignments.iter().map(|(c, a)| (*c, a)),
            &zones("a", 1),
            GRID,
        );
        let mut state = HashMap::new();
        render(&targets, &[[255; 3]; 4], &mut state, 0.9, 1.0);
        let mut frames = HashMap::new();
        for _ in 0..300 {
            frames = render(&targets, &[[0; 3]; 4], &mut state, 0.9, 1.0);
        }
        assert_eq!(frames["a"], vec![[0, 0, 0]]);
    }

    #[test]
    fn saturation_moves_a_color_from_grey_and_keeps_its_brightest_channel() {
        assert_eq!(saturate([200.0, 150.0, 100.0], 1.0), [200.0, 150.0, 100.0]);
        assert_eq!(saturate([200.0, 150.0, 100.0], 2.0), [200.0, 100.0, 0.0]);
        assert_eq!(saturate([120.0, 120.0, 120.0], 2.0), [120.0, 120.0, 120.0]);
    }

    #[test]
    fn a_box_with_more_segments_than_columns_covers_every_segment() {
        let rect = Rect::between(Cell { row: 0, col: 0 }, Cell { row: 0, col: 15 });
        let mut covered: Vec<usize> = fill(rect, 0, 59)
            .into_iter()
            .flat_map(|(_, first, count)| first..first + count)
            .collect();
        covered.sort_unstable();
        assert_eq!(covered, (0..60).collect::<Vec<_>>());
        let first = fill(rect, 0, 59)[0];
        assert_eq!((first.1, first.2), (0, 3));
    }

    #[test]
    fn to_below_from_runs_the_segments_the_other_way() {
        let rect = Rect::between(Cell { row: 0, col: 0 }, Cell { row: 1, col: 15 });
        let runs: HashMap<Cell, (usize, usize)> = fill(rect, 59, 0)
            .into_iter()
            .map(|(cell, first, count)| (cell, (first, count)))
            .collect();
        // The left column ends the range at 59; the right one starts it at 0.
        let (first, count) = runs[&Cell { row: 1, col: 0 }];
        assert_eq!(first + count - 1, 59);
        assert_eq!(runs[&Cell { row: 0, col: 15 }].0, 0);
    }

    #[test]
    fn a_box_with_fewer_segments_than_columns_gives_each_one_segment() {
        let rect = Rect::between(Cell { row: 0, col: 2 }, Cell { row: 0, col: 5 });
        let runs: Vec<(usize, usize)> = fill(rect, 0, 3)
            .into_iter()
            .map(|(_, f, c)| (f, c))
            .collect();
        assert_eq!(runs, vec![(0, 1), (1, 1), (2, 1), (3, 1)]);
        let tall = Rect::between(Cell { row: 0, col: 0 }, Cell { row: 9, col: 0 });
        let runs: Vec<usize> = fill(tall, 0, 4).into_iter().map(|(_, f, _)| f).collect();
        assert_eq!((runs[0], runs[9]), (0, 4));
    }

    #[test]
    fn square_rows_follow_the_shape_of_the_screen() {
        assert_eq!(square_rows(16, 16.0 / 9.0), 9);
        assert_eq!(square_rows(32, 21.0 / 9.0), 14);
        assert_eq!(square_rows(1, 32.0 / 9.0), 1);
    }
}
