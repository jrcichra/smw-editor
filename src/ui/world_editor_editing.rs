use egui::Pos2;

use super::world_editor::{
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
                        self.write_l1_tile(tx, ty, 0x00, 0x00);
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
                        let t1 = (self.draw_palette << 2) | (self.draw_tile_attr & 0xC0);
                        self.write_l1_tile(tx, ty, self.draw_tile_num, t1);
                        self.rebuild_and_upload();
                    }
                }
            }
            _ => {}
        }
    }

    fn write_l1_tile(&mut self, map16_x: u32, map16_y: u32, tile_num: u8, attr: u8) {
        let (crop_x, crop_y) = visible_map_crop(self.submap);
        let tile_x = (map16_x * 16 + crop_x) / 8;
        let tile_y = (map16_y * 16 + crop_y) / 8;
        let addr = tilemap_vram_addr(VRAM_L1_TILEMAP_BASE, tile_x, tile_y);
        if addr + 1 < self.cpu.mem.vram.len() {
            self.cpu.mem.vram[addr] = tile_num;
            self.cpu.mem.vram[addr + 1] = attr;
        }
    }

    #[allow(dead_code)]
    fn write_l2_tile(&mut self, tile_x: u32, tile_y: u32, tile_num: u8, attr: u8) {
        let addr = tilemap_vram_addr(VRAM_L2_TILEMAP_BASE, tile_x, tile_y);
        if addr + 1 < self.cpu.mem.vram.len() {
            self.cpu.mem.vram[addr] = tile_num;
            self.cpu.mem.vram[addr + 1] = attr;
        }
    }

    fn rebuild_and_upload(&mut self) {
        let l2_scroll_x = i16::from_le_bytes(self.cpu.mem.load_u16(0x001E).to_le_bytes()) as i32;
        let l2_scroll_y = i16::from_le_bytes(self.cpu.mem.load_u16(0x0020).to_le_bytes()) as i32;
        let l1 = build_bg_tiles(&self.cpu.mem.vram, VRAM_L1_TILEMAP_BASE, self.submap, l2_scroll_x, l2_scroll_y);
        let l2 = build_bg_tiles(&self.cpu.mem.vram, VRAM_L2_TILEMAP_BASE, self.submap, l2_scroll_x, l2_scroll_y);
        let mut r = self.renderer.lock().expect("Cannot lock renderer");
        r.set_tiles(&self.gl, l1, l2);
    }

    #[allow(dead_code)]
    pub(super) fn handle_undo(&mut self) {}
    #[allow(dead_code)]
    pub(super) fn handle_redo(&mut self) {}
}
