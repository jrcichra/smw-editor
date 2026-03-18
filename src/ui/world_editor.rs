//! World Map (Overworld) Editor UI.
//!
//! The overworld tilemap is stored in WRAM at $7EC800 (Map16TilesLow) after the
//! game's init routines run. The data is copied directly from ROM via MVN with
//! no transformation, stored in row-major order:
//!   idx = y * 32 + x
//!
//! Main map uses indices 0x000-0x3FF (32 rows × 32 cols)
//! Submaps use indices 0x400-0x7FF (also 32×32, accessed via +0x400 offset)
//! Each byte is the Map16 tile-type ID (0–190).
//!
//! Layer 2 ($7F4000 / OWLayer2Tilemap): row-major 64 cols × 64 rows,
//! indexed as ((Y * 64) + X) * 2. Each entry is [tile_num_u8, YXPCCCTT_u8]
//! stored interleaved by LC_RLE2 (two-pass decompressor). Reading as LE u16
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

/// Layer 1 (Map16 blocks): 32×32 blocks per submap, each 16×16 pixels = 512×512 pixels total.
const MAP16_COLS: u32 = 32;
const MAP16_ROWS: u32 = 32;

/// Layer 2 (VRAM tiles): 64×64 tiles per submap, each 8×8 pixels = 512×512 pixels total.
const VRAM_TILE_COLS: u32 = 64;
const VRAM_TILE_ROWS: u32 = 64;

/// WRAM base for the OW Layer-1 tile-type bytes (u8 each, 0x800 total).
/// Populated by CODE_04DC09 via `MVN $7E,$0C` from OWL1TileData at $0CF7DF.
const MAP16_TILES_LOW: u32 = 0x7EC800;

/// WRAM base for the layer-2 tilemap (u16 each, row-major 64 cols × 64 rows).
const OW_L2_BASE: u32 = 0x7F4000;

/// Layer-2 dimensions: 64 tile-cols × 64 tile-rows (full decompressed tilemap).
/// Each tile is 8×8 game pixels. Formula: ((Y * 64) + X) * 2

// ── SNES overworld tile-index helpers ─────────────────────────────────────────

/// Convert screen (col, row) → memory address in Map16Tiles at $7EC800.
///
/// From ASM OW_TilePos_Calc ($049866), uses quadrant-based indexing:
/// - Bits 0-3: X & 0x0F (column within 16-tile quadrant)
/// - Bits 4-7: Y & 0x0F (row within 16-tile quadrant)
/// - Bit 8: X & 0x10 (selects left/right half of 32-col screen)
/// - Bit 9: Y & 0x10 (selects top/bottom half of 32-row screen)
///
/// Quadrant layout: TL(0x000-0x0FF), TR(0x100-0x1FF), BL(0x200-0x2FF), BR(0x300-0x3FF)
/// Submaps add +0x400 offset.
fn ow_l1_addr(col: u32, row: u32, submap: u8) -> u32 {
    // X contribution: (X & 0x0F) | ((X & 0x10) << 4)  // puts X bit 4 at address bit 8
    let x_part = (col & 0x0F) | ((col & 0x10) << 4);

    // Y contribution: ((Y & 0x0F) << 4) | ((Y & 0x10) << 5)  // puts Y bit 4 at address bit 9
    let y_part = ((row & 0x0F) << 4) | ((row & 0x10) << 5);

    let idx = x_part + y_part;

    // Submaps use the second half of the 0x800 byte buffer
    let final_idx = if submap != 0 { idx + 0x400 } else { idx };

    MAP16_TILES_LOW + final_idx
}

/// Layer 2 dimensions: 64 columns × 64 rows (full tilemap).
/// Tiles stored in interleaved [tile_num][YXPCCCTT] format at $7F4000.
/// Index formula: ((Y * 64) + X) * 2 (row-major, 64 columns wide).
/// The full tilemap is 64×64 tiles = 512×512 pixels to match L1's 512×512.
const OW_L2_COLS: u32 = 64;

fn ow_l2_addr(col: u32, row: u32) -> u32 {
    OW_L2_BASE + ((row * OW_L2_COLS + col) * 2) as u32
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

        // Debug: Log tile counts
        log::info!("Loaded submap {}: L1={} tiles, L2={} tiles", self.submap, l1.len(), l2.len());

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
            // L1 tilemap stores u8 tile-type IDs (1 byte per tile)
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
            // OWLayer1Translevel: level number stored at $7ED000 (u8 indexed, same layout as tilemap)
            let xlevel_addr = 0x7ED000_u32 + (ow_l1_addr(x, y, self.submap) - MAP16_TILES_LOW);
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
        let canvas_w = MAP16_COLS as f32 * map16_sz; // 32 * 16 = 512 game px
        let canvas_h = MAP16_ROWS as f32 * map16_sz; // 32 * 16 = 512 game px
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
            if (0..MAP16_COLS as i32).contains(&tx) && (0..MAP16_ROWS as i32).contains(&ty) {
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
    let mut tiles = Vec::with_capacity((MAP16_COLS * MAP16_ROWS) as usize * 4);

    // Map16Pointers table in WRAM: 0x7E0FBE.
    // CODE_04DC09 sets Map16Pointers[tid] = offset from $05:0000 into OWL1CharData.
    // Full address = $05:0000 + pointer_value (where pointer_value = $D000 + tid*8)
    let ptr_base: u32 = 0x7E_0FBE;
    let char_bank: u32 = 0x05_0000;

    // Debug: Track unique tile types
    let mut unique_tiles = std::collections::HashSet::new();

    // Only render 32 rows per submap
    for row in 0..MAP16_ROWS {
        for col in 0..MAP16_COLS {
            // Read tile-type ID: u8 from the tile array at $7EC800 (1 byte per tile).
            let addr = ow_l1_addr(col, row, submap);
            let tile_id = cpu.mem.load_u8(addr) as u32;

            if tile_id != 0 {
                unique_tiles.insert(tile_id);
            }

            // Map16Pointers[tile_id] = 16-bit offset from $05:0000 into OWL1CharData.
            // Full SNES address = $05:0000 | pointer_value
            let char_ptr = cpu.mem.load_u16(ptr_base + tile_id * 2) as u32;
            let gfx_addr = char_bank | char_ptr;

            let px = col * 16;
            let py = row * 16;
            // 4 sub-tiles: word 0->TL, word 1->BL, word 2->TR, word 3->BR
            for (si, (ox, oy)) in [(0u32, 0u32), (0u32, 8u32), (8u32, 0u32), (8u32, 8u32)].iter().enumerate() {
                let sub_tile = cpu.mem.load_u16(gfx_addr + si as u32 * 2);
                tiles.push(ow_tile(px + ox, py + oy, sub_tile));
            }
        }
    }

    log::info!("build_l1_tiles: {} unique non-zero tile types used", unique_tiles.len());
    tiles
}

/// Build draw list for Layer 2 (render 64×64 tiles for the overworld background).
/// L2 uses row-major addressing: ((Y * 64) + X) * 2 at $7F4000.
/// The full tilemap is 64×64 tiles = 512×512 pixels to match L1's dimensions.
fn build_l2_tiles(cpu: &mut Cpu) -> Vec<Tile> {
    // L2 is 64×64 tiles = 512×512 pixels to match L1's 512×512 pixel area
    let l2_rows = VRAM_TILE_ROWS; // 64 rows

    let mut tiles = Vec::with_capacity((OW_L2_COLS * l2_rows) as usize);

    for row in 0..l2_rows {
        for col in 0..OW_L2_COLS {
            let addr = ow_l2_addr(col, row);
            // L2 format: [tile_num_low][YXPCCCTT] interleaved
            let t0 = cpu.mem.load_u8(addr) as u16;
            let t1 = cpu.mem.load_u8(addr + 1) as u16;
            let tile_num = t0 | ((t1 & 3) << 8);
            let palette = (t1 >> 2) & 7;
            let flip_x = (t1 & 0x40) != 0;
            let flip_y = (t1 & 0x80) != 0;
            // Pack into format expected by ow_tile: [tile(10bits)|pal(3bits)|flip(2bits)|scale(8bits)]
            let t = tile_num | (palette << 10) | ((flip_x as u16) << 14) | ((flip_y as u16) << 15);
            // L2 tiles are 8×8 pixels, render at (col*8, row*8)
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
