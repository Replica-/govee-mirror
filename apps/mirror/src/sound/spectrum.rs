//! From samples to the level of each band, on a log frequency scale.

use std::f32::consts::PI;

/// Samples in one analysis window: about 43 ms at 48 kHz.
pub const SIZE: usize = 2048;
/// Bands from [`LOW_HZ`] to [`HIGH_HZ`].
pub const BANDS: usize = 48;
const LOW_HZ: f32 = 30.0;
const HIGH_HZ: f32 = 16_000.0;
/// The level that shows as 0. Full scale shows as 1.
const FLOOR_DB: f32 = -70.0;

pub struct Analyzer {
    window: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
}

impl Analyzer {
    pub fn new() -> Self {
        #[allow(clippy::cast_precision_loss)]
        let window = (0..SIZE)
            .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / SIZE as f32).cos())
            .collect();
        Self {
            window,
            re: vec![0.0; SIZE],
            im: vec![0.0; SIZE],
        }
    }

    /// The level of each band from 0 to 1, lowest band first. `samples`
    /// holds [`SIZE`] mono samples at `rate` Hz, from -1 to 1.
    pub fn bands(&mut self, samples: &[f32], rate: u32) -> [f32; BANDS] {
        for (i, (re, im)) in self.re.iter_mut().zip(&mut self.im).enumerate() {
            *re = samples.get(i).copied().unwrap_or(0.0) * self.window[i];
            *im = 0.0;
        }
        fft(&mut self.re, &mut self.im);
        #[allow(clippy::cast_precision_loss)]
        let bin_hz = rate as f32 / SIZE as f32;
        // A Hann window halves a sine's peak: 4 / SIZE gives its amplitude.
        #[allow(clippy::cast_precision_loss)]
        let amplitude = |bin: usize| {
            let bin = bin.clamp(1, SIZE / 2 - 1);
            self.re[bin].hypot(self.im[bin]) * 4.0 / SIZE as f32
        };
        std::array::from_fn(|band| {
            let (low, high) = (edge(band), edge(band + 1));
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let (first, last) = ((low / bin_hz).ceil() as usize, (high / bin_hz) as usize);
            // A low band narrower than one bin takes the bin at its center.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let peak = if first > last {
                amplitude(((low * high).sqrt() / bin_hz).round() as usize)
            } else {
                (first..=last).map(amplitude).fold(0.0, f32::max)
            };
            let db = 20.0 * peak.max(1e-9).log10();
            (1.0 - db / FLOOR_DB).clamp(0.0, 1.0)
        })
    }
}

/// The lower frequency of `band`, in Hz.
#[allow(clippy::cast_precision_loss)]
fn edge(band: usize) -> f32 {
    LOW_HZ * (HIGH_HZ / LOW_HZ).powf(band as f32 / BANDS as f32)
}

/// The bands with their center from `low` up to `high`, in Hz.
pub fn within(low: f32, high: f32) -> std::ops::Range<usize> {
    let center = |band: usize| (edge(band) * edge(band + 1)).sqrt();
    let first = (0..BANDS).take_while(|&b| center(b) < low).count();
    let end = (0..BANDS).take_while(|&b| center(b) < high).count();
    first..end.max(first)
}

/// An in-place radix-2 FFT. The length must be a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        #[allow(clippy::cast_precision_loss)]
        let angle = -2.0 * PI / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                #[allow(clippy::cast_precision_loss)]
                let (sin, cos) = (angle * k as f32).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cos - im[b] * sin;
                let ti = re[b] * sin + im[b] * cos;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, amplitude: f32) -> Vec<f32> {
        #[allow(clippy::cast_precision_loss)]
        (0..SIZE)
            .map(|i| amplitude * (2.0 * PI * hz * i as f32 / 48_000.0).sin())
            .collect()
    }

    #[test]
    fn a_bass_tone_lights_a_bass_band_and_leaves_the_treble_dark() {
        let bands = Analyzer::new().bands(&tone(60.0, 0.5), 48_000);
        let bass = bands[within(40.0, 100.0)]
            .iter()
            .copied()
            .fold(0.0, f32::max);
        // 0.5 is -6 dB: 1 - 6 / 70.
        assert!((bass - 0.914).abs() < 0.02, "{bass}");
        assert!(bands[BANDS - 1] < 0.1);
    }

    #[test]
    fn silence_shows_as_zero() {
        assert_eq!(Analyzer::new().bands(&[0.0; SIZE], 48_000), [0.0; BANDS]);
    }

    #[test]
    fn a_frequency_range_takes_the_bands_centered_in_it() {
        let kick = within(40.0, 100.0);
        assert!(kick.start > 0 && kick.end > kick.start);
        assert!(edge(kick.start + 1) > 40.0 && edge(kick.end - 1) < 100.0);
        assert_eq!(within(0.0, 150.0).start, 0);
    }
}
