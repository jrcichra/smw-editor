use egui::Pos2;

use super::{object_layer::EditableObject, UiLevelEditor};
use crate::ui::editing_mode::EditingMode;

impl UiLevelEditor {
    pub(super) fn handle_editing_interaction(&mut self, resp: &egui::Response, origin: Pos2, tile_sz: f32) {
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

    fn object_at(&self, pos: Pos2, origin: Pos2, tile_sz: f32) -> Option<usize> {
        let rel = (pos - origin) / tile_sz;
        let tx = rel.x.floor();
        let ty = rel.y.floor();

        self.layer1.read(|layer| {
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
        if let Some(idx) = self.object_at(pos, origin, tile_sz) {
            // Read object bounds before deleting.
            let (ox, oy, ow, oh) = self.layer1.read(|layer| {
                let obj = &layer.objects[idx];
                let w = if obj.is_extended { 1 } else { (obj.settings & 0x0F) + 1 };
                let h = if obj.is_extended { 1 } else { (obj.settings >> 4) + 1 };
                (obj.x, obj.y, w as u32, h as u32)
            });

            // Delete the object.
            self.layer1.write(|layer| {
                layer.objects.remove(idx);
            });
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

        let new_idx = self.layer1.read(|layer| layer.objects.len());
        self.layer1.write(|layer| {
            layer.objects.push(new_obj);
        });
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
        if self.selected_object_indices.is_empty() {
            return;
        }
        // Read object bounds before deleting.
        let objects_to_blank: Vec<(u32, u32, u32, u32)> = self.layer1.read(|layer| {
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
        self.layer1.write(|layer| {
            let mut keep = Vec::with_capacity(layer.objects.len());
            for (i, obj) in layer.objects.drain(..).enumerate() {
                if !indices.contains(&i) {
                    keep.push(obj);
                }
            }
            layer.objects = keep;
        });
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
        self.layer1.undo();
        self.selected_object_indices.clear();
    }

    pub(super) fn handle_redo(&mut self) {
        self.layer1.redo();
        self.selected_object_indices.clear();
    }
}
