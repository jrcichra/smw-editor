use egui::Pos2;

use super::{
    build_bg_tiles, tilemap_vram_addr, visible_map_crop, UiWorldEditor, VRAM_L1_TILEMAP_BASE, VRAM_L2_TILEMAP_BASE,
};
use crate::ui::editing_mode::EditingMode;

impl UiWorldEditor {
    pub(super) fn handle_editing_interaction(&mut self, resp: &egui::Response, origin: Pos2, tile_sz: f32) {
        match self.editing_mode {
            EditingMode::Select | EditingMode::Probe => {
                if resp.clicked_by(egui::PointerButton::Primary) {
                    if let Some(pos) = resp.hover_pos() {
                        let rel = (pos - origin) / tile_sz;
                        let tx = rel.x.floor() as u32;
                        let ty = rel.y.floor() as u32;
                        self.selected_tile = Some((tx, ty));
                    }
                }
            }
            EditingMode::Erase => {
                if resp.clicked_by(egui::PointerButton::Primary) {
                    if let Some(pos) = resp.hover_pos() {
                        let rel = (pos - origin) / tile_sz;
                        let tx = rel.x.floor() as u32;
                        let ty = rel.y.floor() as u32;
                        if self.edit_layer == 1 {
                            self.set_source_l1_tile_at_view(tx, ty, 0x00);
                        } else {
                            self.write_tile(tx, ty, 0x00, 0x00);
                        }
                        self.rebuild_and_upload();
                    }
                }
            }
            EditingMode::Draw => {
                if resp.clicked_by(egui::PointerButton::Primary) {
                    if let Some(pos) = resp.hover_pos() {
                        let rel = (pos - origin) / tile_sz;
                        let tx = rel.x.floor() as u32;
                        let ty = rel.y.floor() as u32;
                        if self.edit_layer == 1 {
                            self.set_source_l1_tile_at_view(tx, ty, self.draw_tile_num);
                        } else {
                            let t1 = (self.draw_palette << 2) | (self.draw_tile_attr & 0xC0);
                            self.write_tile(tx, ty, self.draw_tile_num, t1);
                        }
                        self.rebuild_and_upload();
                    }
                }
            }
            _ => {}
        }
    }

    fn write_tile(&mut self, map16_x: u32, map16_y: u32, tile_num: u8, attr: u8) {
        let base = if self.edit_layer == 2 { VRAM_L2_TILEMAP_BASE } else { VRAM_L1_TILEMAP_BASE };
        if self.edit_layer == 2 {
            let addr = tilemap_vram_addr(base, map16_x, map16_y);
            let word = u16::from_le_bytes([tile_num, attr]);
            let idx = (addr.saturating_sub(VRAM_L2_TILEMAP_BASE)) / 2;
            self.edit_state.write(|s| {
                if let Some(slot) = s.layer2_words.get_mut(idx) {
                    *slot = word;
                }
            });
            self.has_edits = true;
            let wram_base = (0x7F4000 - 0x7E0000) as usize;
            let wram_addr = wram_base + idx * 2;
            if wram_addr + 1 < self.cpu.mem.wram.len() {
                let [lo, hi] = word.to_le_bytes();
                self.cpu.mem.wram[wram_addr] = lo;
                self.cpu.mem.wram[wram_addr + 1] = hi;
            }
        }
        if base == VRAM_L2_TILEMAP_BASE {
            let addr = tilemap_vram_addr(base, map16_x, map16_y);
            if addr + 1 < self.cpu.mem.vram.len() {
                self.cpu.mem.vram[addr] = tile_num;
                self.cpu.mem.vram[addr + 1] = attr;
            }
        } else {
            let (crop_x, crop_y) = visible_map_crop(self.submap);
            let tile_x = (map16_x * 16 + crop_x) / 8;
            let tile_y = (map16_y * 16 + crop_y) / 8;
            let addr = tilemap_vram_addr(base, tile_x, tile_y);
            if addr + 1 < self.cpu.mem.vram.len() {
                self.cpu.mem.vram[addr] = tile_num;
                self.cpu.mem.vram[addr + 1] = attr;
            }
        }
    }

    pub(super) fn rebuild_and_upload(&mut self) {
        self.has_edits = true;
        self.upload_tiles_from_vram();
    }

    fn upload_tiles_from_vram(&mut self) {
        let l2_scroll_x = i16::from_le_bytes(self.cpu.mem.load_u16(0x001E).to_le_bytes()) as i32;
        let l2_scroll_y = i16::from_le_bytes(self.cpu.mem.load_u16(0x0020).to_le_bytes()) as i32;
        let l1 = build_bg_tiles(&self.cpu.mem.vram, VRAM_L1_TILEMAP_BASE, self.submap, l2_scroll_x, l2_scroll_y);
        let l2 = build_bg_tiles(&self.cpu.mem.vram, VRAM_L2_TILEMAP_BASE, self.submap, l2_scroll_x, l2_scroll_y);
        let mut r = self.renderer.lock().expect("Cannot lock renderer");
        r.set_tiles(&self.gl, l1, l2);
    }

    pub(super) fn handle_undo(&mut self) {
        self.edit_state.undo();
        self.has_edits = self.edit_state.can_undo();
        self.sync_vram_from_edit_state();
        self.upload_tiles_from_vram();
    }

    pub(super) fn handle_redo(&mut self) {
        self.edit_state.redo();
        self.has_edits = true;
        self.sync_vram_from_edit_state();
        self.upload_tiles_from_vram();
    }

    fn sync_vram_from_edit_state(&mut self) {
        let layer1_tiles = self.edit_state.read(|s| s.layer1_tiles.clone());
        let offset = if self.submap == 0 { 0usize } else { 0x400 };
        let n = (layer1_tiles.len().saturating_sub(offset)).min(0x400);
        for idx in 0..n {
            let col = (idx % 32) as u32;
            let row = (idx / 32) as u32;
            let tile_id = layer1_tiles[offset + idx];
            self.write_source_l1_block_words(col, row, tile_id);
        }

        let layer2_words = self.edit_state.read(|s| s.layer2_words.clone());
        let wram_base = (0x7F4000 - 0x7E0000) as usize;
        for (idx, word) in layer2_words.iter().enumerate() {
            let wram_addr = wram_base + idx * 2;
            if wram_addr + 1 < self.cpu.mem.wram.len() {
                let [lo, hi] = word.to_le_bytes();
                self.cpu.mem.wram[wram_addr] = lo;
                self.cpu.mem.wram[wram_addr + 1] = hi;
            }
            let vram_addr = VRAM_L2_TILEMAP_BASE + idx * 2;
            if vram_addr + 1 < self.cpu.mem.vram.len() {
                let [lo, hi] = word.to_le_bytes();
                self.cpu.mem.vram[vram_addr] = lo;
                self.cpu.mem.vram[vram_addr + 1] = hi;
            }
        }
    }
}
