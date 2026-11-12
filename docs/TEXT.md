# Text

Small text is where an engine shows what it is. A word at eleven pixels is
eight letters, each of them a shape a pixel wide, and the eye is very good at
seeing when those shapes do not agree with one another. This is the contract
the text pipeline keeps.

## 0. What is implemented today

| Part | State | Where |
|---|---|---|
| Shaping (ligatures, kerning, marks, bidi) | shipped | `mre-text`, rustybuzz |
| Font fallback by coverage | shipped | `mre-text` |
| Line breaking, word wrap | shipped | `mre-text`, unicode-linebreak |
| Hinting to the pixel grid | shipped | `mre-raster/src/outline.rs`, skrifa |
| Exact-area coverage | shipped | `mre-raster/src/glyph.rs`, zeno |
| Subpixel pen position | shipped | `mre-raster/src/lib.rs` |
| Whole-pixel baselines | shipped | `mre-raster/src/lib.rs` |
| Text weight correction (Skia's mask gamma) | shipped | `mre-raster/src/gamma.rs` |
| Optical centring (cap height, per style) | shipped | `mre-text/src/engine.rs` |
| Baseline alignment across a row | shipped | `mre-layout/src/engine.rs` |
| LCD subpixel rendering | not implemented | grayscale only |
| Variable font axes (`wght`, `opsz`) | not implemented | |

## 1. The stack

The text is drawn with the parts the browsers and Skia use, so that what the
engine draws can be compared with what a browser draws and comes out the
same.

| Step | What it uses | What it is for |
|---|---|---|
| Shaping | rustybuzz | the same shaper swash and cosmic-text use |
| Hinting | skrifa | FreeType's TrueType interpreter and FreeType's automatic hinter |
| Coverage | zeno | FreeType's smooth rasteriser: the exact area of the outline in each pixel |
| Weight | Skia's `SkMaskGamma` | the table that puts the coverage back where a blend in linear light would have left it |

Two things decide whether a word looks like one piece.

**The heights are fitted to the grid.** The hinter moves the outline's points
so that the x-height and the cap height land on whole pixels. The baseline is
put on a whole device pixel when the glyph is drawn, because the grid is only
the grid if the baseline is on it. Without this, a round letter overshoots the
baseline by a fraction of a pixel and lands a row below the flat letters
beside it, and one letter of a word looks a pixel above another.

**The widths are the design's.** The hinter is told to keep the horizontal
metrics linear, and the advance of every letter is left as the font gives it.
Rounding the advance would move each letter by up to half a pixel and the
error would pile up along the word; the pen's position is what is rounded, to
a sixteenth of a pixel, when the glyph is drawn. This is what a browser on
Linux does: HarfBuzz's advances are fractional and the position is rounded at
paint time.

The coverage is the exact area of the outline inside each pixel, not a
supersample of it. A scanline rasteriser with four scanlines to a pixel
gives a stem of uneven weights and soft edges; the exact area gives every
pixel the weight the shape really gives it.

The coverage then goes through the weight table, chosen by the colour's
luminance. The blend happens in encoded sRGB and the eye reads the result as
light, so light text on a dark background reads thinner than it is; the table
puts the coverage back where a blend in linear light would have left it.

## 2. The rules

* The font is the system's interface font, picked by name from a list of the
  families an interface uses, and only those that have a bold face of their
  own — a family with no bold of its own would have its bold text set in a
  different family from its regular text.
* The hinter runs at one times the scale. Above that there are enough pixels
  that the design is already crisp and the hinter only bends it.
* The baseline is on a whole device pixel; the pen's horizontal position is
  kept to a sixteenth of a pixel.
* The advance of a letter is never rounded.
* Text is centred by its cap height, not by its line box: the box is not the
  same shape as the ink on the page.
* The letters of a row share a baseline: the row's line is the text of its
  items and the first line of a stacked item, and the outermost item owns the
  line. The whole item moves, never the label alone.

## 3. How it is checked

The contract is checked on the pixels, not on the code.

`crates/mre-raster/tests/typography.rs` draws a word and reads the pixels
back:

* the letters of a word start on the same row and end on the same row;
* the text is the same whether it starts on a pixel or a third of a pixel
  above one.

`crates/mre-raster/src/glyph.rs` checks the coverage itself: a square is
covered outright, and an edge that falls in the middle of a pixel gives that
pixel half of its weight.

The golden image (`assets/golden/text-card.rgba`) is the reference card, byte
for byte. When the text metrics change, it is regenerated with
`MRE_UPDATE_GOLDEN=1 cargo test -p mre-raster golden_image`.

The comparison against FreeType is done by rendering the same string in the
same face at the same size with `convert -font ... label:...` and comparing
the ink runs and the gaps between the letters.
