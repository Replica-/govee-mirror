//! One frame of the mirror: sample, render, scale, write.

use std::time::{Duration, Instant};

use super::{Context, Writer, lock, writer};
use crate::mapping::render;
use crate::{capture, pattern, sound};

impl Context {
    pub(super) fn frame(&mut self) {
        let Some(mirror) = self.mirror.as_mut() else {
            return;
        };
        let grid = self.settings.grid;
        let seconds = if self.settings.moving {
            mirror.started.elapsed().as_secs_f32()
        } else {
            0.0
        };
        let (source, speed) = (self.settings.source, self.settings.animation_speed);
        let cells = pattern::draw(source, grid.cols, grid.rows, seconds, speed).or_else(|| {
            let frame = capture::lock(&self.capture).frame.clone();
            frame.map(|frame| frame.cells(grid.cols, grid.rows))
        });
        if let Some(cells) = cells {
            let mut frames = render(
                &mirror.plan,
                &cells,
                &mut mirror.state,
                self.settings.smoothing,
                self.settings.saturation,
            );
            let pulse = if self.settings.sound.enabled {
                let hit = sound::take_hit(&self.sound);
                mirror.pulse.next(hit, self.settings.sound.fade)
            } else {
                0.0
            };
            for (id, colors) in &mut frames {
                let razer = matches!(mirror.writers.get(id), Some(Writer::Razer { .. }));
                dim(colors, self.settings.scale(id, razer, pulse));
            }
            if let Some((id, segment, started)) = &mirror.highlight {
                match writer::blink(started.elapsed()) {
                    Some(on) => {
                        if let Some(colors) = frames.get_mut(id) {
                            colors.fill([0; 3]);
                            if let Some(color) = colors.get_mut(*segment).filter(|_| on) {
                                *color = [255; 3];
                            }
                        }
                    }
                    None => mirror.highlight = None,
                }
            }
            for (id, colors) in &frames {
                if let Some(writer) = mirror.writers.get(id) {
                    writer.write(&self.govee, id, colors);
                }
            }
            mirror.rendered += 1;
        }
        if mirror.reported.elapsed() >= Duration::from_secs(1) {
            let mut status = lock(&self.status);
            status.fps = mirror.rendered;
            status.stats = mirror
                .writers
                .iter()
                .map(|(id, w)| (id.clone(), w.stats()))
                .collect();
            mirror.rendered = 0;
            mirror.reported = Instant::now();
            drop(status);
            (self.repaint)();
        }
    }
}

/// Multiply every channel by `scale`, from 0 to 1.
fn dim(colors: &mut [[u8; 3]], scale: f32) {
    if scale >= 1.0 {
        return;
    }
    for channel in colors.iter_mut().flatten() {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            *channel = (f32::from(*channel) * scale.max(0.0)).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dim_scales_every_channel_and_full_scale_changes_nothing() {
        let mut colors = [[200, 100, 0]];
        dim(&mut colors, 1.0);
        assert_eq!(colors, [[200, 100, 0]]);
        dim(&mut colors, 0.3);
        assert_eq!(colors, [[60, 30, 0]]);
    }
}
