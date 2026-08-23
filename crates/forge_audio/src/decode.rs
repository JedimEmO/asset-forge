//! Decode an audio file to interleaved f32 samples.

use std::path::Path;

use symphonia::core::audio::{Channels, SampleBuffer};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Decoded audio, as planar-free interleaved f32.
#[derive(Debug, Clone)]
pub struct Audio {
    /// Interleaved samples, nominally -1.0..=1.0 but not clamped — values
    /// beyond that range are exactly the clipping we want to be able to see.
    pub samples: Vec<f32>,
    /// Samples per second, per channel.
    pub sample_rate: u32,
    /// Channel count. Interleaved, so frame `n` starts at `n * channels`.
    pub channels: usize,
}

impl Audio {
    /// Number of frames (samples per channel).
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len().checked_div(self.channels).unwrap_or(0)
    }

    /// Length in seconds.
    #[must_use]
    pub fn duration(&self) -> f32 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames() as f32 / self.sample_rate as f32
        }
    }

    /// Channels summed to mono, for analysis and plotting.
    #[must_use]
    pub fn mono(&self) -> Vec<f32> {
        if self.channels <= 1 {
            return self.samples.clone();
        }
        self.samples
            .chunks(self.channels)
            .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
            .collect()
    }
}

/// Why decoding failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The file could not be opened.
    Open(String),
    /// No decoder matched, or the container was unreadable.
    Unsupported(String),
    /// The stream was readable but produced no audio.
    Empty,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open(msg) => write!(f, "could not open the file: {msg}"),
            Self::Unsupported(msg) => write!(f, "could not decode: {msg}"),
            Self::Empty => f.write_str("decoded successfully but contained no audio"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Decode any format symphonia was built with (wav, ogg/vorbis, mp3, flac).
///
/// # Errors
///
/// Returns [`DecodeError`] if the file cannot be opened, no decoder matches, or
/// the stream yields no samples.
pub fn decode(path: impl AsRef<Path>) -> Result<Audio, DecodeError> {
    let path = path.as_ref();
    let file = std::fs::File::open(path).map_err(|e| DecodeError::Open(e.to_string()))?;
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());

    // The extension is a hint only; symphonia still probes the actual bytes, so
    // a mislabelled file still decodes.
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| DecodeError::Unsupported(e.to_string()))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| DecodeError::Unsupported("no audio track".into()))?;
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| DecodeError::Unsupported(e.to_string()))?;

    let mut samples: Vec<f32> = Vec::new();
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut channels = track.codec_params.channels.map_or(0, Channels::count);
    let mut buffer: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // Symphonia signals a clean end of stream as an IO error, and a
            // mid-stream format change as ResetRequired; both mean "stop
            // reading", and anything else is a real failure worth surfacing.
            Err(
                symphonia::core::errors::Error::IoError(_)
                | symphonia::core::errors::Error::ResetRequired,
            ) => break,
            Err(e) => return Err(DecodeError::Unsupported(e.to_string())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(chunk) => {
                let layout = *chunk.spec();
                sample_rate = layout.rate;
                channels = layout.channels.count();
                let buf = buffer.get_or_insert_with(|| {
                    SampleBuffer::<f32>::new(chunk.capacity() as u64, layout)
                });
                buf.copy_interleaved_ref(chunk);
                samples.extend_from_slice(buf.samples());
            }
            // A corrupt packet mid-file should not throw away everything
            // decoded so far — that is exactly the defect worth seeing.
            Err(symphonia::core::errors::Error::DecodeError(_)) => {}
            Err(e) => return Err(DecodeError::Unsupported(e.to_string())),
        }
    }

    if samples.is_empty() || channels == 0 {
        return Err(DecodeError::Empty);
    }

    Ok(Audio {
        samples,
        sample_rate,
        channels,
    })
}
