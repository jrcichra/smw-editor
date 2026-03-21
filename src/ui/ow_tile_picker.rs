use egui::TextureHandle;

/// Grid layout for the VRAM tile picker texture.
/// Show tiles 0x000-0x3FF (1024 tiles) in a 32×32 grid.
const COLS: usize = 32;
const ROWS: usize = 32;
const NUM_TILES: usize = COLS * ROWS;

/// Size of each 8×8 tile in pixels.
const TILE_PX: usize = 8;

/// Width/height of the full tile picker texture.
const TEX_W: usize = COLS * TILE_PX; // 256
const TEX_H: usize = ROWS * TILE_PX; // 256

pub(super) struct OwTilePicker {
    pixels: Vec<u8>, // RGBA
    dirty: bool,
    texture: Option<TextureHandle>,
}

impl OwTilePicker {
    pub fn new() -> Self {
        Self { pixels: vec![0u8; TEX_W * TEX_H * 4], dirty: true, texture: None }
    }

    /// Decode VRAM tiles into the pixel buffer using the given CGRAM palette.
    pub fn rebuild(&mut self, vram: &[u8], cgram: &[u8]) {
        self.pixels.fill(0);

        // Show the first 1024 tiles (32KB of VRAM) with palette row 0 for preview.
        // Users can see different palettes in the actual level rendering.
        let palette_row = 0;

        for tile_idx in 0..NUM_TILES {
            let col = tile_idx % COLS;
            let row = tile_idx / COLS;
            let x0 = (col * TILE_PX) as u32;
            let y0 = (row * TILE_PX) as u32;

            let tile_base = tile_idx * 32;
            if tile_base + 32 > vram.len() {
                continue;
            }

            for ty in 0..8u32 {
                for tx in 0..8u32 {
                    let row_off = tile_base + (ty as usize) * 2;
                    let b0 = vram[row_off];
                    let b1 = vram[row_off + 1];
                    let b2 = vram[row_off + 16];
                    let b3 = vram[row_off + 17];
                    let bit = 7 - tx as usize;
                    let color_idx = (((b0 >> bit) & 1)
                        | (((b1 >> bit) & 1) << 1)
                        | (((b2 >> bit) & 1) << 2)
                        | (((b3 >> bit) & 1) << 3)) as usize;

                    if color_idx == 0 {
                        continue;
                    }

                    let pal_idx = palette_row * 16 + color_idx;
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
                    if off + 3 < self.pixels.len() {
                        self.pixels[off] = r;
                        self.pixels[off + 1] = g;
                        self.pixels[off + 2] = b;
                        self.pixels[off + 3] = 255;
                    }
                }
            }
        }
        self.dirty = true;
    }

    pub fn texture(&mut self, ctx: &egui::Context) -> TextureHandle {
        if self.texture.is_none() {
            let image = egui::ColorImage::from_rgba_unmultiplied([TEX_W, TEX_H], &self.pixels);
            let handle = ctx.load_texture("ow_tile_picker", image, egui::TextureOptions::NEAREST);
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

    pub fn tile_at_pixel(&self, px: f32, py: f32) -> Option<u8> {
        if px < 0.0 || py < 0.0 {
            return None;
        }
        let col = (px as usize) / TILE_PX;
        let row = (py as usize) / TILE_PX;
        if col < COLS && row < ROWS {
            let id = (row * COLS + col) as u16;
            if id < 256 {
                Some(id as u8)
            } else {
                None
            }
        } else {
            None
        }
    }
}
