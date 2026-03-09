use egui::{
    Align2, Color32, FontId, Rect, Rounding, Sense, Stroke, Ui, Vec2, vec2,
};

use super::UiLevelEditor;

// Pixels per game tile at zoom=1
const TILE_PX: f32 = 16.0;

// ── Object colours by category ────────────────────────────────────────────────
fn object_color(id: u8) -> (Color32, Color32) {
    // (fill, label-text)
    match id {
        0x00..=0x0F => (Color32::from_rgb(70,  130, 70),  Color32::WHITE),  // terrain / solid
        0x10..=0x1F => (Color32::from_rgb(50,  90,  170), Color32::WHITE),  // pipes / water
        0x20..=0x2F => (Color32::from_rgb(150, 110, 40),  Color32::WHITE),  // slopes / dirt
        0x30..=0x3F => (Color32::from_rgb(170, 70,  70),  Color32::WHITE),  // special blocks
        0x40..=0x4F => (Color32::from_rgb(130, 50,  150), Color32::WHITE),  // moving / rotating
        0x50..=0x5F => (Color32::from_rgb(50,  150, 150), Color32::BLACK),  // coins / items
        0x60..=0x6F => (Color32::from_rgb(190, 150, 30),  Color32::BLACK),  // platforms
        _           => (Color32::from_rgb(90,  90,  90),  Color32::WHITE),  // unknown
    }
}

fn object_label(id: u8) -> &'static str {
    match id {
        0x00 => "Ground",  0x01 => "Slope\\", 0x02 => "Slope/",
        0x03 => "Ledge",   0x04 => "Wall",     0x05 => "Ceiling",
        0x06 => "Pit",     0x07 => "Fill",     0x08 => "Pipe↑",
        0x09 => "Pipe↓",   0x0A => "Pipe→",    0x0B => "Pipe←",
        0x0C => "Water",   0x0D => "Lava",     0x0E => "Cloud",
        0x0F => "Platform",0x24 => "?Block",   0x25 => "Brick",
        0x26 => "Coin",    0x2B => "NoteBlk",  0x31 => "YoshiCoin",
        0x74 => "Door",    _ => "",
    }
}

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

        // ── Level background colour ───────────────────────────
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

        // ── Draw placed objects ───────────────────────────────
        for obj in &self.layer1.objects {
            let ox = obj.x as f32 * tile_sz;
            let oy = obj.y as f32 * tile_sz;
            let sz = tile_sz.max(4.0);
            let pos = origin + vec2(ox, oy);

            // Skip off-screen objects
            if pos.x > view_rect.max.x + sz || pos.y > view_rect.max.y + sz
                || pos.x + sz < view_rect.min.x || pos.y + sz < view_rect.min.y
            {
                continue;
            }

            let obj_rect = Rect::from_min_size(pos, Vec2::splat(sz));
            let (fill, text_col) = object_color(obj.id);
            let fill_a = Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), 210);

            painter.rect_filled(obj_rect, Rounding::same(1.5 * z.min(1.0)), fill_a);
            painter.rect_stroke(
                obj_rect,
                Rounding::same(1.5 * z.min(1.0)),
                Stroke::new(1.0, Color32::from_white_alpha(180)),
            );

            if z >= 0.8 {
                let lbl = if !object_label(obj.id).is_empty() {
                    object_label(obj.id).to_string()
                } else {
                    format!("{:02X}:{:02X}", obj.id, obj.settings)
                };
                painter.text(
                    obj_rect.center(),
                    Align2::CENTER_CENTER,
                    &lbl,
                    FontId::proportional(7.5 * z.min(1.5)),
                    text_col,
                );
            } else {
                // At tiny zoom: just a filled dot
                painter.circle_filled(obj_rect.center(), 2.5, fill);
            }
        }

        // ── Draw level exits ──────────────────────────────────
        for exit in &self.layer1.exits {
            let sx = if props.is_vertical { 0 } else { exit.screen as u32 };
            let sy = if props.is_vertical { exit.screen as u32 } else { 0 };
            let ex = (sx * scr_w) as f32 * tile_sz;
            let ey = (sy * scr_h) as f32 * tile_sz;
            let er = Rect::from_min_size(
                origin + vec2(ex, ey),
                Vec2::splat(tile_sz * 2.0),
            );
            painter.rect_filled(
                er, Rounding::same(4.0),
                Color32::from_rgba_unmultiplied(255, 220, 0, 180),
            );
            if z >= 0.8 {
                let dest = format!("→{:03X}", exit.id);
                painter.text(
                    er.center(),
                    Align2::CENTER_CENTER,
                    &dest,
                    FontId::proportional(7.5 * z.min(1.5)),
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
