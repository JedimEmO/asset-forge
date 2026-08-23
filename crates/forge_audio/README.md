# forge_audio

Decode game audio, measure it, and render it as something you can look at —
no engine, no sound card. The animation half of
[asset-forge](https://github.com/JedimEmO/asset-forge) turns motion into
contact sheets because an agent cannot watch; this is the same idea for
sound: an agent cannot listen, so a file becomes a waveform, a spectrogram
and a set of numbers that fail loudly.

```rust,no_run
let audio = forge_audio::decode("assets/audio/sfx/door_slam.wav")?;
let metrics = forge_audio::measure(&audio);
for warning in metrics.warnings() {
    eprintln!("{warning}");
}
let plot = forge_audio::render(&audio, &metrics, "door_slam", &forge_audio::PlotLayout::default());
plot.save_png("out/audio/door_slam.png")?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What is here

| Module | What it does |
|---|---|
| `decode` | `decode` — WAV, OGG/Vorbis, MP3, FLAC through symphonia, to interleaved f32 |
| `metrics` | `measure` — duration, peak dBFS, RMS, an approximate integrated loudness, leading and trailing silence, clipping runs; `Metrics::warnings` names what is wrong |
| `plot` | `render` — waveform over spectrogram with the numbers in the header, on a `forge_raster` canvas; the default width stays under the edge vision models downscale past |
| `cli` | `inspect` (one file, optionally plotted; exit 1 if silent or clipped) and `list` (a directory, one row each) — what `forge audio inspect` and `forge audio list` call |

## Two measurement notes

**Clipping is a run, not a count.** Peak-normalising to 0 dBFS puts a sample
at the rail by construction; counting full-scale samples once flagged five
of eight shipped effects as broken. Only sustained flat-topping warns.

**Loudness is approximate and says so.** The K-weighting is a high-pass
stand-in — enough to compare assets in one library (a bark 20 LU below its
neighbours is obvious in a column), not enough to certify a master, and the
column header does not promise the second.

Deliberately no audio *output* backend: analysis has to work in CI and on a
machine with no sound card. Hearing a file is the studio's job.
