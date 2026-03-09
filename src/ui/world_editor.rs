use std::sync::Arc;

use egui::*;
use smwe_rom::{
    graphics::palette::{ColorPalette, OverworldState},
    SmwRom,
};

use crate::ui::tool::DockableEditorTool;

// ── Layout constants ──────────────────────────────────────────────────────────
const OW_NATIVE_W: f32 = 512.0;
const OW_NATIVE_H: f32 = 256.0;
const TILE_PX: f32 = 16.0; // 16x16 map tile

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

/// Level nodes: (level_num, label, tile_x, tile_y) per submap.
/// Tile positions are in 16-px tile units from map top-left.
const NODES: &[&[(u16, &str, f32, f32)]] = &[
    // Yoshi's Island (submap 0)
    &[
        (0x105, "YI 1",   2.5,  5.5),
        (0x104, "YI 2",   7.0,  5.5),
        (0x106, "YI 3",  11.5,  6.5),
        (0x107, "YI 4",  16.0,  5.5),
        (0x026, "Iggy",  20.5,  4.5),
        (0x10A, "House",  1.0,  3.0),
    ],
    // Donut Plains (submap 1)
    &[
        (0x001, "DP 1",   5.0,  7.0),
        (0x002, "DP 2",  10.0,  5.0),
        (0x003, "DP 3",  15.0,  7.0),
        (0x004, "DP 4",  19.0,  5.5),
        (0x005, "DP S1",  7.5,  3.0),
        (0x006, "DP S2", 12.5,  3.0),
        (0x025, "Morton",21.0,  4.0),
        (0x10B, "House2", 2.0,  4.0),
    ],
    // Vanilla Dome (submap 2)
    &[
        (0x007, "VD 1",   4.0,  4.0),
        (0x008, "VD 2",   8.5,  6.0),
        (0x009, "VD 3",  13.0,  4.5),
        (0x00A, "VD 4",  17.5,  6.0),
        (0x00B, "VD S1",  6.0,  2.0),
        (0x00C, "VD S2", 11.0,  2.0),
        (0x024, "Lemmy",  20.0,  4.5),
    ],
    // Twin Bridges (submap 3)
    &[
        (0x00D, "CB 1",   3.0,  8.0),
        (0x00E, "CB 2",   8.0,  7.0),
        (0x00F, "BB 1",  13.0,  7.5),
        (0x010, "BB 2",  18.0,  6.0),
        (0x022, "Ludwig", 21.0, 5.0),
        (0x023, "Roy",    10.0, 3.0),
    ],
    // Forest of Illusion (submap 4)
    &[
        (0x011, "FI 1",   5.0,  6.0),
        (0x012, "FI 2",  10.0,  8.0),
        (0x013, "FI 3",  15.0,  6.0),
        (0x014, "FI 4",  20.0,  8.0),
        (0x015, "FI S",   8.0,  3.5),
        (0x021, "Larry",  22.0, 7.0),
    ],
    // Chocolate Island (submap 5)
    &[
        (0x016, "CI 1",   5.0,  5.0),
        (0x017, "CI 2",  10.0,  7.5),
        (0x018, "CI 3",  15.0,  5.5),
        (0x019, "CI 4",  19.0,  7.0),
        (0x01A, "CI S",   8.0,  2.5),
        (0x020, "Wendy",  22.0, 6.0),
    ],
    // Valley of Bowser (submap 6)
    &[
        (0x01B, "VB 1",   4.0,  5.5),
        (0x01C, "VB 2",   9.0,  7.5),
        (0x01D, "VB 3",  14.0,  5.0),
        (0x01E, "VB 4",  19.0,  7.5),
        (0x01F, "VB S",   7.0,  3.0),
        (0x01F, "Bowser", 22.5, 6.5),
    ],
    // Star World (submap 7)
    &[
        (0x0DC, "SW 1",   4.0,  7.0),
        (0x0DD, "SW 2",   8.0,  4.0),
        (0x0DE, "SW 3",  13.0,  7.5),
        (0x0DF, "SW 4",  18.0,  4.5),
        (0x0E0, "SW 5",  22.0,  7.5),
    ],
    // Special World (submap 8)
    &[
        (0x0EF, "Gnarly",  2.5, 5.0),
        (0x0F0, "Tubular",  6.5, 5.0),
        (0x0F1, "Way Cool", 10.5, 5.0),
        (0x0F2, "Awesome", 14.5, 5.0),
        (0x0F3, "Groovy",  18.0, 5.5),
        (0x0F4, "Mondo",    2.5, 9.0),
        (0x0F5, "Outrageous",6.5,9.0),
        (0x0F6, "Funky",   10.5, 9.0),
    ],
];

/// Background colours (fallback, from OW palette index 0x40) per submap
const BG_FALLBACKS: &[Color32] = &[
    Color32::from_rgb(24, 56, 144),   // YI: sky blue
    Color32::from_rgb(100, 180, 80),  // DP: green
    Color32::from_rgb(80, 100, 160),  // VD: cave blue
    Color32::from_rgb(60, 60, 120),   // TB: dusk
    Color32::from_rgb(30, 80, 40),    // FI: forest
    Color32::from_rgb(140, 80, 60),   // CI: brown
    Color32::from_rgb(40, 40, 60),    // VB: dark
    Color32::from_rgb(20, 20, 60),    // SW: night
    Color32::from_rgb(200, 200, 255), // SP: special
];

// ── Struct ────────────────────────────────────────────────────────────────────

pub struct UiWorldEditor {
    rom:             Arc<SmwRom>,
    zoom:            f32,
    offset:          Vec2,
    selected_level:  Option<u16>,
    submap:          usize,
    show_grid:       bool,
    show_nodes:      bool,
    palette:         Vec<Color32>,
    /// Cached per-submap GFX texture (index 0 = OW GFX tile sheet)
    gfx_texture:     Option<TextureHandle>,
    /// Track which submap the texture was built for
    gfx_texture_sub: usize,
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
            gfx_texture: None,
            gfx_texture_sub: usize::MAX,
        }
    }

    /// Build a 16×(N÷16) tile GFX sheet texture from overworld GFX files 00 & 01.
    fn ensure_gfx_texture(&mut self, ctx: &Context) {
        if self.gfx_texture_sub == self.submap && self.gfx_texture.is_some() {
            return;
        }
        self.gfx_texture = build_ow_gfx_texture(ctx, &self.rom, self.submap);
        self.gfx_texture_sub = self.submap;
    }
}

impl DockableEditorTool for UiWorldEditor {
    fn title(&self) -> WidgetText {
        "World Map Editor".into()
    }

    fn update(&mut self, ui: &mut Ui) {
        self.ensure_gfx_texture(ui.ctx());
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
        ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(6.0);
            ui.heading("World Map");
            ui.separator();

            ui.label(RichText::new("Submap").strong());
            for (i, name) in SUBMAP_NAMES.iter().enumerate() {
                if ui.selectable_label(self.submap == i, *name).clicked() {
                    self.submap = i;
                    self.palette = build_ow_palette(&self.rom, i.min(5));
                    self.offset = Vec2::ZERO;
                    self.selected_level = None;
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
                    ui.label(format!("Mode:   {:02X}", h.level_mode()));
                    ui.label(format!("Music:  {}", h.music()));
                    ui.label(format!("Timer:  {}", h.timer()));
                    ui.label(format!(
                        "Layout: {}",
                        if level.secondary_header.vertical_level() { "Vertical" } else { "Horizontal" }
                    ));
                    ui.label(format!("GFX:    {:X}", h.fg_bg_gfx()));
                    ui.add_space(6.0);
                    if ui.button("🎮  Open in Level Editor").clicked() {
                        ui.data_mut(|d| d.insert_temp(Id::new("open_level_request"), lvl));
                    }
                }
            } else {
                ui.label(RichText::new("Click a node to select").color(Color32::GRAY).italics());
            }

            ui.separator();
            ui.label(RichText::new("GFX Preview").strong());
            ui.label(RichText::new("Overworld tile sheet (GFX 00‑01)").small().color(Color32::GRAY));
            // Show the tile sheet texture if available
            if let Some(tex) = &self.gfx_texture {
                let available = ui.available_width();
                let sz = tex.size_vec2();
                let scale = (available / sz.x).min(1.0);
                ui.add(
                    egui::Image::new(tex)
                        .fit_to_exact_size(sz * scale)
                        .maintain_aspect_ratio(true),
                );
            }

            ui.separator();
            ui.label(RichText::new("Stats").strong());
            ui.label(format!("{} levels parsed", self.rom.levels.len()));
            ui.label(format!("{} GFX files", self.rom.gfx.files.len()));
        });
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
                let old_zoom = self.zoom;
                self.zoom = (self.zoom * (1.0 + scroll * 0.001)).clamp(0.25, 10.0);
                // Zoom toward cursor
                if let Some(pos) = resp.hover_pos() {
                    let canvas_tl = resp.rect.min + self.offset * old_zoom;
                    let rel = pos - canvas_tl;
                    self.offset += rel / old_zoom - rel / self.zoom;
                }
            }
        }

        let z = self.zoom;
        let canvas_tl = resp.rect.min + self.offset * z;

        // ── Background ────────────────────────────────────────────────────────
        let bg = self.palette.get(0x40).copied()
            .unwrap_or_else(|| BG_FALLBACKS.get(self.submap).copied()
                .unwrap_or(Color32::from_rgb(24, 56, 144)));
        painter.rect_filled(resp.rect, Rounding::ZERO, bg);

        // ── Draw overworld GFX tile sheet as the map background texture ────────
        if let Some(tex) = &self.gfx_texture {
            // Tile the GFX sheet across the map canvas boundary
            let map_rect = Rect::from_min_size(canvas_tl, vec2(OW_NATIVE_W * z, OW_NATIVE_H * z));
            let visible = map_rect.intersect(resp.rect);
            if visible.is_positive() {
                // Draw at lower opacity so nodes are clearly visible
                painter.image(
                    tex.id(),
                    map_rect.with_max_x(map_rect.max.x.min(resp.rect.max.x))
                           .with_max_y(map_rect.max.y.min(resp.rect.max.y)),
                    Rect::from_min_size(Pos2::ZERO, vec2(1.0, 1.0)),
                    Color32::from_rgba_unmultiplied(255, 255, 255, 200),
                );
            }
        }

        // ── Optional tile grid ────────────────────────────────────────────────
        if self.show_grid {
            let cell = TILE_PX * z;
            let stroke = Stroke::new(0.5, Color32::from_white_alpha(25));
            let ox = (self.offset.x * z).rem_euclid(cell);
            let oy = (self.offset.y * z).rem_euclid(cell);
            let mut x = resp.rect.min.x + ox - cell;
            while x <= resp.rect.max.x {
                painter.vline(x, resp.rect.min.y..=resp.rect.max.y, stroke);
                x += cell;
            }
            let mut y = resp.rect.min.y + oy - cell;
            while y <= resp.rect.max.y {
                painter.hline(resp.rect.min.x..=resp.rect.max.x, y, stroke);
                y += cell;
            }
        }

        // ── Map boundary box ──────────────────────────────────────────────────
        let map_size = vec2(OW_NATIVE_W * z, OW_NATIVE_H * z);
        painter.rect_stroke(
            Rect::from_min_size(canvas_tl, map_size),
            Rounding::ZERO,
            Stroke::new(2.0, Color32::from_rgb(255, 255, 80)),
        );

        // ── Submap label ──────────────────────────────────────────────────────
        painter.text(
            canvas_tl + vec2(8.0, 6.0),
            Align2::LEFT_TOP,
            SUBMAP_NAMES.get(self.submap).copied().unwrap_or("?"),
            FontId::proportional(13.0 * z.sqrt().max(0.8)),
            Color32::from_rgba_unmultiplied(255, 255, 200, 230),
        );

        // ── Level nodes ───────────────────────────────────────────────────────
        if self.show_nodes {
            let nodes = NODES.get(self.submap).copied().unwrap_or(&[]);
            let mut clicked_level: Option<u16> = None;
            let pointer_pos = resp.interact_pointer_pos();
            let was_clicked = resp.clicked();

            for &(lvl_num, label, tx, ty) in nodes {
                let pos = canvas_tl + vec2(tx * TILE_PX * z, ty * TILE_PX * z);
                let r = (10.0 * z.sqrt()).max(6.0);
                let selected = self.selected_level == Some(lvl_num);

                // Glow ring for selected
                if selected {
                    painter.circle_filled(pos, r + 4.0,
                        Color32::from_rgba_unmultiplied(255, 220, 0, 60));
                }

                // Shadow
                painter.circle_filled(pos + vec2(2.0, 2.0) * z.sqrt(), r,
                    Color32::from_black_alpha(80));

                // Node fill — gradient-ish via two circles
                let fill = if selected {
                    Color32::from_rgb(255, 210, 30)
                } else {
                    Color32::from_rgb(200, 50, 50)
                };
                let fill_hi = if selected {
                    Color32::from_rgb(255, 240, 120)
                } else {
                    Color32::from_rgb(240, 90, 90)
                };
                painter.circle_filled(pos, r, fill);
                painter.circle_filled(pos - vec2(r * 0.25, r * 0.3), r * 0.55, fill_hi);

                // Border
                painter.circle_stroke(pos, r,
                    Stroke::new(if selected { 2.5 } else { 1.5 },
                        Color32::from_gray(if selected { 255 } else { 200 })));

                // Level number inside
                painter.text(
                    pos,
                    Align2::CENTER_CENTER,
                    format!("{lvl_num:X}"),
                    FontId::monospace((6.5 * z.sqrt()).max(6.0)),
                    Color32::BLACK,
                );

                // Label to the right
                if z >= 1.0 {
                    // Label background pill
                    let label_pos = pos + vec2(r + 4.0, 0.0);
                    let label_font = FontId::proportional((9.5 * z.sqrt()).max(8.0));
                    let galley = painter.layout_no_wrap(label.to_string(), label_font.clone(), Color32::WHITE);
                    let lw = galley.size().x + 6.0;
                    let lh = galley.size().y + 2.0;
                    let pill_rect = Rect::from_center_size(
                        label_pos + vec2(lw / 2.0, 0.0),
                        vec2(lw, lh),
                    );
                    painter.rect_filled(pill_rect, Rounding::same(3.0),
                        Color32::from_black_alpha(140));
                    painter.text(
                        label_pos,
                        Align2::LEFT_CENTER,
                        label,
                        label_font,
                        Color32::WHITE,
                    );
                }

                // Hit-test
                if was_clicked {
                    if let Some(p) = pointer_pos {
                        if p.distance(pos) <= r * 1.8 {
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

        // ── Tile coordinate readout ───────────────────────────────────────────
        if let Some(cursor) = resp.hover_pos() {
            let rel = (cursor - canvas_tl) / z;
            let tx = (rel.x / TILE_PX) as i32;
            let ty = (rel.y / TILE_PX) as i32;
            if rel.x >= 0.0 && rel.y >= 0.0
                && rel.x < OW_NATIVE_W && rel.y < OW_NATIVE_H
            {
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

// ── Palette helpers ───────────────────────────────────────────────────────────

fn build_ow_palette(rom: &SmwRom, submap: usize) -> Vec<Color32> {
    match rom.gfx.color_palettes.get_submap_palette(submap, OverworldState::PreSpecial) {
        Ok(pal) => (0..16_usize)
            .flat_map(|row| (0..16_usize).map(move |col| (row, col)))
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

/// Build an egui texture showing the decoded overworld GFX tiles (files 00 & 01),
/// arranged as a grid of 8×8 tiles. Returns None if GFX data unavailable.
fn build_ow_gfx_texture(ctx: &Context, rom: &SmwRom, submap: usize) -> Option<TextureHandle> {
    use smwe_render::color::Abgr1555;

    // Fetch the OW palette rows 4-7 (layer 2 object palette) from SpecificOverworldColorPalette
    let ow_pal = rom.gfx.color_palettes
        .get_submap_palette(submap.min(5), OverworldState::PreSpecial)
        .ok()?;

    // Build a 256-color CGRAM from the OW palette (rows 0-F, cols 0-F)
    let mut cgram: Vec<Abgr1555> = vec![Abgr1555::TRANSPARENT; 256];
    for row in 0..16usize {
        for col in 0..16usize {
            if let Some(c) = ow_pal.get_color_at(row, col) {
                cgram[row * 16 + col] = c;
            }
        }
    }

    // Collect tiles from GFX files 00 and 01 (overworld graphics)
    let mut all_tiles: Vec<&smwe_rom::graphics::gfx_file::Tile> = Vec::new();
    for file_idx in 0..2usize {
        if let Some(gfx) = rom.gfx.files.get(file_idx) {
            for tile in &gfx.tiles {
                all_tiles.push(tile);
            }
        }
    }

    if all_tiles.is_empty() {
        return None;
    }

    // Arrange tiles in rows of 16
    let cols: usize = 16;
    let rows = (all_tiles.len() + cols - 1) / cols;
    let img_w = cols * 8;
    let img_h = rows * 8;

    let mut pixels = vec![Color32::TRANSPARENT; img_w * img_h];

    for (tile_idx, tile) in all_tiles.iter().enumerate() {
        let tile_col = tile_idx % cols;
        let tile_row = tile_idx / cols;
        // OW tiles are 3bpp using palette rows 4-7; pick row 4 (index 4 in 16-row CGRAM)
        // Subpalette offset: row 4, col 0 = index 64
        let pal_row = 4usize; // OW layer1 palette starts at row 4
        let pal_offset = pal_row * 16;
        let sub_palette: Vec<Abgr1555> = cgram[pal_offset..pal_offset + 16].to_vec();

        for (pix_idx, &color_idx) in tile.color_indices.iter().enumerate() {
            let px = tile_col * 8 + (pix_idx % 8);
            let py = tile_row * 8 + (pix_idx / 8);
            if px < img_w && py < img_h {
                let abgr = if color_idx == 0 {
                    // transparent → dark checkerboard so tile boundaries show
                    if (px / 4 + py / 4) % 2 == 0 {
                        Color32::from_gray(60)
                    } else {
                        Color32::from_gray(45)
                    }
                } else {
                    let c = sub_palette.get(color_idx as usize)
                        .copied()
                        .unwrap_or(Abgr1555::MAGENTA);
                    Color32::from(c)
                };
                pixels[py * img_w + px] = abgr;
            }
        }
    }

    let color_image = ColorImage { size: [img_w, img_h], pixels };
    Some(ctx.load_texture(
        format!("ow_gfx_sheet_{submap}"),
        color_image,
        TextureOptions::NEAREST, // pixel art — no filtering
    ))
}
