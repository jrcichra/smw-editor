use std::sync::Arc;

use egui::{vec2, Align2, Color32, FontId, PaintCallback, Rect, Rounding, Sense, Stroke, Ui, Vec2};
use egui_glow::CallbackFn;

use super::UiLevelEditor;

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

        // ── GL tile rendering (actual SNES graphics) ────────────
        {
            let level_renderer = Arc::clone(&self.level_renderer);
            let ppp = ui.ctx().pixels_per_point();
            let screen_size_px = view_rect.size() * ppp;
            // The paint callback renders in view-local coordinates, so the GL
            // offset must use the same local pan basis as the egui overlays.
            let gl_offset = self.offset;
            let gl_zoom = z * ppp;
            ui.painter().add(PaintCallback {
                rect: view_rect,
                callback: Arc::new(CallbackFn::new(move |_info, painter| {
                    let mut r = level_renderer.lock().expect("Cannot lock level_renderer");
                    r.set_offset(gl_offset);
                    r.paint(painter.gl(), screen_size_px, gl_zoom);
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
        for exit in &self.layer1.exits {
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

        // ── Object overlay (debug/structure view) ─────────────
        if self.show_object_overlay {
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

            for obj in &self.layer1.objects {
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
                let fill = obj_color(obj.id, obj.is_extended);
                painter.rect_filled(rect, Rounding::same(2.0), fill);
                painter.rect_stroke(rect, Rounding::same(2.0), Stroke::new(1.0, fill.linear_multiply(2.0)));

                if self.show_object_labels && z >= 0.9 {
                    let label =
                        if obj.is_extended { format!("E{:02X}", obj.extended_id) } else { format!("{:02X}", obj.id) };
                    painter.text(
                        rect.left_top() + vec2(2.0, 2.0),
                        Align2::LEFT_TOP,
                        label,
                        FontId::monospace(9.0),
                        Color32::BLACK,
                    );
                }
            }
        }

        // ── Hover status bar ──────────────────────────────────
        if let Some(cursor) = resp.hover_pos() {
            let rel = (cursor - origin) / tile_sz;
            let tx = rel.x as i32;
            let ty = rel.y as i32;
            painter.text(
                view_rect.right_bottom() - vec2(6.0, 6.0),
                Align2::RIGHT_BOTTOM,
                format!("({tx}, {ty})  {:.0}%", z * 100.0),
                FontId::monospace(10.0),
                Color32::from_white_alpha(160),
            );
        }
    }
}
