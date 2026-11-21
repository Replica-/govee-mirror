//! From the screen grid to the colors of each target.
//!
//! An assignment gives one cell to one segment of one target. A segment that
//! several cells feed takes their average. A segment that no cell feeds stays
//! black.

use std::collections::{BTreeMap, HashMap};

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
pub fn plan(
    assignments: &BTreeMap<Cell, Assignment>,
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
        sources.cells[assignment.segment].push(cell.row * grid.cols + cell.col);
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

/// Rainbow rings around the center of the grid, moving outward: one color per
/// cell, row by row. `seconds` is the time since the animation started, and
/// `speed` is how many times a second one ring takes the place of the next.
pub fn circular(cols: usize, rows: usize, seconds: f32, speed: f32) -> Vec<[u8; 3]> {
    // Two full rainbows from the center to a corner.
    const RINGS: f32 = 2.0;
    #[allow(clippy::cast_precision_loss)]
    let (cx, cy) = (cols as f32 / 2.0, rows as f32 / 2.0);
    let corner = cx.hypot(cy).max(f32::EPSILON);
    (0..rows)
        .flat_map(|row| (0..cols).map(move |col| (row, col)))
        .map(|(row, col)| {
            #[allow(clippy::cast_precision_loss)]
            let distance = (col as f32 + 0.5 - cx).hypot(row as f32 + 0.5 - cy) / corner;
            hue((distance * RINGS - seconds * speed).rem_euclid(1.0))
        })
        .collect()
}

/// A fully saturated color at `hue`, from 0 to 1.
fn hue(hue: f32) -> [u8; 3] {
    let channel = |offset: f32| {
        let k = (hue * 6.0 + offset).rem_euclid(6.0);
        let value = 1.0 - (k.min(4.0 - k).clamp(0.0, 1.0));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let byte = (value * 255.0).round() as u8;
        byte
    };
    [channel(5.0), channel(3.0), channel(1.0)]
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
        (
            Cell { row, col },
            Assignment {
                target: target.to_owned(),
                segment,
            },
        )
    }

    fn zones(target: &str, count: usize) -> HashMap<String, usize> {
        HashMap::from([(target.to_owned(), count)])
    }

    #[test]
    fn a_segment_takes_the_average_of_its_cells_and_a_segment_with_no_cell_stays_black() {
        let assignments = BTreeMap::from([at(0, 0, "a", 0), at(0, 1, "a", 0)]);
        let targets = plan(&assignments, &zones("a", 2), GRID);
        let cells = [[200, 0, 0], [0, 100, 0], [9, 9, 9], [9, 9, 9]];
        let frames = render(&targets, &cells, &mut HashMap::new(), 0.0, 1.0);
        assert_eq!(frames["a"], vec![[100, 50, 0], [0, 0, 0]]);
    }

    #[test]
    fn an_assignment_outside_the_grid_past_the_count_or_to_an_absent_target_is_left_out() {
        let assignments = BTreeMap::from([at(2, 0, "a", 0), at(0, 0, "a", 5), at(0, 1, "b", 0)]);
        assert!(plan(&assignments, &zones("a", 2), GRID).is_empty());
    }

    #[test]
    fn smoothing_reaches_the_value_it_moves_to() {
        let assignments = BTreeMap::from([at(0, 0, "a", 0)]);
        let targets = plan(&assignments, &zones("a", 1), GRID);
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
    fn the_rings_start_red_at_the_center_and_move_outward() {
        assert_eq!(hue(0.0), [255, 0, 0]);
        assert_eq!(hue(1.0 / 3.0), [0, 255, 0]);
        assert_eq!(hue(2.0 / 3.0), [0, 0, 255]);
        let still = circular(2, 2, 0.0, 1.0);
        assert_eq!(still.len(), 4);
        // The four cells sit at the same distance from the center.
        assert!(still.iter().all(|c| *c == still[0]));
        assert_ne!(circular(2, 2, 0.25, 1.0)[0], still[0]);
    }

    #[test]
    fn square_rows_follow_the_shape_of_the_screen() {
        assert_eq!(square_rows(16, 16.0 / 9.0), 9);
        assert_eq!(square_rows(32, 21.0 / 9.0), 14);
        assert_eq!(square_rows(1, 32.0 / 9.0), 1);
    }
}
