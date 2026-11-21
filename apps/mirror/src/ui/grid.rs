//! The screen preview with the grid over it. A left drag paints the brush's
//! segment into the cells it crosses; a right drag clears them.

use eframe::egui::{
    self, Align2, Color32, FontId, PointerButton, Rect as Area, Sense, Stroke, TextureHandle, Ui,
    pos2, vec2,
};

use crate::engine::Target;
use crate::settings::{Assignment, Cell, Settings};
use crate::ui::{Brush, segment_color, text_on};

/// Draw the preview and the grid. Answers whether a cell changed.
pub fn show(
    ui: &mut Ui,
    settings: &mut Settings,
    targets: &[Target],
    preview: Option<&TextureHandle>,
    animation: Option<&[[u8; 3]]>,
    brush: &Brush,
) -> bool {
    let ratio = preview.map_or(16.0 / 9.0, |texture| {
        let [w, h] = texture.size();
        w as f32 / h.max(1) as f32
    });
    let room = ui.available_size() - vec2(0.0, 24.0);
    let width = room.x.min(room.y * ratio).max(100.0);
    let (area, response) =
        ui.allocate_exact_size(vec2(width, width / ratio), Sense::click_and_drag());
    let painter = ui.painter_at(area);

    match preview {
        Some(texture) => {
            let uv = Area::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), area, uv, Color32::WHITE);
        }
        None => {
            painter.rect_filled(area, 0.0, Color32::BLACK);
        }
    }

    let grid = settings.grid;
    let cw = area.width() / grid.cols as f32;
    let ch = area.height() / grid.rows as f32;
    let cell_area = |cell: Cell| {
        Area::from_min_size(
            pos2(
                area.left() + cell.col as f32 * cw,
                area.top() + cell.row as f32 * ch,
            ),
            vec2(cw, ch),
        )
    };
    // The animation stands in for the screen: one color per cell.
    if let Some(colors) = animation {
        for (index, [r, g, b]) in colors.iter().enumerate() {
            let cell = Cell {
                row: index / grid.cols,
                col: index % grid.cols,
            };
            painter.rect_filled(cell_area(cell), 0.0, Color32::from_rgb(*r, *g, *b));
        }
    }
    // Paint before drawing, so that the stroke shows in the same frame.
    let mut changed = false;
    if response.is_pointer_button_down_on()
        && let Some(pos) = response
            .interact_pointer_pos()
            .filter(|p| area.contains(*p))
    {
        let cell = Cell {
            row: (((pos.y - area.top()) / ch) as usize).min(grid.rows - 1),
            col: (((pos.x - area.left()) / cw) as usize).min(grid.cols - 1),
        };
        let erase = ui.input(|i| i.pointer.button_down(PointerButton::Secondary));
        if erase {
            changed |= settings.assignments.remove(&cell).is_some();
        } else if let Some(target) = &brush.target {
            let assignment = Assignment {
                target: target.clone(),
                segment: brush.segment,
            };
            if settings.assignments.get(&cell) != Some(&assignment) {
                settings.assignments.insert(cell, assignment);
                changed = true;
            }
        }
    }

    let zones = |id: &str| {
        targets
            .iter()
            .find(|t| t.id == id)
            .map_or(1, |t| settings.zones(t))
    };
    let font = FontId::proportional((cw.min(ch) * 0.4).clamp(8.0, 18.0));
    for (cell, assignment) in &settings.assignments {
        if cell.row >= grid.rows || cell.col >= grid.cols {
            continue;
        }
        let rect = cell_area(*cell).shrink(1.0);
        let color = segment_color(assignment.segment, zones(&assignment.target));
        let active = brush.target.as_ref() == Some(&assignment.target);
        let fill = if active {
            color.gamma_multiply(0.8)
        } else {
            color.gamma_multiply(0.25)
        };
        painter.rect_filled(rect, 2.0, fill);
        if active && cw.min(ch) >= 14.0 {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                assignment.segment + 1,
                font.clone(),
                text_on(color),
            );
        }
    }

    let line = Stroke::new(1.0, Color32::from_white_alpha(50));
    for col in 1..grid.cols {
        let x = area.left() + col as f32 * cw;
        painter.line_segment([pos2(x, area.top()), pos2(x, area.bottom())], line);
    }
    for row in 1..grid.rows {
        let y = area.top() + row as f32 * ch;
        painter.line_segment([pos2(area.left(), y), pos2(area.right(), y)], line);
    }

    if let Some(pos) = response.hover_pos() {
        let cell = Cell {
            row: (((pos.y - area.top()) / ch) as usize).min(grid.rows - 1),
            col: (((pos.x - area.left()) / cw) as usize).min(grid.cols - 1),
        };
        painter.rect_stroke(
            cell_area(cell),
            0.0,
            Stroke::new(2.0, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
        if let Some(assignment) = settings.assignments.get(&cell) {
            let name = targets
                .iter()
                .find(|t| t.id == assignment.target)
                .map_or(assignment.target.as_str(), |t| t.name.as_str());
            response
                .on_hover_text_at_pointer(format!("{name} · segment {}", assignment.segment + 1));
        }
    }

    ui.label(
        egui::RichText::new(
            "Pick a segment on the right, then click or drag over cells. Right-click clears a cell.",
        )
        .weak(),
    );
    changed
}
