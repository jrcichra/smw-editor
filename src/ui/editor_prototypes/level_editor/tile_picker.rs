use egui::TextureHandle;
use smwe_emu::Cpu;

/// Number of Map16 blocks in the tileset (9-bit IDs: 0-511).
const NUM_BLOCKS: usize = 512;

/// Grid layout for the tile picker texture.
const COLS: usize = 16;
const ROWS: usize = NUM_BLOCKS / COLS; // 32

/// Size of each Map16 block in pixels (2×2 8×8 sub-tiles).
const BLOCK_PX: usize = 16;

/// Width/height of the full tile picker texture.
const TEX_W: usize = COLS * BLOCK_PX; // 256
const TEX_H: usize = ROWS * BLOCK_PX; // 512

pub(super) struct TilePicker {
    pixels: Vec<u8>, // RGBA, TEX_W * TEX_H * 4
    texture: Option<TextureHandle>,
    dirty: bool,
}

impl TilePicker {
    pub fn new() -> Self {
        Self { pixels: vec![0u8; TEX_W * TEX_H * 4], texture: None, dirty: true }
    }

    /// Decode all 512 Map16 blocks from the current VRAM/CGRAM state.
    pub fn rebuild(&mut self, cpu: &mut Cpu) {
        self.pixels.fill(0);

        let map16_bank = cpu.mem.cart.resolve("Map16Common").unwrap_or(0) & 0xFF0000;

        // Pre-compute all block pointers (needs mutable load_u16).
        let mut block_ptrs = [0u32; NUM_BLOCKS];
        for block_id in 0..NUM_BLOCKS {
            let ptr_lo = 0x0FBE + block_id * 2;
            if ptr_lo + 1 < 0x10000 {
                block_ptrs[block_id] = cpu.mem.load_u16(ptr_lo as u32) as u32 + map16_bank;
            }
        }

        let vram = &cpu.mem.vram;
        let cgram = &cpu.mem.cgram;

        for block_id in 0..NUM_BLOCKS {
            let col = block_id % COLS;
            let row = block_id / COLS;
            let x0 = (col * BLOCK_PX) as u32;
            let y0 = (row * BLOCK_PX) as u32;

            let block_ptr = block_ptrs[block_id];
            if block_ptr == map16_bank {
                continue;
            }

            let sub_offsets = [(0u32, 0u32), (0, 8), (8, 0), (8, 8)];
            for (sub_i, (sx, sy)) in sub_offsets.into_iter().enumerate() {
                let tile_word_addr = block_ptr + (sub_i as u32) * 2;
                let lo = cpu.mem.cart.read(tile_word_addr).unwrap_or(0);
                let hi = cpu.mem.cart.read(tile_word_addr + 1).unwrap_or(0);
                let t = lo as u16 | ((hi as u16) << 8);

                render_sub_tile(vram, cgram, t, x0 + sx, y0 + sy, &mut self.pixels);
            }
        }

        self.dirty = true;
    }

    /// Get the egui texture handle, creating or updating as needed.
    pub fn texture(&mut self, ctx: &egui::Context) -> TextureHandle {
        if self.texture.is_none() {
            let image = egui::ColorImage::from_rgba_unmultiplied([TEX_W, TEX_H], &self.pixels);
            let handle = ctx.load_texture("level_tile_picker", image, egui::TextureOptions::NEAREST);
            self.texture = Some(handle);
            self.dirty = false;
        }
        if self.dirty {
            let image = egui::ColorImage::from_rgba_unmultiplied([TEX_W, TEX_H], &self.pixels);
            self.texture.as_mut().unwrap().set(image, egui::TextureOptions::NEAREST);
            self.dirty = false;
        }
        self.texture.as_ref().unwrap().clone()
    }

    /// Convert a pixel position within the texture to a block ID (0-511).
    pub fn block_at_pixel(&self, px: f32, py: f32) -> Option<u16> {
        if px < 0.0 || py < 0.0 {
            return None;
        }
        let col = (px as usize) / BLOCK_PX;
        let row = (py as usize) / BLOCK_PX;
        if col < COLS && row < ROWS {
            let id = (row * COLS + col) as u16;
            if id < NUM_BLOCKS as u16 {
                Some(id)
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// Decode a single 8×8 SNES 4bpp tile from VRAM and write RGBA pixels.
fn render_sub_tile(vram: &[u8], cgram: &[u8], t: u16, x0: u32, y0: u32, pixels: &mut [u8]) {
    let tile_num = (t & 0x3FF) as usize;
    let pal = ((t >> 10) & 0x7) as usize;
    let flip_x = (t & 0x4000) != 0;
    let flip_y = (t & 0x8000) != 0;

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

            let px_abs = x0 + tx;
            let py_abs = y0 + ty;
            let off = ((py_abs as usize) * TEX_W + px_abs as usize) * 4;
            if off + 3 < pixels.len() {
                pixels[off] = r;
                pixels[off + 1] = g;
                pixels[off + 2] = b;
                pixels[off + 3] = 255;
            }
        }
    }
}
