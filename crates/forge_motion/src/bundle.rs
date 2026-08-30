//! One body and any number of clips, merged into a single `.glb` to hand to
//! somebody outside the toolkit.
//!
//! # A bundle is a merge, not a bake
//!
//! Nothing here re-derives motion. A baked clip carries a rotation curve per
//! bone plus one translation curve on the root, each channel targeting a node
//! **by name**; a body carries the rig contract's names, hierarchy and rest
//! rotations. So re-pointing every channel at the body's node of the same
//! name is all a bundle is, and the samplers' bytes travel unchanged: the
//! output accessor of a channel in the bundle is the source clip's accessor,
//! copied. That is why the merge lives in this crate rather than beside the
//! library's promote doors — it is a `.glb` writer, and this crate's
//! private `glb` module owns the container framing and the four-byte
//! alignment rule that any second writer has to share or produce files which
//! load in one importer and not the next.
//!
//! The one number it does apply is `motion_scale`: the root translation track
//! is in **metres**, measured against the reference skeleton's legs, so a
//! body fitted to its own proportions travels wrong unless the track is
//! scaled by its leg ratio (`designs/forge2.md`, the fitted-skeleton design).
//! Rotations are never touched, and neither is any translation channel other
//! than the root's — the Blender-era clips carried constant translation
//! curves restating bone rest offsets, and scaling one of those would move a
//! skeleton rather than a character.
//!
//! # What it refuses
//!
//! A clip that drives a bone the body has no node for, naming the bone: the
//! channel would bind to nothing, and an engine reports that at no log level.
//! Two clips that would carry one animation name. A file that points at its
//! buffer by `uri`, which is not self-contained and so cannot be merged into
//! one that is.
//!
//! # Determinism
//!
//! The same body, the same clips in the same order and the same scale give
//! the same bytes. The body's document is edited as generic JSON — so
//! materials, textures and any extension this crate does not model come
//! through untouched — copied views are appended in channel order, and the
//! container is framed by the same code every other writer here uses.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::glb;
use crate::skeleton::{self, JOINTS};
use crate::{BakeError, Result};

/// One clip to merge in.
#[derive(Debug, Clone, Copy)]
pub struct ClipSource<'a> {
    /// What the caller called it — a library name or a file stem. Used in
    /// refusals, and as the animation's name when the source `.glb` does not
    /// name its own animation.
    pub name: &'a str,
    /// The clip's `.glb` bytes.
    pub bytes: &'a [u8],
}

/// What a merge produced.
#[derive(Debug, Clone)]
pub struct Bundled {
    /// The bundle's `.glb` bytes.
    pub bytes: Vec<u8>,
    /// The animation names it carries, in the order the clips were given.
    pub animations: Vec<String>,
}

/// Merge `clips` into `body` as named animations.
///
/// Every channel is re-pointed at the body's node of the same name and every
/// sampler's accessors are copied verbatim, except the root translation
/// track, whose values are multiplied by `motion_scale` (1.0 leaves the bytes
/// alone). An animation is named by the name the source `.glb` gives it —
/// the clip's name inside the file, which is what an engine binds by —
/// falling back to [`ClipSource::name`] when the source names none.
///
/// # Errors
///
/// The body or a clip is not a readable, self-contained `.glb`; a clip has no
/// animation; a clip drives a bone the body lacks (the bone is named); two
/// clips would share one animation name; the scale is not a positive finite
/// number.
pub fn bundle(body: &[u8], clips: &[ClipSource<'_>], motion_scale: f32) -> Result<Bundled> {
    if !motion_scale.is_finite() || motion_scale <= 0.0 {
        return Err(BakeError::BadMotionScale(motion_scale));
    }

    let (json_chunk, bin_chunk) = glb::split(body)?;
    let mut doc: Value = serde_json::from_slice(json_chunk)
        .map_err(|e| BakeError::Glb(format!("the body's JSON chunk: {e}")))?;
    let mut merge = Merge {
        nodes: node_index(&doc)?,
        bin: payload(&doc, bin_chunk, "the body")?.to_vec(),
        views: array(&doc, "bufferViews"),
        accessors: array(&doc, "accessors"),
        animations: array(&doc, "animations"),
    };

    let mut names: BTreeSet<String> = merge
        .animations
        .iter()
        .filter_map(|a| a.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    let mut animations = Vec::with_capacity(clips.len());
    for clip in clips {
        let name = merge.add(clip, motion_scale)?;
        if !names.insert(name.clone()) {
            return Err(BakeError::BundleDuplicateAnimation {
                name,
                clip: clip.name.to_owned(),
            });
        }
        animations.push(name);
    }

    doc["bufferViews"] = Value::Array(merge.views);
    doc["accessors"] = Value::Array(merge.accessors);
    doc["animations"] = Value::Array(merge.animations);
    doc["buffers"] = json!([{ "byteLength": merge.bin.len() }]);
    doc["asset"]["generator"] = json!(format!(
        "forge_motion {} (bundle)",
        env!("CARGO_PKG_VERSION")
    ));

    let json = serde_json::to_vec(&doc)
        .map_err(|e| BakeError::Glb(format!("serializing the bundle's JSON: {e}")))?;
    Ok(Bundled {
        bytes: glb::container(&json, &merge.bin),
        animations,
    })
}

/// The body's document under construction: the arrays a clip appends to, its
/// binary chunk, and the node index every channel is re-pointed through.
struct Merge {
    /// Body node index by name.
    nodes: BTreeMap<String, usize>,
    /// The binary chunk, grown four-byte aligned by the shared appender.
    bin: Vec<u8>,
    /// `bufferViews`, in the body's order then append order.
    views: Vec<Value>,
    /// `accessors`, likewise.
    accessors: Vec<Value>,
    /// `animations`: whatever the body had, then one per clip.
    animations: Vec<Value>,
}

impl Merge {
    /// Copy one clip's first animation in, and say what it is called.
    fn add(&mut self, clip: &ClipSource<'_>, motion_scale: f32) -> Result<String> {
        let refuse = |detail: String| BakeError::Bundle {
            clip: clip.name.to_owned(),
            detail,
        };
        let (json_chunk, bin_chunk) = glb::split(clip.bytes)?;
        let source: Value = serde_json::from_slice(json_chunk)
            .map_err(|e| refuse(format!("its JSON chunk does not parse: {e}")))?;
        let bin = payload(&source, bin_chunk, &format!("clip {}", clip.name))?;

        let animation = source
            .get("animations")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .ok_or_else(|| refuse(String::from("it carries no animation")))?;
        let name = animation
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(clip.name)
            .to_owned();

        let nodes = source.get("nodes").and_then(Value::as_array);
        let samplers = animation
            .get("samplers")
            .and_then(Value::as_array)
            .ok_or_else(|| refuse(String::from("its animation has no samplers")))?;
        let source_channels = animation
            .get("channels")
            .and_then(Value::as_array)
            .ok_or_else(|| refuse(String::from("its animation has no channels")))?;

        // One entry per source accessor, per scaled-or-not: the 28 channels of
        // a baked clip share one input accessor, and copying it 28 times would
        // make the bundle both larger and different from a bundle of the same
        // clip written any other way.
        let mut copied: BTreeMap<(usize, bool), usize> = BTreeMap::new();
        let mut channels = Vec::with_capacity(source_channels.len());
        let mut new_samplers = Vec::with_capacity(source_channels.len());

        for channel in source_channels {
            let target = channel
                .get("target")
                .ok_or_else(|| refuse(String::from("a channel has no target")))?;
            let path = target
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| refuse(String::from("a channel target has no path")))?;
            let node = index(target, "node")
                .ok_or_else(|| refuse(String::from("a channel target names no node")))?;
            let bone = nodes
                .and_then(|nodes| nodes.get(node))
                .and_then(|node| node.get("name"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    refuse(format!("a channel targets node {node}, which has no name"))
                })?;
            let on_body = *self
                .nodes
                .get(bone)
                .ok_or_else(|| BakeError::BundleMissingBone {
                    clip: clip.name.to_owned(),
                    bone: bone.to_owned(),
                })?;

            let sampler = index(channel, "sampler")
                .and_then(|i| samplers.get(i))
                .ok_or_else(|| refuse(String::from("a channel names no sampler")))?;
            let (input, output) = (
                index(sampler, "input")
                    .ok_or_else(|| refuse(String::from("a sampler has no input")))?,
                index(sampler, "output")
                    .ok_or_else(|| refuse(String::from("a sampler has no output")))?,
            );
            // The root's translation track is the character's travel, in
            // metres against the reference legs. Every other channel — every
            // rotation, and the constant translation curves a Blender-era
            // clip carries — is bytes in, bytes out.
            let scaled = path == "translation"
                && bone == JOINTS[skeleton::ROOT]
                && (motion_scale - 1.0).abs() > f32::EPSILON;

            let mut into = |accessor: usize, scaled: bool| -> Result<usize> {
                if let Some(already) = copied.get(&(accessor, scaled)) {
                    return Ok(*already);
                }
                let new = self.copy_accessor(&source, bin, accessor, scaled, motion_scale, clip)?;
                copied.insert((accessor, scaled), new);
                Ok(new)
            };
            let input = into(input, false)?;
            let output = into(output, scaled)?;

            let mut sampler = sampler.clone();
            sampler["input"] = json!(input);
            sampler["output"] = json!(output);
            new_samplers.push(sampler);

            let mut channel = channel.clone();
            channel["sampler"] = json!(new_samplers.len() - 1);
            channel["target"]["node"] = json!(on_body);
            channels.push(channel);
        }

        let mut merged = animation.clone();
        merged["name"] = json!(name);
        merged["channels"] = Value::Array(channels);
        merged["samplers"] = Value::Array(new_samplers);
        self.animations.push(merged);
        Ok(name)
    }

    /// Copy one accessor and the view under it into the body, and say which
    /// accessor it became.
    ///
    /// Unscaled, the view's bytes are copied whole and the accessor keeps its
    /// own `byteOffset` into them, so what the accessor reads is byte for
    /// byte what it read in the clip. Scaled, only the accessor's own
    /// elements are read, multiplied and written tightly — a view shared with
    /// another accessor must not be rewritten under it.
    fn copy_accessor(
        &mut self,
        source: &Value,
        bin: &[u8],
        accessor: usize,
        scaled: bool,
        motion_scale: f32,
        clip: &ClipSource<'_>,
    ) -> Result<usize> {
        let refuse = |detail: String| BakeError::Bundle {
            clip: clip.name.to_owned(),
            detail,
        };
        let mut accessor = source
            .get("accessors")
            .and_then(Value::as_array)
            .and_then(|a| a.get(accessor))
            .cloned()
            .ok_or_else(|| {
                refuse(format!(
                    "a sampler names accessor {accessor}, which is absent"
                ))
            })?;
        let view_index = index(&accessor, "bufferView").ok_or_else(|| {
            refuse(String::from(
                "an animation accessor has no buffer view; a bundle has no \
                 values to copy from a sparse or zero-filled one",
            ))
        })?;
        let view = source
            .get("bufferViews")
            .and_then(Value::as_array)
            .and_then(|v| v.get(view_index))
            .ok_or_else(|| {
                refuse(format!(
                    "accessor names buffer view {view_index}, which is absent"
                ))
            })?;
        let offset = index(view, "byteOffset").unwrap_or(0);
        let length = index(view, "byteLength")
            .ok_or_else(|| refuse(format!("buffer view {view_index} has no byteLength")))?;
        let data = bin.get(offset..offset + length).ok_or_else(|| {
            refuse(format!(
                "buffer view {view_index} runs past the binary chunk"
            ))
        })?;

        let (data, view) = if scaled {
            let count = index(&accessor, "count")
                .ok_or_else(|| refuse(String::from("an accessor has no count")))?;
            let components = match accessor.get("type").and_then(Value::as_str) {
                Some("VEC3") => 3,
                other => {
                    return Err(refuse(format!(
                        "the root translation accessor is {other:?}, not VEC3"
                    )));
                }
            };
            if accessor.get("componentType").and_then(Value::as_u64) != Some(5126) {
                return Err(refuse(String::from(
                    "the root translation accessor is not float; a scale \
                     cannot be applied to it without changing what it means",
                )));
            }
            let start = index(&accessor, "byteOffset").unwrap_or(0);
            let stride = index(view, "byteStride").unwrap_or(components * 4);
            let mut values = Vec::with_capacity(count * components);
            for element in 0..count {
                for component in 0..components {
                    let at = start + element * stride + component * 4;
                    let word = data
                        .get(at..at + 4)
                        .and_then(|b| b.try_into().ok())
                        .ok_or_else(|| {
                            refuse(String::from(
                                "the root translation track runs past its view",
                            ))
                        })?;
                    values.push(f32::from_le_bytes(word) * motion_scale);
                }
            }
            accessor["byteOffset"] = json!(0);
            if accessor.get("min").is_some() {
                accessor["min"] = bound(&values, components, f32::min);
            }
            if accessor.get("max").is_some() {
                accessor["max"] = bound(&values, components, f32::max);
            }
            let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
            (bytes, json!({ "buffer": 0 }))
        } else {
            (data.to_vec(), view.clone())
        };

        let at = glb::append_view(&mut self.bin, &data);
        let mut view = view;
        view["buffer"] = json!(0);
        view["byteOffset"] = json!(at);
        view["byteLength"] = json!(data.len());
        self.views.push(view);
        accessor["bufferView"] = json!(self.views.len() - 1);
        self.accessors.push(accessor);
        Ok(self.accessors.len() - 1)
    }
}

/// Every node name in a document, and where it is.
///
/// Duplicate names are refused: bones are addressed by name, so a second
/// `Hips` makes "the" bone a question the file cannot answer, and a channel
/// would be re-pointed at whichever one this map happened to keep.
fn node_index(doc: &Value) -> Result<BTreeMap<String, usize>> {
    let mut index = BTreeMap::new();
    for (at, node) in doc
        .get("nodes")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        if let Some(name) = node.get("name").and_then(Value::as_str)
            && index.insert(name.to_owned(), at).is_some()
        {
            return Err(BakeError::DuplicateNodeName(name.to_owned()));
        }
    }
    Ok(index)
}

/// The bytes a document's one buffer declares, out of the chunk it was stored
/// in — which is padded to four bytes and so is usually longer.
fn payload<'a>(doc: &Value, bin: &'a [u8], what: &str) -> Result<&'a [u8]> {
    let buffers = doc
        .get("buffers")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if buffers.iter().any(|b| b.get("uri").is_some()) {
        return Err(BakeError::NotSelfContained(what.to_owned()));
    }
    let [buffer] = buffers else {
        return Err(BakeError::Glb(format!(
            "{what} has {} buffers; a self-contained .glb has one",
            buffers.len()
        )));
    };
    let length = index(buffer, "byteLength")
        .ok_or_else(|| BakeError::Glb(format!("{what}'s buffer does not say how long it is")))?;
    bin.get(..length).ok_or_else(|| {
        BakeError::Glb(format!(
            "{what} declares {length} bytes of buffer and its binary chunk holds {}",
            bin.len()
        ))
    })
}

/// An unsigned integer field, as an index.
fn index(value: &Value, key: &str) -> Option<usize> {
    usize::try_from(value.get(key)?.as_u64()?).ok()
}

/// The per-component minimum or maximum of tightly packed values, which is
/// what glTF wants an accessor's bounds to be.
fn bound(values: &[f32], components: usize, pick: fn(f32, f32) -> f32) -> Value {
    let folded: Vec<Value> = (0..components)
        .map(|component| {
            values
                .iter()
                .skip(component)
                .step_by(components)
                .copied()
                .reduce(pick)
                .map_or(Value::Null, |v| Value::from(f64::from(v)))
        })
        .collect();
    Value::Array(folded)
}

/// The named array of a document, cloned, or an empty one.
fn array(doc: &Value, key: &str) -> Vec<Value> {
    doc.get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}
