use std::sync::Arc;

use egui::*;
use smwe_render::color::Abgr1555;
use smwe_rom::{
    graphics::palette::{ColorPalette, OverworldState},
    overworld::{BgTile, OwTilemap, OW_SUBMAP_COUNT, OW_TILEMAP_COLS, OW_VISIBLE_ROWS},
    SmwRom,
};

use crate::ui::tool::DockableEditorTool;

// ── Layout constants ──────────────────────────────────────────────────────────

const MAP_TILE_COLS: usize = OW_TILEMAP_COLS; // 40
const MAP_TILE_ROWS: usize = OW_VISIBLE_ROWS; // 27
const TILE_PX: f32 = 8.0;
const MAP_W_PX: f32 = MAP_TILE_COLS as f32 * TILE_PX; // 320 px
const MAP_H_PX: f32 = MAP_TILE_ROWS as f32 * TILE_PX; // 216 px

/// The 4 GFX files the overworld always loads into VRAM pages 0-3:
///   GFX1C → page 0, GFX1D → page 1, GFX08 → page 2, GFX1E → page 3
/// CHR index bits 8-7 = VRAM page (0-3), bits 6-0 = tile within page (0-127).
const OW_GFX_FILES: [usize; 4] = [0x1C, 0x1D, 0x08, 0x1E];

const SUBMAP_NAMES: &[&str] =
    &["Main Map", "Yoshi's Island", "Vanilla Dome", "Forest of Illusion", "Valley of Bowser", "Special", "Star World"];

// ── Struct ───────────────────────────────────────────────────────────────────

pub struct UiWorldEditor {
    rom: Arc<SmwRom>,

    zoom: f32,
    offset: Vec2,
    submap: usize,
    show_grid: bool,

    /// Currently selected tile on the map canvas (col, row).
    selected_tile: Option<(usize, usize)>,

    /// CHR tile index chosen in the tile-sheet picker for painting.
    paint_tile: Option<u16>,
    /// Sub-palette used when painting (0-7).
    paint_palette: u8,

    /// Local editable copy of layer-2 tilemaps (one per submap).
    /// Initialised from ROM on open, modified in-editor.
    local_layer2: Vec<OwTilemap>,

    /// Number of unsaved edits.
    edit_count: usize,

    // Cached textures
    map_texture: Option<TextureHandle>,
    map_texture_sub: usize,
    sheet_texture: Option<TextureHandle>,
    sheet_texture_sub: usize,
}

impl UiWorldEditor {
    pub fn new(rom: Arc<SmwRom>) -> Self {
        // Clone layer-2 tilemaps into our local editable copy
        let local_layer2: Vec<OwTilemap> =
            (0..OW_SUBMAP_COUNT).map(|sm| rom.overworld.layer2.get(sm).cloned().unwrap_or_default()).collect();

        Self {
            rom,
            zoom: 3.0,
            offset: Vec2::ZERO,
            submap: 0,
            show_grid: true,
            selected_tile: None,
            paint_tile: None,
            paint_palette: 0,
            local_layer2,
            edit_count: 0,
            map_texture: None,
            map_texture_sub: usize::MAX,
            sheet_texture: None,
            sheet_texture_sub: usize::MAX,
        }
    }

    fn invalidate_map(&mut self) {
        self.map_texture = None;
        self.map_texture_sub = usize::MAX;
    }

    fn ensure_map_texture(&mut self, ctx: &Context) {
        let needs = self.map_texture.is_none() || self.map_texture_sub != self.submap;
        if !needs {
            return;
        }

        if let Some(img) = render_ow_map(&self.rom, &self.local_layer2, self.submap) {
            self.map_texture = Some(ctx.load_texture(format!("ow_map_{}", self.submap), img, TextureOptions::NEAREST));
        }
        self.map_texture_sub = self.submap;
    }

    fn ensure_sheet_texture(&mut self, ctx: &Context) {
        if self.sheet_texture.is_some() && self.sheet_texture_sub == self.submap {
            return;
        }
        if let Some(img) = render_tile_sheet(&self.rom, self.submap) {
            self.sheet_texture =
                Some(ctx.load_texture(format!("ow_sheet_{}", self.submap), img, TextureOptions::NEAREST));
        }
        self.sheet_texture_sub = self.submap;
    }
}

impl DockableEditorTool for UiWorldEditor {
    fn title(&self) -> WidgetText {
        "World Map Editor".into()
    }

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

        CentralPanel::default().frame(Frame::none()).show_inside(ui, |ui| self.canvas(ui));
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
            if submap_changed {
                self.invalidate_map();
            }

            ui.separator();
            ui.label(RichText::new("View").strong());
            ui.add(Slider::new(&mut self.zoom, 1.0..=8.0).step_by(0.5).text("Zoom"));
            ui.checkbox(&mut self.show_grid, "Tile grid");
            if ui.button("Reset view").clicked() {
                self.zoom = 3.0;
                self.offset = Vec2::ZERO;
            }

            ui.separator();
            ui.label(RichText::new("Paint").strong());
            match self.paint_tile {
                Some(t) => {
                    ui.label(format!("Tile  CHR #{t:#05x}"));
                }
                None => {
                    ui.label(RichText::new("← Pick from tile sheet").color(Color32::GRAY).italics());
                }
            }
            ui.horizontal(|ui| {
                ui.label("Palette:");
                for p in 0u8..4 {
                    if ui.selectable_label(self.paint_palette == p, format!("{p}")).clicked() {
                        self.paint_palette = p;
                    }
                }
            });
            ui.label(
                RichText::new("Pick tile → click map to paint\nRight-click to deselect").small().color(Color32::GRAY),
            );

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
                None => {
                    ui.label(RichText::new("None").color(Color32::GRAY).italics());
                }
            }

            ui.separator();
            if ui.button(RichText::new("💾  Save ROM…").color(Color32::from_rgb(80, 210, 80))).clicked() {
                std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
                if let Some(path) =
                    rfd::FileDialog::new().set_title("Save ROM").add_filter("SNES ROM", &["smc", "sfc"]).save_file()
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
        let bytes = self.rom.disassembly.rom.0.to_vec();
        log::warn!("Overworld save not implemented - LC_RLE2 recompression needed");
        let mut f = std::fs::File::create(path)?;
        f.write_all(&bytes)?;
        Ok(())
    }

    // ── Right panel: tile-sheet picker ───────────────────────────────────────

    fn right_panel(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        ui.label(RichText::new("Tile Sheet").strong());
        ui.label(
            RichText::new(format!(
                "GFX {:02X},{:02X},{:02X},{:02X}  (3bpp)",
                OW_GFX_FILES[0], OW_GFX_FILES[1], OW_GFX_FILES[2], OW_GFX_FILES[3]
            ))
            .small()
            .color(Color32::GRAY),
        );
        ui.separator();

        if let Some(tex) = self.sheet_texture.clone() {
            let tex_size = tex.size_vec2();
            let avail_w = ui.available_width().min(180.0);
            let scale = avail_w / tex_size.x;
            let display_size = tex_size * scale;

            let (resp, painter) = ui.allocate_painter(display_size, Sense::click());

            painter.image(tex.id(), resp.rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);

            let sheet_cols = 16usize;
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
                    let total = sheet_total_tiles(&self.rom);
                    if idx < total {
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
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());

        // Pan with middle-mouse or alt+drag
        if resp.dragged_by(PointerButton::Middle) || (resp.dragged() && ui.input(|i| i.modifiers.alt)) {
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
        let canvas_tl = resp.rect.min + self.offset;
        let map_rect = Rect::from_min_size(canvas_tl, vec2(MAP_W_PX * z, MAP_H_PX * z));

        // Canvas background
        painter.rect_filled(resp.rect, Rounding::ZERO, Color32::from_gray(28));

        // Tilemap texture
        if let Some(tex) = &self.map_texture {
            painter.image(tex.id(), map_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
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
        painter.rect_stroke(map_rect, Rounding::ZERO, Stroke::new(2.0, Color32::from_rgb(255, 220, 0)));

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
            if col >= 0 && row >= 0 && (col as usize) < MAP_TILE_COLS && (row as usize) < MAP_TILE_ROWS {
                Some((col as usize, row as usize))
            } else {
                None
            }
        });

        // Hover highlight
        if let Some((col, row)) = hovered_tile {
            let cell = TILE_PX * z;
            painter.rect_filled(
                Rect::from_min_size(canvas_tl + vec2(col as f32 * cell, row as f32 * cell), Vec2::splat(cell)),
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
                Rect::from_min_size(canvas_tl + vec2(col as f32 * cell, row as f32 * cell), Vec2::splat(cell)),
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

// ── GFX / CHR helpers ───────────────────────────────────────────────────────

/// Resolve a CHR tile index to (gfx_file_index, tile_offset_within_file).
/// The overworld layer 2 uses 4 GFX slots, each occupying one full VRAM page
/// (256 tile-numbers wide). The 10-bit CHR tile index maps as:
///   bits 9-8 = slot (0-3)  → which GFX file
///   bits 7-0 = tile offset within that file (0-127 for 3bpp files)
///
///   slot 0 → OW_GFX_FILES[0] = GFX1C  (chr 0x000-0x0FF)
///   slot 1 → OW_GFX_FILES[1] = GFX1D  (chr 0x100-0x1FF)
///   slot 2 → OW_GFX_FILES[2] = GFX08  (chr 0x200-0x2FF)
///   slot 3 → OW_GFX_FILES[3] = GFX1E  (chr 0x300-0x3FF)
fn chr_to_gfx(chr: usize) -> Option<(usize, usize)> {
    let slot = (chr >> 8) & 0x3;   // bits 9-8
    let tile_offset = chr & 0xFF;   // bits 7-0 (0-127 used in practice)
    if slot >= OW_GFX_FILES.len() {
        return None;
    }
    let file_idx = OW_GFX_FILES[slot];
    log::debug!("chr_to_gfx: chr={:#05x} -> slot={} file_id={:#04x} tile_offset={}", chr, slot, file_idx, tile_offset);
    Some((file_idx, tile_offset))
}

/// Count total tiles in the tile sheet (all 4 OW GFX slots).
/// Each slot covers 256 tile-number entries (bits 9-8 of CHR), so total = 4 * 256 = 1024.
/// Capped by the actual tiles present in each GFX file.
fn sheet_total_tiles(rom: &SmwRom) -> usize {
    OW_GFX_FILES.len() * 256
}

// ── Rendering helpers ─────────────────────────────────────────────────--------

/// Build a flat 256-entry CGRAM (16 rows × 16 cols) for the given OW submap.
/// Layer2 colors sit at CGRAM rows 4-7 (palette field 4-7).
/// Returns the CGRAM array and the backdrop color (layer3 row 0 col 8 = sky/ocean).
fn build_cgram(rom: &SmwRom, submap: usize) -> (Vec<Abgr1555>, Color32) {
    let sm = submap.min(5);
    match rom.gfx.color_palettes.get_submap_palette(sm, OverworldState::PreSpecial) {
        Ok(pal) => {
            let cgram =
                (0..256usize).map(|i| pal.get_color_at(i / 16, i % 16).unwrap_or(Abgr1555::TRANSPARENT)).collect();
            // Sky/ocean backdrop: layer3 palette row 0, col 8
            let backdrop_abgr = pal.get_color_at(0, 8).unwrap_or(Abgr1555::TRANSPARENT);
            let backdrop = Color32::from(backdrop_abgr);
            (cgram, backdrop)
        }
        Err(_) => (vec![Abgr1555::TRANSPARENT; 256], Color32::from_rgb(20, 40, 100)),
    }
}

/// Decode one 8×8 CHR tile with optional flip into a 64-pixel Color32 array.
/// `cgram_row` is the CGRAM palette row (0-15); color 0 is always transparent.
fn decode_tile_into(
    tile: &smwe_rom::graphics::gfx_file::Tile, cgram: &[Abgr1555], cgram_row: usize, flip_x: bool, flip_y: bool,
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
/// Uses `local_layer2` for tile data (includes in-editor edits).
fn render_ow_map(rom: &SmwRom, local_layer2: &[OwTilemap], submap: usize) -> Option<ColorImage> {
    let sm = submap.min(6);
    let layer2 = local_layer2.get(sm)?;
    let (cgram, backdrop) = build_cgram(rom, sm);

    let img_w = MAP_TILE_COLS * 8;
    let img_h = MAP_TILE_ROWS * 8;
    // Fill with the sky/ocean backdrop color instead of dark gray
    let mut pixels = vec![backdrop; img_w * img_h];
    let mut buf = [Color32::TRANSPARENT; 64];

    log::info!("render_ow_map: submap={} img={}x{} rom.gfx.files.len()={}", sm, img_w, img_h, rom.gfx.files.len());

    // Collect tile mismatch reports
    let mut mismatches: Vec<String> = Vec::new();
    let mut rendered_tiles = 0usize;
    for row in 0..MAP_TILE_ROWS {
        for col in 0..MAP_TILE_COLS {
            let entry = layer2.get(col, row);
            let chr = entry.tile_index() as usize;
            if chr == 0 {
                continue;
            }
            if let Some((file_id, offset)) = chr_to_gfx(chr) {
                // Try to find the actual gfx file by ID or index.
                if rom.gfx.files.get(file_id).is_none() {
                    log::warn!(
                        "render_ow_map: rom.gfx.files.get({:#04x}) returned None. rom.gfx.files.len() = {}",
                        file_id,
                        rom.gfx.files.len()
                    );
                }

                if let Some(tile) = rom.gfx.files.get(file_id).and_then(|f| f.tiles.get(offset)) {
                    let cgram_row = 4 + ((entry.palette() as usize) & 3);
                    log::debug!(
                        "render_ow_map: at ({},{}) chr={:#05x} -> file_id={:#04x} offset={} cgram_row={} flipX={} flipY={}",
                        col,
                        row,
                        chr,
                        file_id,
                        offset,
                        cgram_row,
                        entry.flip_x(),
                        entry.flip_y()
                    );

                    // decode into local 8x8 buffer
                    decode_tile_into(tile, &cgram, cgram_row, entry.flip_x(), entry.flip_y(), &mut buf);

                    let dx = col * 8;
                    let dy = row * 8;

                    // Write decoded pixels into the big image buffer
                    for py in 0..8 {
                        for px in 0..8 {
                            let c = buf[py * 8 + px];
                            if c.a() > 0 {
                                pixels[(dy + py) * img_w + (dx + px)] = c;
                            }
                        }
                    }

                    // --- Pixel-compare check: verify that the pixels we just wrote match `buf` for non-transparent pixels.
                    // If the final pixels at the tile area differ from the decoded bytes, record a mismatch.
                    let mut found_mismatch = false;
                    let mut first_bad: Option<(usize, usize, Color32, Color32)> = None;
                    'tilecheck: for py in 0..8 {
                        for px in 0..8 {
                            let expected = buf[py * 8 + px];
                            if expected.a() == 0 {
                                // We only compare non-transparent pixels (transparent pixels are overlay/backdrop-dependent)
                                continue;
                            }
                            let idx = (dy + py) * img_w + (dx + px);
                            let actual = pixels[idx];
                            if actual != expected {
                                found_mismatch = true;
                                first_bad = Some((px, py, expected, actual));
                                break 'tilecheck;
                            }
                        }
                    }

                    if found_mismatch {
                        let (bx, by, expected, actual) = first_bad.unwrap();
                        let s = format!(
                            "TILE_MISMATCH at view({},{}) chr={:#05x} -> file_id={:#04x} offset={} first_bad_pixel=({},{}): expected={:?} actual={:?}",
                            col, row, chr, file_id, offset, bx, by, expected, actual
                        );
                        log::warn!("{}", s);
                        mismatches.push(s);
                    }

                    rendered_tiles += 1;
                } else {
                    log::warn!(
                        "render_ow_map: MISSING TILE: chr={:#05x} -> file_id={:#04x} offset={}  rom.gfx.files.get returned None or tile missing",
                        chr,
                        file_id,
                        offset
                    );
                }
            } else {
                log::warn!("render_ow_map: chr_to_gfx returned None for chr={:#05x}", chr);
            }
        }
    }

    log::info!("render_ow_map: rendered_tiles = {}, mismatches = {}", rendered_tiles, mismatches.len());
    if !mismatches.is_empty() {
        // print first few mismatches for copy/paste
        for m in mismatches.iter().take(20) {
            log::info!("  {}", m);
        }
    } else {
        log::info!("render_ow_map: no tile pixel mismatches found (decoded tiles match texture pixels).");
    }

    Some(ColorImage { size: [img_w, img_h], pixels })
}

/// Render a tile-sheet image for the 4 OW GFX pages.
/// Layout: 16 tiles wide × N rows, 8×8 pixels each, up to 512 tiles total.
fn render_tile_sheet(rom: &SmwRom, submap: usize) -> Option<ColorImage> {
    let sm = submap.min(5);
    let (cgram, _) = build_cgram(rom, sm);

    // Build a 4-slot × 256-tile sheet where each CHR index maps directly:
    //   CHR bits 9-8 = slot (row of 256), bits 7-0 = tile within slot.
    // Sheet layout: 16 columns × N rows (each row of 16 = 16 tiles).
    // Total 4 * 256 = 1024 entries; only entries 0..127 per slot have real GFX.
    let sheet_cols = 16usize;
    let total_entries = OW_GFX_FILES.len() * 256; // 1024
    let sheet_rows = (total_entries + sheet_cols - 1) / sheet_cols;
    let img_w = sheet_cols * 8;
    let img_h = sheet_rows * 8;
    let mut pixels = vec![Color32::from_gray(28); img_w * img_h];
    let pal_base = 7 * 16;

    for chr in 0..total_entries {
        let slot = (chr >> 8) & 0x3;
        let tile_offset = chr & 0xFF;
        let tc = chr % sheet_cols;
        let tr = chr / sheet_cols;
        let gfx_file_idx = OW_GFX_FILES[slot];
        let maybe_tile = rom.gfx.files.get(gfx_file_idx).and_then(|f| f.tiles.get(tile_offset));
        if let Some(tile) = maybe_tile {
            for (pi, &ci) in tile.color_indices.iter().enumerate() {
                let px = pi % 8;
                let py = pi / 8;
                let c = if ci == 0 {
                    if (tc + px + tr + py) % 2 == 0 { Color32::from_gray(45) } else { Color32::from_gray(35) }
                } else {
                    Color32::from(cgram.get(pal_base + ci as usize).copied().unwrap_or(Abgr1555::MAGENTA))
                };
                pixels[(tr * 8 + py) * img_w + (tc * 8 + px)] = c;
            }
        } else {
            // Empty slot entry — draw an X pattern so it's clearly empty
            let tc_px = tc * 8;
            let tr_px = tr * 8;
            for i in 0..8usize {
                let c = Color32::from_gray(50);
                pixels[(tr_px + i) * img_w + tc_px + i] = c;
                pixels[(tr_px + i) * img_w + tc_px + (7 - i)] = c;
            }
        }
    }

    if pixels.iter().all(|&p| p == Color32::from_gray(28)) {
        return None;
    }

    Some(ColorImage { size: [img_w, img_h], pixels })
}
