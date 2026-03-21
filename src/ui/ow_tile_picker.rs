use egui::TextureHandle;

/// Grid layout for the overworld tile picker.
const COLS: usize = 16;
const TILE_PX: usize = 16; // Display each sub-tile at 16×16 for visibility

pub(super) struct OwTilePicker {
    pixels: Vec<u8>,            // RGBA
    used_tiles: Vec<(u16, u8)>, // (tile_num, palette_row) — unique tiles used by this submap
    texture: Option<TextureHandle>,
    tex_w: usize,
    tex_h: usize,
}

impl OwTilePicker {
    pub fn new() -> Self {
        Self { pixels: Vec::new(), used_tiles: Vec::new(), texture: None, tex_w: 0, tex_h: 0 }
    }

    /// Scan the L1 and L2 tilemaps to find unique tile+palette combos, then render them.
    pub fn rebuild(&mut self, vram: &[u8], cgram: &[u8], l1_base: usize, l2_base: usize) {
        // Collect unique (tile_num, palette_row) from both tilemaps
        let mut seen = std::collections::HashSet::new();
        self.used_tiles.clear();

        for base in [l1_base, l2_base] {
            for row in 0..64u32 {
                for col in 0..64u32 {
                    let addr = tilemap_addr(base, col, row);
                    if addr + 1 >= vram.len() {
                        continue;
                    }
                    let t0 = vram[addr] as u16;
                    let t1 = vram[addr + 1] as u16;
                    let tile_num = t0 | ((t1 & 3) << 8);
                    let pal = ((t1 >> 2) & 7) as u8;
                    let key = (tile_num, pal);
                    if !seen.contains(&key) {
                        seen.insert(key);
                        self.used_tiles.push(key);
                    }
                }
            }
        }

        // Sort by tile number for consistent display
        self.used_tiles.sort_by_key(|(t, p)| (*t, *p));

        // Render into a grid
        let rows = ((self.used_tiles.len() + COLS - 1) / COLS).max(1);
        self.tex_w = COLS * TILE_PX;
        self.tex_h = rows * TILE_PX;
        self.pixels = vec![0u8; self.tex_w * self.tex_h * 4];

        for (i, &(tile_num, pal_row)) in self.used_tiles.iter().enumerate() {
            let col = i % COLS;
            let row = i / COLS;
            let x0 = (col * TILE_PX) as u32;
            let y0 = (row * TILE_PX) as u32;

            let tile_base = (tile_num as usize) * 32;
            if tile_base + 32 > vram.len() {
                continue;
            }

            // Render 8×8 sub-tile scaled to 16×16 (2x nearest neighbor)
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

                    let pal_idx = (pal_row as usize) * 16 + color_idx;
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

                    // 2× nearest neighbor
                    for dy in 0..2u32 {
                        for dx in 0..2u32 {
                            let px = x0 + tx * 2 + dx;
                            let py = y0 + ty * 2 + dy;
                            let off = ((py as usize) * self.tex_w + px as usize) * 4;
                            if off + 3 < self.pixels.len() {
                                self.pixels[off] = r;
                                self.pixels[off + 1] = g;
                                self.pixels[off + 2] = b;
                                self.pixels[off + 3] = 255;
                            }
                        }
                    }
                }
            }
        }
        self.texture = None;
    }

    pub fn texture(&mut self, ctx: &egui::Context) -> TextureHandle {
        if self.pixels.is_empty() {
            // Return a dummy 1×1 texture
            let img = egui::ColorImage::from_rgba_unmultiplied([1, 1], &[0, 0, 0, 0]);
            return ctx.load_texture("ow_tile_picker_empty", img, egui::TextureOptions::NEAREST);
        }
        if self.texture.is_none() {
            let image = egui::ColorImage::from_rgba_unmultiplied([self.tex_w, self.tex_h], &self.pixels);
            self.texture = Some(ctx.load_texture("ow_tile_picker", image, egui::TextureOptions::NEAREST));
        }
        self.texture.as_ref().unwrap().clone()
    }

    /// Get the (tile_num, palette) at a pixel position in the picker.
    pub fn tile_at_pixel(&self, px: f32, py: f32) -> Option<(u8, u8)> {
        if px < 0.0 || py < 0.0 || self.used_tiles.is_empty() {
            return None;
        }
        let col = (px as usize) / TILE_PX;
        let row = (py as usize) / TILE_PX;
        let idx = row * COLS + col;
        if idx < self.used_tiles.len() {
            let (tile_num, pal) = self.used_tiles[idx];
            Some((tile_num as u8, pal))
        } else {
            None
        }
    }
}

fn tilemap_addr(base: usize, col: u32, row: u32) -> usize {
    let quadrant = ((row / 32) * 2) + (col / 32);
    let sub_row = row % 32;
    let sub_col = col % 32;
    let quadrant_offset = quadrant * 32 * 32 * 2;
    base + (quadrant_offset + ((sub_row * 32 + sub_col) * 2)) as usize
}
