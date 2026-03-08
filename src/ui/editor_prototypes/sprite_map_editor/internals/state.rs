use egui::Context;
use smwe_rom::graphics::palette::ColorPalette;

use super::super::UiSpriteMapEditor;

impl UiSpriteMapEditor {
    pub(in super::super) fn reset_state(&mut self, ctx: &Context) {
        if self.state_needs_reset {
            self.update_renderers();
            self.pixels_per_point = ctx.pixels_per_point();
            self.state_needs_reset = false;
        }
    }

    pub(in super::super) fn update_cpu(&mut self) {
        // Previously ran the emulator to decompress level data.
        // Now data is loaded directly from the ROM.
        self.update_renderers();
    }

    pub(in super::super) fn update_renderers(&mut self) {
        // Build 512-byte CGRAM from level palette
        let level_idx = self.level_num as usize;
        let palette_result = self
            .rom
            .levels
            .get(level_idx)
            .and_then(|lvl| self.rom.gfx.color_palettes.get_level_palette(&lvl.primary_header).ok());

        let mut cgram = vec![0u8; 512];
        if let Some(palette) = palette_result {
            for row in 0..=0xF_usize {
                for col in 0..=0xF_usize {
                    let color = palette
                        .get_color_at(row, col)
                        .unwrap_or(smwe_rom::graphics::palette::ColorPalettes::TRANSPARENT);
                    let idx = (row * 16 + col) * 2;
                    let le = color.0.to_le_bytes();
                    cgram[idx] = le[0];
                    cgram[idx + 1] = le[1];
                }
            }
        }
        self.gfx_bufs.upload_palette(&self.gl, &cgram);

        // Build 0x10000-byte VRAM from decoded GFX files (4bpp planar)
        let mut vram = vec![0u8; 0x10000];
        for (file_slot, gfx_file) in self.rom.gfx.files.iter().enumerate() {
            let base = file_slot * 0x80 * 32;
            for (tile_idx, tile) in gfx_file.tiles.iter().enumerate().take(0x80) {
                let tile_base = base + tile_idx * 32;
                if tile_base + 32 > vram.len() {
                    break;
                }
                for row in 0..8_usize {
                    let mut p0 = 0u8;
                    let mut p1 = 0u8;
                    let mut p2 = 0u8;
                    let mut p3 = 0u8;
                    for col in 0..8_usize {
                        let ci = tile.color_indices[row * 8 + col];
                        let bit = 7 - col;
                        p0 |= ((ci >> 0) & 1) << bit;
                        p1 |= ((ci >> 1) & 1) << bit;
                        p2 |= ((ci >> 2) & 1) << bit;
                        p3 |= ((ci >> 3) & 1) << bit;
                    }
                    vram[tile_base + row * 2 + 0] = p0;
                    vram[tile_base + row * 2 + 1] = p1;
                    vram[tile_base + row * 2 + 16] = p2;
                    vram[tile_base + row * 2 + 17] = p3;
                }
            }
        }
        self.gfx_bufs.upload_vram(&self.gl, &vram);
    }

    pub(in super::super) fn upload_tiles(&self) {
        self.sprite_renderer
            .lock()
            .expect("Cannot lock mutex on sprite renderer")
            .set_tiles(&self.gl, self.sprite_tiles.read(|tiles| tiles.0.clone()));
    }

    pub(in super::super) fn update_tile_palette(&mut self) {
        for tile in self.tile_palette.iter_mut() {
            tile[3] &= 0xC0FF;
            tile[3] |= (self.selected_palette + 8) << 8;
        }
        self.vram_renderer
            .lock()
            .expect("Cannot lock mutex on VRAM renderer")
            .set_tiles(&self.gl, self.tile_palette.clone());
    }
}
