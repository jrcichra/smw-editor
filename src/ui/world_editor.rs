use std::sync::Arc;

use egui::*;
use smwe_rom::{
    graphics::palette::OverworldState,
    overworld::{BgTile, OwTilemap, OW_SUBMAP_COUNT, OW_TILEMAP_COLS, OW_VISIBLE_ROWS},
    SmwRom,
};
use smwe_render::color::Abgr1555;

use crate::ui::tool::DockableEditorTool;

// ── Layout constants ──────────────────────────────────────────────────────────

const MAP_TILE_COLS: usize = OW_TILEMAP_COLS; // 32
const MAP_TILE_ROWS: usize = OW_VISIBLE_ROWS; // 27
const TILE_PX: f32 = 8.0;
const MAP_W_PX: f32 = MAP_TILE_COLS as f32 * TILE_PX; // 256 px
const MAP_H_PX: f32 = MAP_TILE_ROWS as f32 * TILE_PX; // 216 px

const SUBMAP_NAMES: &[&str] = &[
    "Yoshi's Island",
    "Donut Plains",
    "Vanilla Dome",
    "Twin Bridges",
    "Forest of Illusion",
    "Chocolate Island",
];

// ── Struct ────────────────────────────────────────────────────────────────────

pub struct UiWorldEditor {
    rom: Arc<SmwRom>,

    zoom:        f32,
    offset:      Vec2,
    submap:      usize,
    show_grid:   bool,
    show_layer1: bool,

    /// Currently selected tile on the map canvas (col, row).
    selected_tile: Option<(usize, usize)>,

    /// CHR tile index chosen in the tile-sheet picker for painting.
    paint_tile:    Option<u16>,
    /// Sub-palette used when painting (0-3).
    paint_palette: u8,

    /// Local editable copy of layer-2 tilemaps (one per submap).
    /// Initialised from ROM on open, modified in-editor.
    local_layer2: Vec<OwTilemap>,

    /// Number of unsaved edits.
    edit_count: usize,

    // Cached textures
    map_texture:       Option<TextureHandle>,
    map_texture_sub:   usize,
    map_texture_l1:    bool,
    sheet_texture:     Option<TextureHandle>,
    sheet_texture_sub: usize,
}

impl UiWorldEditor {
    pub fn new(rom: Arc<SmwRom>) -> Self {
        // Clone layer-2 tilemaps into our local editable copy
        let local_layer2: Vec<OwTilemap> = (0..OW_SUBMAP_COUNT)
            .map(|sm| {
                rom.overworld
                    .layer2
                    .get(sm)
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();

        Self {
            rom,
            zoom: 3.0,
            offset: Vec2::ZERO,
            submap: 0,
            show_grid: true,
            show_layer1: true,
            selected_tile: None,
            paint_tile: None,
            paint_palette: 0,
            local_layer2,
            edit_count: 0,
            map_texture: None,
            map_texture_sub: usize::MAX,
            map_texture_l1: false,
            sheet_texture: None,
            sheet_texture_sub: usize::MAX,
        }
    }

    fn invalidate_map(&mut self) {
        self.map_texture = None;
        self.map_texture_sub = usize::MAX;
    }

    fn ensure_map_texture(&mut self, ctx: &Context) {
        let needs = self.map_texture.is_none()
            || self.map_texture_sub != self.submap
            || self.map_texture_l1 != self.show_layer1;
        if !needs { return; }

        if let Some(img) = render_ow_map(
            &self.rom,
            &self.local_layer2,
            self.submap,
            self.show_layer1,
        ) {
            self.map_texture = Some(ctx.load_texture(
                format!("ow_map_{}_{}", self.submap, self.show_layer1),
                img,
                TextureOptions::NEAREST,
            ));
        }
        self.map_texture_sub = self.submap;
        self.map_texture_l1 = self.show_layer1;
    }

    fn ensure_sheet_texture(&mut self, ctx: &Context) {
        if self.sheet_texture.is_some() && self.sheet_texture_sub == self.submap { return; }
        if let Some(img) = render_tile_sheet(&self.rom, self.submap) {
            self.sheet_texture = Some(ctx.load_texture(
                format!("ow_sheet_{}", self.submap),
                img,
                TextureOptions::NEAREST,
            ));
        }
        self.sheet_texture_sub = self.submap;
    }
}

impl DockableEditorTool for UiWorldEditor {
    fn title(&self) -> WidgetText { "World Map Editor".into() }

    fn update(&mut self, ui: &mut Ui) {
        self.ensure_map_texture(ui.ctx());
        self.ensure_sheet_texture(ui.ctx());

        SidePanel::left("world_editor.left")
            .resizable(true)
            .default_width(210.0)
            .show_inside(ui, |ui| self.left_panel(ui));

        SidePanel::right("world_editor.right")
            .resizable(true)
            .default_width(195.0)
            .show_inside(ui, |ui| self.right_panel(ui));

        CentralPanel::default()
            .frame(Frame::none())
            .show_inside(ui, |ui| self.canvas(ui));
    }
}

impl UiWorldEditor {
    // ── Left panel ───────────────────────────────────────────────────────────

    fn left_panel(&mut self, ui: &mut Ui) {
        ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(6.0);
            ui.heading("🗺  World Map Editor");
            ui.separator();

            // Submap selector
            ui.label(RichText::new("Submap").strong());
            let mut submap_changed = false;
            for (i, name) in SUBMAP_NAMES.iter().enumerate() {
                if ui.selectable_label(self.submap == i, *name).clicked() && self.submap != i {
                    self.submap = i;
                    self.offset = Vec2::ZERO;
                    self.selected_tile = None;
                    submap_changed = true;
                }
            }
            if submap_changed { self.invalidate_map(); }

            ui.separator();
            ui.label(RichText::new("View").strong());
            ui.add(Slider::new(&mut self.zoom, 1.0..=8.0).step_by(0.5).text("Zoom"));
            if ui.checkbox(&mut self.show_layer1, "Layer 1 (paths/events)").changed() {
                self.invalidate_map();
            }
            ui.checkbox(&mut self.show_grid, "Tile grid");
            if ui.button("Reset view").clicked() {
                self.zoom = 3.0;
                self.offset = Vec2::ZERO;
            }

            ui.separator();
            ui.label(RichText::new("Paint").strong());
            match self.paint_tile {
                Some(t) => { ui.label(format!("Tile  CHR #{t:#05x}")); }
                None    => { ui.label(RichText::new("← Pick from tile sheet").color(Color32::GRAY).italics()); }
            }
            ui.horizontal(|ui| {
                ui.label("Palette:");
                for p in 0u8..4 {
                    if ui.selectable_label(self.paint_palette == p,
                        format!("{p}")).clicked()
                    {
                        self.paint_palette = p;
                    }
                }
            });
            ui.label(RichText::new("Pick tile → click map to paint\nRight-click to deselect").small().color(Color32::GRAY));

            if self.edit_count > 0 {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("⚠ {} unsaved edit(s)", self.edit_count))
                        .color(Color32::from_rgb(255, 200, 60)),
                );
            }

            ui.separator();
            ui.label(RichText::new("Selected tile").strong());
            match self.selected_tile {
                Some((col, row)) => {
                    ui.label(format!("Position: ({col}, {row})"));
                    if let Some(tm) = self.local_layer2.get(self.submap) {
                        let t = tm.get(col, row);
                        ui.label(format!("CHR  #{:03X}", t.tile_index()));
                        ui.label(format!("Pal  {}", t.palette()));
                        ui.label(format!("Flip X={}  Y={}", t.flip_x(), t.flip_y()));
                    }
                }
                None => { ui.label(RichText::new("None").color(Color32::GRAY).italics()); }
            }

            ui.separator();
            if ui.button(RichText::new("💾  Save ROM…").color(Color32::from_rgb(80, 210, 80))).clicked() {
                std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Save ROM")
                    .add_filter("SNES ROM", &["smc", "sfc"])
                    .save_file()
                {
                    match self.save_rom_to(&path) {
                        Ok(()) => {
                            self.edit_count = 0;
                            log::info!("Saved ROM to {}", path.display());
                        }
                        Err(e) => log::error!("Save failed: {e}"),
                    }
                }
            }

            ui.separator();
            ui.label(RichText::new("Stats").strong());
            ui.label(format!("GFX files: {}", self.rom.gfx.files.len()));
            ui.label(format!("Levels:    {}", self.rom.levels.len()));
        });
    }

    /// Write the ROM with our local layer-2 edits patched in.
    fn save_rom_to(&self, path: &std::path::Path) -> anyhow::Result<()> {
        use std::io::Write;
        let mut bytes = self.rom.disassembly.rom.0.to_vec();
        // Patch our local (possibly edited) layer2 maps
        smwe_rom::overworld::write_layer2_to_rom(&mut bytes, &self.local_layer2);
        // Patch layer1 from original ROM (unchanged)
        self.rom.overworld.write_layer1_to_rom(&mut bytes);
        let mut f = std::fs::File::create(path)?;
        f.write_all(&bytes)?;
        Ok(())
    }

    // ── Right panel: tile-sheet picker ───────────────────────────────────────

    fn right_panel(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        ui.label(RichText::new("Tile Sheet").strong());
        ui.label(RichText::new("GFX 00 + 01  (3bpp)").small().color(Color32::GRAY));
        ui.separator();

        if let Some(tex) = self.sheet_texture.clone() {
            let tex_size = tex.size_vec2();
            let avail_w = ui.available_width().min(180.0);
            let scale = avail_w / tex_size.x;
            let display_size = tex_size * scale;

            let (resp, painter) = ui.allocate_painter(display_size, Sense::click());

            painter.image(
                tex.id(),
                resp.rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );

            let sheet_cols = (tex_size.x as usize / 8).max(1);
            let cell_px = display_size.x / sheet_cols as f32;

            // Highlight selected tile
            if let Some(sel) = self.paint_tile {
                let sc = (sel as usize) % sheet_cols;
                let sr = (sel as usize) / sheet_cols;
                painter.rect_stroke(
                    Rect::from_min_size(
                        resp.rect.min + vec2(sc as f32 * cell_px, sr as f32 * cell_px),
                        Vec2::splat(cell_px),
                    ),
                    Rounding::ZERO,
                    Stroke::new(1.5, Color32::from_rgb(255, 220, 0)),
                );
            }

            // Hover highlight + tooltip
            if let Some(pos) = resp.hover_pos() {
                let rel = pos - resp.rect.min;
                let sc = (rel.x / cell_px) as usize;
                let sr = (rel.y / cell_px) as usize;
                painter.rect_stroke(
                    Rect::from_min_size(
                        resp.rect.min + vec2(sc as f32 * cell_px, sr as f32 * cell_px),
                        Vec2::splat(cell_px),
                    ),
                    Rounding::ZERO,
                    Stroke::new(1.0, Color32::from_white_alpha(100)),
                );
                let idx = sr * sheet_cols + sc;
                painter.text(
                    resp.rect.left_bottom() + vec2(2.0, -12.0),
                    Align2::LEFT_BOTTOM,
                    format!("CHR #{idx:#05x}"),
                    FontId::monospace(9.0),
                    Color32::from_white_alpha(220),
                );
            }

            // Click: choose paint tile
            if resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let rel = pos - resp.rect.min;
                    let sc = (rel.x / cell_px) as usize;
                    let sr = (rel.y / cell_px) as usize;
                    let idx = sr * sheet_cols + sc;
                    let total = self.rom.gfx.files.get(0).map(|f| f.tiles.len()).unwrap_or(0)
                              + self.rom.gfx.files.get(1).map(|f| f.tiles.len()).unwrap_or(0);
                    if idx < total.min(256) {
                        self.paint_tile = Some(idx as u16);
                    }
                }
            }
        } else {
            ui.label(RichText::new("No GFX data.").color(Color32::GRAY).italics());
        }
    }

    // ── Canvas ───────────────────────────────────────────────────────────────

    fn canvas(&mut self, ui: &mut Ui) {
        let (resp, painter) =
            ui.allocate_painter(ui.available_size(), Sense::click_and_drag());

        // Pan with middle-mouse or alt+drag
        if resp.dragged_by(PointerButton::Middle)
            || (resp.dragged() && ui.input(|i| i.modifiers.alt))
        {
            self.offset += resp.drag_delta() / self.zoom;
        }

        // Scroll to zoom
        if resp.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                let old = self.zoom;
                self.zoom = (self.zoom * (1.0 + scroll * 0.0015)).clamp(0.5, 12.0);
                if let Some(pos) = resp.hover_pos() {
                    let tl = resp.rect.min + self.offset * old;
                    let rel = pos - tl;
                    self.offset += rel / old - rel / self.zoom;
                }
            }
        }

        let z = self.zoom;
        let canvas_tl = resp.rect.min + self.offset * z;
        let map_rect = Rect::from_min_size(canvas_tl, vec2(MAP_W_PX * z, MAP_H_PX * z));

        // Canvas background
        painter.rect_filled(resp.rect, Rounding::ZERO, Color32::from_gray(28));

        // Tilemap texture
        if let Some(tex) = &self.map_texture {
            painter.image(
                tex.id(),
                map_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            painter.rect_filled(map_rect, Rounding::ZERO, Color32::from_rgb(20, 40, 100));
            painter.text(
                map_rect.center(),
                Align2::CENTER_CENTER,
                "Rendering…",
                FontId::proportional(14.0),
                Color32::WHITE,
            );
        }

        // Map border
        painter.rect_stroke(
            map_rect,
            Rounding::ZERO,
            Stroke::new(2.0, Color32::from_rgb(255, 220, 0)),
        );

        // Submap name label
        if z >= 1.5 {
            painter.text(
                canvas_tl + vec2(6.0, 4.0),
                Align2::LEFT_TOP,
                SUBMAP_NAMES.get(self.submap).copied().unwrap_or("?"),
                FontId::proportional((11.0 * z.sqrt()).max(9.0)),
                Color32::from_rgba_unmultiplied(255, 240, 180, 200),
            );
        }

        // Tile grid
        if self.show_grid {
            let cell = TILE_PX * z;
            let stroke = Stroke::new(0.5, Color32::from_white_alpha(18));
            for col in 0..=MAP_TILE_COLS {
                let x = canvas_tl.x + col as f32 * cell;
                painter.vline(x, canvas_tl.y..=canvas_tl.y + MAP_H_PX * z, stroke);
            }
            for row in 0..=MAP_TILE_ROWS {
                let y = canvas_tl.y + row as f32 * cell;
                painter.hline(canvas_tl.x..=canvas_tl.x + MAP_W_PX * z, y, stroke);
            }
        }

        // Hovered tile coordinates
        let hovered_tile: Option<(usize, usize)> = resp.hover_pos().and_then(|pos| {
            let rel = (pos - canvas_tl) / z;
            let col = (rel.x / TILE_PX) as i32;
            let row = (rel.y / TILE_PX) as i32;
            if col >= 0 && row >= 0
                && (col as usize) < MAP_TILE_COLS
                && (row as usize) < MAP_TILE_ROWS
            {
                Some((col as usize, row as usize))
            } else {
                None
            }
        });

        // Hover highlight
        if let Some((col, row)) = hovered_tile {
            let cell = TILE_PX * z;
            painter.rect_filled(
                Rect::from_min_size(
                    canvas_tl + vec2(col as f32 * cell, row as f32 * cell),
                    Vec2::splat(cell),
                ),
                Rounding::ZERO,
                Color32::from_white_alpha(22),
            );
            painter.text(
                resp.rect.right_bottom() - vec2(6.0, 6.0),
                Align2::RIGHT_BOTTOM,
                format!("({col}, {row})  {:.0}%", z * 100.0),
                FontId::monospace(11.0),
                Color32::from_white_alpha(180),
            );
        }

        // Selection highlight
        if let Some((col, row)) = self.selected_tile {
            let cell = TILE_PX * z;
            painter.rect_stroke(
                Rect::from_min_size(
                    canvas_tl + vec2(col as f32 * cell, row as f32 * cell),
                    Vec2::splat(cell),
                ),
                Rounding::ZERO,
                Stroke::new(1.5, Color32::from_rgb(255, 60, 60)),
            );
        }

        // Left-click: paint or select
        if resp.clicked() {
            if let Some((col, row)) = hovered_tile {
                if let Some(chr) = self.paint_tile {
                    // Paint the tile
                    let new_tile = BgTile::new(chr, self.paint_palette, false, false, false);
                    if let Some(tm) = self.local_layer2.get_mut(self.submap) {
                        tm.set(col, row, new_tile);
                        self.edit_count += 1;
                    }
                    self.invalidate_map();
                } else {
                    self.selected_tile = Some((col, row));
                }
            }
        }

        // Right-click: cancel paint / deselect
        if resp.clicked_by(PointerButton::Secondary) {
            self.paint_tile = None;
            self.selected_tile = None;
        }
    }
}

// ── Rendering helpers ─────────────────────────────────────────────────────────

/// Build a 256-entry CGRAM for the given OW submap.
fn build_cgram(rom: &SmwRom, submap: usize) -> Vec<Abgr1555> {
    use smwe_rom::graphics::palette::ColorPalette;
    let sm = submap.min(5);
    match rom.gfx.color_palettes.get_submap_palette(sm, OverworldState::PreSpecial) {
        Ok(pal) => (0..256usize)
            .map(|i| pal.get_color_at(i / 16, i % 16).unwrap_or(Abgr1555::TRANSPARENT))
            .collect(),
        Err(_) => vec![Abgr1555::TRANSPARENT; 256],
    }
}

/// Decode one 8×8 CHR tile with flip and sub-palette into 64 Color32 pixels.
/// `cgram_row` selects which of the 16 palette rows (×16 colours each) to use.
fn decode_tile_into(
    tile: &smwe_rom::graphics::gfx_file::Tile,
    cgram: &[Abgr1555],
    cgram_row: usize,
    flip_x: bool,
    flip_y: bool,
    out: &mut [Color32; 64],
) {
    let base = cgram_row * 16;
    for py in 0..8usize {
        for px in 0..8usize {
            let src_py = if flip_y { 7 - py } else { py };
            let src_px = if flip_x { 7 - px } else { px };
            let ci = tile.color_indices.get(src_py * 8 + src_px).copied().unwrap_or(0) as usize;
            out[py * 8 + px] = if ci == 0 {
                Color32::TRANSPARENT
            } else {
                Color32::from(cgram.get(base + ci).copied().unwrap_or(Abgr1555::TRANSPARENT))
            };
        }
    }
}

/// Render the OW map for a submap into a 256×216 RGBA image.
/// Uses `local_layer2` for layer-2 (to include any edits) and rom.overworld.layer1 for layer-1.
fn render_ow_map(
    rom: &SmwRom,
    local_layer2: &[OwTilemap],
    submap: usize,
    show_layer1: bool,
) -> Option<ColorImage> {
    let sm = submap.min(5);
    let layer2 = local_layer2.get(sm)?;
    let cgram = build_cgram(rom, sm);

    let img_w = MAP_TILE_COLS * 8;
    let img_h = MAP_TILE_ROWS * 8;
    let mut pixels = vec![Color32::from_gray(20); img_w * img_h];
    let mut buf = [Color32::TRANSPARENT; 64];

    let get_tile = |chr: usize| -> Option<&smwe_rom::graphics::gfx_file::Tile> {
        if chr < 128 {
            rom.gfx.files.get(0)?.tiles.get(chr)
        } else {
            rom.gfx.files.get(1)?.tiles.get(chr - 128)
        }
    };

    // ── Layer 2: terrain background ──────────────────────────────────────────
    // OW sub-palette 0-3 → CGRAM rows 4-7
    for row in 0..MAP_TILE_ROWS {
        for col in 0..MAP_TILE_COLS {
            let entry = layer2.get(col, row);
            let chr = entry.tile_index() as usize;
            if let Some(tile) = get_tile(chr) {
                let cgram_row = 4 + (entry.palette() as usize & 3);
                decode_tile_into(tile, &cgram, cgram_row, entry.flip_x(), entry.flip_y(), &mut buf);
                let dx = col * 8;
                let dy = row * 8;
                for py in 0..8 {
                    for px in 0..8 {
                        let c = buf[py * 8 + px];
                        if c.a() > 0 {
                            pixels[(dy + py) * img_w + (dx + px)] = c;
                        }
                    }
                }
            }
        }
    }

    // ── Layer 1: paths / events ──────────────────────────────────────────────
    // OW layer-1 typically uses CGRAM rows 2-3
    if show_layer1 {
        if let Some(layer1) = rom.overworld.layer1.get(sm) {
            for row in 0..MAP_TILE_ROWS {
                for col in 0..MAP_TILE_COLS {
                    let entry = layer1.get(col, row);
                    if entry.tile_index() == 0 { continue; }
                    let chr = entry.tile_index() as usize;
                    if let Some(tile) = get_tile(chr) {
                        let cgram_row = 2 + (entry.palette() as usize & 1);
                        decode_tile_into(tile, &cgram, cgram_row, entry.flip_x(), entry.flip_y(), &mut buf);
                        let dx = col * 8;
                        let dy = row * 8;
                        for py in 0..8 {
                            for px in 0..8 {
                                let c = buf[py * 8 + px];
                                if c.a() > 0 {
                                    pixels[(dy + py) * img_w + (dx + px)] = c;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Some(ColorImage { size: [img_w, img_h], pixels })
}

/// Render a 128×128 tile-sheet image (16 cols × 16 rows, 8px each) of GFX 00+01.
fn render_tile_sheet(rom: &SmwRom, submap: usize) -> Option<ColorImage> {
    let sm = submap.min(5);
    let cgram = build_cgram(rom, sm);

    let mut tiles: Vec<&smwe_rom::graphics::gfx_file::Tile> = Vec::with_capacity(256);
    for fi in 0..2usize {
        if let Some(gfx) = rom.gfx.files.get(fi) {
            for t in &gfx.tiles {
                tiles.push(t);
                if tiles.len() >= 256 { break; }
            }
        }
        if tiles.len() >= 256 { break; }
    }
    if tiles.is_empty() { return None; }

    let sheet_cols = 16usize;
    let sheet_rows = (tiles.len() + sheet_cols - 1) / sheet_cols;
    let img_w = sheet_cols * 8;
    let img_h = sheet_rows * 8;
    let mut pixels = vec![Color32::from_gray(28); img_w * img_h];
    let pal_base = 4 * 16; // use sub-palette 0 = CGRAM row 4 for preview

    for (tidx, tile) in tiles.iter().enumerate() {
        let tc = tidx % sheet_cols;
        let tr = tidx / sheet_cols;
        for (pi, &ci) in tile.color_indices.iter().enumerate() {
            let px = pi % 8;
            let py = pi / 8;
            let c = if ci == 0 {
                if (tc + px + tr + py) % 2 == 0 { Color32::from_gray(45) }
                else { Color32::from_gray(35) }
            } else {
                Color32::from(
                    cgram.get(pal_base + ci as usize).copied().unwrap_or(Abgr1555::MAGENTA)
                )
            };
            pixels[(tr * 8 + py) * img_w + (tc * 8 + px)] = c;
        }
    }

    Some(ColorImage { size: [img_w, img_h], pixels })
}
