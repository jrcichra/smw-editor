use egui_glow::glow::*;

/// Texture atlas of pre-decoded SNES 4bpp tiles.
///
/// All 512 Map16 block sub-tiles are decoded from VRAM/CGRAM on the CPU once,
/// packed into a single atlas texture.  Level tiles are then rendered as simple
/// textured quads that sample the atlas — no per-frame VRAM/CGRAM decoding.
pub struct TileAtlas {
    texture: Texture,
    /// How many tile slots are used (tiles decoded so far).
    tile_count: usize,
    destroyed: bool,
}

/// Number of 8×8 sub-tiles the atlas can hold (1024 tiles, 32 columns × 32 rows).
const ATLAS_COLS: usize = 32;
const ATLAS_ROWS: usize = 32;
const ATLAS_TILE_PX: usize = 8;
pub const ATLAS_TILES: usize = ATLAS_COLS * ATLAS_ROWS; // 1024
pub const ATLAS_W: usize = ATLAS_COLS * ATLAS_TILE_PX; // 256
pub const ATLAS_H: usize = ATLAS_ROWS * ATLAS_TILE_PX; // 256

impl TileAtlas {
    pub fn new(gl: &Context) -> Self {
        let texture = unsafe {
            let tex = gl.create_texture().expect("Failed to create atlas texture");
            gl.bind_texture(TEXTURE_2D, Some(tex));
            // Allocate empty atlas, filled by decode_and_upload()
            let empty = vec![0u8; ATLAS_W * ATLAS_H * 4];
            gl.tex_image_2d(
                TEXTURE_2D,
                0,
                RGBA8 as i32,
                ATLAS_W as i32,
                ATLAS_H as i32,
                0,
                RGBA,
                UNSIGNED_BYTE,
                Some(&empty),
            );
            gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_MIN_FILTER, NEAREST as i32);
            gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_MAG_FILTER, NEAREST as i32);
            gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_WRAP_S, CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_WRAP_T, CLAMP_TO_EDGE as i32);
            gl.bind_texture(TEXTURE_2D, None);
            tex
        };
        Self { texture, tile_count: 0, destroyed: false }
    }

    pub fn destroy(&mut self, gl: &Context) {
        if self.destroyed {
            return;
        }
        unsafe { gl.delete_texture(self.texture) }
        self.destroyed = true;
    }

    pub fn texture_id(&self) -> Texture {
        self.texture
    }

    /// Decode the given tile IDs from VRAM/CGRAM into the atlas and upload.
    /// `tiles`: list of (tile_word) values — the raw SNES tile attribute word.
    /// Returns a map from tile_word to atlas slot index.
    pub fn rebuild(
        &mut self, gl: &Context, vram: &[u8], cgram: &[u8], tile_words: &[u16],
    ) -> std::collections::HashMap<u16, usize> {
        let mut pixels = vec![0u8; ATLAS_W * ATLAS_H * 4];
        let mut slot_map = std::collections::HashMap::with_capacity(tile_words.len());
        let mut slot: usize = 0;

        // Deduplicate: only decode each unique tile_word once
        let mut seen = std::collections::HashSet::new();

        for &tw in tile_words {
            if !seen.insert(tw) || slot >= ATLAS_TILES {
                continue;
            }
            let col = slot % ATLAS_COLS;
            let row = slot / ATLAS_COLS;
            let x0 = col * ATLAS_TILE_PX;
            let y0 = row * ATLAS_TILE_PX;
            decode_tile_to_pixels(vram, cgram, tw, x0, y0, &mut pixels, ATLAS_W);
            slot_map.insert(tw, slot);
            slot += 1;
        }
        self.tile_count = slot;

        unsafe {
            gl.bind_texture(TEXTURE_2D, Some(self.texture));
            gl.tex_sub_image_2d(
                TEXTURE_2D,
                0,
                0,
                0,
                ATLAS_W as i32,
                ATLAS_H as i32,
                RGBA,
                UNSIGNED_BYTE,
                PixelUnpackData::Slice(&pixels),
            );
            gl.bind_texture(TEXTURE_2D, None);
        }
        slot_map
    }

    /// UV rectangle for a given atlas slot index: (u0, v0, u1, v1).
    pub fn slot_uv(slot: usize) -> (f32, f32, f32, f32) {
        let col = slot % ATLAS_COLS;
        let row = slot / ATLAS_COLS;
        let u0 = col as f32 * ATLAS_TILE_PX as f32 / ATLAS_W as f32;
        let v0 = row as f32 * ATLAS_TILE_PX as f32 / ATLAS_H as f32;
        let du = ATLAS_TILE_PX as f32 / ATLAS_W as f32;
        let dv = ATLAS_TILE_PX as f32 / ATLAS_H as f32;
        (u0, v0, u0 + du, v0 + dv)
    }
}

/// Decode a single SNES 4bpp 8×8 tile from VRAM into RGBA pixels.
fn decode_tile_to_pixels(
    vram: &[u8], cgram: &[u8], tile_word: u16, x0: usize, y0: usize, pixels: &mut [u8], stride: usize,
) {
    let tile_num = (tile_word & 0x3FF) as usize;
    let pal = ((tile_word >> 10) & 0x7) as usize;
    let flip_x = (tile_word & 0x4000) != 0;
    let flip_y = (tile_word & 0x8000) != 0;

    let tile_base = tile_num * 32;
    for ty in 0..8u32 {
        for tx in 0..8u32 {
            let px = if flip_x { 7 - tx } else { tx };
            let py = if flip_y { 7 - ty } else { ty };
            let row_off = tile_base + (py as usize) * 2;
            if row_off + 17 >= vram.len() {
                continue;
            }
            let b0 = vram[row_off];
            let b1 = vram[row_off + 1];
            let b2 = vram[row_off + 16];
            let b3 = vram[row_off + 17];
            let bit = 7 - px as usize;
            let color_idx =
                (((b0 >> bit) & 1) | (((b1 >> bit) & 1) << 1) | (((b2 >> bit) & 1) << 2) | (((b3 >> bit) & 1) << 3))
                    as usize;
            if color_idx == 0 {
                continue;
            }
            let pal_idx = pal * 16 + color_idx;
            let off_color = pal_idx * 2;
            if off_color + 1 >= cgram.len() {
                continue;
            }
            let lo = cgram[off_color] as u16;
            let hi = cgram[off_color + 1] as u16;
            let rgb = lo | (hi << 8);
            let r = ((rgb & 0x1F) << 3) as u8;
            let g = (((rgb >> 5) & 0x1F) << 3) as u8;
            let b = (((rgb >> 10) & 0x1F) << 3) as u8;

            let abs_x = x0 + tx as usize;
            let abs_y = y0 + ty as usize;
            let off = (abs_y * stride + abs_x) * 4;
            if off + 3 < pixels.len() {
                pixels[off] = r;
                pixels[off + 1] = g;
                pixels[off + 2] = b;
                pixels[off + 3] = 255;
            }
        }
    }
}
