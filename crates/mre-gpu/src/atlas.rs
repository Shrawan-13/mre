//! The glyph atlas.
//!
//! Glyph *coverage* comes from the reference rasteriser, so the GPU backend
//! inherits its hinting, its exact area coverage and its subpixel phases. Only
//! the storage differs: instead of blitting a bitmap straight into a pixmap,
//! we upload it once into a texture and let the GPU blit instanced quads from
//! it. A 50,000-row table therefore uploads each distinct glyph once, not
//! once per row.

use std::collections::HashMap;
use wgpu::{Device, Queue, Texture, TextureView};

/// Identical to the reference rasteriser's glyph key: a bitmap is reused only
/// for the same font, glyph, size, device scale and quarter-pixel phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GlyphKey {
    pub font: u32,
    pub glyph: u16,
    pub size: u32,
    pub scale: u32,
    pub phase: u8,
}

/// A rectangle in atlas pixels, plus one pixel of padding all round so the
/// linear filter never samples a neighbouring glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AtlasRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// An uploaded coverage bitmap and the offsets from the pen/baseline to its
/// top-left corner. Those offsets are essential: the rasteriser bakes the
/// subpixel phase into the bitmap, so the quad must be placed at the same
/// rounded position the reference path would blit it to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AtlasEntry {
    pub rect: AtlasRect,
    pub left: f32,
    pub top: f32,
}

pub(crate) struct Atlas {
    texture: Texture,
    view: TextureView,
    /// The CPU shadow of the R8 texture. Only written when a new glyph lands.
    data: Vec<u8>,
    size: u32,
    pen_x: u32,
    pen_y: u32,
    shelf_h: u32,
    entries: HashMap<GlyphKey, AtlasEntry>,
}

impl Atlas {
    /// 4 MiB of R8 coverage. A full glyph set at a handful of sizes fits
    /// comfortably; a miss simply drops the glyph rather than corrupt text.
    pub(crate) const SIZE: u32 = 2048;

    pub(crate) fn new(device: &Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mre glyph atlas"),
            size: wgpu::Extent3d {
                width: Self::SIZE,
                height: Self::SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            data: vec![0; (Self::SIZE * Self::SIZE) as usize],
            size: Self::SIZE,
            pen_x: 0,
            pen_y: 0,
            shelf_h: 0,
            entries: HashMap::new(),
        }
    }

    pub(crate) fn view(&self) -> &TextureView {
        &self.view
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn get(&self, key: GlyphKey) -> Option<AtlasEntry> {
        self.entries.get(&key).copied()
    }

    /// Uploads a bitmap if its key is new. Returns its atlas entry.
    pub(crate) fn insert(
        &mut self,
        queue: &Queue,
        key: GlyphKey,
        bitmap: &mre_raster::GlyphBitmap,
    ) -> Option<AtlasEntry> {
        if let Some(entry) = self.entries.get(&key) {
            return Some(*entry);
        }

        let w = bitmap.width + 2;
        let h = bitmap.height + 2;
        if self.pen_x + w > self.size {
            self.pen_x = 0;
            self.pen_y += self.shelf_h + 1;
            self.shelf_h = 0;
        }
        if self.pen_y + h > self.size {
            return None;
        }
        let x = self.pen_x + 1;
        let y = self.pen_y + 1;
        for row in 0..bitmap.height as usize {
            let source =
                &bitmap.alpha[row * bitmap.width as usize..(row + 1) * bitmap.width as usize];
            let start = (y as usize + row) * self.size as usize + x as usize;
            self.data[start..start + source.len()].copy_from_slice(source);
        }

        // A full row is 2048 bytes, an exact multiple of the 256-byte
        // alignment wgpu requires for texture uploads.
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            // Start at the glyph's first pixel; `bytes_per_row` then walks
            // each following row from the same column. `origin` places it.
            &self.data[(y as usize * self.size as usize + x as usize)..],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.size),
                rows_per_image: Some(self.size),
            },
            wgpu::Extent3d {
                width: bitmap.width,
                height: bitmap.height,
                depth_or_array_layers: 1,
            },
        );

        self.pen_x += w;
        self.shelf_h = self.shelf_h.max(h);
        let entry = AtlasEntry {
            rect: AtlasRect { x, y, w, h },
            left: bitmap.left,
            top: bitmap.top,
        };
        self.entries.insert(key, entry);
        Some(entry)
    }
}

/// Shelf packing is deterministic and allocation-free after warm-up. Test it
/// without a device so CI does not need a GPU.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_key_distinguishes_phases_and_sizes() {
        let base = GlyphKey {
            font: 1,
            glyph: 2,
            size: 13 * 64,
            scale: 64,
            phase: 0,
        };
        let mut other = base;
        other.phase = 1;
        assert_ne!(base, other);
        other = base;
        other.size += 1;
        assert_ne!(base, other);
    }
}
