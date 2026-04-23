use egui::{Color32, Context, Rect, Sense, Slider, Vec2, vec2};

use super::{tile_picker::render_sub_tile, UiLevelEditor};

const PREVIEW_PX: usize = 32; // display size for each 8x8 sub-tile preview

impl UiLevelEditor {
    pub(super) fn map16_editor_window(&mut self, ctx: &Context) {
        if !self.show_map16_editor {
            return;
        }
        let mut open = self.show_map16_editor;
        egui::Window::new("Map16 Block Editor")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                // Block selector
                ui.horizontal(|ui| {
                    ui.label("Block:");
                    let mut bid = self.selected_map16_block_for_edit.unwrap_or(self.draw_block_id);
                    if ui.add(Slider::new(&mut bid, 0..=0x1FF).hexadecimal(3, false, true)).changed() {
                        self.selected_map16_block_for_edit = Some(bid);
                    }
                    if ui.small_button("Use draw block").clicked() {
                        self.selected_map16_block_for_edit = Some(self.draw_block_id);
                    }
                });

                let block_id = self.selected_map16_block_for_edit.unwrap_or(self.draw_block_id);

                // Get current tile words (from edits or ROM)
                let mut tile_words = self.get_block_tile_words(block_id);

                ui.separator();

                // Labels for sub-tile positions
                let sub_labels = ["Upper Left", "Lower Left", "Upper Right", "Lower Right"];
                let mut changed = false;

                for (sub_i, label) in sub_labels.iter().enumerate() {
                    ui.group(|ui| {
                        ui.label(*label);
                        let t = tile_words[sub_i];

                        // Render sub-tile preview
                        let mut pixels = vec![0u8; PREVIEW_PX * PREVIEW_PX * 4];
                        // Fill checkerboard background for transparency
                        for y in 0..PREVIEW_PX {
                            for x in 0..PREVIEW_PX {
                                let off = (y * PREVIEW_PX + x) * 4;
                                let checker = ((x / 4 + y / 4) % 2 == 0) as u8;
                                let shade = if checker == 0 { 64u8 } else { 96u8 };
                                pixels[off] = shade;
                                pixels[off + 1] = shade;
                                pixels[off + 2] = shade;
                                pixels[off + 3] = 255;
                            }
                        }
                        // Scale factor: PREVIEW_PX / 8 = 4
                        let scale = PREVIEW_PX / 8;
                        let mut raw_pixels = vec![0u8; 8 * 8 * 4];
                        render_sub_tile(&self.cpu.mem.vram, &self.cpu.mem.cgram, t, 0, 0, &mut raw_pixels, 8);
                        // Upscale into preview
                        for sy in 0..8usize {
                            for sx in 0..8usize {
                                let src = (sy * 8 + sx) * 4;
                                if raw_pixels[src + 3] > 0 {
                                    for dy in 0..scale {
                                        for dx in 0..scale {
                                            let dst = ((sy * scale + dy) * PREVIEW_PX + sx * scale + dx) * 4;
                                            pixels[dst] = raw_pixels[src];
                                            pixels[dst + 1] = raw_pixels[src + 1];
                                            pixels[dst + 2] = raw_pixels[src + 2];
                                            pixels[dst + 3] = 255;
                                        }
                                    }
                                }
                            }
                        }
                        let image =
                            egui::ColorImage::from_rgba_unmultiplied([PREVIEW_PX, PREVIEW_PX], &pixels);
                        let tex = ui.ctx().load_texture(
                            format!("map16_sub_{block_id}_{sub_i}_{t}"),
                            image,
                            egui::TextureOptions::NEAREST,
                        );
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::splat(PREVIEW_PX as f32),
                            Sense::hover(),
                        );
                        ui.painter().image(
                            tex.id(),
                            rect,
                            Rect::from_min_size(egui::pos2(0., 0.), vec2(1., 1.)),
                            Color32::WHITE,
                        );

                        // Tile word fields
                        let mut tile_num = (t & 0x3FF) as i32;
                        let mut palette = ((t >> 10) & 0x7) as i32;
                        let mut flip_x = (t & 0x4000) != 0;
                        let mut flip_y = (t & 0x8000) != 0;
                        let mut priority = (t & 0x2000) != 0;
                        let mut sub_changed = false;

                        ui.horizontal(|ui| {
                            ui.label("Tile:");
                            sub_changed |= ui
                                .add(Slider::new(&mut tile_num, 0..=0x3FF).hexadecimal(3, false, true))
                                .changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Palette:");
                            sub_changed |= ui.add(Slider::new(&mut palette, 0..=7)).changed();
                        });
                        ui.horizontal(|ui| {
                            sub_changed |= ui.checkbox(&mut flip_x, "Flip X").changed();
                            sub_changed |= ui.checkbox(&mut flip_y, "Flip Y").changed();
                            sub_changed |= ui.checkbox(&mut priority, "Priority").changed();
                        });
                        ui.monospace(format!("Word: {:04X}", t));

                        if sub_changed {
                            let new_t = (tile_num as u16 & 0x3FF)
                                | ((palette as u16 & 0x7) << 10)
                                | (if priority { 0x2000 } else { 0 })
                                | (if flip_x { 0x4000 } else { 0 })
                                | (if flip_y { 0x8000 } else { 0 });
                            tile_words[sub_i] = new_t;
                            changed = true;
                        }
                    });
                    if sub_i == 1 {
                        ui.separator();
                    }
                }

                if changed {
                    self.map16_edits.insert(block_id, tile_words);
                    self.mark_edited();
                }

                // Revert button
                if self.map16_edits.contains_key(&block_id) {
                    ui.separator();
                    if ui.button("Revert to ROM").clicked() {
                        self.map16_edits.remove(&block_id);
                    }
                }
            });
        self.show_map16_editor = open;
    }

    pub(super) fn get_block_tile_words(&self, block_id: u16) -> [u16; 4] {
        if let Some(&words) = self.map16_edits.get(&block_id) {
            return words;
        }
        if let Some(&snes_addr) = self.map16_block_ptrs.get(block_id as usize) {
            if snes_addr != 0 {
                use smwe_rom::snes_utils::addr::{AddrPc, AddrSnes};
                let rom_bytes = self.rom.disassembly.rom_bytes();
                if let Ok(pc) = AddrPc::try_from_lorom(AddrSnes(snes_addr)) {
                    let base = pc.as_index();
                    let mut words = [0u16; 4];
                    for (i, w) in words.iter_mut().enumerate() {
                        let off = base + i * 2;
                        if off + 1 < rom_bytes.len() {
                            *w = rom_bytes[off] as u16 | ((rom_bytes[off + 1] as u16) << 8);
                        }
                    }
                    return words;
                }
            }
        }
        [0u16; 4]
    }
}
