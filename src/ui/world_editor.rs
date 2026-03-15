//! World Map (Overworld) Editor UI.
//!
//! The overworld tilemap is stored in WRAM at $7EC800 (Map16TilesLow) after the
//! game's init routines run.  The index bit layout is `%-----YYX yyyyxxxx`
//! (SMW overworld data format spec):
//!
//!   idx = (x & 0x1F) | ((y_actual & 0x3F) << 5)
//!
//! where y_actual = display_row for the main map (rows 0–31)
//! and   y_actual = display_row + 32 for submaps (rows 32–63).
//! Combined tilemap: 32 cols × 64 rows = 2048 u8 entries at $7EC800.
//! Each byte is the Map16 tile-type ID (0–190).  The game copies the raw
//! OWL1TileData bytes directly via MVN with no stride expansion.
//!
//! Layer 2 ($7F4000 / OWLayer2Tilemap): row-major 40 cols × 28 rows,
//! indexed as ((Y * 40) + X) * 2.  Each entry is [tile_num_u8, YXPCCCTT_u8]
//! stored interleaved by LC_RLE2 (two-pass decompressor).  Reading as LE u16
//! gives the correct 10-bit tile number and flip/palette attributes.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use egui::{
    vec2, CentralPanel, Color32, Frame, PaintCallback, Rect, Rounding, Sense, SidePanel, Stroke, Ui, Vec2, WidgetText,
};
use egui_glow::CallbackFn;
use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};
use smwe_render::{
    gfx_buffers::GfxBuffers,
    tile_renderer::{Tile, TileRenderer, TileUniforms},
};
use smwe_rom::{overworld::SUBMAP_NAMES, SmwRom};

use crate::ui::tool::DockableEditorTool;

// ── Layout constants ──────────────────────────────────────────────────────────

/// Game pixels per Map16 block (L1 tiles are 16×16 game pixels each).
const MAP16_PX: f32 = 16.0;
/// Game pixels per 8×8 BG tile (L2 tiles).
const TILE_PX: f32 = 8.0;

/// Layer-1: 32 Map16 columns × 64 Map16 rows total buffer (each block is 16×16 game px).
/// Canvas = 32*16=512 wide, 32*16=512 tall per submap.
const OW_COLS: u32 = 32;
const OW_ROWS: u32 = 64;
const OW_ROWS_PER_SUBMAP: u32 = 32;

/// WRAM base for the OW Layer-1 tile-type bytes (u8 each, 0x800 total).
/// Populated by CODE_04DC09 via `MVN $7E,$0C` from OWL1TileData at $0CF7DF.
const MAP16_TILES_LOW: u32 = 0x7EC800;

/// WRAM base for the layer-2 tilemap (u16 each, row-major 64 cols × 64 rows).
const OW_L2_BASE: u32 = 0x7F4000;

/// Layer-2 dimensions: 64 tile-cols × 64 tile-rows (full decompressed tilemap).
/// Each tile is 8×8 game pixels. Formula: ((Y * 64) + X) * 2
const OW_L2_COLS: u32 = 64;
const OW_L2_ROWS: u32 = 64;

// ── SNES overworld tile-index helpers ─────────────────────────────────────────

/// Convert (col, row, submap) → word-index into Map16TilesLow ($7EC800).
///
/// Convert (col, row, submap) → word-index into Map16TilesLow ($7EC800).
///
/// The WRAM layout uses a `%-----YYX yyyyxxxx` bit packing (SMW overworld spec):
///   idx = (x & 0x1F) | ((y_actual & 0x3F) << 5)
/// where y_actual = row for the main map and row + 32 for submaps.
fn ow_l1_idx(col: u32, row: u32, submap: u8) -> u32 {
    let y_actual = row + if submap != 0 { 32 } else { 0 };
    (col & 0x1F) | ((y_actual & 0x3F) << 5)
}

/// Byte address of the tile-type byte in WRAM (u8, not u16).
fn ow_l1_addr(col: u32, row: u32, submap: u8) -> u32 {
    MAP16_TILES_LOW + ow_l1_idx(col, row, submap)
}

/// Layer-2 tilemap: simple row-major, 64 columns wide.
fn ow_l2_addr(col: u32, row: u32) -> u32 {
    OW_L2_BASE + (row * OW_L2_COLS + col) * 2
}

// ── OpenGL renderer ───────────────────────────────────────────────────────────

#[derive(Debug)]
struct OverworldRenderer {
    layer1: TileRenderer,
    layer2: TileRenderer,
    gfx_bufs: GfxBuffers,
    offset: Vec2,
    destroyed: bool,
}

impl OverworldRenderer {
    fn new(gl: &glow::Context) -> Self {
        Self {
            layer1: TileRenderer::new(gl),
            layer2: TileRenderer::new(gl),
            gfx_bufs: GfxBuffers::new(gl),
            offset: Vec2::ZERO,
            destroyed: false,
        }
    }

    fn destroy(&mut self, gl: &glow::Context) {
        if self.destroyed {
            return;
        }
        self.gfx_bufs.destroy(gl);
        self.layer1.destroy(gl);
        self.layer2.destroy(gl);
        self.destroyed = true;
    }

    fn upload_gfx(&self, gl: &glow::Context, data: &[u8]) {
        if !self.destroyed {
            self.gfx_bufs.upload_vram(gl, data);
        }
    }

    fn upload_palette(&self, gl: &glow::Context, data: &[u8]) {
        if !self.destroyed {
            self.gfx_bufs.upload_palette(gl, data);
        }
    }

    fn set_tiles(&mut self, gl: &glow::Context, l1: Vec<Tile>, l2: Vec<Tile>) {
        if !self.destroyed {
            self.layer1.set_tiles(gl, l1);
            self.layer2.set_tiles(gl, l2);
        }
    }

    fn paint(&self, gl: &glow::Context, screen_size: Vec2, zoom: f32, offset: Vec2, draw_l1: bool, draw_l2: bool) {
        if self.destroyed {
            return;
        }
        let uniforms = TileUniforms { gfx_bufs: self.gfx_bufs, screen_size, offset, zoom };
        if draw_l2 {
            self.layer2.paint(gl, &uniforms);
        }
        if draw_l1 {
            self.layer1.paint(gl, &uniforms);
        }
    }
}

// ── Editor ────────────────────────────────────────────────────────────────────

pub struct UiWorldEditor {
    gl: Arc<glow::Context>,
    #[allow(dead_code)]
    rom: Arc<SmwRom>,
    cpu: Cpu,
    renderer: Arc<Mutex<OverworldRenderer>>,

    submap: u8,

    offset: Vec2,
    zoom: f32,
    show_grid: bool,
    show_layer1: bool,
    show_layer2: bool,
    selected_tile: Option<(u32, u32)>,
}

impl UiWorldEditor {
    pub fn new(gl: Arc<glow::Context>, rom: Arc<SmwRom>, rom_path: PathBuf) -> Self {
        let renderer = Arc::new(Mutex::new(OverworldRenderer::new(&gl)));

        let raw = std::fs::read(&rom_path).expect("cannot read ROM for emulator");
        let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
        let mut emu_rom = EmuRom::new(rom_bytes);
        emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
        let cpu = Cpu::new(CheckedMem::new(Arc::new(emu_rom)));

        let mut editor = Self {
            gl,
            rom,
            cpu,
            renderer,
            submap: 0,
            offset: Vec2::ZERO,
            zoom: 2.0,
            show_grid: false,
            show_layer1: true,
            show_layer2: true,
            selected_tile: None,
        };
        editor.load_submap();
        editor
    }

    fn load_submap(&mut self) {
        smwe_emu::emu::load_overworld(&mut self.cpu, self.submap);

        let mut r = self.renderer.lock().expect("Cannot lock overworld renderer");
        r.upload_palette(&self.gl, &self.cpu.mem.cgram);
        r.upload_gfx(&self.gl, &self.cpu.mem.vram);

        let l1 = build_l1_tiles(&mut self.cpu, self.submap);
        let l2 = build_l2_tiles(&mut self.cpu);
        r.set_tiles(&self.gl, l1, l2);

        self.offset = Vec2::ZERO;
        self.selected_tile = None;
    }
}

impl DockableEditorTool for UiWorldEditor {
    fn title(&self) -> WidgetText {
        "World Map Editor".into()
    }

    fn update(&mut self, ui: &mut Ui) {
        SidePanel::left("world_editor.left_panel").resizable(false).show_inside(ui, |ui| self.left_panel(ui));
        CentralPanel::default().frame(Frame::none().inner_margin(0.)).show_inside(ui, |ui| self.central_panel(ui));
    }

    fn on_closed(&mut self) {
        self.renderer.lock().expect("Cannot lock overworld renderer").destroy(&self.gl);
    }
}

// ── UI ────────────────────────────────────────────────────────────────────────

impl UiWorldEditor {
    fn left_panel(&mut self, ui: &mut Ui) {
        ui.heading("Overworld");
        ui.add_space(4.0);

        // Submap selector
        ui.horizontal(|ui| {
            ui.label("Submap");
            let prev = self.submap;
            egui::ComboBox::from_id_source("world_editor.submap")
                .selected_text(SUBMAP_NAMES.get(self.submap as usize).copied().unwrap_or("Submap"))
                .show_ui(ui, |ui| {
                    for (i, name) in SUBMAP_NAMES.iter().enumerate() {
                        ui.selectable_value(&mut self.submap, i as u8, *name);
                    }
                });
            if self.submap != prev {
                self.load_submap();
            }
        });
        if ui.button("Reload Submap").clicked() {
            self.load_submap();
        }

        ui.separator();

        // Zoom
        ui.add(egui::Slider::new(&mut self.zoom, 0.5..=8.0).step_by(0.25).text("Zoom"));
        if ui.button("Reset View").clicked() {
            self.offset = Vec2::ZERO;
            self.zoom = 2.0;
        }

        ui.separator();

        ui.checkbox(&mut self.show_layer1, "Show Layer 1");
        ui.checkbox(&mut self.show_layer2, "Show Layer 2");
        ui.checkbox(&mut self.show_grid, "Show Grid");

        ui.separator();

        // Selected tile info
        if let Some((x, y)) = self.selected_tile {
            ui.label(format!("Selected: ({x}, {y})"));
            // L1 tilemap stores u8 tile-type IDs (MVN copy from OWL1TileData)
            let tile_id = self.cpu.mem.load_u8(ow_l1_addr(x, y, self.submap)) as u32;
            ui.monospace(format!("Tile type: {tile_id} (0x{tile_id:02X})"));
            // Look up sub-tiles in OWL1CharData via Map16Pointers
            let ptr_base = 0x7E0FBE_u32;
            let char_ptr = self.cpu.mem.load_u16(ptr_base + tile_id * 2) as u32;
            let char_addr = 0x05_0000_u32 | char_ptr;
            let sub0 = self.cpu.mem.load_u16(char_addr);
            let tile_num = (sub0 & 0x3FF) as u32;
            let pal = ((sub0 >> 10) & 0x7) as u32;
            let flip_x = (sub0 & 0x4000) != 0;
            let flip_y = (sub0 & 0x8000) != 0;
            ui.monospace(format!("  TL vram #{tile_num:03X}  pal {pal}"));
            if flip_x || flip_y {
                ui.monospace(format!("  flip x={flip_x} y={flip_y}"));
            }
            // OWLayer1Translevel: level number stored at $7ED000 (u8 indexed)
            let xlevel_addr = 0x7ED000_u32 + ow_l1_idx(x, y, self.submap);
            let xlevel = self.cpu.mem.load_u8(xlevel_addr) as u32;
            if xlevel != 0 {
                ui.monospace(format!("  level 0x{xlevel:03X}"));
            }
        } else {
            ui.label("Selected: (none)");
        }

        ui.add_space(ui.available_height() - 40.0);
        ui.weak("Drag/MMB: pan   Wheel: zoom");
        ui.weak("Click: select tile");
    }

    fn central_panel(&mut self, ui: &mut Ui) {
        let available = vec2(ui.available_width(), ui.available_height());
        let (view_rect, resp) = ui.allocate_exact_size(available, Sense::click_and_drag());
        let painter = ui.painter_at(view_rect);

        // ── Input ────────────────────────────────────────────────────────────
        let is_pan = resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Primary);
        if is_pan {
            self.offset += resp.drag_delta() / self.zoom;
        }

        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll != 0.0 && resp.hovered() {
            let factor = 1.0 + scroll * 0.001;
            self.zoom = (self.zoom * factor).clamp(0.25, 16.0);
        }

        // ── Background ───────────────────────────────────────────────────────
        painter.rect_filled(view_rect, Rounding::ZERO, Color32::from_rgb(16, 16, 20));

        let z = self.zoom;
        // L1 Map16 blocks are 16×16 game pixels.  The canvas border and all
        // hover/grid/selection overlays use map16_sz so they align with L1.
        let map16_sz = MAP16_PX * z;
        let canvas_w = OW_COLS as f32 * map16_sz; // 32 * 16 = 512 game px
        let canvas_h = OW_ROWS_PER_SUBMAP as f32 * map16_sz; // 32 * 16 = 512 game px
        let origin = view_rect.min + self.offset * z;
        let ow_rect = Rect::from_min_size(origin, vec2(canvas_w, canvas_h));

        // ── GL render ────────────────────────────────────────────────────────
        {
            let renderer = Arc::clone(&self.renderer);
            let draw_l1 = self.show_layer1;
            let draw_l2 = self.show_layer2;
            let ppp = ui.ctx().pixels_per_point();
            let screen_sz = view_rect.size() * ppp;
            // gl_offset: how many game pixels the top-left of the viewport is
            // offset from the canvas origin (passed directly to the shader).
            let gl_offset = -(self.offset + view_rect.min.to_vec2() / z);
            let gl_zoom = z * ppp;

            ui.painter().add(PaintCallback {
                rect: view_rect,
                callback: Arc::new(CallbackFn::new(move |_info, painter| {
                    let mut r = renderer.lock().expect("Cannot lock overworld renderer");
                    r.paint(painter.gl(), screen_sz, gl_zoom, gl_offset, draw_l1, draw_l2);
                })),
            });
        }

        // ── Border around canvas ──────────────────────────────────────────────
        painter.rect_stroke(ow_rect, Rounding::ZERO, Stroke::new(2.0, Color32::from_white_alpha(140)));

        // ── Grid (Map16 block grid, aligned to L1) ───────────────────────────
        if self.show_grid || ui.input(|i| i.modifiers.shift_only()) {
            let stroke = Stroke::new(0.5, Color32::from_white_alpha(25));
            let start_col = ((view_rect.min.x - origin.x) / map16_sz).floor() as i32;
            let end_col = ((view_rect.max.x - origin.x) / map16_sz).ceil() as i32;
            for c in start_col..=end_col {
                let px = origin.x + c as f32 * map16_sz;
                painter.vline(px, view_rect.y_range(), stroke);
            }
            let start_row = ((view_rect.min.y - origin.y) / map16_sz).floor() as i32;
            let end_row = ((view_rect.max.y - origin.y) / map16_sz).ceil() as i32;
            for r in start_row..=end_row {
                let py = origin.y + r as f32 * map16_sz;
                painter.hline(view_rect.x_range(), py, stroke);
            }
        }

        // ── Hover / click (Map16 block granularity) ───────────────────────────
        if let Some(cursor) = resp.hover_pos() {
            let rel = (cursor - origin) / map16_sz;
            let tx = rel.x.floor() as i32;
            let ty = rel.y.floor() as i32;
            if (0..OW_COLS as i32).contains(&tx) && (0..OW_ROWS_PER_SUBMAP as i32).contains(&ty) {
                let x = tx as u32;
                let y = ty as u32;
                let tile_id = self.cpu.mem.load_u8(ow_l1_addr(x, y, self.submap));
                let tile_rect =
                    Rect::from_min_size(origin + vec2(x as f32 * map16_sz, y as f32 * map16_sz), Vec2::splat(map16_sz));
                painter.rect_stroke(tile_rect, Rounding::ZERO, Stroke::new(1.0, Color32::WHITE));

                if resp.clicked_by(egui::PointerButton::Primary) {
                    self.selected_tile = Some((x, y));
                }

                painter.text(
                    view_rect.right_bottom() - vec2(6.0, 6.0),
                    egui::Align2::RIGHT_BOTTOM,
                    format!("({tx},{ty})  L1={tile_id:#04x}  {:.0}%", z * 100.0),
                    egui::FontId::monospace(10.0),
                    Color32::from_white_alpha(170),
                );
            }
        }

        // ── Selected tile highlight ───────────────────────────────────────────
        if let Some((x, y)) = self.selected_tile {
            let r = Rect::from_min_size(origin + vec2(x as f32 * map16_sz, y as f32 * map16_sz), Vec2::splat(map16_sz));
            painter.rect_stroke(r, Rounding::ZERO, Stroke::new(2.0, Color32::from_rgb(255, 220, 0)));
        }
    }
}

// ── Tile list builders ────────────────────────────────────────────────────────

/// Build draw list for Layer 1.
///
/// $7EC800 holds 0x800 u8 tile-type IDs (copied verbatim from OWL1TileData by MVN).
/// CODE_04DC09 fills Map16Pointers at WRAM $7E0FBE: a table of 0x200 u16 entries
/// where entry[tile_id] = 16-bit offset into OWL1CharData (bank $05D000).
/// Each OWL1CharData block is 8 bytes = 4 u16 sub-tile attribute words for the
/// four 8×8 pixels that make up a 16×16 Map16 block.
fn build_l1_tiles(cpu: &mut Cpu, submap: u8) -> Vec<Tile> {
    let mut tiles = Vec::with_capacity((OW_COLS * OW_ROWS_PER_SUBMAP) as usize * 4);

    // Map16Pointers table in WRAM: 0x7E0FBE.
    // CODE_04DC09 sets Map16Pointers[tid] = 0xD000 + tid*8  (for 0x200 entries).
    let ptr_base: u32 = 0x7E_0FBE;

    // OWL1CharData bank: $05.  Bank-relative pointers stored in Map16Pointers.
    let char_bank: u32 = 0x05_0000;

    // Only render 32 rows per submap (main map uses rows 0-31, submaps use rows 32-63 in buffer)
    for row in 0..OW_ROWS_PER_SUBMAP {
        for col in 0..OW_COLS {
            // Read tile-type ID: one byte from the u8 tile array at $7EC800.
            let tile_id = cpu.mem.load_u8(ow_l1_addr(col, row, submap)) as u32;

            // Map16Pointers[tile_id] = 16-bit bank-relative offset into OWL1CharData.
            let char_ptr = cpu.mem.load_u16(ptr_base + tile_id * 2) as u32;
            let gfx_addr = char_bank | (char_ptr & 0xFFFF);

            let px = col * 16;
            let py = row * 16;
            // 4 sub-tiles in order: top-left, bottom-left, top-right, bottom-right.
            for (si, (ox, oy)) in [(0u32, 0u32), (0u32, 8u32), (8u32, 0u32), (8u32, 8u32)].iter().enumerate() {
                let sub_tile = cpu.mem.load_u16(gfx_addr + si as u32 * 2);
                tiles.push(ow_tile(px + ox, py + oy, sub_tile));
            }
        }
    }
    tiles
}

/// Build draw list for Layer 2 (row-major 40-wide decompressed tilemap).
fn build_l2_tiles(cpu: &mut Cpu) -> Vec<Tile> {
    let mut tiles = Vec::with_capacity((OW_L2_COLS * OW_L2_ROWS) as usize);
    for row in 0..OW_L2_ROWS {
        for col in 0..OW_L2_COLS {
            let addr = ow_l2_addr(col, row);
            let t = cpu.mem.load_u16(addr);
            // L2 is BG mode 1 — each tile is a single 8×8 VRAM tile entry.
            tiles.push(ow_tile(col * 8, row * 8, t));
        }
    }
    tiles
}

/// Convert a raw u16 SNES tile attribute word into a renderer Tile.
fn ow_tile(x: u32, y: u32, t: u16) -> Tile {
    let t32 = t as u32;
    let tile = t32 & 0x3FF;
    let pal = (t32 >> 10) & 0x7;
    let scale = 8u32;
    let params = scale | (pal << 8) | (t32 & 0xC000);
    Tile([x, y, tile, params])
}
