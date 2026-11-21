//! What one frame of colors goes out through, per light.

use std::sync::{Arc, Mutex, PoisonError};

use govee_toolkit::{DeviceId, Govee, Mode, Paint, SegmentStream, StreamOptions};
use tokio::time::sleep;

use super::targets::Target;

/// What a writer reports once per second.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub sent: u64,
    pub error: Option<String>,
}

pub enum Writer {
    /// The segment channel: a write never blocks, and the stream sends the
    /// newest frame at the rate the device file measured.
    Stream(SegmentStream),
    /// One zone, through the `color` command. That command waits for the
    /// device, so one runs at a time and a newer color replaces a waiting one.
    Color(Arc<ColorQueue>),
}

#[derive(Default)]
pub struct ColorQueue {
    state: Mutex<ColorState>,
}

#[derive(Default)]
struct ColorState {
    pending: Option<[u8; 3]>,
    last: Option<[u8; 3]>,
    busy: bool,
    closed: bool,
    stats: Stats,
}

/// Power the light on, wait the command gap, and open its writer.
///
/// The light must be on first: an armed dark strip paints nothing.
pub async fn open(
    govee: &Govee,
    target: &Target,
    frame: govee_toolkit::Resolution,
    gradient: bool,
) -> govee_toolkit::Result<Writer> {
    let id = DeviceId::new(&target.id);
    let handle = govee.device(&id, Some(Mode::Lan))?;
    handle.power(true).await?;
    sleep(handle.command_gap()?).await;
    if !target.streams() {
        return Ok(Writer::Color(Arc::default()));
    }
    let stream = handle
        .open_stream(StreamOptions {
            resolution: frame,
            gradient,
            ..StreamOptions::default()
        })
        .await?;
    Ok(Writer::Stream(stream))
}

/// Light one segment white and the others black, with no writer open.
pub async fn highlight(
    govee: &Govee,
    target: &Target,
    frame: govee_toolkit::Resolution,
    zones: usize,
    segment: usize,
) -> govee_toolkit::Result<()> {
    let id = DeviceId::new(&target.id);
    let handle = govee.device(&id, Some(Mode::Lan))?;
    handle.power(true).await?;
    sleep(handle.command_gap()?).await;
    if !target.streams() {
        handle.color([255, 255, 255]).await?;
        return Ok(());
    }
    let mut colors = vec![[0u8; 3]; zones];
    if let Some(color) = colors.get_mut(segment) {
        *color = [255, 255, 255];
    }
    handle
        .segment(&Paint {
            zones: None,
            colors: &colors,
            resolution: frame,
            gradient: false,
        })
        .await?;
    Ok(())
}

impl Writer {
    pub fn zones(&self) -> usize {
        match self {
            Self::Stream(stream) => stream.zones(),
            Self::Color(_) => 1,
        }
    }

    pub fn write(&self, govee: &Govee, id: &str, colors: &[[u8; 3]]) {
        match self {
            // The count is the stream's own, so the only refusal left is a
            // closed stream, which `stats` reports.
            Self::Stream(stream) => drop(stream.set_all(colors)),
            Self::Color(queue) => {
                if let Some(&color) = colors.first() {
                    ColorQueue::push(queue, govee, id, color);
                }
            }
        }
    }

    pub fn stats(&self) -> Stats {
        match self {
            Self::Stream(stream) => Stats {
                sent: stream.frames_sent(),
                error: stream.error().map(|e| e.to_string()),
            },
            Self::Color(queue) => queue.lock().stats.clone(),
        }
    }

    pub async fn close(self) {
        match self {
            Self::Stream(stream) => drop(stream.close().await),
            Self::Color(queue) => queue.lock().closed = true,
        }
    }
}

impl ColorQueue {
    fn lock(&self) -> std::sync::MutexGuard<'_, ColorState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(queue: &Arc<Self>, govee: &Govee, id: &str, color: [u8; 3]) {
        {
            let mut state = queue.lock();
            if state.closed || state.last == Some(color) {
                return;
            }
            state.last = Some(color);
            state.pending = Some(color);
            if state.busy {
                return;
            }
            state.busy = true;
        }
        let queue = Arc::clone(queue);
        let govee = govee.clone();
        let id = DeviceId::new(id);
        tokio::spawn(async move {
            loop {
                let next = {
                    let mut state = queue.lock();
                    match state.pending.take() {
                        Some(color) if !state.closed => color,
                        _ => {
                            state.busy = false;
                            return;
                        }
                    }
                };
                let result = match govee.device(&id, Some(Mode::Lan)) {
                    Ok(handle) => handle.color(next).await.map(drop),
                    Err(error) => Err(error),
                };
                let mut state = queue.lock();
                match result {
                    Ok(()) => {
                        state.stats.sent += 1;
                        state.stats.error = None;
                    }
                    Err(error) => state.stats.error = Some(error.to_string()),
                }
            }
        });
    }
}
