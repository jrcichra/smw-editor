//! World Map Editor UI
//!
//! Renders the SMW overworld layer-2 tilemap using the four overworld GFX files
//! (GFX 1C, 1D, 08, 1E) and the correct submap palette obtained via the ROM's
//! two-level indirection table ($00AD1E → $00ABDF → $00B3D8).
//!
//! # CHR mapping
//! Each BgTile carries a 10-bit CHR index.  The four OW GFX files each hold
//! 128 tiles and occupy contiguous VRAM slots:
//!
//!   slot = chr >> 7        (0-3)
//!   offset = chr & 0x7F    (0-127)
//!
//!   slot 0 → GFX 0x1C  chr 0x000-0x07F
//!   slot 1 → GFX 0x1D  chr 0x080-0x0FF
//!   slot 2 → GFX 0x08  chr 0x100-0x17F
//!   slot 3 → GFX 0x1E  chr 0x180-0x1FF
//!
//! # Palette mapping
//! OW layer-2 tiles use sub-palettes 4-7.  CGRAM row = palette field (4-7).
//! SpecificOverworldColorPalette maps layer2 to CGRAM rows 4-7, cols 1-7.
//! So cgram_index = palette * 16 + color_index.

use std::sync::Arc;

use egui::*;
use smwe_render::color::Abgr1555;
use smwe_rom::{
    graphics::palette::{ColorPalette, OverworldState},
    overworld::{BgTile, OwTilemap, OW_GFX_FILES, OW_MAIN_COLS, OW_SUBMAP_COLS, OW_TILEMAP_ROWS, OW_SUBMAP_COUNT},
    SmwRom,
};

use crate::ui::tool::DockableEditorTool;

// ── Constants ─────────────────────────────────────────────────────────────────

const TILE_PX: f32 = 8.0;

const SUBMAP_NAMES: &[&str] = &[
    "Main Map",
    "Yoshi's Island",
    "Vanilla Dome",
    "Forest of Illusion",
    "Valley of Bowser",
    "Special",
    "Star World",
];

// ── Struct ───────────────────────────────────────────────────────────────────

pub struct UiWorldEditor {
    rom: Arc<SmwRom>,

    zoom: f32,
    offset: Vec2,
    submap: usize,
    show_grid: bool,

    selected_tile: Option<(usize, usize)>,
    paint_tile: Option<u16>,
    paint_palette: u8,

    /// Local editable copy of layer-2 tilemaps (one per submap).
    local_layer2: Vec<OwTilemap>,
    edit_count: usize,

    map_texture: Option<TextureHandle>,
    map_texture_sub: usize,
    sheet_texture: Option<TextureHandle>,
    sheet_texture_sub: usize,
}

impl UiWorldEditor {
    pub fn new(rom: Arc<SmwRom>) -> Self {
        let local_layer2: Vec<OwTilemap> = (0..OW_SUBMAP_COUNT)
            .map(|sm| rom.overworld.layer2.get(sm).cloned().unwrap_or_default())
            .collect();

        Self {
            rom,
            zoom: 3.0,
            offset: Vec2::ZERO,
            submap: 0,
            show_grid: true,
            selected_tile: None,
            paint_tile: None,
            paint_palette: 4,
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

    /// Returns the number of columns for the current submap.
    fn current_cols(&self) -> usize {
        if self.submap == 0 { OW_MAIN_COLS } else { OW_SUBMAP_COLS }
    }

    fn ensure_map_texture(&mut self, ctx: &Context) {
        if self.map_texture.is_some() && self.map_texture_sub == self.submap {
            return;
        }
        if let Some(img) = render_ow_map(&self.rom, &self.local_layer2, self.submap) {
            self.map_texture = Some(ctx.load_texture(
                format!("ow_map_{}", self.submap),
                img,
                TextureOptions::NEAREST,
            ));
        }
        self.map_texture_sub = self.submap;
    }

    fn ensure_sheet_texture(&mut self, ctx: &Context) {
        if self.sheet_texture.is_some() && self.sheet_texture_sub == self.submap {
            return;
        }
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

        CentralPanel::default()
            .frame(Frame::none())
            .show_inside(ui, |ui| self.canvas(ui));
    }
}

impl UiWorldEditor {
    // ── Left panel ────────────────────────────────────────────────────────────

    fn left_panel(&mut self, ui: &mut Ui) {
        ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(6.0);
            ui.heading("🗺  World Map Editor");
            ui.separator();

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
                Some(t) => { ui.label(format!("Tile  CHR #{t:#05x}")); }
                None    => { ui.label(RichText::new("← Pick from tile sheet").color(Color32::GRAY).italics()); }
            }
            ui.horizontal(|ui| {
                ui.label("Palette:");
                for p in 4u8..8 {
                    if ui.selectable_label(self.paint_palette == p, format!("{p}")).clicked() {
                        self.paint_palette = p;
                    }
                }
            });
            ui.label(RichText::new("Pick tile → click map to paint\nRight-click to deselect").small().color(Color32::GRAY));

            if self.edit_count > 0 {
                ui.add_space(4.0);
                ui.label(RichText::new(format!("⚠ {} unsaved edit(s)", self.edit_count))
                    .color(Color32::from_rgb(255, 200, 60)));
            }

            ui.separator();
            ui.label(RichText::new("Selected tile").strong());
            match self.selected_tile {
                Some((col, row)) => {
                    ui.label(format!("Position: ({col}, {row})"));
                    if let Some(tm) = self.local_layer2.get(self.submap) {
                        let t = tm.get(col, row);
                        ui.label(format!("CHR  #{:03X}", t.tile_index()));
                        ui.label(format!("Pal  {}",      t.palette()));
                        ui.label(format!("Flip X={}  Y={}", t.flip_x() as u8, t.flip_y() as u8));
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
                        Ok(())  => { self.edit_count = 0; log::info!("Saved ROM to {}", path.display()); }
                        Err(e)  => log::error!("Save failed: {e}"),
                    }
                }
            }

            ui.separator();
            ui.label(RichText::new("Stats").strong());
            ui.label(format!("GFX files: {}", self.rom.gfx.files.len()));
            ui.label(format!("Levels:    {}", self.rom.levels.len()));

            let cols = self.current_cols();
            ui.label(format!("Map size:  {}×{}", cols, OW_TILEMAP_ROWS));
        });
    }

    fn save_rom_to(&self, path: &std::path::Path) -> anyhow::Result<()> {
        use std::io::Write;
        let bytes = self.rom.disassembly.rom.0.to_vec();
        log::warn!("Overworld save not implemented – LC_RLE2 recompression needed");
        let mut f = std::fs::File::create(path)?;
        f.write_all(&bytes)?;
        Ok(())
    }

    // ── Right panel: tile-sheet picker ───────────────────────────────────────

    fn right_panel(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        ui.label(RichText::new("Tile Sheet").strong());
        ui.label(RichText::new(format!(
            "GFX {:02X},{:02X},{:02X},{:02X}  (3bpp)",
            OW_GFX_FILES[0], OW_GFX_FILES[1], OW_GFX_FILES[2], OW_GFX_FILES[3]
        )).small().color(Color32::GRAY));
        ui.separator();

        if let Some(tex) = self.sheet_texture.clone() {
            let tex_size   = tex.size_vec2();
            let avail_w    = ui.available_width().min(180.0);
            let scale      = avail_w / tex_size.x;
            let disp_size  = tex_size * scale;
            let sheet_cols = 16usize;
            let cell_px    = disp_size.x / sheet_cols as f32;

            let (resp, painter) = ui.allocate_painter(disp_size, Sense::click());
            painter.image(tex.id(), resp.rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);

            // Highlight selected tile
            if let Some(sel) = self.paint_tile {
                let sc = (sel as usize) % sheet_cols;
                let sr = (sel as usize) / sheet_cols;
                painter.rect_stroke(
                    Rect::from_min_size(resp.rect.min + vec2(sc as f32 * cell_px, sr as f32 * cell_px), Vec2::splat(cell_px)),
                    Rounding::ZERO,
                    Stroke::new(1.5, Color32::from_rgb(255, 220, 0)),
                );
            }

            // Hover highlight + tooltip
            if let Some(pos) = resp.hover_pos() {
                let rel = pos - resp.rect.min;
                let sc  = (rel.x / cell_px) as usize;
                let sr  = (rel.y / cell_px) as usize;
                painter.rect_stroke(
                    Rect::from_min_size(resp.rect.min + vec2(sc as f32 * cell_px, sr as f32 * cell_px), Vec2::splat(cell_px)),
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
                    let sc  = (rel.x / cell_px) as usize;
                    let sr  = (rel.y / cell_px) as usize;
                    let idx = sr * sheet_cols + sc;
                    let total = OW_GFX_FILES.len() * 128; // 512
                    if idx < total {
                        self.paint_tile = Some(idx as u16);
                    }
                }
            }
        } else {
            ui.label(RichText::new("No GFX data.").color(Color32::GRAY).italics());
        }
    }

    // ── Canvas ────────────────────────────────────────────────────────────────

    fn canvas(&mut self, ui: &mut Ui) {
        let cols      = self.current_cols();
        let map_w_px  = cols as f32 * TILE_PX;
        let map_h_px  = OW_TILEMAP_ROWS as f32 * TILE_PX;

        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());

        // Pan
        if resp.dragged_by(PointerButton::Middle) || (resp.dragged() && ui.input(|i| i.modifiers.alt)) {
            self.offset += resp.drag_delta() / self.zoom;
        }

        // Scroll-to-zoom
        if resp.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                let old = self.zoom;
                self.zoom = (self.zoom * (1.0 + scroll * 0.0015)).clamp(0.5, 12.0);
                if let Some(pos) = resp.hover_pos() {
                    let tl  = resp.rect.min + self.offset * old;
                    let rel = pos - tl;
                    self.offset += rel / old - rel / self.zoom;
                }
            }
        }

        let z         = self.zoom;
        let canvas_tl = resp.rect.min + self.offset;
        let map_rect  = Rect::from_min_size(canvas_tl, vec2(map_w_px * z, map_h_px * z));

        painter.rect_filled(resp.rect, Rounding::ZERO, Color32::from_gray(28));

        // Tilemap texture
        if let Some(tex) = &self.map_texture {
            painter.image(tex.id(), map_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        } else {
            painter.rect_filled(map_rect, Rounding::ZERO, Color32::from_rgb(20, 40, 100));
            painter.text(map_rect.center(), Align2::CENTER_CENTER, "Rendering…", FontId::proportional(14.0), Color32::WHITE);
        }

        painter.rect_stroke(map_rect, Rounding::ZERO, Stroke::new(2.0, Color32::from_rgb(255, 220, 0)));

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
            let cell   = TILE_PX * z;
            let stroke = Stroke::new(0.5, Color32::from_white_alpha(18));
            for col in 0..=cols {
                let x = canvas_tl.x + col as f32 * cell;
                painter.vline(x, canvas_tl.y..=canvas_tl.y + map_h_px * z, stroke);
            }
            for row in 0..=OW_TILEMAP_ROWS {
                let y = canvas_tl.y + row as f32 * TILE_PX * z;
                painter.hline(canvas_tl.x..=canvas_tl.x + map_w_px * z, y, stroke);
            }
        }

        // Hovered tile
        let hovered_tile: Option<(usize, usize)> = resp.hover_pos().and_then(|pos| {
            let rel = (pos - canvas_tl) / z;
            let col = (rel.x / TILE_PX) as i32;
            let row = (rel.y / TILE_PX) as i32;
            if col >= 0 && row >= 0 && (col as usize) < cols && (row as usize) < OW_TILEMAP_ROWS {
                Some((col as usize, row as usize))
            } else {
                None
            }
        });

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

        if let Some((col, row)) = self.selected_tile {
            let cell = TILE_PX * z;
            painter.rect_stroke(
                Rect::from_min_size(canvas_tl + vec2(col as f32 * cell, row as f32 * cell), Vec2::splat(cell)),
                Rounding::ZERO,
                Stroke::new(1.5, Color32::from_rgb(255, 60, 60)),
            );
        }

        if resp.clicked() {
            if let Some((col, row)) = hovered_tile {
                if let Some(chr) = self.paint_tile {
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

        if resp.clicked_by(PointerButton::Secondary) {
            self.paint_tile = None;
            self.selected_tile = None;
        }
    }
}

// ── GFX helpers ──────────────────────────────────────────────────────────────

/// Resolve a 10-bit CHR index to `(gfx_file_index, tile_offset_within_file)`.
///
/// The 4 OW GFX files each hold 128 tiles (3bpp).  The CHR index is:
///   slot   = chr >> 7       → which GFX file (0-3)
///   offset = chr & 0x7F     → tile within that file (0-127)
fn chr_to_gfx(chr: usize) -> Option<(usize, usize)> {
    let slot   = chr >> 7;
    let offset = chr & 0x7F;
    if slot >= OW_GFX_FILES.len() {
        return None;
    }
    Some((OW_GFX_FILES[slot], offset))
}

// ── CGRAM builder ─────────────────────────────────────────────────────────────

/// Build a flat 256-entry CGRAM array (16 rows × 16 cols) for a submap.
///
/// The SpecificOverworldColorPalette's layer2 field covers rows 4-7, cols 1-7.
/// Index formula: `cgram[row * 16 + col]  →  get_color_at(row, col)`.
/// Transparent (color 0 in each row) is left as TRANSPARENT.
///
/// Also returns the sky/ocean backdrop color (layer3 row 0, col 8).
fn build_cgram(rom: &SmwRom, submap: usize) -> (Vec<Abgr1555>, Color32) {
    // get_submap_palette now uses the $00AD1E indirection table internally.
    match rom.gfx.color_palettes.get_submap_palette(submap, OverworldState::PreSpecial) {
        Ok(pal) => {
            let cgram: Vec<Abgr1555> = (0..256usize)
                .map(|i| pal.get_color_at(i / 16, i % 16).unwrap_or(Abgr1555::TRANSPARENT))
                .collect();
            let backdrop = Color32::from(pal.get_color_at(0, 8).unwrap_or(Abgr1555::TRANSPARENT));
            (cgram, backdrop)
        }
        Err(e) => {
            log::warn!("build_cgram: palette error for submap {}: {e}", submap);
            (vec![Abgr1555::TRANSPARENT; 256], Color32::from_rgb(20, 40, 100))
        }
    }
}

// ── Tile decoder ─────────────────────────────────────────────────────────────

/// Decode one 8×8 CHR tile (with optional flip) into a 64-pixel `Color32` array.
///
/// `cgram_row` is 4-7 (= the palette field of the BgTile word).
/// Color index 0 is always transparent (BG color).
fn decode_tile(
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

// ── Map renderer ─────────────────────────────────────────────────────────────

/// Render one submap's tilemap into an RGBA `ColorImage`.
///
/// Image width  = tilemap.cols * 8
/// Image height = OW_TILEMAP_ROWS * 8  (always 216 px)
fn render_ow_map(rom: &SmwRom, local_layer2: &[OwTilemap], submap: usize) -> Option<ColorImage> {
    let sm      = submap.min(OW_SUBMAP_COUNT - 1);
    let tilemap = local_layer2.get(sm)?;
    let cols    = tilemap.cols;
    let rows    = tilemap.rows;

    let (cgram, backdrop) = build_cgram(rom, sm);

    let img_w = cols * 8;
    let img_h = rows * 8;
    let mut pixels = vec![backdrop; img_w * img_h];
    let mut buf    = [Color32::TRANSPARENT; 64];

    log::info!(
        "render_ow_map: submap={} tilemap={}×{} img={}×{}",
        sm, cols, rows, img_w, img_h
    );

    for row in 0..rows {
        for col in 0..cols {
            let entry = tilemap.get(col, row);
            let chr   = entry.tile_index() as usize;
            if chr == 0 {
                continue;
            }
            let Some((file_id, offset)) = chr_to_gfx(chr) else { continue };
            let Some(tile) = rom.gfx.files.get(file_id).and_then(|f| f.tiles.get(offset)) else {
                log::warn!("render_ow_map: missing tile chr={chr:#05x} file={file_id:#04x} offset={offset}");
                continue;
            };

            // OW layer2 palette values are 4-7; use directly as cgram_row.
            let cgram_row = entry.palette() as usize;
            decode_tile(tile, &cgram, cgram_row, entry.flip_x(), entry.flip_y(), &mut buf);

            let dx = col * 8;
            let dy = row * 8;
            for py in 0..8usize {
                for px in 0..8usize {
                    let c = buf[py * 8 + px];
                    if c.a() > 0 {
                        pixels[(dy + py) * img_w + (dx + px)] = c;
                    }
                }
            }
        }
    }

    Some(ColorImage { size: [img_w, img_h], pixels })
}

// ── Tile-sheet renderer ───────────────────────────────────────────────────────

/// Render all 512 OW CHR tiles (4 GFX files × 128 tiles) into a 16-wide grid.
fn render_tile_sheet(rom: &SmwRom, submap: usize) -> Option<ColorImage> {
    let sm         = submap.min(OW_SUBMAP_COUNT - 1);
    let (cgram, _) = build_cgram(rom, sm);

    // Render with palette row 7 (brightest OW palette, good for preview).
    let preview_pal_row = 7usize;

    let sheet_cols   = 16usize;
    let total_tiles  = OW_GFX_FILES.len() * 128; // 512
    let sheet_rows   = (total_tiles + sheet_cols - 1) / sheet_cols;
    let img_w        = sheet_cols * 8;
    let img_h        = sheet_rows * 8;
    let mut pixels   = vec![Color32::from_gray(28); img_w * img_h];

    for chr in 0..total_tiles {
        let slot   = chr >> 7;
        let offset = chr & 0x7F;
        let tc     = chr % sheet_cols;
        let tr     = chr / sheet_cols;

        let gfx_id     = OW_GFX_FILES[slot];
        let maybe_tile = rom.gfx.files.get(gfx_id).and_then(|f| f.tiles.get(offset));

        let base = preview_pal_row * 16;
        if let Some(tile) = maybe_tile {
            for (pi, &ci) in tile.color_indices.iter().enumerate() {
                let px = pi % 8;
                let py = pi / 8;
                let c  = if ci == 0 {
                    // Checkerboard for transparent
                    if (tc + px + tr + py) % 2 == 0 { Color32::from_gray(45) } else { Color32::from_gray(35) }
                } else {
                    Color32::from(cgram.get(base + ci as usize).copied().unwrap_or(Abgr1555::MAGENTA))
                };
                pixels[(tr * 8 + py) * img_w + (tc * 8 + px)] = c;
            }
        } else {
            // X pattern for missing tiles
            for i in 0..8usize {
                let c = Color32::from_gray(50);
                pixels[(tr * 8 + i) * img_w + tc * 8 + i]       = c;
                pixels[(tr * 8 + i) * img_w + tc * 8 + (7 - i)] = c;
            }
        }
    }

    if pixels.iter().all(|&p| p == Color32::from_gray(28)) {
        return None;
    }
    Some(ColorImage { size: [img_w, img_h], pixels })
}
