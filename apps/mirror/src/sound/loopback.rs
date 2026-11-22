//! The default output, captured in loopback through WASAPI. Every call is
//! COM, so this module holds `unsafe` code.

#![allow(unsafe_code)]

use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
};
use windows::core::GUID;

use super::spectrum::{Analyzer, SIZE};
use super::{Shared, lock};

/// How often the thread reads the capture buffer.
const TICK: Duration = Duration::from_millis(10);
/// How often the thread analyses the newest window.
const ANALYSIS: Duration = Duration::from_millis(20);
/// How often the thread checks that the default output did not change. The
/// headset switch changes it.
const CHECK: Duration = Duration::from_secs(1);
/// How long the thread waits after a failure, or while nobody wants it.
const IDLE: Duration = Duration::from_millis(250);
const RETRY: Duration = Duration::from_secs(2);
/// Loopback delivers no packet while nothing plays. After this long with no
/// packet, the window counts as silence.
const QUIET: Duration = Duration::from_millis(100);
/// The capture buffer, in 100 ns units.
const BUFFER: i64 = 1_000_000;

const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
/// `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT`.
const SUBTYPE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

struct Stream {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    id: String,
    rate: u32,
    channels: usize,
    float: bool,
    /// Bytes per sample of one channel.
    width: usize,
    heard: Instant,
}

pub fn run(shared: &Shared) {
    let enumerator: IMMDeviceEnumerator = match unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
    } {
        Ok(enumerator) => enumerator,
        Err(error) => {
            lock(shared).error = Some(error.to_string());
            return;
        }
    };
    let mut analyzer = Analyzer::new();
    let mut samples: Vec<f32> = vec![0.0; SIZE];
    let mut stream: Option<Stream> = None;
    let (mut analysed, mut checked) = (Instant::now(), Instant::now());
    loop {
        if !lock(shared).settings.enabled {
            if stream.take().is_some() {
                lock(shared).silence();
            }
            thread::sleep(IDLE);
            continue;
        }
        if stream.is_some() && checked.elapsed() >= CHECK {
            checked = Instant::now();
            let current = default_id(&enumerator).ok();
            if stream.as_ref().map(|s| &s.id) != current.as_ref() {
                stream = None;
            }
        }
        let Some(open) = stream.as_mut() else {
            match Stream::open(&enumerator) {
                Ok(opened) => stream = Some(opened),
                Err(error) => {
                    lock(shared).error = Some(error);
                    thread::sleep(RETRY);
                }
            }
            continue;
        };
        if let Err(error) = open.read(&mut samples) {
            stream = None;
            lock(shared).error = Some(error);
            thread::sleep(RETRY);
            continue;
        }
        if analysed.elapsed() >= ANALYSIS {
            analysed = Instant::now();
            let bands = analyzer.bands(&samples, open.rate);
            lock(shared).update(&bands);
        }
        thread::sleep(TICK);
    }
}

fn default_id(enumerator: &IMMDeviceEnumerator) -> Result<String, String> {
    unsafe {
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| e.to_string())?;
        let raw = device.GetId().map_err(|e| e.to_string())?;
        let id = raw.to_string().map_err(|e| e.to_string());
        CoTaskMemFree(Some(raw.0.cast_const().cast()));
        id
    }
}

impl Stream {
    fn open(enumerator: &IMMDeviceEnumerator) -> Result<Self, String> {
        let text = |e: windows::core::Error| e.to_string();
        unsafe {
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(text)?;
            let id = default_id(enumerator)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(text)?;
            let format = client.GetMixFormat().map_err(text)?;
            // The format is packed: read it by value.
            let head: WAVEFORMATEX = format.read_unaligned();
            let float = match head.wFormatTag {
                WAVE_FORMAT_IEEE_FLOAT => true,
                WAVE_FORMAT_EXTENSIBLE => {
                    let full = format.cast::<WAVEFORMATEXTENSIBLE>().read_unaligned();
                    let subformat = full.SubFormat;
                    subformat == SUBTYPE_FLOAT
                }
                _ => false,
            };
            let initialized = client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                BUFFER,
                0,
                format,
                None,
            );
            CoTaskMemFree(Some(format.cast_const().cast()));
            initialized.map_err(text)?;
            let (bits, channels) = (head.wBitsPerSample, head.nChannels);
            if !(float && bits == 32 || !float && bits == 16) || channels == 0 {
                return Err(format!(
                    "the output mixes {bits}-bit samples, which the analysis does not read"
                ));
            }
            let capture: IAudioCaptureClient = client.GetService().map_err(text)?;
            client.Start().map_err(text)?;
            Ok(Self {
                client,
                capture,
                id,
                rate: head.nSamplesPerSec,
                channels: usize::from(channels),
                float,
                width: usize::from(bits / 8),
                heard: Instant::now(),
            })
        }
    }

    /// Add every waiting packet to `samples` as mono, and keep the newest
    /// [`SIZE`].
    fn read(&mut self, samples: &mut Vec<f32>) -> Result<(), String> {
        let text = |e: windows::core::Error| e.to_string();
        loop {
            let waiting = unsafe { self.capture.GetNextPacketSize() }.map_err(text)?;
            if waiting == 0 {
                break;
            }
            let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
            unsafe {
                self.capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                    .map_err(text)?;
            }
            let count = frames as usize;
            #[allow(clippy::cast_sign_loss)]
            let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
            if silent || data.is_null() {
                samples.extend(std::iter::repeat_n(0.0, count));
            } else {
                let frame = self.width * self.channels;
                // SAFETY: WASAPI hands `frames` whole frames at `data`, valid
                // until the release below.
                let bytes = unsafe { std::slice::from_raw_parts(data, count * frame) };
                samples.extend(bytes.chunks_exact(frame).map(|frame| self.mono(frame)));
            }
            unsafe { self.capture.ReleaseBuffer(frames) }.map_err(text)?;
            self.heard = Instant::now();
        }
        if self.heard.elapsed() >= QUIET {
            samples.fill(0.0);
        }
        let extra = samples.len().saturating_sub(SIZE);
        samples.drain(..extra);
        Ok(())
    }

    /// The mean of the channels of one frame.
    fn mono(&self, frame: &[u8]) -> f32 {
        let sum: f32 = frame
            .chunks_exact(self.width)
            .map(|sample| match sample {
                [a, b, c, d] if self.float => f32::from_le_bytes([*a, *b, *c, *d]),
                [a, b] => f32::from(i16::from_le_bytes([*a, *b])) / 32_768.0,
                _ => 0.0,
            })
            .sum();
        #[allow(clippy::cast_precision_loss)]
        let mean = sum / self.channels as f32;
        mean
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = unsafe { self.client.Stop() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opens the default output of this machine: `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs a sound output"]
    fn the_default_output_opens_and_reads() {
        let enumerator: IMMDeviceEnumerator = unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
        }
        .unwrap_or_else(|e| panic!("{e}"));
        let mut stream = Stream::open(&enumerator).unwrap_or_else(|e| panic!("{e}"));
        let mut samples = vec![0.0; SIZE];
        for _ in 0..50 {
            stream.read(&mut samples).unwrap_or_else(|e| panic!("{e}"));
            thread::sleep(TICK);
        }
        let bands = Analyzer::new().bands(&samples, stream.rate);
        println!(
            "{} Hz, float {}, bands {bands:?}",
            stream.rate, stream.float
        );
        assert_eq!(samples.len(), SIZE);
    }
}
