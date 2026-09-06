//! Streaming consumer processing: original recordings remain untouched on disk.
use bevy::{
    audio::{ChannelCount, Decodable, SampleRate, Source},
    prelude::*,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Resource, Clone)]
pub struct VoiceProcessing(pub Arc<AtomicBool>);
impl Default for VoiceProcessing {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
}
#[derive(Asset, TypePath, Clone)]
pub struct ProcessedVoice {
    pub source: AudioSource,
    pub enabled: Arc<AtomicBool>,
}
impl Decodable for ProcessedVoice {
    type Decoder = VoiceStream;
    fn decoder(&self) -> VoiceStream {
        let dry = self.source.decoder();
        let rate = dry.sample_rate();
        let channels = dry.channels().get() as usize;
        let duration = dry
            .total_duration()
            .map(|d| d + Duration::from_secs_f32(1.2));
        VoiceStream {
            source: Box::new(dry),
            rate,
            channels,
            duration,
            processor: Processor::new(rate.get() as f32),
            enabled: self.enabled.clone(),
            tail: (rate.get() as f32 * 1.2) as usize,
            ended: false,
            right: None,
        }
    }
}
pub struct VoiceStream {
    source: Box<dyn Iterator<Item = f32> + Send>,
    rate: SampleRate,
    channels: usize,
    duration: Option<Duration>,
    processor: Processor,
    enabled: Arc<AtomicBool>,
    tail: usize,
    ended: bool,
    right: Option<f32>,
}
impl Iterator for VoiceStream {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        let mut mono = 0.;
        if !self.ended {
            if let Some(first) = self.source.next() {
                mono = first;
                for _ in 1..self.channels {
                    mono += self.source.next().unwrap_or(0.);
                }
                mono /= self.channels as f32;
            } else {
                self.ended = true;
            }
        }
        if self.ended {
            if self.tail == 0 {
                return None;
            }
            self.tail -= 1;
        }
        let pair = self
            .processor
            .frame(mono, self.enabled.load(Ordering::Relaxed));
        self.right = Some(pair[1]);
        Some(pair[0])
    }
}
impl Source for VoiceStream {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        self.duration
    }
}
struct Comb {
    samples: Vec<f32>,
    index: usize,
    damp: f32,
}
impl Comb {
    fn new(rate: f32, seconds: f32) -> Self {
        Self {
            samples: vec![0.; (rate * seconds).round() as usize],
            index: 0,
            damp: 0.,
        }
    }
    fn tick(&mut self, input: f32) -> f32 {
        let delayed = self.samples[self.index];
        self.damp += 0.25 * (delayed - self.damp);
        self.samples[self.index] = input + self.damp * 0.72;
        self.index = (self.index + 1) % self.samples.len();
        delayed
    }
}
struct Processor {
    low: f32,
    highpass: f32,
    dark: f32,
    coeff: [f32; 3],
    predelay: Vec<f32>,
    cursor: usize,
    left: Vec<Comb>,
    right: Vec<Comb>,
}
impl Processor {
    fn new(rate: f32) -> Self {
        Self {
            low: 0.,
            highpass: 0.,
            dark: 0.,
            coeff: [45., 180., 2600.].map(|hz| 1. - (-std::f32::consts::TAU * hz / rate).exp()),
            predelay: vec![0.; (rate * 0.018) as usize],
            cursor: 0,
            left: [0.0297, 0.0371, 0.0411, 0.0437]
                .into_iter()
                .map(|s| Comb::new(rate, s))
                .collect(),
            right: [0.0307, 0.0383, 0.0427, 0.0451]
                .into_iter()
                .map(|s| Comb::new(rate, s))
                .collect(),
        }
    }
    fn frame(&mut self, input: f32, enabled: bool) -> [f32; 2] {
        self.highpass += self.coeff[0] * (input - self.highpass);
        let clean = input - self.highpass;
        self.low += self.coeff[1] * (clean - self.low);
        let bass = clean + 1. * self.low; // +6 dB low shelf, 180 Hz
        self.dark += self.coeff[2] * (bass - self.dark);
        let dry = (self.dark + (bass - self.dark) * 0.45) * 0.78;
        let delayed = self.predelay[self.cursor];
        self.predelay[self.cursor] = dry;
        self.cursor = (self.cursor + 1) % self.predelay.len();
        let l = self.left.iter_mut().map(|c| c.tick(delayed)).sum::<f32>() * 0.25;
        let r = self.right.iter_mut().map(|c| c.tick(delayed)).sum::<f32>() * 0.25;
        if !enabled {
            return [input, input];
        }
        // Short, damped stereo chamber. Dry diction stays centered; tail spreads.
        [dry + l * 0.32, dry + r * 0.32].map(|v| v.clamp(-0.92, 0.92))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reverb_has_a_stereo_tail_and_decays_to_silence() {
        let mut p = Processor::new(24000.);
        let mut tail = 0.;
        let mut stereo = 0.;
        let mut late = 0.;
        for i in 0..48000 {
            let v = p.frame(if i == 0 { 1. } else { 0. }, true);
            if i > 1000 && i < 12000 {
                tail += v[0].abs();
                stereo += (v[0] - v[1]).abs();
            }
            if i > 45000 {
                late += v[0].abs();
            }
            assert!(v.iter().all(|s| s.is_finite() && s.abs() <= 0.92));
        }
        assert!(tail > 0.05 && stereo > 0.05);
        assert!(late < 0.001);
    }
    #[test]
    fn bypass_preserves_dry_samples_and_eq_favors_bass() {
        fn energy(hz: f32) -> f32 {
            let mut p = Processor::new(24000.);
            (0..24000)
                .map(|i| {
                    let x = (i as f32 * std::f32::consts::TAU * hz / 24000.).sin() * 0.1;
                    let v = p.frame(x, true);
                    if i > 12000 { v[0] * v[0] } else { 0. }
                })
                .sum()
        }
        assert!(energy(120.) > energy(7000.) * 3.);
        let mut p = Processor::new(24000.);
        for x in [0., 0.1, -0.7, 0.3] {
            assert_eq!(p.frame(x, false), [x, x]);
        }
    }
}
