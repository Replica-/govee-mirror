//! What one frame of colors goes out through, per light.

use std::sync::{Arc, Mutex, PoisonError};

use std::time::{Duration, Instant};

use govee_toolkit::{DeviceId, Govee, Mode, SegmentStream, StreamOptions};
use tokio::time::sleep;

use super::targets::Target;
use crate::razer::{self, Controller};

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
    /// One port of the Razer ARGB controller. A write never blocks: the
    /// controller's thread sends the newest frame.
    Razer {
        controller: Arc<Controller>,
        port: usize,
        zones: usize,
        /// LEDs on the port. Each segment covers an even run of them.
        leds: usize,
        blend: bool,
    },
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

/// Open one port of the Razer ARGB controller: `zones` segments over `leds`
/// LEDs, faded into each other with `blend`.
///
/// # Errors
///
/// What the USB stack answered, where no controller is connected or
/// another program holds it.
pub fn open_razer(port: usize, zones: usize, leds: usize, blend: bool) -> Result<Writer, String> {
    Ok(Writer::Razer {
        controller: razer::acquire()?,
        port,
        zones,
        leds,
        blend,
    })
}

/// Blink one LED of a Razer port white, the others dark. The LEDs go back to
/// the controller's own effect after it, unless the mirror drives the port.
///
/// # Errors
///
/// As for [`open_razer`].
pub async fn highlight_razer(
    port: usize,
    zones: usize,
    leds: usize,
    segment: usize,
) -> Result<(), String> {
    let controller = razer::acquire()?;
    let mut lit = vec![[0u8; 3]; zones];
    if let Some(color) = lit.get_mut(segment) {
        *color = [255, 255, 255];
    }
    let lit = razer::spread(&lit, leds, false);
    let dark = vec![[0u8; 3]; leds];
    let started = Instant::now();
    while let Some(on) = blink(started.elapsed()) {
        controller.set(port, if on { &lit } else { &dark });
        sleep(Duration::from_millis(20)).await;
    }
    // The last frame must leave before the hand-back.
    sleep(Duration::from_millis(50)).await;
    Ok(())
}

/// How many times a highlight lights its segment.
const BLINKS: u32 = 4;
/// How long each half of one blink lasts.
const BLINK: Duration = Duration::from_millis(250);

/// Whether the segment is lit `elapsed` into a highlight, and `None` once the
/// highlight has ended. The segment is lit in the first half of each blink.
pub fn blink(elapsed: Duration) -> Option<bool> {
    let half = elapsed.as_millis() / BLINK.as_millis();
    (half < u128::from(BLINKS) * 2).then_some(half.is_multiple_of(2))
}

/// Blink one segment white with the others black, with no writer open.
///
/// The blink goes out over a stream of its own. Its close disarms the
/// channel, so the light goes back to the color it showed before.
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
        // One zone through the color command, which waits for the light: the
        // blink is as fast as the light answers, and it ends white.
        for _ in 0..BLINKS {
            handle.color([255, 255, 255]).await?;
            sleep(BLINK).await;
            handle.color([0, 0, 0]).await?;
            sleep(BLINK).await;
        }
        handle.color([255, 255, 255]).await?;
        return Ok(());
    }
    let mut lit = vec![[0u8; 3]; zones];
    if let Some(color) = lit.get_mut(segment) {
        *color = [255, 255, 255];
    }
    let dark = vec![[0u8; 3]; zones];
    let stream = handle
        .open_stream(StreamOptions {
            resolution: frame,
            ..StreamOptions::default()
        })
        .await?;
    let started = Instant::now();
    while let Some(on) = blink(started.elapsed()) {
        stream.set_all(if on { &lit } else { &dark })?;
        sleep(Duration::from_millis(20)).await;
    }
    stream.close().await
}

impl Writer {
    pub fn zones(&self) -> usize {
        match self {
            Self::Stream(stream) => stream.zones(),
            Self::Color(_) => 1,
            Self::Razer { zones, .. } => *zones,
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
            Self::Razer {
                controller,
                port,
                leds,
                blend,
                ..
            } => controller.set(*port, &razer::spread(colors, *leds, *blend)),
        }
    }

    pub fn stats(&self) -> Stats {
        match self {
            Self::Stream(stream) => Stats {
                sent: stream.frames_sent(),
                error: stream.error().map(|e| e.to_string()),
            },
            Self::Color(queue) => queue.lock().stats.clone(),
            Self::Razer { controller, .. } => {
                let (sent, error) = controller.stats();
                Stats { sent, error }
            }
        }
    }

    pub async fn close(self) {
        match self {
            Self::Stream(stream) => drop(stream.close().await),
            Self::Color(queue) => queue.lock().closed = true,
            // The port goes dark; the controller hands the LEDs back once the
            // last port lets go of it.
            Self::Razer {
                controller, port, ..
            } => controller.set(port, &[]),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blink_lights_the_first_half_of_each_period_and_then_ends() {
        let at = |ms| blink(Duration::from_millis(ms));
        assert_eq!(at(0), Some(true));
        assert_eq!(at(260), Some(false));
        assert_eq!(at(510), Some(true));
        assert_eq!(at(1990), Some(false));
        assert_eq!(at(2000), None);
    }
}
