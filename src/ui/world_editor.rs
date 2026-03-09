use std::sync::Arc;

use egui::*;
use smwe_rom::{
    graphics::palette::{ColorPalette, OverworldState},
    SmwRom,
};

use crate::ui::tool::DockableEditorTool;

const OW_NATIVE_W: f32 = 512.0;
const OW_NATIVE_H: f32 = 256.0;

const SUBMAP_NAMES: &[&str] = &[
    "Yoshi's Island",
    "Donut Plains",
    "Vanilla Dome",
    "Twin Bridges",
    "Forest of Illusion",
    "Chocolate Island",
    "Valley of Bowser",
    "Star World",
    "Special World",
];

// (level_num, label, tile_x, tile_y) — approximate node positions for Yoshi's Island
const YOSHI_ISLAND_NODES: &[(u16, &str, f32, f32)] = &[
    (0x105, "YI 1",  2.5,  5.5),
    (0x104, "YI 2",  7.0,  5.5),
    (0x106, "YI 3", 11.5,  6.5),
    (0x107, "YI 4", 16.0,  5.5),
    (0x026, "Iggy",  20.5,  4.5),
    (0x10A, "House",  1.0,  3.0),
];

pub struct UiWorldEditor {
    rom:            Arc<SmwRom>,
    zoom:           f32,
    offset:         Vec2,
    selected_level: Option<u16>,
    submap:         usize,
    show_grid:      bool,
    show_nodes:     bool,
    palette:        Vec<Color32>,
}

impl UiWorldEditor {
    pub fn new(rom: Arc<SmwRom>) -> Self {
        let palette = build_ow_palette(&rom, 0);
        Self {
            rom,
            zoom: 2.0,
            offset: Vec2::ZERO,
            selected_level: None,
            submap: 0,
            show_grid: false,
            show_nodes: true,
            palette,
        }
    }
}

impl DockableEditorTool for UiWorldEditor {
    fn title(&self) -> WidgetText {
        "World Map Editor".into()
    }

    fn update(&mut self, ui: &mut Ui) {
        SidePanel::left("world_editor.left")
            .resizable(true)
            .default_width(200.0)
            .show_inside(ui, |ui| self.left_panel(ui));
        CentralPanel::default()
            .frame(Frame::none())
            .show_inside(ui, |ui| self.canvas(ui));
    }
}

impl UiWorldEditor {
    fn left_panel(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        ui.heading("World Map");
        ui.separator();

        ui.label(RichText::new("Submap").strong());
        for (i, name) in SUBMAP_NAMES.iter().enumerate() {
            if ui.selectable_label(self.submap == i, *name).clicked() {
                self.submap = i;
                self.palette = build_ow_palette(&self.rom, i.min(5));
                self.offset = Vec2::ZERO;
            }
        }

        ui.separator();
        ui.label(RichText::new("View").strong());
        ui.add(Slider::new(&mut self.zoom, 0.5..=6.0).step_by(0.25).text("Zoom"));
        ui.checkbox(&mut self.show_grid, "Show tile grid");
        ui.checkbox(&mut self.show_nodes, "Show level nodes");
        if ui.button("Reset view").clicked() {
            self.zoom = 2.0;
            self.offset = Vec2::ZERO;
        }

        ui.separator();
        if let Some(lvl) = self.selected_level {
            ui.label(RichText::new("Selected Level").strong());
            ui.label(format!("Level #{lvl:03X}"));
            if (lvl as usize) < self.rom.levels.len() {
                let level = &self.rom.levels[lvl as usize];
                let h = &level.primary_header;
                ui.label(format!("Mode:  {:02X}", h.level_mode()));
                ui.label(format!("Music: {}", h.music()));
                ui.label(format!("Timer: {}", h.timer()));
                ui.label(format!(
                    "Layout: {}",
                    if level.secondary_header.vertical_level() { "Vertical" } else { "Horizontal" }
                ));
                ui.label(format!("GFX:   {:X}", h.fg_bg_gfx()));
                ui.add_space(6.0);
                if ui.button("Open in Level Editor").clicked() {
                    ui.data_mut(|d| d.insert_temp(Id::new("open_level_request"), lvl));
                }
            }
        } else {
            ui.label(RichText::new("Click a node to select").color(Color32::GRAY).italics());
        }

        ui.separator();
        ui.label(RichText::new("Stats").strong());
        ui.label(format!("{} levels parsed", self.rom.levels.len()));
        ui.label(format!("{} GFX files", self.rom.gfx.files.len()));
    }

    fn canvas(&mut self, ui: &mut Ui) {
        let (resp, painter) =
            ui.allocate_painter(ui.available_size(), Sense::click_and_drag());

        // Pan
        if resp.dragged() {
            self.offset += resp.drag_delta() / self.zoom;
        }
        // Zoom
        if resp.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                self.zoom = (self.zoom * (1.0 + scroll * 0.001)).clamp(0.25, 10.0);
            }
        }

        let z = self.zoom;
        let canvas_tl = resp.rect.min;

        // Background fill
        let bg = self.palette.get(0x40).copied().unwrap_or(Color32::from_rgb(24, 56, 144));
        painter.rect_filled(resp.rect, Rounding::ZERO, bg);

        // Optional tile grid
        if self.show_grid {
            let cell = 16.0 * z;
            let stroke = Stroke::new(0.5, Color32::from_white_alpha(25));
            let ox = (self.offset.x * z).rem_euclid(cell);
            let oy = (self.offset.y * z).rem_euclid(cell);
            let mut x = canvas_tl.x + ox - cell;
            while x <= resp.rect.max.x {
                painter.vline(x, canvas_tl.y..=resp.rect.max.y, stroke);
                x += cell;
            }
            let mut y = canvas_tl.y + oy - cell;
            while y <= resp.rect.max.y {
                painter.hline(canvas_tl.x..=resp.rect.max.x, y, stroke);
                y += cell;
            }
        }

        // Map boundary box
        let tl = canvas_tl + self.offset * z;
        let map_size = vec2(OW_NATIVE_W * z, OW_NATIVE_H * z);
        painter.rect_stroke(
            Rect::from_min_size(tl, map_size),
            Rounding::ZERO,
            Stroke::new(2.0, Color32::WHITE),
        );

        // Submap label
        painter.text(
            tl + vec2(6.0, 6.0),
            Align2::LEFT_TOP,
            SUBMAP_NAMES.get(self.submap).copied().unwrap_or("?"),
            FontId::proportional(12.0 * z.sqrt()),
            Color32::from_white_alpha(200),
        );

        // Level nodes (Yoshi's Island only for now)
        if self.show_nodes && self.submap == 0 {
            let mut clicked_level: Option<u16> = None;
            let pointer_pos = resp.interact_pointer_pos();
            let was_clicked = resp.clicked();

            for &(lvl_num, label, tx, ty) in YOSHI_ISLAND_NODES {
                let pos = tl + vec2(tx * 16.0 * z, ty * 16.0 * z);
                let r = (9.0 * z.sqrt()).max(5.0);
                let selected = self.selected_level == Some(lvl_num);
                let fill = if selected {
                    Color32::from_rgb(255, 200, 0)
                } else {
                    Color32::from_rgb(210, 50, 50)
                };
                painter.circle(
                    pos,
                    r,
                    fill,
                    Stroke::new(if selected { 2.5 } else { 1.5 }, Color32::from_gray(220)),
                );
                painter.text(
                    pos,
                    Align2::CENTER_CENTER,
                    format!("{lvl_num:X}"),
                    FontId::monospace(7.0 * z.sqrt()),
                    Color32::BLACK,
                );
                if z >= 1.2 {
                    painter.text(
                        pos + vec2(r + 3.0, 0.0),
                        Align2::LEFT_CENTER,
                        label,
                        FontId::proportional(9.0 * z.sqrt()),
                        Color32::WHITE,
                    );
                }
                // Hit-test
                if was_clicked {
                    if let Some(p) = pointer_pos {
                        if p.distance(pos) <= r * 1.5 {
                            clicked_level = Some(lvl_num);
                        }
                    }
                }
            }

            if let Some(l) = clicked_level {
                self.selected_level = Some(l);
            } else if was_clicked {
                self.selected_level = None;
            }
        }

        // Tile coordinate readout
        if let Some(cursor) = resp.hover_pos() {
            let rel = (cursor - tl) / z;
            let tx = (rel.x / 16.0) as i32;
            let ty = (rel.y / 16.0) as i32;
            if rel.x >= 0.0 && rel.y >= 0.0 {
                painter.text(
                    resp.rect.right_bottom() - vec2(6.0, 6.0),
                    Align2::RIGHT_BOTTOM,
                    format!("Tile ({tx}, {ty})  {:.0}%", z * 100.0),
                    FontId::monospace(11.0),
                    Color32::from_white_alpha(160),
                );
            }
        }
    }
}

fn build_ow_palette(rom: &SmwRom, submap: usize) -> Vec<Color32> {
    match rom.gfx.color_palettes.get_submap_palette(submap, OverworldState::PreSpecial) {
        Ok(pal) => (0..16_usize)
            .flat_map(|row| {
                (0..16_usize).map(move |col| (row, col))
            })
            .map(|(row, col)| {
                let c = pal
                    .get_color_at(row, col)
                    .unwrap_or(smwe_rom::graphics::palette::ColorPalettes::TRANSPARENT);
                Color32::from(c)
            })
            .collect(),
        Err(_) => vec![Color32::from_rgb(24, 56, 144); 256],
    }
}
