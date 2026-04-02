use egui::{vec2, Color32, Rect, Sense, Slider, Ui};
use smwe_widgets::value_switcher::{ValueSwitcher, ValueSwitcherButtons};

use super::UiLevelEditor;
use crate::ui::editing_mode::EditingMode;

impl UiLevelEditor {
    pub(super) fn left_panel(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(ui.spacing().item_spacing.y);
            ui.group(|ui| {
                ui.allocate_space(vec2(ui.available_width(), 0.));
                self.controls_panel(ui);
            });
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
        ui.checkbox(&mut self.show_sprite_overlay, "Show sprite overlay");
        ui.checkbox(&mut self.show_object_labels, "Show object labels");

        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Edit:");
            for (label, sprites) in [("Objects", false), ("Sprites", true)] {
                let active = self.edit_sprites == sprites;
                let fill = if active { Some(Color32::from_rgb(70, 130, 200)) } else { None };
                let btn = egui::Button::new(label);
                let btn = if let Some(f) = fill { btn.fill(f) } else { btn };
                if ui.add(btn).clicked() {
                    self.edit_sprites = sprites;
                    self.selected_object_indices.clear();
                    self.selected_sprite_indices.clear();
                }
            }
        });

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

        // ── Layer selector ──────────────────────────────────
        ui.horizontal(|ui| {
            ui.label("Layer:");
            let modes = [("L1", 1u8), ("L2", 2u8)];
            for (label, layer) in modes {
                let active = self.edit_layer == layer;
                let fill = if active { Some(Color32::from_rgb(70, 130, 200)) } else { None };
                let btn = egui::Button::new(label);
                let btn = if let Some(f) = fill { btn.fill(f) } else { btn };
                if ui.add(btn).clicked() {
                    self.edit_layer = layer;
                    self.preview_for = None; // Force preview refresh
                }
            }
            if !self.level_properties.has_layer2 {
                ui.weak("(no L2)");
            }
        });

        // ── Draw mode tile picker ──────────────────────────
        if self.editing_mode == EditingMode::Draw && self.edit_sprites {
            ui.separator();
            ui.label("Place sprite:");
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Sprite: {:#04X} {}",
                    self.draw_sprite_id,
                    super::sprite_catalog::sprite_name(self.draw_sprite_id)
                ));
                let mut sid = self.draw_sprite_id as u16;
                if ui.add(Slider::new(&mut sid, 0..=0xFF).hexadecimal(2, false, false).show_value(false)).changed() {
                    self.draw_sprite_id = sid as u8;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Extra bits:");
                let mut extra = self.draw_sprite_extra_bits as u16;
                if ui.add(Slider::new(&mut extra, 0..=3)).changed() {
                    self.draw_sprite_extra_bits = extra as u8;
                }
            });
            ui.add(egui::TextEdit::singleline(&mut self.sprite_search).hint_text("Search sprites"));
            self.sprite_picker(ui);
        } else if self.editing_mode == EditingMode::Draw {
            ui.separator();
            ui.label("Paint block:");
            ui.horizontal(|ui| {
                ui.label(format!("Selected: {:#05X}", self.draw_block_id));
                // Small inline slider for precise control
                let mut bid = self.draw_block_id;
                if ui.add(Slider::new(&mut bid, 0..=0x1FF).hexadecimal(3, false, false).show_value(false)).changed() {
                    self.draw_block_id = bid;
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

            let bg_l2_mode = self.edit_layer == 2 && self.layer2_objects.is_none();
            if bg_l2_mode {
                ui.small("Layer 2 background mode uses background tile IDs and repeats across the background strip.");
            }

            let tex = if bg_l2_mode { self.bg_tile_picker.texture(ui.ctx()) } else { self.tile_picker.texture(ui.ctx()) };
            let tex_size = tex.size();
            let max_w = ui.available_width().min(300.0);
            let display_w = max_w.min(tex_size[0] as f32 * 2.0);
            let display_h = display_w * (tex_size[1] as f32 / tex_size[0] as f32);
            let (rect, resp) = ui.allocate_exact_size(vec2(display_w, display_h), Sense::click());
            ui.painter().image(
                tex.id(),
                rect,
                Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(1.0, 1.0)),
                Color32::WHITE,
            );

            // Click to select a block
            if resp.clicked_by(egui::PointerButton::Primary) {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let rel = pos - rect.min;
                    let px = rel.x / display_w * tex_size[0] as f32;
                    let py = rel.y / display_h * tex_size[1] as f32;
                    let block_id = if bg_l2_mode {
                        self.bg_tile_picker.block_at_pixel(px, py)
                    } else {
                        self.tile_picker.block_at_pixel(px, py)
                    };
                    if let Some(block_id) = block_id {
                        self.draw_block_id = block_id;
                    }
                }
            }

            // Highlight the selected block
            let block_px = tex_size[0] as f32 / 16.0; // pixels per block in texture
            let (sel_col, sel_row) = if bg_l2_mode {
                self.bg_tile_picker
                    .block_grid_pos(self.draw_block_id.min(0xFF) as u8)
                    .unwrap_or((self.draw_block_id as usize % 16, self.draw_block_id as usize / 16))
            } else {
                (self.draw_block_id as usize % 16, self.draw_block_id as usize / 16)
            };
            let scale_x = display_w / tex_size[0] as f32;
            let scale_y = display_h / tex_size[1] as f32;
            let sel_rect = Rect::from_min_size(
                rect.min + vec2(sel_col as f32 * block_px * scale_x, sel_row as f32 * block_px * scale_y),
                vec2(block_px * scale_x, block_px * scale_y),
            );
            ui.painter().rect_stroke(sel_rect, egui::Rounding::ZERO, egui::Stroke::new(2.0, Color32::YELLOW));
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

        // ── Tile preview ────────────────────────────────────
        // In draw mode, show the draw block. Otherwise, show the selected tile.
        let preview_block = if self.editing_mode == EditingMode::Draw && !self.edit_sprites {
            Some(("Paint", self.draw_block_id, 0xFFFF_FFFF)) // sentinel for draw mode
        } else if let Some((x, y)) = self.selected_tile {
            self.block_id_at(x, y).map(|bid| ("Tile", bid, ((x & 0xFFF) | ((y & 0xFFF) << 12)) as u32))
        } else {
            None
        };

        if let Some((label, block_id, cache_key)) = preview_block {
            ui.label(format!("{label}: {block_id:#05X}"));
            if self.preview_for.map(|(x, y)| (x as u32) | ((y as u32) << 16)) != Some(cache_key) {
                let image = if self.edit_layer == 2 && self.layer2_objects.is_none() {
                    super::tile_picker::render_bg_block_image(block_id.min(0xFF) as u8, &mut self.cpu)
                } else {
                    super::tile_picker::render_block_image(block_id, &mut self.cpu)
                };
                let handle =
                    ui.ctx().load_texture(format!("block_preview_{cache_key}"), image, egui::TextureOptions::NEAREST);
                self.preview_texture = Some(handle);
                self.preview_for = Some((cache_key & 0xFFFF, cache_key >> 16));
            }
            if let Some(ref tex) = self.preview_texture {
                let display_size = 64.0;
                let (rect, _) = ui.allocate_exact_size(vec2(display_size, display_size), Sense::hover());
                ui.painter().image(
                    tex.id(),
                    rect,
                    Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }

        // Selected tile details (position, screen)
        if let Some((x, y)) = self.selected_tile {
            ui.monospace(format!("Pos: ({x}, {y}) [L{}]", self.edit_layer));
            if let Some(block_id) = self.block_id_at(x, y) {
                ui.monospace(format!("  Block: {block_id:#04X}"));
                let screen = if is_vertical { y / 512 } else { x / 256 };
                ui.monospace(format!("  Screen: {screen:X}"));
            }
        }

        // Selected object properties
        if self.edit_sprites && !self.selected_sprite_indices.is_empty() {
            ui.separator();
            ui.label("Selected Sprite:");
            let selected = self.sprites.read(|sprites| {
                self.selected_sprite_indices.iter().filter_map(|&i| sprites.sprites.get(i).copied()).collect::<Vec<_>>()
            });
            if selected.len() == 1 {
                let spr = selected[0];
                ui.label(format!("  ID: {:02X} {}", spr.sprite_id, super::sprite_catalog::sprite_name(spr.sprite_id)));
                ui.label(format!("  Pos: ({}, {})", spr.x, spr.y));
                ui.label(format!("  Extra bits: {}", spr.extra_bits));

                let mut changed = false;
                let mut new_x = spr.x as i32;
                let mut new_y = spr.y as i32;
                let mut new_id = spr.sprite_id as i32;
                let mut new_extra = spr.extra_bits as i32;

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
                ui.horizontal(|ui| {
                    ui.label("Extra:");
                    changed |= ui.add(Slider::new(&mut new_extra, 0..=3)).changed();
                });

                if changed {
                    let indices: Vec<usize> = self.selected_sprite_indices.iter().copied().collect();
                    let idx = indices[0];
                    self.sprites.write(|sprites| {
                        if let Some(spr) = sprites.sprites.get_mut(idx) {
                            spr.x = new_x.max(0) as u32;
                            spr.y = new_y.max(0) as u32;
                            spr.sprite_id = new_id as u8;
                            spr.extra_bits = new_extra as u8;
                        }
                    });
                    self.rebuild_sprite_tiles();
                }
            }
        } else if !self.selected_object_indices.is_empty() {
            ui.separator();
            ui.label("Selected Object:");

            // Read object data first
            let Some(layer_data) = self.editing_objects() else {
                ui.label("  No object-backed data on this layer");
                return;
            };
            let selected_data: Vec<_> = layer_data.read(|layer| {
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
                    self.editing_objects_mut().expect("editable object layer missing").write(|layer| {
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

    fn sprite_picker(&mut self, ui: &mut Ui) {
        let selected_preview = self.sprite_preview_texture(ui.ctx(), self.draw_sprite_id);
        ui.horizontal(|ui| {
            ui.image((selected_preview.id(), vec2(32.0, 32.0)));
            ui.vertical(|ui| {
                ui.monospace(format!("ID {:02X}", self.draw_sprite_id));
                ui.small(super::sprite_catalog::sprite_name(self.draw_sprite_id));
            });
        });

        let mut visible_ids = Vec::new();
        for id in 0u8..=0xFF {
            if super::sprite_catalog::sprite_matches_search(id, &self.sprite_search) {
                visible_ids.push(id);
            }
        }

        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for chunk in visible_ids.chunks(2) {
                ui.columns(2, |cols| {
                    for (col, id) in chunk.iter().copied().enumerate() {
                        let selected = self.draw_sprite_id == id;
                        cols[col].group(|ui| {
                            let preview = self.sprite_preview_texture(ui.ctx(), id);
                            ui.horizontal(|ui| {
                                let image = egui::ImageButton::new((preview.id(), vec2(32.0, 32.0))).selected(selected);
                                if ui.add(image).clicked() {
                                    self.draw_sprite_id = id;
                                }
                                ui.vertical(|ui| {
                                    if ui.selectable_label(selected, format!("{id:02X}")).clicked() {
                                        self.draw_sprite_id = id;
                                    }
                                    if ui.small_button(super::sprite_catalog::sprite_name(id)).clicked() {
                                        self.draw_sprite_id = id;
                                    }
                                });
                            });
                        });
                    }
                });
            }
        });
    }

    fn sprite_preview_texture(&mut self, ctx: &egui::Context, sprite_id: u8) -> egui::TextureHandle {
        if let Some(tex) = self.sprite_preview_textures.get(&sprite_id) {
            return tex.clone();
        }

        let mut cpu = self.cpu.clone();
        let sprite_tiles = self.sprite_oam_tiles(sprite_id);
        let mut image = super::tile_picker::render_sprite_preview_image(&sprite_tiles, &mut cpu);
        let mut best_score = score_sprite_preview(&image);

        // If the current level's sprite GFX set produces a weak preview,
        // search the vanilla sprite tilesets using the real UploadSpriteGFX path.
        if best_score < 220 {
            for tileset in 0u8..=15 {
                let mut cpu_try = self.cpu.clone();
                smwe_emu::emu::upload_sprite_tileset(&mut cpu_try, tileset);
                let tiles_try = smwe_emu::emu::sprite_oam_tiles(&mut cpu_try, sprite_id);
                let image_try = super::tile_picker::render_sprite_preview_image(&tiles_try, &mut cpu_try);
                let score_try = score_sprite_preview(&image_try);
                if score_try > best_score {
                    best_score = score_try;
                    image = image_try;
                }
            }
        }

        let handle = ctx.load_texture(
            format!("level_sprite_preview_{:03X}_{sprite_id:02X}", self.level_num),
            image,
            egui::TextureOptions::NEAREST,
        );
        self.sprite_preview_textures.insert(sprite_id, handle.clone());
        handle
    }
}

fn score_sprite_preview(image: &egui::ColorImage) -> i32 {
    let [w, h] = image.size;
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    let mut opaque = 0i32;
    let mut distinct = std::collections::BTreeSet::new();
    let mut mask = vec![false; w * h];

    for y in 0..h {
        for x in 0..w {
            let px = image.pixels[y * w + x];
            if px.a() == 0 {
                continue;
            }
            opaque += 1;
            mask[y * w + x] = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            distinct.insert((px.r(), px.g(), px.b()));
        }
    }

    if opaque == 0 {
        return -10_000;
    }

    let area = ((max_x - min_x + 1) * (max_y - min_y + 1)) as i32;
    let density = opaque * 100 / area.max(1);
    let color_bonus = (distinct.len().min(16) as i32) * 8;
    let largest_component = largest_opaque_component(&mask, w, h) as i32;
    let component_penalty = (opaque - largest_component).max(0) * 3;
    let fullness_penalty = if area > ((w * h) as i32 * 3 / 4) { area / 3 } else { 0 };
    opaque + density + color_bonus + largest_component * 2 - component_penalty - area / 8 - fullness_penalty
}

fn largest_opaque_component(mask: &[bool], w: usize, h: usize) -> usize {
    let mut seen = vec![false; mask.len()];
    let mut best = 0usize;
    let mut stack = Vec::new();

    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            if !mask[idx] || seen[idx] {
                continue;
            }

            seen[idx] = true;
            stack.push((x, y));
            let mut size = 0usize;

            while let Some((cx, cy)) = stack.pop() {
                size += 1;
                for (nx, ny) in [
                    (cx.wrapping_sub(1), cy),
                    (cx + 1, cy),
                    (cx, cy.wrapping_sub(1)),
                    (cx, cy + 1),
                ] {
                    if nx >= w || ny >= h {
                        continue;
                    }
                    let nidx = ny * w + nx;
                    if !mask[nidx] || seen[nidx] {
                        continue;
                    }
                    seen[nidx] = true;
                    stack.push((nx, ny));
                }
            }

            best = best.max(size);
        }
    }

    best
}
