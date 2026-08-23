//! Render audio as a picture: waveform over spectrogram, with the numbers on it.
//!
//! An agent cannot listen, and a human scrubbing a 30-second track to find one
//! click is slow. Both problems have the same answer — draw it. Clipping shows
//! as a flat-topped waveform, dead air as a gap, a truncated tail as a cliff,
//! and over-compression as a solid block with no dynamics.

use forge_raster::Canvas;
use rustfft::{FftPlanner, num_complex::Complex};

use crate::decode::Audio;
use crate::metrics::{Metrics, to_db};

/// Long-edge budget before a vision model resamples the image.
pub const VISION_LONG_EDGE: u32 = 1568;

const BG: [u8; 4] = [18, 18, 22, 255];
const FG: [u8; 4] = [235, 235, 240, 255];
const DIM: [u8; 4] = [110, 114, 124, 255];
const WAVE: [u8; 4] = [92, 174, 242, 255];
const WAVE_HOT: [u8; 4] = [242, 92, 92, 255];
const GRID: [u8; 4] = [44, 46, 54, 255];

/// Plot size and layout.
#[derive(Debug, Clone)]
pub struct PlotLayout {
    /// Overall width in pixels.
    pub width: u32,
    /// Height of the waveform panel.
    pub wave_height: u32,
    /// Height of the spectrogram panel.
    pub spec_height: u32,
    /// Height reserved for the header text.
    pub header_height: u32,
    /// Integer text scale.
    pub text_scale: u32,
    /// FFT size for the spectrogram. Powers of two only.
    pub fft_size: usize,
    /// Height reserved below the spectrogram for the time axis.
    pub axis_height: u32,
}

impl Default for PlotLayout {
    fn default() -> Self {
        // 1400 wide keeps the whole plot under the downscale threshold while
        // leaving roughly a pixel per 20 ms on a 30-second track.
        Self {
            width: 1400,
            wave_height: 260,
            spec_height: 300,
            header_height: 96,
            text_scale: 2,
            fft_size: 1024,
            axis_height: 22,
        }
    }
}

impl PlotLayout {
    /// Total image height for this layout.
    #[must_use]
    pub fn total_height(&self) -> u32 {
        self.header_height + self.wave_height + self.spec_height + 12 + self.axis_height
    }
}

/// Draw `audio` and its `metrics` as a labelled plot.
#[must_use]
pub fn render(audio: &Audio, metrics: &Metrics, title: &str, layout: &PlotLayout) -> Canvas {
    let w = layout.width.max(64);
    let mut canvas = Canvas::new(w, layout.total_height(), BG);
    let line = Canvas::line_height(layout.text_scale);

    // Header: the numbers, so the picture and the measurement travel together.
    canvas.text(8, 6, title, FG, layout.text_scale);
    let facts = format!(
        "{:.2}S  {}HZ  {}CH  PEAK {:.1}DB  RMS {:.1}DB  LUFS {:.1}  CREST {:.1}DB",
        metrics.duration,
        metrics.sample_rate,
        metrics.channels,
        metrics.peak_db,
        metrics.rms_db,
        metrics.loudness_lufs,
        metrics.crest_db,
    );
    canvas.text(8, 6 + line + 4, &facts, DIM, layout.text_scale);

    let warnings = metrics.warnings();
    if warnings.is_empty() {
        canvas.text(8, 6 + (line + 4) * 2, "CLEAN", DIM, layout.text_scale);
    } else {
        // Only the first two fit; the report carries the rest.
        for (i, warning) in warnings.iter().take(2).enumerate() {
            canvas.text(
                8,
                6 + (line + 4) * (2 + i as u32),
                &warning.to_ascii_uppercase(),
                WAVE_HOT,
                layout.text_scale,
            );
        }
    }

    let wave_top = layout.header_height;
    draw_waveform(&mut canvas, audio, wave_top, layout.wave_height);
    let spec_top = wave_top + layout.wave_height + 12;
    draw_spectrogram(
        &mut canvas,
        audio,
        spec_top,
        layout.spec_height,
        layout.fft_size,
    );
    draw_time_axis(
        &mut canvas,
        metrics.duration,
        spec_top + layout.spec_height + 2,
        layout.text_scale,
    );

    canvas
}

/// Min/max envelope per pixel column, which is what makes clipping visible:
/// a clipped region draws as a flat top rather than a rounded one.
fn draw_waveform(canvas: &mut Canvas, audio: &Audio, top: u32, height: u32) {
    let mono = audio.mono();
    let w = canvas.width();
    let mid = top + height / 2;
    let half = f64::from(height) / 2.0;

    canvas.fill_rect(0, top, w, height, [24, 24, 30, 255]);
    // Reference lines at 0 and +/- full scale.
    canvas.h_line(0, w - 1, mid, GRID);
    canvas.h_line(0, w - 1, top, GRID);
    canvas.h_line(0, w - 1, top + height - 1, GRID);

    if mono.is_empty() {
        return;
    }
    let per_px = (mono.len() as f64 / f64::from(w)).max(1.0);
    for x in 0..w {
        let start = (f64::from(x) * per_px) as usize;
        let end = (((f64::from(x) + 1.0) * per_px) as usize).min(mono.len());
        if start >= end {
            continue;
        }
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for &s in &mono[start..end] {
            lo = lo.min(s);
            hi = hi.max(s);
        }
        // Colour the column if anything in it reached full scale, so a single
        // clipped sample in a long file is still findable by eye.
        let colour = if lo <= -0.999 || hi >= 0.999 {
            WAVE_HOT
        } else {
            WAVE
        };
        let y_hi = (f64::from(mid) - f64::from(hi.clamp(-1.0, 1.0)) * half) as u32;
        let y_lo = (f64::from(mid) - f64::from(lo.clamp(-1.0, 1.0)) * half) as u32;
        canvas.v_line(x, y_hi.min(y_lo), y_hi.max(y_lo), colour);
    }
}

/// Log-frequency spectrogram. Log because pitch is logarithmic — a linear axis
/// squashes everything musical into the bottom eighth of the panel.
fn draw_spectrogram(canvas: &mut Canvas, audio: &Audio, top: u32, height: u32, fft_size: usize) {
    let mono = audio.mono();
    let w = canvas.width();
    canvas.fill_rect(0, top, w, height, [12, 12, 16, 255]);
    if mono.len() < fft_size || audio.sample_rate == 0 {
        canvas.text(8, top + 8, "TOO SHORT FOR A SPECTROGRAM", DIM, 2);
        return;
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(fft_size);
    // Hann window, so a tone shows as one bright line rather than smearing.
    let window: Vec<f32> = (0..fft_size)
        .map(|i| {
            0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos())
        })
        .collect();

    let hop = ((mono.len().saturating_sub(fft_size)) / w.max(1) as usize).max(1);
    let nyquist = audio.sample_rate as f32 / 2.0;
    let min_hz = 30.0f32;
    let mut buffer = vec![Complex::new(0.0f32, 0.0); fft_size];

    for x in 0..w {
        let start = x as usize * hop;
        if start + fft_size > mono.len() {
            break;
        }
        for (i, slot) in buffer.iter_mut().enumerate() {
            *slot = Complex::new(mono[start + i] * window[i], 0.0);
        }
        fft.process(&mut buffer);

        for y in 0..height {
            // Row 0 is the top of the panel and the highest frequency.
            let t =
                f32::from(u16::try_from(height - 1 - y).unwrap_or(0)) / (height - 1).max(1) as f32;
            let hz = min_hz * (nyquist / min_hz).powf(t);
            let bin = (hz / nyquist * (fft_size as f32 / 2.0)) as usize;
            if bin >= fft_size / 2 {
                continue;
            }
            let mag = buffer[bin].norm() / (fft_size as f32 / 4.0);
            canvas.put(x, top + y, heat(to_db(mag)));
        }
    }
}

/// Map dBFS to a dark-to-hot ramp over a 72 dB range.
fn heat(db: f32) -> [u8; 4] {
    // 90 dB of range, with the top ~20 dB reserved for the bright end. A
    // narrower range saturates on ordinary content and every file looks
    // uniformly hot, which hides exactly the structure worth seeing.
    let t = ((db + 90.0) / 90.0).clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.55 {
        let k = t / 0.55;
        (0.02 * k, 0.06 * k, 0.10 + 0.45 * k)
    } else if t < 0.78 {
        let k = (t - 0.55) / 0.23;
        (0.10 * k, 0.06 + 0.62 * k, 0.55 + 0.25 * k)
    } else {
        let k = (t - 0.78) / 0.22;
        (0.10 + 0.90 * k, 0.68 + 0.30 * k, 0.80 - 0.55 * k)
    };
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255]
}

fn draw_time_axis(canvas: &mut Canvas, duration: f32, y: u32, scale: u32) {
    let w = canvas.width();
    canvas.h_line(0, w - 1, y, GRID);
    if duration <= 0.0 {
        return;
    }
    // Aim for roughly one tick every 220 px, snapped to a readable interval.
    // Denser than that and the labels collide at this font size.
    let target = f64::from(duration) / (f64::from(w) / 220.0);
    let step = [0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0]
        .into_iter()
        .find(|s| *s >= target)
        .unwrap_or(60.0);
    let mut t = 0.0;
    while t <= f64::from(duration) {
        let x = ((t / f64::from(duration)) * f64::from(w - 1)) as u32;
        canvas.v_line(x, y, y + 4, DIM);
        let label = if step >= 1.0 {
            format!("{t:.0}S")
        } else {
            format!("{t:.2}S")
        };
        canvas.text(
            x.min(w.saturating_sub(48)) + 3,
            y + 6,
            &label,
            DIM,
            scale.max(2) / 2,
        );
        t += step;
    }
}
