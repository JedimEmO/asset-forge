# forge_raster

A tiny CPU raster surface with a built-in 5×7 bitmap font. Asset-review
images — animation contact sheets, mesh views, audio plots — all need the
same three things: somewhere to put pixels, a way to blit a rectangle, and
legible labels burnt into the result. This provides exactly that and nothing
else.

```rust
use forge_raster::Canvas;

let mut canvas = Canvas::new(320, 80, [24, 24, 28, 255]);
canvas.fill_rect(8, 8, 120, 24, [60, 60, 70, 255]);
canvas.text(12, 14, "frame 3  t=0.150s", [235, 235, 235, 255], 2);
// canvas.save_png("out/label.png")?;
```

`Canvas::new`, `fill_rect`, `blit_rgba`, `text` (with an integer scale),
`line_height`, `text_width`, `save_png`. The font is a table compiled into
the binary rather than a file on disk, so a review image never depends on an
asset being present to say "frame 3" or "-14.2 LUFS". The only dependency
is `image`, for the PNG encoder.

Part of [asset-forge](https://github.com/JedimEmO/asset-forge);
`forge_capture` composes its sheets on it and `forge_audio` draws its plots
on it.
