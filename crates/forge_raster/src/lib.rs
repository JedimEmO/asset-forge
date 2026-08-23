//! A tiny CPU raster surface with a built-in bitmap font.
//!
//! Asset-review images — animation contact sheets, audio waveforms — all need
//! the same three things: somewhere to put pixels, a way to blit a rectangle,
//! and legible labels burnt into the result. This provides exactly that and
//! nothing else.
//!
//! The font is a 5x7 table compiled into the binary rather than a font file on
//! disk, so a review image never depends on an asset being present to say
//! "frame 3" or "-14.2 LUFS".

mod font;

pub use font::text_width;

use font::{GLYPH_H, GLYPH_W, TRACKING, glyph};

/// An RGBA8 pixel buffer you can draw into.
#[derive(Debug, Clone)]
pub struct Canvas {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Canvas {
    /// A canvas filled with `background`.
    #[must_use]
    pub fn new(width: u32, height: u32, background: [u8; 4]) -> Self {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..(width * height) {
            data.extend_from_slice(&background);
        }
        Self {
            width,
            height,
            data,
        }
    }

    /// Width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The raw tightly packed RGBA8 bytes.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Consume the canvas and take its bytes.
    #[must_use]
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }

    /// Set one pixel, blending when `rgba` is not fully opaque.
    ///
    /// Out-of-bounds writes are dropped rather than panicking: plot code
    /// clamps constantly, and an off-by-one at the edge of a waveform should
    /// not take the process down.
    pub fn put(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = ((y * self.width + x) * 4) as usize;
        if rgba[3] == 255 {
            self.data[i..i + 4].copy_from_slice(&rgba);
            return;
        }
        let a = u32::from(rgba[3]);
        for (c, &channel) in rgba.iter().take(3).enumerate() {
            let src = u32::from(channel) * a;
            let dst = u32::from(self.data[i + c]) * (255 - a);
            self.data[i + c] = ((src + dst) / 255) as u8;
        }
    }

    /// Fill an axis-aligned rectangle.
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, rgba: [u8; 4]) {
        for dy in 0..h {
            for dx in 0..w {
                self.put(x + dx, y + dy, rgba);
            }
        }
    }

    /// A one-pixel vertical run, the workhorse of waveform drawing.
    pub fn v_line(&mut self, x: u32, y0: u32, y1: u32, rgba: [u8; 4]) {
        let (lo, hi) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        for y in lo..=hi {
            self.put(x, y, rgba);
        }
    }

    /// A one-pixel horizontal run.
    pub fn h_line(&mut self, x0: u32, x1: u32, y: u32, rgba: [u8; 4]) {
        let (lo, hi) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        for x in lo..=hi {
            self.put(x, y, rgba);
        }
    }

    /// Copy tightly packed RGBA8 pixels in at `(x, y)`, ignoring their alpha.
    ///
    /// Opaque copy rather than blend, so anti-aliased edges of a rendered
    /// frame do not pick up the canvas background.
    pub fn blit_rgba(&mut self, x: u32, y: u32, src: &[u8], src_w: u32, src_h: u32) {
        for sy in 0..src_h {
            for sx in 0..src_w {
                let i = ((sy * src_w + sx) * 4) as usize;
                if i + 3 >= src.len() {
                    continue;
                }
                self.put(x + sx, y + sy, [src[i], src[i + 1], src[i + 2], 255]);
            }
        }
    }

    /// Draw `text` with its top-left at `(x, y)`, uppercased, at integer `scale`.
    pub fn text(&mut self, x: u32, y: u32, text: &str, rgba: [u8; 4], scale: u32) {
        let scale = scale.max(1);
        let mut pen = x;
        for c in text.chars() {
            let bits = glyph(c);
            for (col, column) in bits.iter().enumerate() {
                for row in 0..GLYPH_H {
                    if column & (1 << row) == 0 {
                        continue;
                    }
                    let px = pen + col as u32 * scale;
                    let py = y + row as u32 * scale;
                    for dy in 0..scale {
                        for dx in 0..scale {
                            self.put(px + dx, py + dy, rgba);
                        }
                    }
                }
            }
            pen += (GLYPH_W + TRACKING) as u32 * scale;
        }
    }

    /// Draw `text` on a translucent plate, so it stays legible over anything.
    pub fn label(&mut self, x: u32, y: u32, text: &str, fg: [u8; 4], scale: u32) {
        let scale = scale.max(1);
        let w = text_width(text, scale as usize) as u32;
        let h = GLYPH_H as u32 * scale;
        self.fill_rect(
            x.saturating_sub(2),
            y.saturating_sub(2),
            w + 4,
            h + 4,
            [0, 0, 0, 160],
        );
        self.text(x, y, text, fg, scale);
    }

    /// Height of one line of text at `scale`, in pixels.
    #[must_use]
    pub fn line_height(scale: u32) -> u32 {
        GLYPH_H as u32 * scale.max(1)
    }

    /// Write the canvas to a PNG.
    ///
    /// Alpha is preserved, unlike some engine-provided screenshot helpers that
    /// quietly drop it.
    ///
    /// # Errors
    ///
    /// Returns an error if the buffer is the wrong size or the write fails.
    pub fn save_png(&self, path: impl AsRef<std::path::Path>) -> Result<(), SaveError> {
        save_png(self.width, self.height, &self.data, path)
    }
}

/// Write tightly packed RGBA8 pixels to a PNG.
///
/// # Errors
///
/// Returns [`SaveError`] if `data` is not `width * height * 4` bytes, or if the
/// encoder or filesystem rejects the write.
pub fn save_png(
    width: u32,
    height: u32,
    data: &[u8],
    path: impl AsRef<std::path::Path>,
) -> Result<(), SaveError> {
    let expected = (width as usize) * (height as usize) * 4;
    if data.len() != expected {
        return Err(SaveError::UnexpectedLayout {
            got: data.len(),
            expected,
        });
    }
    let buffer = image::RgbaImage::from_raw(width, height, data.to_vec()).ok_or(
        SaveError::UnexpectedLayout {
            got: data.len(),
            expected,
        },
    )?;
    buffer
        .save(path.as_ref())
        .map_err(|err| SaveError::Io(err.to_string()))
}

/// Why writing an image failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// The buffer was not tightly packed 8-bit RGBA of the stated size.
    UnexpectedLayout {
        /// Bytes actually present.
        got: usize,
        /// Bytes expected for `width * height * 4`.
        expected: usize,
    },
    /// The encoder or filesystem rejected the write.
    Io(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedLayout { got, expected } => write!(
                f,
                "image is {got} bytes, expected {expected} (tightly packed RGBA8)"
            ),
            Self::Io(msg) => write!(f, "could not write image: {msg}"),
        }
    }
}

impl std::error::Error for SaveError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_bounds_writes_are_dropped_not_panics() {
        let mut canvas = Canvas::new(4, 4, [0, 0, 0, 255]);
        canvas.put(99, 99, [255, 255, 255, 255]);
        canvas.fill_rect(2, 2, 100, 100, [255, 255, 255, 255]);
        assert_eq!(canvas.data().len(), 4 * 4 * 4);
    }

    #[test]
    fn text_advances_by_glyph_plus_tracking() {
        assert_eq!(text_width("", 1), 0);
        assert_eq!(text_width("A", 1), GLYPH_W);
        assert_eq!(text_width("AB", 1), GLYPH_W * 2 + TRACKING);
        assert_eq!(text_width("AB", 2), (GLYPH_W * 2 + TRACKING) * 2);
    }

    #[test]
    fn opaque_writes_replace_and_translucent_writes_blend() {
        let mut canvas = Canvas::new(1, 1, [0, 0, 0, 255]);
        canvas.put(0, 0, [255, 255, 255, 255]);
        assert_eq!(&canvas.data()[0..3], &[255, 255, 255]);
        canvas.put(0, 0, [0, 0, 0, 128]);
        assert!(canvas.data()[0] < 255 && canvas.data()[0] > 0);
    }
}
