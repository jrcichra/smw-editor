use egui::{Color32, Context, Rect, Sense, Ui, Vec2, vec2};

use super::UiLevelEditor;

const CELL_SIZE: f32 = 20.0;
const COLS: usize = 12;

impl UiLevelEditor {
    pub(super) fn palette_editor_window(&mut self, ctx: &Context) {
        if !self.show_palette_editor {
            return;
        }
        let mut open = self.show_palette_editor;
        egui::Window::new("Palette Editor")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Click a color swatch to edit it. Changes save with Ctrl+S.");
                ui.separator();

                self.palette_group(ui, "BG Palette", 0);
                ui.separator();
                self.palette_group(ui, "FG Palette", 1);
                ui.separator();
                self.palette_group(ui, "Sprite Palette", 2);
            });
        self.show_palette_editor = open;
    }

    fn palette_group(&mut self, ui: &mut Ui, label: &str, group: usize) {
        let p = &self.level_properties;
        let (index, rom_label) = match group {
            0 => (p.palette_bg, format!("(index {:X})", p.palette_bg)),
            1 => (p.palette_fg, format!("(index {:X})", p.palette_fg)),
            _ => (p.palette_sprite, format!("(index {:X})", p.palette_sprite)),
        };
        ui.label(format!("{} {}", label, rom_label));

        let colors: &mut [u16; 12] = match group {
            0 => &mut self.palette_bg_colors,
            1 => &mut self.palette_fg_colors,
            _ => &mut self.palette_sprite_colors,
        };

        // Draw grid of colored cells
        let total_w = CELL_SIZE * COLS as f32;
        let (grid_rect, _) = ui.allocate_exact_size(vec2(total_w, CELL_SIZE), Sense::hover());

        let mut changed = false;
        for col in 0..COLS {
            let raw = colors[col];
            let cell_min = grid_rect.min + vec2(col as f32 * CELL_SIZE, 0.0);
            let cell_rect = Rect::from_min_size(cell_min, Vec2::splat(CELL_SIZE));

            let r = ((raw & 0x1F) as f32 / 31.0 * 255.0) as u8;
            let g = (((raw >> 5) & 0x1F) as f32 / 31.0 * 255.0) as u8;
            let b = (((raw >> 10) & 0x1F) as f32 / 31.0 * 255.0) as u8;
            let c32 = Color32::from_rgb(r, g, b);

            // Fill the cell
            ui.painter().rect_filled(cell_rect, egui::CornerRadius::ZERO, c32);
            // Thin border
            ui.painter().rect_stroke(
                cell_rect,
                egui::CornerRadius::ZERO,
                egui::Stroke::new(1.0, Color32::from_gray(80)),
                egui::StrokeKind::Outside,
            );

            // Highlight selected cell
            let selected = self.selected_palette_group == group as u8
                && self.selected_palette_idx == col
                && self.selected_palette_group < 3;
            if selected {
                ui.painter().rect_stroke(
                    cell_rect,
                    egui::CornerRadius::ZERO,
                    egui::Stroke::new(2.0, Color32::WHITE),
                    egui::StrokeKind::Outside,
                );
            }

            // Detect click
            let resp = ui.interact(cell_rect, egui::Id::new(("pal_cell", group, col, index)), Sense::click());
            if resp.clicked() {
                self.selected_palette_group = group as u8;
                self.selected_palette_idx = col;
            }
        }

        // If this group has a selected cell, show a color picker below
        if self.selected_palette_group == group as u8 && self.selected_palette_idx < COLS {
            let col = self.selected_palette_idx;
            let raw = colors[col];
            let mut c32 = Color32::from_rgb(
                ((raw & 0x1F) as f32 / 31.0 * 255.0) as u8,
                (((raw >> 5) & 0x1F) as f32 / 31.0 * 255.0) as u8,
                (((raw >> 10) & 0x1F) as f32 / 31.0 * 255.0) as u8,
            );
            ui.horizontal(|ui| {
                ui.label(format!("Color {}:", col));
                if ui.color_edit_button_srgba(&mut c32).changed() {
                    // Convert sRGBA back to ABGR1555
                    let r5 = (c32.r() as u16 * 31 / 255) & 0x1F;
                    let g5 = (c32.g() as u16 * 31 / 255) & 0x1F;
                    let b5 = (c32.b() as u16 * 31 / 255) & 0x1F;
                    colors[col] = r5 | (g5 << 5) | (b5 << 10);
                    changed = true;
                }
                let raw2 = colors[col];
                ui.monospace(format!("{:04X}", raw2));
            });
        }

        if changed {
            self.palette_dirty = true;
            self.mark_edited();
        }
    }
}
