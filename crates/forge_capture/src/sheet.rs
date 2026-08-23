//! Compose captured frames into a labelled contact sheet.
//!
//! # Sizing
//!
//! The defaults exist to land under the ~1568 px long edge that vision models
//! downscale past. Overshooting is worse than rendering smaller: you pay for
//! pixels and then lose them to a resample, which turns burnt-in labels to
//! mush. A standing figure is portrait, so cells default to 3:4 rather than
//! square — a square cell spends roughly 40% of itself on empty air.

use bevy::image::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::{asset::RenderAssetUsages, math::UVec2};

use forge_raster::Canvas;

/// Long-edge budget, in pixels, before a vision model resamples the image.
pub const VISION_LONG_EDGE: u32 = 1568;

/// How cells are arranged and decorated.
#[derive(Debug, Clone)]
pub struct SheetLayout {
    /// Cells per row.
    pub columns: u32,
    /// Pixels between cells.
    pub gutter: u32,
    /// Sheet background, RGBA.
    pub background: [u8; 4],
    /// Label and header colour, RGBA.
    pub foreground: [u8; 4],
    /// Integer scale for label text.
    pub label_scale: u32,
    /// Height reserved above the grid for the header, 0 to omit it.
    pub header_height: u32,
}

impl Default for SheetLayout {
    fn default() -> Self {
        Self {
            columns: 4,
            gutter: 2,
            background: [18, 18, 22, 255],
            foreground: [235, 235, 240, 255],
            label_scale: 2,
            header_height: 26,
        }
    }
}

impl SheetLayout {
    /// Total sheet size for `count` cells of `cell` pixels each.
    #[must_use]
    pub fn sheet_size(&self, count: u32, cell: UVec2) -> UVec2 {
        let columns = self.columns.max(1).min(count.max(1));
        let rows = count.div_ceil(columns);
        UVec2::new(
            columns * cell.x + columns.saturating_sub(1) * self.gutter,
            self.header_height + rows * cell.y + rows.saturating_sub(1) * self.gutter,
        )
    }

    /// Largest cell size that keeps the finished sheet within `VISION_LONG_EDGE`.
    ///
    /// Returns `cell` unchanged when it already fits. Callers should report the
    /// final resolution either way — a silently shrunk sheet reads as a
    /// deliberate choice when it was actually a clamp.
    #[must_use]
    pub fn fit_to_budget(&self, count: u32, cell: UVec2) -> UVec2 {
        let mut cell = cell;
        for _ in 0..16 {
            let size = self.sheet_size(count, cell);
            let long = size.x.max(size.y);
            if long <= VISION_LONG_EDGE || cell.x <= 32 || cell.y <= 32 {
                break;
            }
            let scale = f64::from(VISION_LONG_EDGE) / f64::from(long);
            let next = UVec2::new(
                ((f64::from(cell.x) * scale) as u32).max(32),
                ((f64::from(cell.y) * scale) as u32).max(32),
            );
            if next == cell {
                break;
            }
            cell = next;
        }
        cell
    }
}

/// One captured frame plus the caption burnt into its corner.
pub struct SheetCell {
    /// The rendered frame. All cells in a sheet must share dimensions.
    pub image: Image,
    /// Caption drawn bottom-left, e.g. `"#3 0.42S"`.
    pub label: String,
}

/// Why a sheet could not be composed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SheetError {
    /// No cells were supplied.
    Empty,
    /// Cells disagreed on size, or one carried no pixels.
    Ragged {
        /// Index of the offending cell.
        index: usize,
    },
}

impl std::fmt::Display for SheetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("no cells to compose"),
            Self::Ragged { index } => write!(
                f,
                "cell {index} has a different size or no pixel data than the first cell"
            ),
        }
    }
}

impl std::error::Error for SheetError {}

/// Compose `cells` into a single labelled sheet.
///
/// # Errors
///
/// Returns [`SheetError`] if `cells` is empty or the frames disagree on size.
pub fn compose(
    cells: &[SheetCell],
    layout: &SheetLayout,
    header: &str,
) -> Result<Image, SheetError> {
    let first = cells.first().ok_or(SheetError::Empty)?;
    let cell = UVec2::new(first.image.width(), first.image.height());
    for (index, c) in cells.iter().enumerate() {
        if c.image.width() != cell.x || c.image.height() != cell.y || c.image.data.is_none() {
            return Err(SheetError::Ragged { index });
        }
    }

    let count = cells.len() as u32;
    let columns = layout.columns.max(1).min(count);
    let size = layout.sheet_size(count, cell);

    let mut canvas = Canvas::new(size.x, size.y, layout.background);

    if layout.header_height > 0 && !header.is_empty() {
        let baseline = (layout
            .header_height
            .saturating_sub(Canvas::line_height(layout.label_scale)))
            / 2;
        canvas.text(4, baseline, header, layout.foreground, layout.label_scale);
    }

    for (index, c) in cells.iter().enumerate() {
        let index = index as u32;
        let col = index % columns;
        let row = index / columns;
        let origin = UVec2::new(
            col * (cell.x + layout.gutter),
            layout.header_height + row * (cell.y + layout.gutter),
        );
        if let Some(pixels) = c.image.data.as_ref() {
            canvas.blit_rgba(origin.x, origin.y, pixels, cell.x, cell.y);
        }
        if !c.label.is_empty() {
            let pad = 3 * layout.label_scale;
            let text_h = Canvas::line_height(layout.label_scale);
            // label() draws its own dark plate, so light text stays readable
            // over a light render.
            canvas.label(
                origin.x + pad,
                origin.y + cell.y - text_h - pad,
                &c.label,
                layout.foreground,
                layout.label_scale,
            );
        }
    }

    Ok(into_image(canvas))
}

/// Wrap a finished canvas as a Bevy [`Image`].
fn into_image(canvas: Canvas) -> Image {
    Image::new(
        Extent3d {
            width: canvas.width(),
            height: canvas.height(),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        canvas.into_data(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_lands_under_the_vision_budget() {
        // The documented default: 8 frames, 4 columns, 384x512 cells.
        let layout = SheetLayout::default();
        let size = layout.sheet_size(8, UVec2::new(384, 512));
        assert!(
            size.x.max(size.y) <= VISION_LONG_EDGE,
            "default sheet {size:?} exceeds the {VISION_LONG_EDGE}px budget"
        );
    }

    #[test]
    fn oversized_requests_are_clamped() {
        let layout = SheetLayout::default();
        let cell = layout.fit_to_budget(8, UVec2::new(1024, 1365));
        let size = layout.sheet_size(8, cell);
        assert!(size.x.max(size.y) <= VISION_LONG_EDGE);
        assert!(cell.x < 1024, "expected a shrink, got {cell:?}");
    }

    #[test]
    fn already_small_requests_are_left_alone() {
        let layout = SheetLayout::default();
        let cell = UVec2::new(192, 256);
        assert_eq!(layout.fit_to_budget(8, cell), cell);
    }
}
