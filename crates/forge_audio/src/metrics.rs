//! Measurements that turn "sounds wrong" into something specific.

use crate::decode::Audio;

/// Samples at or beyond this magnitude are sitting at full scale.
///
/// Not exactly 1.0: a lossy codec routinely overshoots full scale by a hair on
/// reconstruction, and counting those would flag every ogg ever encoded.
const CLIP_THRESHOLD: f32 = 0.999;
/// Consecutive full-scale samples before it counts as real clipping.
///
/// Isolated samples at full scale are what *normalisation* looks like — peak
/// normalising to 0 dBFS puts one sample exactly there by construction, and
/// flagging that would mark most of a normalised SFX library as broken. Audible
/// distortion is flat-topping: a run of samples all pinned at the rail. Three
/// is the usual threshold, and it is measured per channel.
const CLIP_RUN: usize = 3;
/// Below this magnitude a sample counts as silence when trimming head/tail.
///
/// About -60 dBFS — quiet enough to be inaudible in context, loud enough that
/// dither and codec noise do not read as signal.
const SILENCE_FLOOR: f32 = 0.001;

/// What a decoded file measures.
#[derive(Debug, Clone)]
pub struct Metrics {
    /// Length in seconds.
    pub duration: f32,
    /// Samples per second.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: usize,
    /// Loudest absolute sample, linear.
    pub peak: f32,
    /// Loudest absolute sample in dBFS.
    pub peak_db: f32,
    /// Root-mean-square level over the whole file, dBFS.
    pub rms_db: f32,
    /// Rough integrated loudness in LUFS.
    ///
    /// K-weighting is approximated by a high-pass, so treat this as indicative
    /// rather than a broadcast-compliant measurement — it is for comparing
    /// assets in one library, not for certifying a master.
    pub loudness_lufs: f32,
    /// Peak-to-RMS ratio in dB. Small values mean heavy compression.
    pub crest_db: f32,
    /// Samples sitting at full scale. Informational: on its own this is
    /// normalisation, not a fault.
    pub full_scale_samples: usize,
    /// Longest run of consecutive full-scale samples in any one channel.
    /// This is the one that means distortion.
    pub longest_clip_run: usize,
    /// Silence before the first audible sample, seconds.
    pub lead_silence: f32,
    /// Silence after the last audible sample, seconds.
    pub tail_silence: f32,
    /// Mean sample value. A non-zero offset wastes headroom and can thump.
    pub dc_offset: f32,
    /// True when nothing in the file exceeds the silence floor.
    pub silent: bool,
    /// Level difference between the first and last half-second, dB.
    ///
    /// The audio analogue of an animation's loop seam: a track that ends far
    /// quieter than it starts jumps audibly at the loop point. Positive means
    /// the tail is quieter than the head.
    pub loop_seam_db: f32,
}

impl Metrics {
    /// Problems worth a human's attention, most serious first.
    ///
    /// Deliberately conservative: a warning nobody trusts is worse than none.
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.silent {
            out.push("file is silent".to_owned());
            return out;
        }
        if self.longest_clip_run >= CLIP_RUN {
            out.push(format!(
                "clipped: {} consecutive samples pinned at full scale ({} total) - audible distortion",
                self.longest_clip_run, self.full_scale_samples
            ));
        }
        if self.dc_offset.abs() > 0.01 {
            out.push(format!(
                "DC offset {:+.3} - wastes headroom and can thump on playback",
                self.dc_offset
            ));
        }
        // A one-shot with a long lead-in fires late in game, which reads as
        // unresponsive rather than as an audio bug.
        if self.lead_silence > 0.05 {
            out.push(format!(
                "{:.0} ms of silence before the first sound - a one-shot will feel late",
                self.lead_silence * 1000.0
            ));
        }
        // Only meaningful for something long enough to loop; a one-shot is
        // supposed to decay to nothing.
        if self.duration > 10.0 && self.loop_seam_db > 9.0 {
            out.push(format!(
                "ends {:.1} dB quieter than it starts - will jump at the loop point",
                self.loop_seam_db
            ));
        }
        if self.crest_db < 6.0 {
            out.push(format!(
                "crest factor {:.1} dB - very compressed, little dynamic range left",
                self.crest_db
            ));
        }
        if self.peak_db < -18.0 {
            out.push(format!(
                "peaks at only {:.1} dBFS - very quiet next to a normalised library",
                self.peak_db
            ));
        }
        out
    }
}

/// Measure decoded audio.
#[must_use]
pub fn measure(audio: &Audio) -> Metrics {
    let mono = audio.mono();
    let n = mono.len();

    let mut peak = 0.0f32;
    let mut sum_squares = 0.0f64;
    let mut sum = 0.0f64;
    let mut clipped = 0usize;
    for &s in &mono {
        let a = s.abs();
        if a > peak {
            peak = a;
        }
        sum_squares += f64::from(s) * f64::from(s);
        sum += f64::from(s);
    }
    // Measured per channel on the raw samples, not the mono mix: averaging
    // channels hides a clip that happens in only one of them, and interleaved
    // data makes "consecutive" meaningless unless you stride by channel count.
    let mut longest_run = 0usize;
    for channel in 0..audio.channels.max(1) {
        let mut run = 0usize;
        for frame in 0..audio.frames() {
            let s = audio.samples[frame * audio.channels + channel];
            if s.abs() >= CLIP_THRESHOLD {
                clipped += 1;
                run += 1;
                longest_run = longest_run.max(run);
            } else {
                run = 0;
            }
        }
    }

    let rms = if n == 0 {
        0.0
    } else {
        (sum_squares / n as f64).sqrt() as f32
    };
    let dc_offset = if n == 0 { 0.0 } else { (sum / n as f64) as f32 };

    let first = mono.iter().position(|s| s.abs() > SILENCE_FLOOR);
    let last = mono.iter().rposition(|s| s.abs() > SILENCE_FLOOR);
    let rate = audio.sample_rate.max(1) as f32;
    let (lead, tail) = match (first, last) {
        (Some(f), Some(l)) => (f as f32 / rate, (n.saturating_sub(l + 1)) as f32 / rate),
        _ => (audio.duration(), 0.0),
    };

    let peak_db = to_db(peak);
    let rms_db = to_db(rms);

    let window = (audio.sample_rate as usize / 2).min(n / 2).max(1);
    let head_rms = rms_of(&mono[..window]);
    let tail_rms = rms_of(&mono[n.saturating_sub(window)..]);

    Metrics {
        duration: audio.duration(),
        sample_rate: audio.sample_rate,
        channels: audio.channels,
        peak,
        peak_db,
        rms_db,
        loudness_lufs: loudness(&mono, audio.sample_rate),
        crest_db: peak_db - rms_db,
        full_scale_samples: clipped,
        longest_clip_run: longest_run,
        lead_silence: lead,
        tail_silence: tail,
        dc_offset,
        silent: first.is_none(),
        loop_seam_db: to_db(head_rms) - to_db(tail_rms),
    }
}

/// RMS of a slice, linear.
fn rms_of(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// Linear amplitude to dBFS, floored so silence does not print as -inf.
#[must_use]
pub fn to_db(linear: f32) -> f32 {
    if linear <= 1e-9 {
        -120.0
    } else {
        20.0 * linear.log10()
    }
}

/// Approximate integrated loudness, LUFS.
///
/// Real ITU-R BS.1770 applies a K-weighting filter, gates quiet blocks, and
/// weights channels. This does the two parts that matter for comparing assets
/// in one library — a high-pass standing in for K-weighting, and gating out
/// near-silent blocks so a long tail does not drag the number down — and skips
/// the rest. Good enough to say "this bark is 9 LU quieter than the others",
/// not good enough to certify a broadcast master.
fn loudness(mono: &[f32], sample_rate: u32) -> f32 {
    if mono.is_empty() || sample_rate == 0 {
        return -120.0;
    }
    // One-pole high-pass at ~60 Hz, standing in for the K-weighting shelf.
    let dt = 1.0 / sample_rate as f32;
    let rc = 1.0 / (2.0 * std::f32::consts::PI * 60.0);
    let alpha = rc / (rc + dt);
    let mut filtered = Vec::with_capacity(mono.len());
    let mut prev_in = 0.0;
    let mut prev_out = 0.0;
    for &s in mono {
        let out = alpha * (prev_out + s - prev_in);
        filtered.push(out);
        prev_in = s;
        prev_out = out;
    }

    // 400 ms blocks, as BS.1770 uses.
    let block = ((sample_rate as f32) * 0.4) as usize;
    if block == 0 {
        return -120.0;
    }
    let mut powers: Vec<f64> = filtered
        .chunks(block)
        .map(|c| c.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / c.len() as f64)
        .filter(|p| *p > 0.0)
        .collect();
    if powers.is_empty() {
        return -120.0;
    }
    // Absolute gate at -70 LUFS, so trailing silence does not pull the mean.
    let gate = 10f64.powf((-70.0 - 0.691) / 10.0);
    powers.retain(|p| *p > gate);
    if powers.is_empty() {
        return -120.0;
    }
    let mean = powers.iter().sum::<f64>() / powers.len() as f64;
    (-0.691 + 10.0 * mean.log10()) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, secs: f32, amp: f32, rate: u32) -> Audio {
        let n = (secs * rate as f32) as usize;
        let samples = (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin())
            .collect();
        Audio {
            samples,
            sample_rate: rate,
            channels: 1,
        }
    }

    #[test]
    fn sine_peak_and_crest_are_what_theory_says() {
        let m = measure(&tone(1000.0, 1.0, 0.5, 48_000));
        assert!((m.peak - 0.5).abs() < 0.01, "peak {}", m.peak);
        // A sine's RMS is peak/sqrt(2), so crest is ~3.01 dB.
        assert!((m.crest_db - 3.01).abs() < 0.2, "crest {}", m.crest_db);
        assert_eq!(m.longest_clip_run, 0);
        assert!(!m.silent);
    }

    #[test]
    fn sustained_flat_topping_is_reported_as_clipping() {
        let m = measure(&tone(1000.0, 0.5, 1.5, 48_000));
        assert!(m.longest_clip_run >= CLIP_RUN, "run {}", m.longest_clip_run);
        assert!(m.warnings().iter().any(|w| w.contains("clipped")));
    }

    #[test]
    fn peak_normalisation_is_not_reported_as_clipping() {
        // Exactly what a normalised one-shot looks like: a couple of isolated
        // samples touching the rail and nothing sustained. Flagging this would
        // mark most of a normalised SFX library as broken.
        let mut audio = tone(1000.0, 0.5, 0.8, 48_000);
        audio.samples[1000] = 1.0;
        audio.samples[9000] = -1.0;
        let m = measure(&audio);
        assert_eq!(m.full_scale_samples, 2);
        assert!(m.longest_clip_run < CLIP_RUN);
        assert!(
            !m.warnings().iter().any(|w| w.contains("clipped")),
            "normalisation must not warn: {:?}",
            m.warnings()
        );
    }

    #[test]
    fn silence_is_reported_rather_than_measured_as_quiet() {
        let audio = Audio {
            samples: vec![0.0; 48_000],
            sample_rate: 48_000,
            channels: 1,
        };
        let m = measure(&audio);
        assert!(m.silent);
        assert_eq!(m.warnings(), vec!["file is silent".to_owned()]);
    }

    #[test]
    fn lead_silence_is_found() {
        let mut audio = tone(1000.0, 1.0, 0.5, 48_000);
        let mut samples = vec![0.0; 24_000];
        samples.append(&mut audio.samples);
        audio.samples = samples;
        let m = measure(&audio);
        assert!(
            (m.lead_silence - 0.5).abs() < 0.01,
            "lead {}",
            m.lead_silence
        );
    }

    #[test]
    fn a_track_that_fades_out_is_flagged_as_a_loop_seam() {
        // 12 s of tone whose last third is 20 dB down: fine as a one-shot,
        // wrong for something meant to loop.
        let mut audio = tone(440.0, 12.0, 0.5, 48_000);
        let n = audio.samples.len();
        for s in &mut audio.samples[n * 2 / 3..] {
            *s *= 0.1;
        }
        let m = measure(&audio);
        assert!(m.loop_seam_db > 9.0, "seam {}", m.loop_seam_db);
        assert!(m.warnings().iter().any(|w| w.contains("loop point")));
    }

    #[test]
    fn a_steady_track_has_no_loop_seam() {
        let m = measure(&tone(440.0, 12.0, 0.5, 48_000));
        assert!(m.loop_seam_db.abs() < 1.0, "seam {}", m.loop_seam_db);
    }

    #[test]
    fn dc_offset_is_detected() {
        let mut audio = tone(1000.0, 1.0, 0.3, 48_000);
        for s in &mut audio.samples {
            *s += 0.2;
        }
        let m = measure(&audio);
        assert!((m.dc_offset - 0.2).abs() < 0.01, "dc {}", m.dc_offset);
        assert!(m.warnings().iter().any(|w| w.contains("DC offset")));
    }
}
