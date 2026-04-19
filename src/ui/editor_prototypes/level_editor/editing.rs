use egui::Pos2;

use super::{object_layer::EditableObject, UiLevelEditor};
use crate::ui::editing_mode::EditingMode;

impl UiLevelEditor {
    pub(super) fn handle_editing_interaction(&mut self, resp: &egui::Response, origin: Pos2, tile_sz: f32) {
        if self.edit_sprites {
            match self.editing_mode {
                EditingMode::Select | EditingMode::Probe => {
                    if resp.clicked_by(egui::PointerButton::Primary) {
                        if let Some(pos) = resp.hover_pos() {
                            self.select_sprite_at(pos, origin, tile_sz);
                        }
                    }
                }
                EditingMode::Erase => {
                    if resp.clicked_by(egui::PointerButton::Primary) {
                        if let Some(pos) = resp.hover_pos() {
                            self.erase_sprite_at(pos, origin, tile_sz);
                        }
                    }
                }
                EditingMode::Draw => {
                    if resp.clicked_by(egui::PointerButton::Primary) {
                        if let Some(pos) = resp.hover_pos() {
                            self.place_sprite_at(pos, origin, tile_sz);
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        match self.editing_mode {
            EditingMode::Select | EditingMode::Probe => {
                if resp.clicked_by(egui::PointerButton::Primary) {
                    if let Some(pos) = resp.hover_pos() {
                        self.select_object_at(pos, origin, tile_sz);
                    }
                }
            }
            EditingMode::Erase => {
                if resp.clicked_by(egui::PointerButton::Primary) {
                    if let Some(pos) = resp.hover_pos() {
                        self.erase_object_at(pos, origin, tile_sz);
                    }
                }
            }
            EditingMode::Draw => {
                if resp.clicked_by(egui::PointerButton::Primary) {
                    if let Some(pos) = resp.hover_pos() {
                        self.place_object_at(pos, origin, tile_sz);
                    }
                }
            }
            _ => {}
        }
    }

    fn sprite_at(&mut self, pos: Pos2, origin: Pos2, _tile_sz: f32) -> Option<usize> {
        let rel_px = (pos - origin) / self.zoom;
        let sprite_entries = self.sprites.read(|sprites| sprites.sprites.clone());
        for (i, spr) in sprite_entries.iter().enumerate().rev() {
            let (min_dx, min_dy, max_dx, max_dy) = self.sprite_pixel_bounds(spr.sprite_id).unwrap_or((0, 0, 16, 16));
            let left = spr.x as f32 * 16.0 + min_dx as f32;
            let top = spr.y as f32 * 16.0 + min_dy as f32;
            let right = spr.x as f32 * 16.0 + max_dx as f32;
            let bottom = spr.y as f32 * 16.0 + max_dy as f32;
            if rel_px.x >= left && rel_px.x < right && rel_px.y >= top && rel_px.y < bottom {
                return Some(i);
            }
        }
        None
    }

    fn select_sprite_at(&mut self, pos: Pos2, origin: Pos2, tile_sz: f32) {
        let idx = self.sprite_at(pos, origin, tile_sz);
        self.selected_sprite_indices.clear();
        if let Some(i) = idx {
            self.selected_sprite_indices.insert(i);
        }
    }

    fn erase_sprite_at(&mut self, pos: Pos2, origin: Pos2, tile_sz: f32) {
        if let Some(idx) = self.sprite_at(pos, origin, tile_sz) {
            self.sprites.write(|sprites| {
                sprites.sprites.remove(idx);
            });
            self.mark_edited();
            self.selected_sprite_indices.clear();
            self.rebuild_sprite_tiles();
        }
    }

    fn place_sprite_at(&mut self, pos: Pos2, origin: Pos2, tile_sz: f32) {
        let rel = (pos - origin) / tile_sz;
        let target_px_x = rel.x.floor() * 16.0;
        let target_px_y = rel.y.floor() * 16.0;
        let (min_dx, min_dy, _, _) = self.sprite_pixel_bounds(self.draw_sprite_id).unwrap_or((0, 0, 16, 16));
        let anchor_x = ((target_px_x - min_dx as f32) / 16.0).round().max(0.0) as u32;
        let anchor_y = ((target_px_y - min_dy as f32) / 16.0).round().max(0.0) as u32;
        let new_idx = self.sprites.read(|sprites| sprites.sprites.len());
        self.sprites.write(|sprites| {
            sprites.sprites.push(super::sprite_layer::EditableSprite {
                x: anchor_x,
                y: anchor_y,
                sprite_id: self.draw_sprite_id,
                extra_bits: self.draw_sprite_extra_bits,
            });
        });
        self.mark_edited();
        self.selected_sprite_indices.clear();
        self.selected_sprite_indices.insert(new_idx);
        self.rebuild_sprite_tiles();
    }

    fn object_at(&self, pos: Pos2, origin: Pos2, tile_sz: f32) -> Option<usize> {
        let rel = (pos - origin) / tile_sz;
        let tx = rel.x.floor();
        let ty = rel.y.floor();

        let Some(layer_data) = self.editing_objects() else {
            return None;
        };
        layer_data.read(|layer| {
            // Iterate in reverse so topmost (last-placed) objects are hit first.
            for (i, obj) in layer.objects.iter().enumerate().rev() {
                let w = if obj.is_extended { 1.0 } else { ((obj.settings & 0x0F) as f32) + 1.0 };
                let h = if obj.is_extended { 1.0 } else { ((obj.settings >> 4) as f32) + 1.0 };
                if tx >= obj.x as f32 && tx < obj.x as f32 + w && ty >= obj.y as f32 && ty < obj.y as f32 + h {
                    return Some(i);
                }
            }
            None
        })
    }

    fn select_object_at(&mut self, pos: Pos2, origin: Pos2, tile_sz: f32) {
        let idx = self.object_at(pos, origin, tile_sz);
        self.selected_object_indices.clear();
        if let Some(i) = idx {
            self.selected_object_indices.insert(i);
        }
    }

    fn erase_object_at(&mut self, pos: Pos2, origin: Pos2, tile_sz: f32) {
        if self.edit_layer == 2 && self.layer2_objects.is_none() {
            let rel = (pos - origin) / tile_sz;
            let tx = rel.x.floor() as u32;
            let ty = rel.y.floor() as u32;
            let idx = self.block_map_index(tx, ty) as usize;
            if let Some(bg) = &mut self.layer2_background {
                bg.write(|layer| {
                    if let Some(tile) = layer.tile_ids.get_mut(idx) {
                        *tile = 0;
                    }
                });
                self.mark_edited();
            }
            self.set_block_id_at(tx, ty, 0);
            self.rebuild_tiles();
            return;
        }
        if let Some(idx) = self.object_at(pos, origin, tile_sz) {
            // Read object bounds before deleting.
            let layer_data = self.editing_objects().expect("editable object layer missing");
            let (ox, oy, ow, oh) = layer_data.read(|layer| {
                let obj = &layer.objects[idx];
                let w = if obj.is_extended { 1 } else { (obj.settings & 0x0F) + 1 };
                let h = if obj.is_extended { 1 } else { (obj.settings >> 4) + 1 };
                (obj.x, obj.y, w as u32, h as u32)
            });

            // Delete the object.
            self.editing_objects_mut().expect("editable object layer missing").write(|layer| {
                layer.objects.remove(idx);
            });
            self.mark_edited();
            self.selected_object_indices.clear();

            // Blank out the tiles.
            for dy in 0..oh {
                for dx in 0..ow {
                    self.set_block_id_at(ox + dx, oy + dy, 0x25);
                }
            }
            self.rebuild_tiles();
        }
    }

    fn place_object_at(&mut self, pos: Pos2, origin: Pos2, tile_sz: f32) {
        let rel = (pos - origin) / tile_sz;
        let tx = rel.x.floor() as u32;
        let ty = rel.y.floor() as u32;

        if self.edit_layer == 2 && self.layer2_objects.is_none() {
            let idx = self.block_map_index(tx, ty) as usize;
            let draw_block = self.draw_block_id.min(0xFF) as u8;
            if let Some(bg) = &mut self.layer2_background {
                bg.write(|layer| {
                    if let Some(tile) = layer.tile_ids.get_mut(idx) {
                        *tile = draw_block;
                    }
                });
                self.mark_edited();
                self.set_block_id_at(tx, ty, draw_block as u16);
                self.rebuild_tiles();
            }
            return;
        }

        let w =
            if self.draw_object_settings & 0x0F == 0 { 1_u32 } else { ((self.draw_object_settings & 0x0F) + 1) as u32 };
        let h = if self.draw_object_settings >> 4 == 0 { 1_u32 } else { ((self.draw_object_settings >> 4) + 1) as u32 };

        let new_obj = EditableObject {
            x: tx,
            y: ty,
            id: self.draw_object_id,
            settings: self.draw_object_settings,
            is_extended: false,
            extended_id: 0,
        };

        let layer_data = self.editing_objects_mut().expect("editable object layer missing");
        let new_idx = layer_data.read(|layer| layer.objects.len());
        layer_data.write(|layer| {
            layer.objects.push(new_obj);
        });
        self.mark_edited();
        self.selected_object_indices.clear();
        self.selected_object_indices.insert(new_idx);

        // Write block IDs into the WRAM block map.
        let block_id = self.draw_block_id;
        for dy in 0..h {
            for dx in 0..w {
                self.set_block_id_at(tx + dx, ty + dy, block_id);
            }
        }
        self.rebuild_tiles();
    }

    pub(super) fn delete_selected_objects(&mut self) {
        if self.edit_sprites {
            if self.selected_sprite_indices.is_empty() {
                return;
            }
            let indices: Vec<usize> = self.selected_sprite_indices.iter().copied().collect();
            self.sprites.write(|sprites| {
                let mut keep = Vec::with_capacity(sprites.sprites.len());
                for (i, spr) in sprites.sprites.drain(..).enumerate() {
                    if !indices.contains(&i) {
                        keep.push(spr);
                    }
                }
                sprites.sprites = keep;
            });
            self.mark_edited();
            self.selected_sprite_indices.clear();
            self.rebuild_sprite_tiles();
            return;
        }
        if self.selected_object_indices.is_empty() {
            return;
        }
        if self.edit_layer == 2 && self.layer2_objects.is_none() {
            return;
        }
        // Read object bounds before deleting.
        let layer_data = self.editing_objects().expect("editable object layer missing");
        let objects_to_blank: Vec<(u32, u32, u32, u32)> = layer_data.read(|layer| {
            self.selected_object_indices
                .iter()
                .filter_map(|&i| layer.objects.get(i))
                .map(|obj| {
                    let w = if obj.is_extended { 1 } else { (obj.settings & 0x0F) as u32 + 1 };
                    let h = if obj.is_extended { 1 } else { (obj.settings >> 4) as u32 + 1 };
                    (obj.x, obj.y, w, h)
                })
                .collect()
        });

        // Collect indices and delete objects.
        let indices: Vec<usize> = self.selected_object_indices.iter().copied().collect();
        self.editing_objects_mut().expect("editable object layer missing").write(|layer| {
            let mut keep = Vec::with_capacity(layer.objects.len());
            for (i, obj) in layer.objects.drain(..).enumerate() {
                if !indices.contains(&i) {
                    keep.push(obj);
                }
            }
            layer.objects = keep;
        });
        self.mark_edited();
        self.selected_object_indices.clear();

        // Blank out the tiles.
        for (ox, oy, w, h) in objects_to_blank {
            for dy in 0..h {
                for dx in 0..w {
                    self.set_block_id_at(ox + dx, oy + dy, 0x25);
                }
            }
        }
        self.rebuild_tiles();
    }

    pub(super) fn handle_undo(&mut self) {
        if self.edit_sprites {
            self.sprites.undo();
            self.selected_sprite_indices.clear();
            self.rebuild_sprite_tiles();
            return;
        }
        if let Some(layer) = self.editing_objects_mut() {
            layer.undo();
        } else if let Some(bg) = &mut self.layer2_background {
            bg.undo();
        }
        self.selected_object_indices.clear();
    }

    pub(super) fn handle_redo(&mut self) {
        if self.edit_sprites {
            self.sprites.redo();
            self.selected_sprite_indices.clear();
            self.rebuild_sprite_tiles();
            return;
        }
        if let Some(layer) = self.editing_objects_mut() {
            layer.redo();
        } else if let Some(bg) = &mut self.layer2_background {
            bg.redo();
        }
        self.selected_object_indices.clear();
    }
}
