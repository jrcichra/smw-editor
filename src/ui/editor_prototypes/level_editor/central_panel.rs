use std::sync::Arc;

use egui::{
    Align2, Color32, FontId, PaintCallback, Rect, Rounding, Sense, Stroke, Ui, Vec2, vec2,
};
use egui_glow::CallbackFn;

use super::UiLevelEditor;

// Pixels per game tile at zoom=1
const TILE_PX: f32 = 16.0;

impl UiLevelEditor {
    pub(super) fn central_panel(&mut self, ui: &mut Ui) {
        let (view_rect, resp) = ui.allocate_exact_size(
            vec2(ui.available_width(), ui.available_height()),
            Sense::click_and_drag(),
        );
        let painter = ui.painter_at(view_rect);
        let z = self.zoom;
        let tile_sz = TILE_PX * z;

        // ── Pan with middle-mouse or left-drag ────────────────
        if resp.dragged_by(egui::PointerButton::Middle)
            || (resp.dragged_by(egui::PointerButton::Primary)
                && ui.input(|i| i.modifiers.is_none()))
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
            .and_then(|_| {
                self.rom
                    .gfx
                    .color_palettes
                    .lv_specific_set
                    .back_area_colors
                    .get(back_area)
                    .copied()
            })
            .map(Color32::from)
            .unwrap_or(Color32::from_rgb(92, 148, 252)); // SMW default sky-blue

        painter.rect_filled(view_rect, Rounding::ZERO, bg);

        // ── GL tile rendering (actual SNES graphics) ────────────
        {
            let level_renderer = Arc::clone(&self.level_renderer);
            let ppp = ui.ctx().pixels_per_point();
            // egui_glow sets the GL viewport to `rect` before calling the callback,
            // so screen_size = rect.size() * ppp  (physical pixels of the viewport).
            // The shader: ndc = ((tile_px + offset)*zoom / screen_size * 2 - 1) * (1,-1)
            // For tile (0,0) to appear at canvas origin (top-left of view_rect),
            // we need offset = Vec2::ZERO when pan=0. Pan shifts tile positions,
            // so offset = self.offset * ppp  (pan in physical pixels, pre-zoom).
            let screen_size_px = view_rect.size() * ppp;
            // offset is in canvas-pixel units (same space as tile positions);
            // gl_zoom = z * ppp already converts canvas→physical pixels.
            // Do NOT multiply offset by ppp — that would over-scale it.
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

        let props = &self.level_properties;
        let (level_w, level_h) = props.level_dimensions_in_tiles();
        let canvas_w = level_w as f32 * tile_sz;
        let canvas_h = level_h as f32 * tile_sz;

        // Level canvas origin in screen space
        let origin = view_rect.min + self.offset * z;

        // ── Clip drawing to view ──────────────────────────────
        let level_rect = Rect::from_min_size(origin, vec2(canvas_w, canvas_h));

        // Fill level canvas with a slightly lighter shade so the boundary is clear
        if let Some(vis) = level_rect.intersect(view_rect).into() {
            painter.rect_filled(vis, Rounding::ZERO,
                bg.linear_multiply(1.15));
        }

        // Level bounding box
        painter.rect_stroke(
            level_rect, Rounding::ZERO,
            Stroke::new(2.0, Color32::WHITE),
        );

        // ── Screen dividers ───────────────────────────────────
        let (scr_w, scr_h) = props.screen_dimensions_in_tiles();
        let screens = props.num_screens();
        for s in 0..screens {
            let (lx, ly) = if props.is_vertical {
                ((s % 2) as f32 * scr_w as f32 * tile_sz,
                 (s / 2) as f32 * scr_h as f32 * tile_sz)
            } else {
                (s as f32 * scr_w as f32 * tile_sz, 0.0)
            };
            let scr_rect = Rect::from_min_size(
                origin + vec2(lx, ly),
                vec2(scr_w as f32 * tile_sz, scr_h as f32 * tile_sz),
            );
            // Only draw visible screens
            if scr_rect.max.x < view_rect.min.x || scr_rect.min.x > view_rect.max.x ||
               scr_rect.max.y < view_rect.min.y || scr_rect.min.y > view_rect.max.y {
                continue;
            }
            painter.rect_stroke(scr_rect, Rounding::ZERO,
                Stroke::new(0.5, Color32::from_white_alpha(55)));
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
            painter.rect_filled(er, Rounding::same(3.0),
                Color32::from_rgba_unmultiplied(255, 220, 0, 120));
            painter.rect_stroke(er, Rounding::same(3.0),
                Stroke::new(1.5, Color32::from_rgba_unmultiplied(255, 200, 0, 200)));
            if z >= 0.8 {
                painter.text(er.center(), Align2::CENTER_CENTER,
                    format!("→{:03X}", exit.id),
                    FontId::proportional(7.0 * z.min(1.5)), Color32::BLACK);
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
