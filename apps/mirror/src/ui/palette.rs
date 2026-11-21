//! The segments of the chosen light, one numbered swatch each. A click picks
//! the segment as the brush and lights it on the light.

use std::collections::BTreeMap;

use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Stroke, Ui, vec2};

use crate::engine::{Command, Status};
use crate::settings::Settings;
use crate::ui::{Brush, segment_color, text_on};

const SWATCH: f32 = 34.0;

/// Answers the commands to send, and whether the settings changed.
pub fn show(
    ui: &mut Ui,
    status: &Status,
    settings: &mut Settings,
    brush: &mut Brush,
) -> (Vec<Command>, bool) {
    let mut commands = Vec::new();
    let mut changed = false;
    ui.heading("Segments");
    let Some(target) = brush
        .target
        .as_ref()
        .and_then(|id| status.targets.iter().find(|t| &t.id == id))
    else {
        ui.label(RichText::new("Choose a light on the left.").weak());
        return (commands, changed);
    };
    let zones = settings.zones(target);
    brush.segment = brush.segment.min(zones.saturating_sub(1));
    ui.label(RichText::new(&target.name).strong());
    ui.label(RichText::new(format!("{zones} segments")).weak());
    if !target.streams() {
        ui.label(
            RichText::new(
                "No device file gives this model the segment channel yet, so it mirrors as one color.",
            )
            .weak(),
        );
    }

    let mut cells: BTreeMap<usize, usize> = BTreeMap::new();
    for assignment in settings
        .assignments
        .values()
        .filter(|a| a.target == target.id)
    {
        *cells.entry(assignment.segment).or_default() += 1;
    }

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for segment in 0..zones {
            let (rect, response) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
            let color = segment_color(segment, zones);
            let painter = ui.painter();
            painter.rect_filled(rect, 3.0, color);
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                segment + 1,
                FontId::proportional(13.0),
                text_on(color),
            );
            if segment == brush.segment {
                painter.rect_stroke(
                    rect,
                    3.0,
                    Stroke::new(3.0, Color32::WHITE),
                    egui::StrokeKind::Outside,
                );
            } else if !cells.contains_key(&segment) {
                // A segment that no cell feeds stays black on the light.
                painter.rect_filled(rect, 3.0, Color32::from_black_alpha(110));
            }
            let count = cells.get(&segment).copied().unwrap_or(0);
            if response
                .on_hover_text(format!("Segment {}: {count} cells", segment + 1))
                .clicked()
            {
                brush.segment = segment;
                commands.push(Command::Highlight(target.id.clone(), segment));
            }
        }
    });

    ui.add_space(8.0);
    ui.label(
        RichText::new(format!(
            "Brush: segment {}. A dimmed swatch has no cell yet, and stays dark.",
            brush.segment + 1
        ))
        .weak(),
    );
    ui.add_space(8.0);
    if ui.button("Clear this light's cells").clicked() {
        settings.assignments.retain(|_, a| a.target != target.id);
        changed = true;
    }
    if !settings.assignments.is_empty() && ui.button("Clear every cell").clicked() {
        settings.assignments.clear();
        changed = true;
    }
    (commands, changed)
}
