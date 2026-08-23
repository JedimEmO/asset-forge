//! Pull a clip's animation channels back out of a `.glb`.
//!
//! This is the measuring side of the bake: the proof tests read shipped clips
//! and freshly baked bytes through the same extractor, and the library audit
//! can use it to re-derive what a clip actually keys without an engine in the
//! loop. It reports what is there rather than judging it — a missing track is
//! `None`, not an error, because for an auditor absence is a finding.

use glam::{Quat, Vec3};

use crate::skeleton::{self, JOINT_COUNT, JOINTS};
use crate::{BakeError, Result, open_glb};

/// One sampled curve: key times and the value at each key.
#[derive(Debug, Clone)]
pub struct Track<T> {
    /// Key times, seconds.
    pub times: Vec<f32>,
    /// One value per key.
    pub values: Vec<T>,
}

/// The channels of a clip's first animation, keyed to cskel27.
#[derive(Debug, Clone)]
pub struct ClipChannels {
    /// The animation's name inside the file — what an engine binds by.
    pub name: Option<String>,
    /// Total channel count, all paths and all nodes. This crate writes 28;
    /// the frozen Blender-era fixtures under `tests/fixtures/blender` carry
    /// 81 — 53 constant translation/scale curves this baker drops.
    pub channel_count: usize,
    /// Rotation track per joint, indexed like [`JOINTS`].
    pub rotations: Vec<Option<Track<Quat>>>,
    /// The `Hips` translation track.
    pub hips_translation: Option<Track<Vec3>>,
}

impl ClipChannels {
    /// Read the first animation's channels out of GLB bytes.
    ///
    /// # Errors
    ///
    /// Fails when the bytes are not a self-contained GLB or carry no
    /// animation at all. Channels targeting nodes outside cskel27, and
    /// translation or scale channels on other bones, are counted in
    /// [`Self::channel_count`] but otherwise ignored.
    pub fn from_glb(bytes: &[u8]) -> Result<Self> {
        let (document, blob) = open_glb(bytes)?;
        let animation = document.animations().next().ok_or(BakeError::NoAnimation)?;

        let mut this = Self {
            name: animation.name().map(str::to_owned),
            channel_count: 0,
            rotations: vec![None; JOINT_COUNT],
            hips_translation: None,
        };

        for channel in animation.channels() {
            this.channel_count += 1;
            let joint = channel.target().node().name().and_then(skeleton::index_of);
            let Some(joint) = joint else {
                continue;
            };
            let reader = channel.reader(|buffer| match buffer.source() {
                gltf::buffer::Source::Bin => Some(blob.as_slice()),
                gltf::buffer::Source::Uri(_) => None,
            });
            let Some(times) = reader.read_inputs() else {
                continue;
            };
            let times: Vec<f32> = times.collect();
            match reader.read_outputs() {
                Some(gltf::animation::util::ReadOutputs::Rotations(rotations)) => {
                    let values = rotations
                        .into_f32()
                        .map(|q| Quat::from_xyzw(q[0], q[1], q[2], q[3]))
                        .collect();
                    this.rotations[joint] = Some(Track { times, values });
                }
                Some(gltf::animation::util::ReadOutputs::Translations(translations))
                    if JOINTS[joint] == "Hips" =>
                {
                    let values = translations.map(Vec3::from_array).collect();
                    this.hips_translation = Some(Track { times, values });
                }
                _ => {}
            }
        }
        Ok(this)
    }
}
