use egui::{vec2, Color32, Slider, Ui};
use smwe_widgets::value_switcher::{ValueSwitcher, ValueSwitcherButtons};

use super::UiLevelEditor;
use crate::ui::editing_mode::EditingMode;

impl UiLevelEditor {
    pub(super) fn left_panel(&mut self, ui: &mut Ui) {
        ui.add_space(ui.spacing().item_spacing.y);
        ui.group(|ui| {
            ui.allocate_space(vec2(ui.available_width(), 0.));
            self.controls_panel(ui);
        });
    }

    fn controls_panel(&mut self, ui: &mut Ui) {
        let level_changed = {
            let switcher = ValueSwitcher::new(&mut self.level_num, "Level", ValueSwitcherButtons::MinusPlus)
                .range(0..=0x1FF)
                .hexadecimal(3, false, true);
            ui.add(switcher).changed()
        };
        if level_changed {
            self.load_level();
        }

        ui.add(Slider::new(&mut self.zoom, 1.0..=3.0).step_by(0.25).text("Zoom"));
        ui.checkbox(&mut self.always_show_grid, "Always show grid");
        ui.checkbox(&mut self.show_object_overlay, "Show object overlay");
        ui.checkbox(&mut self.show_object_labels, "Show object labels");

        // ── Editing mode toolbar ────────────────────────────
        ui.separator();
        ui.label("Mode:");
        ui.horizontal(|ui| {
            let modes = [
                ("Select [1]", EditingMode::Select),
                ("Draw [2]", EditingMode::Draw),
                ("Erase [3]", EditingMode::Erase),
                ("Probe [4]", EditingMode::Probe),
            ];
            for (label, mode) in modes {
                let active = self.editing_mode == mode;
                let fill = if active { Some(Color32::from_rgb(70, 130, 200)) } else { None };
                let btn = egui::Button::new(label);
                let btn = if let Some(f) = fill { btn.fill(f) } else { btn };
                if ui.add(btn).clicked() {
                    self.editing_mode = mode;
                }
            }
        });

        // ── Draw mode object picker ────────────────────────
        if self.editing_mode == EditingMode::Draw {
            ui.separator();
            ui.label("Place object:");
            ui.horizontal(|ui| {
                ui.label("ID:");
                let mut id = self.draw_object_id as u16;
                if ui.add(Slider::new(&mut id, 0..=0x3F).hexadecimal(2, false, false)).changed() {
                    self.draw_object_id = id as u8;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Size:");
                let mut w = (self.draw_object_settings & 0x0F) as u16 + 1;
                let mut h = (self.draw_object_settings >> 4) as u16 + 1;
                let mut changed = false;
                if ui.add(Slider::new(&mut w, 1..=16).prefix("W ")).changed() {
                    changed = true;
                }
                if ui.add(Slider::new(&mut h, 1..=16).prefix("H ")).changed() {
                    changed = true;
                }
                if changed {
                    self.draw_object_settings = ((h.saturating_sub(1) as u8) << 4) | (w.saturating_sub(1) as u8 & 0x0F);
                }
            });
        }

        ui.separator();
        ui.label(format!("Level {:03X}", self.level_num));
        let is_vertical = {
            let props = &self.level_properties;
            ui.label(format!("Mode: {:02X}  GFX: {:X}", props.level_mode, props.fg_bg_gfx));
            ui.label(format!("Music: {}  Timer: {}", props.music, props.timer));
            ui.label(if props.is_vertical { "Vertical" } else { "Horizontal" });
            ui.label(format!("Screens: {}", props.num_screens()));
            let (w, h) = props.level_dimensions_in_tiles();
            ui.label(format!("Size: {}x{} tiles", w, h));
            props.is_vertical
        };

        ui.separator();

        // Selected tile info
        if let Some((x, y)) = self.selected_tile {
            ui.label(format!("Tile: ({x}, {y})"));
            if let Some(block_id) = self.block_id_at(x, y) {
                ui.monospace(format!("  Block ID: {block_id:#04X}"));
                let screen = if is_vertical { y / 512 } else { x / 256 };
                ui.monospace(format!("  Screen: {screen:X}"));
            }
        }

        // Selected object properties
        if !self.selected_object_indices.is_empty() {
            ui.separator();
            ui.label("Selected Object:");

            // Read object data first
            let selected_data: Vec<_> = self.layer1.read(|layer| {
                self.selected_object_indices.iter().filter_map(|&i| layer.objects.get(i).copied()).collect()
            });

            if selected_data.len() == 1 {
                let obj = selected_data[0];
                // We need to edit, but can't borrow layer1 mutably while selected_data exists.
                // Drop selected_data first, then edit.
                ui.label(format!("  ID: {:02X}", if obj.is_extended { obj.extended_id } else { obj.id }));
                ui.label(format!("  Pos: ({}, {})", obj.x, obj.y));
                let w = if obj.is_extended { 1 } else { (obj.settings & 0x0F) + 1 };
                let h = if obj.is_extended { 1 } else { (obj.settings >> 4) + 1 };
                ui.label(format!("  Size: {}x{}", w, h));
                if obj.is_extended {
                    ui.label("  Extended");
                }

                // Editable fields
                drop(selected_data);

                let mut changed = false;
                let mut new_x = obj.x as i32;
                let mut new_y = obj.y as i32;
                let mut new_id = if obj.is_extended { obj.extended_id } else { obj.id } as i32;
                let mut new_w = w as i32;
                let mut new_h = h as i32;

                ui.horizontal(|ui| {
                    ui.label("X:");
                    changed |= ui.add(Slider::new(&mut new_x, 0..=4095)).changed();
                });
                ui.horizontal(|ui| {
                    ui.label("Y:");
                    changed |= ui.add(Slider::new(&mut new_y, 0..=511)).changed();
                });
                ui.horizontal(|ui| {
                    ui.label("ID:");
                    changed |= ui.add(Slider::new(&mut new_id, 0..=0xFF).hexadecimal(2, false, false)).changed();
                });
                if !obj.is_extended {
                    ui.horizontal(|ui| {
                        ui.label("W:");
                        changed |= ui.add(Slider::new(&mut new_w, 1..=16)).changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label("H:");
                        changed |= ui.add(Slider::new(&mut new_h, 1..=16)).changed();
                    });
                }

                if changed {
                    let indices: Vec<usize> = self.selected_object_indices.iter().copied().collect();
                    let idx = indices[0];
                    self.layer1.write(|layer| {
                        if let Some(obj) = layer.objects.get_mut(idx) {
                            obj.x = new_x as u32;
                            obj.y = new_y as u32;
                            if obj.is_extended {
                                obj.extended_id = new_id as u8;
                            } else {
                                obj.id = new_id as u8;
                                obj.settings =
                                    ((new_h.saturating_sub(1) as u8) << 4) | (new_w.saturating_sub(1) as u8 & 0x0F);
                            }
                        }
                    });
                }
            } else {
                ui.label(format!("  {} objects selected", selected_data.len()));
            }
        }
    }
}
