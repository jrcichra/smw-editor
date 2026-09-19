use std::{sync::Arc, time::Duration};

use egui::{
    vec2,
    Align2,
    Color32,
    CornerRadius,
    Event,
    FontId,
    Key,
    PaintCallback,
    Rect,
    Sense,
    Stroke,
    StrokeKind,
    Ui,
    Vec2,
};
use egui_glow::CallbackFn;

use super::UiLevelEditor;
use crate::{custom_tooltips::ObjectKind, ui::editing_mode::EditingMode};

// Pixels per game tile at zoom=1
const TILE_PX: f32 = 16.0;

impl UiLevelEditor {
    pub(super) fn central_panel(&mut self, ui: &mut Ui) {
        let (view_rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), ui.available_height()), Sense::click_and_drag());
        let painter = ui.painter_at(view_rect);
        let z = self.zoom;
        let tile_sz = TILE_PX * z;

        let (level_w, level_h) = self.level_properties.level_dimensions_in_tiles();
        let canvas_w = level_w as f32 * tile_sz;
        let canvas_h = level_h as f32 * tile_sz;

        // Level canvas origin in screen space
        let origin = view_rect.min + self.offset * z;

        // ── LM-style object drag (move/resize) ─────────────────
        // Runs before panning so a drag that starts on the selected
        // object (or one of its handles) suppresses canvas panning.
        // (level_w/level_h are copied first: update_object_drag needs
        // &mut self while `props` below borrows it immutably.)
        self.update_object_drag(&resp, origin, tile_sz, level_w, level_h, self.level_properties.is_vertical);

        let props = &self.level_properties;
        let (scr_w, scr_h) = props.screen_dimensions_in_tiles();
        let num_screens = props.num_screens();
        let is_vertical = props.is_vertical;

        // ── Pan with middle-mouse or left-drag ────────────────
        if resp.dragged_by(egui::PointerButton::Middle)
            || (resp.dragged_by(egui::PointerButton::Primary)
                && ui.input(|i| i.modifiers.is_none())
                && self.object_drag.is_none())
        {
            self.offset += resp.drag_delta() / z;
        }

        // ── Scroll-to-zoom ────────────────────────────────────
        let zoom_delta = ui.input(|i| i.zoom_delta());
        let wheel_delta = ui.input(|i| i.raw_scroll_delta.y);
        if resp.contains_pointer() {
            let factor = if (zoom_delta - 1.0).abs() > f32::EPSILON {
                zoom_delta
            } else if wheel_delta.abs() > f32::EPSILON {
                (wheel_delta * 0.005).exp()
            } else {
                1.0
            };
            if factor != 1.0 {
                self.zoom = (self.zoom * factor).clamp(0.25, 8.0);
            }
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

        painter.rect_filled(view_rect, CornerRadius::ZERO, bg);

        // ── Level bounding box + canvas tint (below GL tiles) ──
        let level_rect = Rect::from_min_size(origin, vec2(canvas_w, canvas_h));
        if let Some(vis) = level_rect.intersect(view_rect).into() {
            painter.rect_filled(vis, CornerRadius::ZERO, bg.linear_multiply(1.15));
        }

        // ── Animated tile ticking ─────────────────────────────
        // SMW advances each animated tile slot once every 8 game-frames at
        // 60 fps, so each distinct animation frame shows for ~133ms.  We tick
        // at the same interval to match the real game's visual speed.
        const ANIM_INTERVAL: Duration = Duration::from_millis(133);
        if self.last_anim_tick.elapsed() >= ANIM_INTERVAL {
            self.last_anim_tick = std::time::Instant::now();
            self.anim_tick += 1;
            // Custom ExAnimation frames for this level (plus the global
            // list) play on the same tick. `disable_original` skips the
            // game's own animated tiles so the custom ones replace them.
            let anim = self.exanimation.for_level(self.level_num);
            if !anim.disable_original {
                smwe_emu::emu::advance_anim_frame(&mut self.cpu);
            }
            let renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
            if anim.frames.is_empty() {
                renderer.upload_gfx(&self.gl, &self.cpu.mem.vram);
            } else {
                smwe_rom::exanimation::apply_tick(
                    &anim,
                    self.anim_tick,
                    &mut self.cpu.mem.vram,
                    &mut self.cpu.mem.cgram,
                );
                renderer.upload_gfx(&self.gl, &self.cpu.mem.vram);
                renderer.upload_palette(&self.gl, &self.cpu.mem.cgram);
            }
        }
        ui.ctx().request_repaint_after(ANIM_INTERVAL);

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
                rect:     view_rect,
                callback: Arc::new(CallbackFn::new(move |_info, painter| {
                    let mut r = level_renderer.lock().expect("Cannot lock level_renderer");
                    r.set_offset(gl_offset);
                    r.paint(painter.gl().as_ref(), screen_size_px, gl_zoom);
                })),
            });
        }

        // Level bounding box
        painter.rect_stroke(level_rect, CornerRadius::ZERO, Stroke::new(2.0_f32, Color32::WHITE), StrokeKind::Outside);

        // ── Screen dividers ───────────────────────────────────
        for s in 0..num_screens {
            let (lx, ly) = if is_vertical {
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
            painter.rect_stroke(
                scr_rect,
                CornerRadius::ZERO,
                Stroke::new(0.5_f32, Color32::from_white_alpha(25)),
                StrokeKind::Outside,
            );
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
        if let Some(layer_data) = self.editing_objects() {
            layer_data.read(|layer| {
                for exit in &layer.exits {
                    let sx = if is_vertical { 0 } else { exit.screen as u32 };
                    let sy = if is_vertical { exit.screen as u32 } else { 0 };
                    let ex = (sx * scr_w) as f32 * tile_sz;
                    let ey = (sy * scr_h) as f32 * tile_sz;
                    let er = Rect::from_min_size(origin + vec2(ex, ey), Vec2::splat(tile_sz * 2.0));
                    painter.rect_filled(er, CornerRadius::same(3), Color32::from_rgba_unmultiplied(255, 220, 0, 120));
                    painter.rect_stroke(
                        er,
                        CornerRadius::same(3),
                        Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(255, 200, 0, 200)),
                        StrokeKind::Outside,
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
        }

        // ── Mario spawn point marker ───────────────────────────
        {
            let (spawn_x, spawn_y) = self.spawn_pos();
            let spawn_pos = origin + vec2(spawn_x as f32 * tile_sz, spawn_y as f32 * tile_sz);
            let spawn_rect = self.entrance_rect(origin, tile_sz);

            let is_hovering = resp.hover_pos().is_some_and(|p| spawn_rect.contains(p));
            let spawn_color = if self.dragging_spawn || is_hovering {
                Color32::from_rgba_unmultiplied(255, 200, 100, 255)
            } else {
                Color32::from_rgba_unmultiplied(255, 100, 100, 255)
            };

            // Selected entrance (sprite editing mode, LM v2.20) gets the
            // same orange selection treatment as selected sprites.
            if self.entrance_selected {
                painter.rect_filled(
                    spawn_rect,
                    CornerRadius::same(2),
                    Color32::from_rgba_unmultiplied(255, 120, 0, 50),
                );
                painter.rect_stroke(
                    spawn_rect,
                    CornerRadius::same(2),
                    Stroke::new(2.0_f32, Color32::from_rgb(255, 120, 0)),
                    StrokeKind::Outside,
                );
            }

            painter.text(
                spawn_pos + vec2(tile_sz / 2.0, tile_sz / 2.0),
                Align2::CENTER_CENTER,
                "M",
                FontId::proportional(tile_sz * 0.8),
                spawn_color,
            );

            // Handle dragging the spawn point (Shift+click+drag on M).
            // Sprite editing mode has its own plain-drag path (LM v2.20,
            // no Shift needed) in editing.rs, so this stays off there.
            if !self.edit_sprites {
                let shift_held = ui.input(|i| i.modifiers.shift);
                let primary_down = ui.input(|i| i.pointer.primary_down());

                // Start drag when shift+click on M
                if is_hovering && shift_held && ui.input(|i| i.pointer.primary_pressed()) {
                    self.dragging_spawn = true;
                    self.begin_spawn_drag();
                }

                // Continue dragging while shift+primary held, update from pointer
                if self.dragging_spawn && shift_held && primary_down {
                    if let Some(pointer_pos) = ui.input(|i| i.pointer.latest_pos()) {
                        let local_pos = pointer_pos - origin;
                        let tile_x = (local_pos.x / tile_sz).max(0.0) as u32;
                        let tile_y = (local_pos.y / tile_sz).max(0.0) as u32;
                        let is_vertical = self.level_properties.is_vertical;
                        self.update_spawn_from_tiles(tile_x, tile_y, is_vertical);
                    }
                } else if !primary_down {
                    // End drag when mouse released (one undo step for the
                    // whole drag)
                    if self.dragging_spawn {
                        self.end_spawn_drag();
                    }
                    self.dragging_spawn = false;
                }
            }

            // Lunar Magic v2.20: Alt+Right-click the red M entrance marker
            // opens the Level Header (its properties). The marker region
            // rarely overlaps an object/sprite, but when it does the Edit
            // Manual dialog takes precedence there (handled in
            // handle_editing_interaction, which runs after this).
            if is_hovering && ui.input(|i| i.modifiers.alt) && resp.clicked_by(egui::PointerButton::Secondary) {
                if let Some(pos) = resp.hover_pos() {
                    let hit = if self.edit_sprites {
                        self.sprite_at(pos, origin, tile_sz).is_some()
                    } else {
                        self.object_at(pos, origin, tile_sz).is_some()
                    };
                    if !hit {
                        self.show_level_header = true;
                    }
                }
            }

            // Lunar Magic v2.20: Alt+Right-click an entrance opens its
            // properties. The marker region rarely overlaps an object/sprite,
            // but when it does the Edit Manual dialog takes precedence there.
            if is_hovering && ui.input(|i| i.modifiers.alt) && resp.clicked_by(egui::PointerButton::Secondary) {
                if let Some(pos) = resp.hover_pos() {
                    let hit = if self.edit_sprites {
                        self.sprite_at(pos, origin, tile_sz).is_some()
                    } else {
                        self.object_at(pos, origin, tile_sz).is_some()
                    };
                    if !hit {
                        self.show_level_header = true;
                    }
                }
            }
        }

        // ── Grid overlay ──────────────────────────────────────
        if self.always_show_grid || ui.input(|i| i.modifiers.shift_only()) {
            let stroke = Stroke::new(0.5_f32, Color32::from_white_alpha(40));
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

        // ── Exit-enabled tile markers (LM v3.31 view option) ───
        if self.mark_exit_tiles {
            self.paint_exit_enabled_overlay(&painter, view_rect, origin, tile_sz, level_w, level_h);
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

            if let Some(layer_data) = self.editing_objects() {
                layer_data.read(|layer| {
                    for (i, obj) in layer.objects.iter().enumerate() {
                        let (w, h) = if obj.is_extended {
                            (1_u32, 1_u32)
                        } else {
                            let w = (obj.settings & 0x0F) as u32 + 1;
                            let h = (obj.settings >> 4) as u32 + 1;
                            (w.max(1), h.max(1))
                        };

                        // Live drag feedback: draw the object at its dragged
                        // position/size while a drag is in progress.
                        let (dx, dy, dw, dh) = match &self.object_drag {
                            Some(drag) if drag.index == i => (drag.cur_x, drag.cur_y, drag.cur_w, drag.cur_h),
                            _ => (obj.x, obj.y, w, h),
                        };
                        let pos = origin + vec2(dx as f32 * tile_sz, dy as f32 * tile_sz);
                        let rect = Rect::from_min_size(pos, vec2(dw as f32 * tile_sz, dh as f32 * tile_sz));
                        if rect.max.x < view_rect.min.x
                            || rect.min.x > view_rect.max.x
                            || rect.max.y < view_rect.min.y
                            || rect.min.y > view_rect.max.y
                        {
                            continue;
                        }

                        let selected = self.selected_object_indices.contains(&i);
                        let fill = obj_color(obj.id, obj.is_extended);
                        painter.rect_filled(rect, CornerRadius::same(2), fill);
                        painter.rect_stroke(
                            rect,
                            CornerRadius::same(2),
                            Stroke::new(1.0_f32, fill.linear_multiply(2.0)),
                            StrokeKind::Outside,
                        );

                        if selected {
                            painter.rect_stroke(
                                rect.expand(1.0),
                                CornerRadius::same(2),
                                Stroke::new(2.0_f32, Color32::from_rgb(255, 220, 0)),
                                StrokeKind::Outside,
                            );
                        }

                        // Lunar Magic-style drag handles: 8 white squares
                        // (corners + edge midpoints) on the single selected
                        // object. Extended (1x1) objects can't be resized,
                        // so they get the selection outline only.
                        if selected && self.selected_object_indices.len() == 1 && !obj.is_extended {
                            let handle_px = (7.0 * z).clamp(6.0, 14.0);
                            for (_, hrect) in super::editing::drag_handle_rects(rect, handle_px) {
                                painter.rect_filled(hrect, CornerRadius::ZERO, Color32::WHITE);
                                painter.rect_stroke(
                                    hrect,
                                    CornerRadius::ZERO,
                                    Stroke::new(1.0_f32, Color32::BLACK),
                                    StrokeKind::Outside,
                                );
                            }
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
        }

        // ── Custom object tooltip on hover (LM v3.60) ─────────
        // Hovering an object shows the user's custom tooltip text, if one
        // was set in the Custom Object Tooltips window. Topmost object wins.
        if let Some(cursor) = resp.hover_pos() {
            if !self.edit_sprites {
                if let Some(layer_data) = self.editing_objects() {
                    let hovered = layer_data.read(|layer| {
                        layer.objects.iter().rev().find_map(|obj| {
                            let (w, h) = if obj.is_extended {
                                (1_u32, 1_u32)
                            } else {
                                let w = (obj.settings & 0x0F) as u32 + 1;
                                let h = (obj.settings >> 4) as u32 + 1;
                                (w.max(1), h.max(1))
                            };
                            let rect = Rect::from_min_size(
                                origin + vec2(obj.x as f32 * tile_sz, obj.y as f32 * tile_sz),
                                vec2(w as f32 * tile_sz, h as f32 * tile_sz),
                            );
                            if rect.contains(cursor) {
                                let kind = if obj.is_extended { ObjectKind::Extended } else { ObjectKind::Standard };
                                let id = if obj.is_extended { obj.extended_id } else { obj.id };
                                Some((kind, id))
                            } else {
                                None
                            }
                        })
                    });
                    if let Some((kind, id)) = hovered {
                        if let Some(tip) = self.custom_tooltips.get(kind, id) {
                            // `on_hover_text_at_pointer` consumes the
                            // response; clone so later code can keep using
                            // `resp`. Hover state is keyed by widget id, so
                            // the tooltip still shows.
                            resp.clone().on_hover_text_at_pointer(tip);
                        }
                    }
                }
            }
        }

        // ── Drag-handle hover cursors (LM feel) ────────────────
        if let Some(cursor) = resp.hover_pos() {
            if self.selected_object_indices.len() == 1
                && !self.edit_sprites
                && (self.editing_mode == EditingMode::Select || self.editing_mode == EditingMode::Probe)
            {
                let idx = *self.selected_object_indices.iter().next().expect("len == 1");
                if let Some(layer_data) = self.editing_objects() {
                    let hit = layer_data.read(|layer| {
                        layer.objects.get(idx).map(|obj| {
                            let (w, h) = super::editing::object_dims(obj.settings, obj.is_extended);
                            let rect = Rect::from_min_size(
                                origin + vec2(obj.x as f32 * tile_sz, obj.y as f32 * tile_sz),
                                vec2(w as f32 * tile_sz, h as f32 * tile_sz),
                            );
                            let handle_px = (7.0 * z).clamp(6.0, 14.0);
                            let on_handle =
                                if obj.is_extended { None } else { super::editing::handle_at(rect, handle_px, cursor) };
                            (on_handle, rect.contains(cursor))
                        })
                    });
                    if let Some((on_handle, on_body)) = hit {
                        use super::editing::DragHandle::*;
                        let icon = match on_handle {
                            Some(Nw) | Some(Se) => egui::CursorIcon::ResizeNwSe,
                            Some(Ne) | Some(Sw) => egui::CursorIcon::ResizeNeSw,
                            Some(N) | Some(S) => egui::CursorIcon::ResizeVertical,
                            Some(E) | Some(W) => egui::CursorIcon::ResizeHorizontal,
                            Option::None if on_body => egui::CursorIcon::Grab,
                            _ => egui::CursorIcon::Default,
                        };
                        if !matches!(icon, egui::CursorIcon::Default) {
                            ui.output_mut(|o| o.cursor_icon = icon);
                        }
                    }
                }
            }
        }

        if self.show_sprite_overlay || self.edit_sprites || !self.selected_sprite_indices.is_empty() {
            let sprite_entries = self.sprites.read(|sprites| sprites.sprites.clone());
            for (i, spr) in sprite_entries.iter().enumerate() {
                let (min_dx, min_dy, max_dx, max_dy) =
                    self.sprite_pixel_bounds(spr.sprite_id).unwrap_or((0, 0, 16, 16));
                let pos = origin
                    + vec2(spr.x as f32 * tile_sz + min_dx as f32 * z, spr.y as f32 * tile_sz + min_dy as f32 * z);
                let rect = Rect::from_min_size(pos, vec2((max_dx - min_dx) as f32 * z, (max_dy - min_dy) as f32 * z));
                if rect.max.x < view_rect.min.x
                    || rect.min.x > view_rect.max.x
                    || rect.max.y < view_rect.min.y
                    || rect.min.y > view_rect.max.y
                {
                    continue;
                }
                let selected = self.selected_sprite_indices.contains(&i);
                let fill = if selected {
                    Color32::from_rgba_unmultiplied(255, 120, 0, 50)
                } else {
                    Color32::from_rgba_unmultiplied(255, 80, 80, 28)
                };
                painter.rect_filled(rect, CornerRadius::same(2), fill);
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(2),
                    Stroke::new(
                        2.0_f32,
                        if selected { Color32::from_rgb(255, 120, 0) } else { Color32::from_rgb(255, 80, 80) },
                    ),
                    StrokeKind::Outside,
                );
                if self.show_object_labels && z >= 0.9 {
                    painter.text(
                        rect.left_top() + vec2(2.0, 2.0),
                        Align2::LEFT_TOP,
                        format!("S{:02X}", spr.sprite_id),
                        FontId::monospace(9.0),
                        Color32::WHITE,
                    );
                }
            }
        }

        // ── Direct Map16 overlay (purple; cyan when selected) ──
        self.dm16_overlay(&painter, origin, tile_sz);
        if self.dm16_placing.is_some() {
            if let Some(cursor) = resp.hover_pos() {
                let rel = (cursor - origin) / tile_sz;
                let tx = rel.x.floor() as i32;
                let ty = rel.y.floor() as i32;
                if tx >= 0 && ty >= 0 && (tx as u32) < level_w && (ty as u32) < level_h {
                    self.dm16_placement_preview(&painter, origin, tile_sz, tx as u32, ty as u32);
                }
            }
        }

        // ── Hover / click (tile granularity) ────────────────────
        if let Some(cursor) = resp.hover_pos() {
            let rel = (cursor - origin) / tile_sz;
            let tx = rel.x.floor() as i32;
            let ty = rel.y.floor() as i32;
            if tx >= 0 && ty >= 0 && (tx as u32) < level_w && (ty as u32) < level_h {
                let tile_rect =
                    Rect::from_min_size(origin + vec2(tx as f32 * tile_sz, ty as f32 * tile_sz), Vec2::splat(tile_sz));
                painter.rect_stroke(
                    tile_rect,
                    CornerRadius::ZERO,
                    Stroke::new(1.0_f32, Color32::WHITE),
                    StrokeKind::Outside,
                );

                // Tile inspection click (only in Select mode with no object selected,
                // or always when holding Alt for quick inspection)
                let inspect_click = resp.clicked_by(egui::PointerButton::Primary)
                    && !self.suppress_click_select
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

        // ── Direct Map16 gestures (placement / flood fill) ───
        // Runs before vanilla object editing so an armed placement click
        // drops the DM16 pattern instead of placing/selecting an object.
        let dm16_consumed = {
            let modifiers = ui.input(|i| i.modifiers);
            self.handle_dm16_canvas_click(&resp, origin, tile_sz, modifiers)
        };

        // ── Editing interaction (object select/place/delete) ───
        if !dm16_consumed {
            self.handle_editing_interaction(&resp, origin, tile_sz);
        }

        // ── Keyboard shortcuts ─────────────────────────────────
        ui.input_mut(|input| {
            if input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, Key::Z)) {
                // Route undo to the layer with the active selection.
                if !self.selected_dm16_indices.is_empty() {
                    self.handle_dm16_undo();
                } else {
                    self.last_undo_was_dm16 = false;
                    self.handle_undo();
                }
            }
            if input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, Key::Y)) {
                // Redo follows the layer of the last undo; any selection or
                // edit elsewhere disarms it back to the vanilla layer.
                if self.last_undo_was_dm16 && self.direct_map16.can_redo() {
                    self.handle_dm16_redo();
                } else {
                    self.last_undo_was_dm16 = false;
                    self.handle_redo();
                }
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

        // ── Clipboard: Lunar Magic-style cut/copy/paste ────────────────
        // Ctrl+C / Ctrl+X arrive as Event::Copy / Event::Cut (the same events
        // TextEdit consumes, so a focused text widget keeps its own copy).
        // Paste arrives as Event::Paste — pushed directly by the integration
        // on Ctrl+V, and on the next frame after a ViewportCommand::RequestPaste
        // (toolbar Paste button). The canvas stands down while the pointer is
        // over a floating editor window with its own clipboard keys (Map16 /
        // 8x8 tile editors).
        let widget_focused = ui.ctx().memory(|m| m.focused().is_some());
        let window_has_copy_intent = self.map16_window_hovered || self.tile_editor_window_hovered;
        if !widget_focused && !window_has_copy_intent {
            if ui.input(|i| i.events.contains(&Event::Copy)) {
                self.clipboard_copy_selection(ui.ctx());
            }
            if ui.input(|i| i.events.contains(&Event::Cut)) {
                self.clipboard_cut_selection(ui.ctx());
            }
            if let Some(text) = crate::ui::clipboard::take_paste_text(ui.ctx()) {
                match crate::ui::clipboard::ClipboardPayload::decode(&text) {
                    Some(payload) => {
                        // Anchor at the hover tile when the pointer is over
                        // the canvas, else just past the copied position.
                        let anchor = resp
                            .hover_pos()
                            .map(|pos| {
                                let rel = (pos - origin) / tile_sz;
                                (rel.x.floor().max(0.0) as u32, rel.y.floor().max(0.0) as u32)
                            })
                            .or(self.clipboard_copy_origin.map(|(x, y)| (x + 1, y + 1)))
                            .unwrap_or((0, 0));
                        self.clipboard_paste_at(&payload, anchor, level_w, level_h);
                    }
                    None => {
                        self.mwl_status =
                            Some("Clipboard doesn't hold smw-editor data — copy a selection first".to_string());
                    }
                }
            }
        }

        // ── Selected tile highlight ────────────────────────────
        if let Some((x, y)) = self.selected_tile {
            let r = Rect::from_min_size(origin + vec2(x as f32 * tile_sz, y as f32 * tile_sz), Vec2::splat(tile_sz));
            painter.rect_stroke(
                r,
                CornerRadius::ZERO,
                Stroke::new(2.0_f32, Color32::from_rgb(255, 220, 0)),
                StrokeKind::Outside,
            );
        }

        // ── Unsaved changes dialog ────────────────────────────
        if self.show_unsaved_dialog {
            egui::Window::new("⚠️  Unsaved Changes")
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label("You have unsaved changes to the spawn point.");
                    ui.label("Do you want to save before switching levels?");
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("💾 Save").clicked() {
                            self.request_rom_save = true;
                            if let Some(new_level) = self.pending_level_num {
                                self.level_num = new_level;
                                self.load_level();
                            }
                            self.show_unsaved_dialog = false;
                            self.pending_level_num = None;
                        }
                        if ui.button("❌ Discard").clicked() {
                            if let Some(new_level) = self.pending_level_num {
                                self.level_num = new_level;
                                self.load_level();
                            }
                            self.show_unsaved_dialog = false;
                            self.pending_level_num = None;
                        }
                        if ui.button("⏸️ Cancel").clicked() {
                            self.show_unsaved_dialog = false;
                            self.pending_level_num = None;
                        }
                    });
                });
        }

        // ── Close attempt with unsaved changes dialog ────────────────────────────
        if self.pending_close {
            egui::Window::new("⚠️  Unsaved Changes")
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label("You have unsaved changes.");
                    ui.label("Do you want to save before closing?");
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("💾 Save & Close").clicked() {
                            self.request_rom_save = true;
                            self.pending_close = false;
                        }
                        if ui.button("❌ Close Without Saving").clicked() {
                            self.has_edits = false;
                            self.pending_close = false;
                        }
                        if ui.button("⏸️ Cancel").clicked() {
                            self.pending_close = false;
                        }
                    });
                    ui.label("Click the X again to close the editor.");
                });
        }
    }

    /// Paint the Lunar Magic v3.31 "Mark exit-enabled tiles" overlay:
    /// Layer 1 tiles (plus the object-backed Layer 2 in level mode 0x01)
    /// whose act-as root is exit-enabled get a translucent green fill.
    /// Tile data comes straight from the WRAM block maps, so the markers
    /// track the live level including unsaved object edits and staged
    /// acts-like changes.
    fn paint_exit_enabled_overlay(
        &mut self, painter: &egui::Painter, view_rect: egui::Rect, origin: egui::Pos2, tile_sz: f32, level_w: u32,
        level_h: u32,
    ) {
        use smwe_rom::{block_behavior::is_exit_enabled, map16_expanded::act_as_of};

        let acts = self.effective_acts_table();
        let level_mode = self.level_properties.level_mode;
        // In level mode 0x01 the object-backed Layer 2 is interactive, so
        // the game evaluates its tiles for exits too.
        let l2_active = level_mode == 0x01 && self.level_properties.has_layer2;
        let l2_off = if self.level_properties.is_vertical { 0x0E * 16 * 32 } else { 0x10 * 16 * 27 };

        let x0 = ((view_rect.min.x - origin.x) / tile_sz).floor().max(0.0) as u32;
        let y0 = ((view_rect.min.y - origin.y) / tile_sz).floor().max(0.0) as u32;
        let x1 = ((view_rect.max.x - origin.x) / tile_sz).ceil().max(0.0).min(level_w as f32) as u32;
        let y1 = ((view_rect.max.y - origin.y) / tile_sz).ceil().max(0.0).min(level_h as f32) as u32;

        let fill = egui::Color32::from_rgba_unmultiplied(70, 220, 110, 70);
        let edge = egui::Color32::from_rgba_unmultiplied(70, 220, 110, 230);
        let stroke = egui::Stroke::new(1.5_f32, edge);

        for ty in y0..y1 {
            for tx in x0..x1 {
                let id = self.raw_block_id_at(tx, ty, 0x7EC800, 0x7FC800);
                let mut exit = id != 0 && is_exit_enabled(act_as_of(&acts, id), level_mode);
                if !exit && l2_active {
                    let id2 = self.raw_block_id_at(tx, ty, 0x7EC800 + l2_off, 0x7FC800 + l2_off);
                    exit = id2 != 0 && is_exit_enabled(act_as_of(&acts, id2), level_mode);
                }
                if exit {
                    let r = egui::Rect::from_min_size(
                        origin + egui::vec2(tx as f32 * tile_sz, ty as f32 * tile_sz),
                        egui::Vec2::splat(tile_sz),
                    );
                    painter.rect_filled(r, egui::CornerRadius::ZERO, fill);
                    painter.rect_stroke(r, egui::CornerRadius::ZERO, stroke, egui::StrokeKind::Inside);
                }
            }
        }
    }
}
