//! The GLB plumbing every writer in this crate shares.
//!
//! `gltf-json` models the document but not the file: the binary chunk's
//! layout and the container framing are the writer's problem. Both live here,
//! apart from the clip baker ([`crate::bake`]), so any second writer this
//! crate grows cannot drift from it on alignment, on how an accessor is
//! spelled, or on how the two chunks are padded — a drift that produces files
//! which load in one importer and not the next. (The character writer that
//! once shared this plumbing retired with the parametric body generator;
//! the rule outlived it.)
//!
//! The layout rule is one buffer view per accessor, appended in call order and
//! four-byte aligned. That is more views than a packed exporter would write and
//! deliberately so: with `byte_offset` zero on every accessor, the emission
//! order of the calls IS the layout, which is what makes the output
//! byte-reproducible from the same inputs.

use gltf::json;
use json::Index;
use json::validation::{Checked::Valid, USize64};

use crate::{BakeError, Result};

/// A glTF document under construction: the JSON tree plus the binary chunk
/// its accessors point into.
#[derive(Default)]
pub(crate) struct Builder {
    pub(crate) root: json::Root,
    pub(crate) bin: Vec<u8>,
}

impl Builder {
    /// Append `data` to the binary chunk as its own buffer view, four-byte
    /// aligned so every component type this crate uses is legally offset.
    pub(crate) fn view(
        &mut self,
        data: &[u8],
        target: Option<json::buffer::Target>,
    ) -> Index<json::buffer::View> {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend_from_slice(data);
        self.root.push(json::buffer::View {
            buffer: Index::new(0),
            byte_length: USize64::from(data.len()),
            byte_offset: Some(USize64::from(offset)),
            byte_stride: None,
            name: None,
            target: target.map(Valid),
            extensions: None,
            extras: <_>::default(),
        })
    }

    /// A float accessor over `view`.
    pub(crate) fn accessor(
        &mut self,
        view: Index<json::buffer::View>,
        type_: json::accessor::Type,
        count: usize,
        bounds: Option<(json::Value, json::Value)>,
    ) -> Index<json::Accessor> {
        self.accessor_of(
            view,
            json::accessor::ComponentType::F32,
            type_,
            count,
            bounds,
        )
    }

    /// An accessor of an explicit component type, read as raw integers.
    pub(crate) fn accessor_of(
        &mut self,
        view: Index<json::buffer::View>,
        component: json::accessor::ComponentType,
        type_: json::accessor::Type,
        count: usize,
        bounds: Option<(json::Value, json::Value)>,
    ) -> Index<json::Accessor> {
        self.push_accessor(view, component, type_, count, bounds, false)
    }

    fn push_accessor(
        &mut self,
        view: Index<json::buffer::View>,
        component: json::accessor::ComponentType,
        type_: json::accessor::Type,
        count: usize,
        bounds: Option<(json::Value, json::Value)>,
        normalized: bool,
    ) -> Index<json::Accessor> {
        let (min, max) = bounds.map_or((None, None), |(min, max)| (Some(min), Some(max)));
        self.root.push(json::Accessor {
            buffer_view: Some(view),
            byte_offset: Some(USize64(0)),
            count: USize64::from(count),
            component_type: Valid(json::accessor::GenericComponentType(component)),
            type_: Valid(type_),
            min,
            max,
            name: None,
            normalized,
            sparse: None,
            extensions: None,
            extras: <_>::default(),
        })
    }

    /// Close the single buffer over the binary chunk and frame the pair as
    /// GLB bytes. Consumes the builder: nothing may be appended afterwards,
    /// because the buffer's declared length would stop matching.
    pub(crate) fn finish(mut self) -> Result<Vec<u8>> {
        self.root.buffers.push(json::Buffer {
            byte_length: USize64::from(self.bin.len()),
            name: None,
            uri: None,
            extensions: None,
            extras: <_>::default(),
        });
        let json = self
            .root
            .to_vec()
            .map_err(|e| BakeError::Glb(format!("serializing glTF JSON: {e}")))?;
        Ok(container(&json, &self.bin))
    }
}

/// A node with every optional field empty; the callers state what they mean.
pub(crate) fn default_node() -> json::Node {
    json::Node {
        camera: None,
        children: None,
        matrix: None,
        mesh: None,
        name: None,
        rotation: None,
        scale: None,
        translation: None,
        skin: None,
        weights: None,
        extensions: None,
        extras: <_>::default(),
    }
}

pub(crate) fn f32_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Frame JSON and binary chunks as a GLB: 12-byte header, then each chunk as
/// length + tag + payload, JSON padded to four bytes with spaces and BIN with
/// zeros, total length in the header.
fn container(json: &[u8], bin: &[u8]) -> Vec<u8> {
    let json_padded = json.len().next_multiple_of(4);
    let bin_padded = bin.len().next_multiple_of(4);
    let total = 12 + 8 + json_padded + 8 + bin_padded;

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());

    out.extend_from_slice(&(json_padded as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(json);
    out.resize(out.len() + (json_padded - json.len()), b' ');

    out.extend_from_slice(&(bin_padded as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(bin);
    out.resize(out.len() + (bin_padded - bin.len()), 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_container_is_padded_and_sized() {
        let glb = container(b"{\"a\":1}", &[1, 2, 3, 4, 5]);
        assert_eq!(&glb[..4], b"glTF");
        let total = u32::from_le_bytes(glb[8..12].try_into().expect("4 bytes")) as usize;
        assert_eq!(total, glb.len());
        assert_eq!(glb.len() % 4, 0);
        // JSON chunk: 7 bytes padded to 8 with a trailing space.
        let json_len = u32::from_le_bytes(glb[12..16].try_into().expect("4 bytes")) as usize;
        assert_eq!(json_len, 8);
        assert_eq!(glb[20 + 7], b' ');
    }

    #[test]
    fn views_are_four_byte_aligned_and_appended_in_call_order() {
        let mut b = Builder::default();
        b.view(&[1, 2, 3], None);
        b.view(&[4, 5], None);
        let views = &b.root.buffer_views;
        assert_eq!(views[0].byte_offset.expect("offset").0, 0);
        assert_eq!(views[1].byte_offset.expect("offset").0, 4);
        assert_eq!(b.bin, [1, 2, 3, 0, 4, 5]);
    }
}
