use std::sync::Arc;

use egui::{vec2, Align2, Color32, FontId, Key, PaintCallback, Rect, Rounding, Sense, Stroke, Ui, Vec2};
use egui_glow::CallbackFn;

use super::{sprite_editor, UiLevelEditor};
use crate::ui::editing_mode::EditingMode;

// Pixels per game tile at zoom=1
const TILE_PX: f32 = 16.0;

impl UiLevelEditor {
    pub(super) fn central_panel(&mut self, ui: &mut Ui) {
        let (view_rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), ui.available_height()), Sense::click_and_drag());
        let painter = ui.painter_at(view_rect);
        let z = self.zoom;
        let tile_sz = TILE_PX * z;

        let props = &self.level_properties;
        let (level_w, level_h) = props.level_dimensions_in_tiles();
        let canvas_w = level_w as f32 * tile_sz;
        let canvas_h = level_h as f32 * tile_sz;

        // Level canvas origin in screen space
        let origin = view_rect.min + self.offset * z;

        // ── Pan with middle-mouse or left-drag ────────────────
        if resp.dragged_by(egui::PointerButton::Middle)
            || (resp.dragged_by(egui::PointerButton::Primary) && ui.input(|i| i.modifiers.is_none()))
        {
            self.offset += resp.drag_delta() / z;
        }

        // ── Scroll-to-zoom ────────────────────────────────────
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll != 0.0 && resp.hovered() {
            let factor = 1.0 + scroll * 0.001;
            self.zoom = (self.zoom * factor).clamp(0.25, 8.0);
        }

        // ── Level background colour (fills entire canvas before GL tiles) ───
        let back_area = self.level_properties.back_area_color as usize;
        let bg = self
            .rom
            .levels
            .get(self.level_num as usize)
            .and_then(|_| self.rom.gfx.color_palettes.lv_specific_set.back_area_colors.get(back_area).copied())
            .map(Color32::from)
            .unwrap_or(Color32::from_rgb(92, 148, 252)); // SMW default sky-blue

        painter.rect_filled(view_rect, Rounding::ZERO, bg);

        // ── Level bounding box + canvas tint (below GL tiles) ──
        let level_rect = Rect::from_min_size(origin, vec2(canvas_w, canvas_h));
        if let Some(vis) = level_rect.intersect(view_rect).into() {
            painter.rect_filled(vis, Rounding::ZERO, bg.linear_multiply(1.15));
        }

        // ── Pre-rendered tile atlas display (no per-frame VRAM decoding) ──
        {
            let atlas_tex = self.tile_atlas.texture_id();
            let atlas_renderer = Arc::clone(&self.atlas_renderer);
            let ppp = ui.ctx().pixels_per_point();
            let screen_w = view_rect.size().x * ppp;
            let screen_h = view_rect.size().y * ppp;
            let gl_offset = self.offset;
            let gl_zoom = z * ppp;
            ui.painter().add(PaintCallback {
                rect: view_rect,
                callback: Arc::new(CallbackFn::new(move |_info, painter| {
                    let r = atlas_renderer.lock().expect("Cannot lock atlas_renderer");
                    r.paint(painter.gl(), atlas_tex, screen_w, screen_h, gl_offset.x, gl_offset.y, gl_zoom);
                })),
            });
        }

        // Level bounding box
        painter.rect_stroke(level_rect, Rounding::ZERO, Stroke::new(2.0, Color32::WHITE));

        // ── Screen dividers ───────────────────────────────────
        let (scr_w, scr_h) = props.screen_dimensions_in_tiles();
        let screens = props.num_screens();
        for s in 0..screens {
            let (lx, ly) = if props.is_vertical {
                (0.0, s as f32 * scr_h as f32 * tile_sz)
            } else {
                (s as f32 * scr_w as f32 * tile_sz, 0.0)
            };
            let scr_rect =
                Rect::from_min_size(origin + vec2(lx, ly), vec2(scr_w as f32 * tile_sz, scr_h as f32 * tile_sz));
            // Only draw visible screens
            if scr_rect.max.x < view_rect.min.x
                || scr_rect.min.x > view_rect.max.x
                || scr_rect.max.y < view_rect.min.y
                || scr_rect.min.y > view_rect.max.y
            {
                continue;
            }
            painter.rect_stroke(scr_rect, Rounding::ZERO, Stroke::new(0.5, Color32::from_white_alpha(25)));
            if z >= 0.8 {
                painter.text(
                    scr_rect.min + vec2(3.0, 2.0),
                    Align2::LEFT_TOP,
                    format!("{s:X}"),
                    FontId::monospace(10.0 * z.sqrt()),
                    Color32::from_white_alpha(100),
                );
            }
        }

        // ── Draw exit markers (subtle gold badges over GL tiles) ───
        self.layer1.read(|layer| {
            for exit in &layer.exits {
                let sx = if props.is_vertical { 0 } else { exit.screen as u32 };
                let sy = if props.is_vertical { exit.screen as u32 } else { 0 };
                let ex = (sx * scr_w) as f32 * tile_sz;
                let ey = (sy * scr_h) as f32 * tile_sz;
                let er = Rect::from_min_size(origin + vec2(ex, ey), Vec2::splat(tile_sz * 2.0));
                painter.rect_filled(er, Rounding::same(3.0), Color32::from_rgba_unmultiplied(255, 220, 0, 120));
                painter.rect_stroke(
                    er,
                    Rounding::same(3.0),
                    Stroke::new(1.5, Color32::from_rgba_unmultiplied(255, 200, 0, 200)),
                );
                if z >= 0.8 {
                    painter.text(
                        er.center(),
                        Align2::CENTER_CENTER,
                        format!("→{:03X}", exit.id),
                        FontId::proportional(7.0 * z.min(1.5)),
                        Color32::BLACK,
                    );
                }
            }
        });

        // ── Grid overlay ──────────────────────────────────────
        if self.always_show_grid || ui.input(|i| i.modifiers.shift_only()) {
            let stroke = Stroke::new(0.5, Color32::from_white_alpha(40));
            let off_x = origin.x.rem_euclid(tile_sz);
            let off_y = origin.y.rem_euclid(tile_sz);

            let mut gx = view_rect.min.x + off_x - tile_sz;
            while gx <= view_rect.max.x {
                painter.vline(gx, view_rect.min.y..=view_rect.max.y, stroke);
                gx += tile_sz;
            }
            let mut gy = view_rect.min.y + off_y - tile_sz;
            while gy <= view_rect.max.y {
                painter.hline(view_rect.min.x..=view_rect.max.x, gy, stroke);
                gy += tile_sz;
            }
        }

        // ── Object overlay (structure view + editing) ─────────
        let show_overlay = self.show_object_overlay
            || self.editing_mode != EditingMode::Select
            || !self.selected_object_indices.is_empty();
        if show_overlay {
            let obj_color = |id: u8, is_ext: bool| -> Color32 {
                if is_ext {
                    Color32::from_rgba_unmultiplied(255, 140, 0, 90)
                } else {
                    let r = 40 + (id as u32 * 53 % 180) as u8;
                    let g = 40 + (id as u32 * 97 % 180) as u8;
                    let b = 40 + (id as u32 * 151 % 180) as u8;
                    Color32::from_rgba_unmultiplied(r, g, b, 70)
                }
            };

            self.layer1.read(|layer| {
                for (i, obj) in layer.objects.iter().enumerate() {
                    let (w, h) = if obj.is_extended {
                        (1_u32, 1_u32)
                    } else {
                        let w = (obj.settings & 0x0F) as u32 + 1;
                        let h = (obj.settings >> 4) as u32 + 1;
                        (w.max(1), h.max(1))
                    };

                    let pos = origin + vec2(obj.x as f32 * tile_sz, obj.y as f32 * tile_sz);
                    let rect = Rect::from_min_size(pos, vec2(w as f32 * tile_sz, h as f32 * tile_sz));
                    if rect.max.x < view_rect.min.x
                        || rect.min.x > view_rect.max.x
                        || rect.max.y < view_rect.min.y
                        || rect.min.y > view_rect.max.y
                    {
                        continue;
                    }

                    let selected = self.selected_object_indices.contains(&i);
                    let fill = obj_color(obj.id, obj.is_extended);
                    painter.rect_filled(rect, Rounding::same(2.0), fill);
                    painter.rect_stroke(rect, Rounding::same(2.0), Stroke::new(1.0, fill.linear_multiply(2.0)));

                    if selected {
                        painter.rect_stroke(
                            rect.expand(1.0),
                            Rounding::same(2.0),
                            Stroke::new(2.0, Color32::from_rgb(255, 220, 0)),
                        );
                    }

                    if self.show_object_labels && z >= 0.9 {
                        let label = if obj.is_extended {
                            format!("E{:02X}", obj.extended_id)
                        } else {
                            format!("{:02X}", obj.id)
                        };
                        painter.text(
                            rect.left_top() + vec2(2.0, 2.0),
                            Align2::LEFT_TOP,
                            label,
                            FontId::monospace(9.0),
                            Color32::BLACK,
                        );
                    }
                }
            });
        }

        // ── Hover / click (tile granularity) ────────────────────
        if let Some(cursor) = resp.hover_pos() {
            let rel = (cursor - origin) / tile_sz;
            let tx = rel.x.floor() as i32;
            let ty = rel.y.floor() as i32;
            if tx >= 0 && ty >= 0 && (tx as u32) < level_w && (ty as u32) < level_h {
                let tile_rect =
                    Rect::from_min_size(origin + vec2(tx as f32 * tile_sz, ty as f32 * tile_sz), Vec2::splat(tile_sz));
                painter.rect_stroke(tile_rect, Rounding::ZERO, Stroke::new(1.0, Color32::WHITE));

                // Tile inspection click (only in Select mode with no object selected,
                // or always when holding Alt for quick inspection)
                let inspect_click = resp.clicked_by(egui::PointerButton::Primary)
                    && (self.editing_mode == EditingMode::Select || ui.input(|i| i.modifiers.alt));
                if inspect_click {
                    self.selected_tile = Some((tx as u32, ty as u32));
                }

                let block_info =
                    self.block_id_at(tx as u32, ty as u32).map(|id| format!("  blk={id:#04X}")).unwrap_or_default();
                painter.text(
                    view_rect.right_bottom() - vec2(6.0, 6.0),
                    Align2::RIGHT_BOTTOM,
                    format!("({tx}, {ty}){block_info}  {:.0}%", z * 100.0),
                    FontId::monospace(10.0),
                    Color32::from_white_alpha(160),
                );
            }
        }

        // ── Editing interaction (object select/place/delete) ───
        self.handle_editing_interaction(&resp, origin, tile_sz);

        // ── Keyboard shortcuts ─────────────────────────────────
        ui.input_mut(|input| {
            if input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, Key::Z)) {
                self.handle_undo();
            }
            if input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, Key::Y)) {
                self.handle_redo();
            }
            if input.key_pressed(Key::Delete) || input.key_pressed(Key::Backspace) {
                self.delete_selected_objects();
            }
            if input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::NONE, Key::Num1)) {
                self.editing_mode = EditingMode::Select;
            }
            if input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::NONE, Key::Num2)) {
                self.editing_mode = EditingMode::Draw;
            }
            if input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::NONE, Key::Num3)) {
                self.editing_mode = EditingMode::Erase;
            }
            if input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::NONE, Key::Num4)) {
                self.editing_mode = EditingMode::Probe;
            }
        });

        // ── Sprite overlay ────────────────────────────────────
        if self.show_sprites {
            // Read sprites once per frame (cheap byte parse, avoids duplicate work).
            let sprites = sprite_editor::read_sprites_from_wram(&self.cpu.mem.wram);
            let is_vertical = self.level_properties.is_vertical;
            for (i, spr) in sprites.iter().enumerate() {
                let (ax, ay) = sprite_editor::sprite_pixel_pos(spr, is_vertical);
                let color = sprite_editor::sprite_color(spr.sprite_id());
                let rect = Rect::from_min_size(origin + vec2(ax as f32 * z, ay as f32 * z), vec2(16.0 * z, 16.0 * z));
                if rect.max.x >= view_rect.min.x
                    && rect.min.x <= view_rect.max.x
                    && rect.max.y >= view_rect.min.y
                    && rect.min.y <= view_rect.max.y
                {
                    painter.rect_filled(rect, Rounding::same(2.0), color);
                    painter.rect_stroke(rect, Rounding::same(2.0), Stroke::new(1.0, color.linear_multiply(2.0)));

                    if self.selected_sprite == Some(i) {
                        painter.rect_stroke(
                            rect.expand(1.0),
                            Rounding::same(2.0),
                            Stroke::new(2.0, Color32::from_rgb(255, 100, 100)),
                        );
                    }

                    if z >= 0.9 {
                        painter.text(
                            rect.left_top() + vec2(2.0, 2.0),
                            Align2::LEFT_TOP,
                            format!("SP:{:02X}", spr.sprite_id()),
                            FontId::monospace(9.0),
                            Color32::WHITE,
                        );
                    }
                }
            }

            // Sprite interaction (right-click) — reuses the same sprite list
            let sprite_changed = sprite_editor::handle_sprite_interaction(
                &mut self.cpu.mem.wram,
                &sprites,
                &resp,
                origin,
                tile_sz,
                is_vertical,
                self.editing_mode,
                &mut self.selected_sprite,
                self.place_sprite_id,
            );

            // Re-upload sprites after edit
            if sprite_changed {
                self.upload_sprites_for_level();
            }
        }

        // ── Selected tile highlight ────────────────────────────
        if let Some((x, y)) = self.selected_tile {
            let r = Rect::from_min_size(origin + vec2(x as f32 * tile_sz, y as f32 * tile_sz), Vec2::splat(tile_sz));
            painter.rect_stroke(r, Rounding::ZERO, Stroke::new(2.0, Color32::from_rgb(255, 220, 0)));
        }
    }
}
