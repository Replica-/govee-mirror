//! The screen, shrunk on the GPU and averaged into grid cells.
//!
//! The capture thread keeps the newest [`Frame`] in a [`Latest`]. The engine
//! and the window read it at their own rates, and a reader that falls behind
//! skips frames rather than queue them.

#[cfg(windows)]
mod dxgi;
#[cfg(windows)]
mod shrink;

use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// A small copy of the screen, four bytes per pixel in RGBA order.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    /// Counts the frames that carried a change of the screen.
    pub sequence: u64,
}

impl Frame {
    /// The average color of each cell, row by row.
    pub fn cells(&self, cols: usize, rows: usize) -> Vec<[u8; 3]> {
        let mut out = vec![[0u8; 3]; cols * rows];
        if self.width == 0 || self.height == 0 {
            return out;
        }
        for row in 0..rows {
            let y0 = row * self.height / rows;
            let y1 = ((row + 1) * self.height / rows)
                .max(y0 + 1)
                .min(self.height);
            for col in 0..cols {
                let x0 = col * self.width / cols;
                let x1 = ((col + 1) * self.width / cols).max(x0 + 1).min(self.width);
                let mut sum = [0u64; 3];
                for y in y0..y1 {
                    let line = &self.rgba[(y * self.width + x0) * 4..(y * self.width + x1) * 4];
                    for pixel in line.as_chunks::<4>().0 {
                        sum[0] += u64::from(pixel[0]);
                        sum[1] += u64::from(pixel[1]);
                        sum[2] += u64::from(pixel[2]);
                    }
                }
                let area = ((y1 - y0) * (x1 - x0)) as u64;
                #[allow(clippy::cast_possible_truncation)]
                {
                    out[row * cols + col] = sum.map(|channel| (channel / area) as u8);
                }
            }
        }
        out
    }
}

/// The newest frame, and the last error of the capture.
#[derive(Debug, Default)]
pub struct Latest {
    pub frame: Option<Arc<Frame>>,
    pub error: Option<String>,
    pub screens: Vec<String>,
    pub demand: Demand,
}

/// Who reads the screen now. With neither, the capture pauses.
#[derive(Debug, Default, Clone, Copy)]
pub struct Demand {
    /// The mirror runs on the screen, at the rate the settings give.
    pub mirror: bool,
    /// The window shows the preview, at [`PREVIEW_FPS`].
    pub window: bool,
}

/// The preview redraws at this rate, so a faster capture is wasted on it.
const PREVIEW_FPS: u32 = 10;
/// How often a paused capture checks for a reader.
const IDLE: Duration = Duration::from_millis(100);

pub type Shared = Arc<Mutex<Latest>>;

pub fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, Latest> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the capture thread reads before each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub display: usize,
    pub fps: u32,
}

pub type Requests = Arc<Mutex<Request>>;

/// Start the capture thread. It runs for the life of the process.
pub fn spawn(requests: Requests) -> Shared {
    let shared: Shared = Arc::default();
    let out = Arc::clone(&shared);
    thread::Builder::new()
        .name("capture".to_owned())
        .spawn(move || run(&requests, &out))
        .map_err(|error| lock(&shared).error = Some(error.to_string()))
        .ok();
    shared
}

#[cfg(windows)]
fn run(requests: &Requests, shared: &Shared) {
    let mut current: Option<(usize, dxgi::Duplicator)> = None;
    let mut sequence = 0u64;
    loop {
        let request = *requests.lock().unwrap_or_else(PoisonError::into_inner);
        let demand = lock(shared).demand;
        let fps = if demand.mirror {
            request.fps
        } else if demand.window {
            PREVIEW_FPS
        } else {
            // An open duplication costs GPU time on each change of the
            // screen, read or not.
            current = None;
            thread::sleep(IDLE);
            continue;
        };
        let period = Duration::from_secs(1) / fps.clamp(1, 120);
        let started = Instant::now();

        if current
            .as_ref()
            .is_none_or(|(display, _)| *display != request.display)
        {
            match dxgi::Duplicator::new(request.display) {
                Ok(duplicator) => {
                    let mut latest = lock(shared);
                    latest.screens = duplicator.screens.clone();
                    latest.error = None;
                    current = Some((request.display, duplicator));
                }
                Err(error) => {
                    lock(shared).error = Some(format!("screen {}: {error}", request.display + 1));
                    current = None;
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
            }
        }

        if let Some((_, duplicator)) = current.as_mut() {
            match duplicator.next(period) {
                Ok(Some((width, height, rgba))) => {
                    sequence += 1;
                    lock(shared).frame = Some(Arc::new(Frame {
                        width,
                        height,
                        rgba,
                        sequence,
                    }));
                }
                // The screen did not change: the last frame stays.
                Ok(None) => {}
                Err(error) => {
                    // Access is lost on a mode change and on the secure desktop.
                    // A new duplicator picks the screen up again.
                    lock(shared).error = Some(error.to_string());
                    current = None;
                    thread::sleep(Duration::from_millis(250));
                }
            }
        }

        if let Some(rest) = period.checked_sub(started.elapsed()) {
            thread::sleep(rest);
        }
    }
}

#[cfg(not(windows))]
fn run(_requests: &Requests, shared: &Shared) {
    lock(shared).error = Some("screen capture is implemented for Windows only".to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_takes_the_average_of_every_pixel_in_it() {
        let mut rgba = vec![0u8; 4 * 2 * 2];
        rgba[0..4].copy_from_slice(&[200, 100, 0, 255]);
        rgba[4..8].copy_from_slice(&[0, 0, 0, 255]);
        rgba[8..12].copy_from_slice(&[0, 100, 40, 255]);
        let frame = Frame {
            width: 2,
            height: 2,
            rgba,
            sequence: 1,
        };
        assert_eq!(frame.cells(1, 1), vec![[50, 50, 10]]);
        assert_eq!(frame.cells(2, 1), vec![[100, 100, 20], [0, 0, 0]]);
    }
}
