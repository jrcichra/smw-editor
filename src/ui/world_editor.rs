//! World Map (Overworld) Editor UI.
//!
//! The overworld tilemap is stored in WRAM at $7EC800 (Map16TilesLow) after the
//! game's init routines run.  The index formula from `OW_TilePos_Calc` is:
//!
//!   idx = (x & 0xF) | ((x & 0x10) << 4) | ((y & 0xF) << 4) | (y >= 16 ? 0x200 : 0)
//!         | (submap != 0 ? 0x400 : 0)
//!
//! giving a packed 4-quadrant layout (top-left, top-right, bottom-left, bottom-right
//! each 16×16 tiles) in SNES VRAM-tilemap order.
//!
//! Main map  : 32 cols × 32 rows, indices 0x000–0x3FF, each u16 at $7EC800+idx*2
//! Submaps   : same size,          indices 0x400–0x7FF, same base address
//!
//! Layer 2 ($7F4000 / OWLayer2Tilemap): 64×64 16-bit entries in simple
//! row-major order:  row*64+col  (40 cols × some rows, from the decompressor).

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use egui::{
    vec2, CentralPanel, Color32, Frame, PaintCallback, Rect, Rounding, Sense, SidePanel, Stroke,
    Ui, Vec2, WidgetText,
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

/// 8×8 pixels per overworld tile.
const TILE_PX: f32 = 8.0;

/// Main map / each submap: 32 tile-columns × 32 tile-rows.
/// (The full screen shown by the game scrolls within this 256×256-pixel area.)
const OW_COLS: u32 = 32;
const OW_ROWS: u32 = 32;

/// WRAM base for the packed Map16 tile entries (u16 each).
const MAP16_TILES_LOW: u32 = 0x7EC800;

/// WRAM base for the layer-2 tilemap (u16 each, row-major 40 cols × variable rows).
const OW_L2_BASE: u32 = 0x7F4000;

/// Layer-2 logical dimensions (40 tile-cols × variable rows; the game decompresses
/// up to 64 rows but only 28 are visible).  We read 40×28 = 1120 entries.
const OW_L2_COLS: u32 = 40;
const OW_L2_ROWS: u32 = 28;

// ── SNES overworld tile-index helpers ─────────────────────────────────────────

/// Convert (col, row, submap) → word-index into Map16TilesLow ($7EC800).
///
/// Formula taken verbatim from `OW_TilePos_Calc` in bank_04.asm:
///   idx = (x & 0xF) | ((x & 0x10) << 4) | ((y & 0xF) << 4)
///         | (y >= 16 ? 0x200 : 0) | (submap ? 0x400 : 0)
fn ow_l1_idx(col: u32, row: u32, submap: u8) -> u32 {
    let x = col;
    let y = row;
    (x & 0xF) | ((x & 0x10) << 4) | ((y & 0xF) << 4) | (if y >= 16 { 0x200 } else { 0 })
        | (if submap != 0 { 0x400 } else { 0 })
}

fn ow_l1_addr(col: u32, row: u32, submap: u8) -> u32 {
    MAP16_TILES_LOW + ow_l1_idx(col, row, submap) * 2
}

/// Layer-2 tilemap: simple row-major, 40 columns wide.
fn ow_l2_addr(col: u32, row: u32) -> u32 {
    OW_L2_BASE + (row * OW_L2_COLS + col) * 2
}

// ── OpenGL renderer ───────────────────────────────────────────────────────────

#[derive(Debug)]
struct OverworldRenderer {
    layer1:    TileRenderer,
    layer2:    TileRenderer,
    gfx_bufs:  GfxBuffers,
    offset:    Vec2,
    destroyed: bool,
}

impl OverworldRenderer {
    fn new(gl: &glow::Context) -> Self {
        Self {
            layer1:    TileRenderer::new(gl),
            layer2:    TileRenderer::new(gl),
            gfx_bufs:  GfxBuffers::new(gl),
            offset:    Vec2::ZERO,
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

    fn paint(
        &self,
        gl: &glow::Context,
        screen_size: Vec2,
        zoom: f32,
        offset: Vec2,
        draw_l1: bool,
        draw_l2: bool,
    ) {
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
    gl:       Arc<glow::Context>,
    #[allow(dead_code)]
    rom:      Arc<SmwRom>,
    cpu:      Cpu,
    renderer: Arc<Mutex<OverworldRenderer>>,

    submap: u8,

    offset:       Vec2,
    zoom:         f32,
    show_grid:    bool,
    show_layer1:  bool,
    show_layer2:  bool,
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
        SidePanel::left("world_editor.left_panel")
            .resizable(false)
            .show_inside(ui, |ui| self.left_panel(ui));
        CentralPanel::default()
            .frame(Frame::none().inner_margin(0.))
            .show_inside(ui, |ui| self.central_panel(ui));
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
            let addr = ow_l1_addr(x, y, self.submap);
            let t = self.cpu.mem.load_u16(addr);
            ui.monospace(format!("Tile entry: 0x{t:04X}"));
            let tile_num = t & 0x1FF;
            let pal      = (t >> 10) & 0x7;
            let flip_x   = (t & 0x4000) != 0;
            let flip_y   = (t & 0x8000) != 0;
            ui.monospace(format!("  tile #{tile_num:03X}  pal {pal}"));
            if flip_x || flip_y {
                ui.monospace(format!("  flip x={flip_x} y={flip_y}"));
            }
            // Level number on this tile
            let xlevel_addr = 0x7ED000 + ow_l1_idx(x, y, self.submap) * 2;
            let xlevel = self.cpu.mem.load_u16(xlevel_addr) & 0x1FF;
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
        let is_pan = resp.dragged_by(egui::PointerButton::Middle)
            || resp.dragged_by(egui::PointerButton::Primary);
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

        let z        = self.zoom;
        let tile_sz  = TILE_PX * z;
        let canvas_w = OW_COLS as f32 * tile_sz;
        let canvas_h = OW_ROWS as f32 * tile_sz;
        let origin   = view_rect.min + self.offset * z;
        let ow_rect  = Rect::from_min_size(origin, vec2(canvas_w, canvas_h));

        // ── GL render ────────────────────────────────────────────────────────
        {
            let renderer   = Arc::clone(&self.renderer);
            let draw_l1    = self.show_layer1;
            let draw_l2    = self.show_layer2;
            let ppp        = ui.ctx().pixels_per_point();
            let screen_sz  = view_rect.size() * ppp;
            // gl_offset: how many game pixels the top-left of the viewport is
            // offset from the canvas origin (passed directly to the shader).
            let gl_offset  = -(self.offset + view_rect.min.to_vec2() / z);
            let gl_zoom    = z * ppp;

            ui.painter().add(PaintCallback {
                rect: view_rect,
                callback: Arc::new(CallbackFn::new(move |_info, painter| {
                    let mut r = renderer.lock().expect("Cannot lock overworld renderer");
                    r.paint(painter.gl(), screen_sz, gl_zoom, gl_offset, draw_l1, draw_l2);
                })),
            });
        }

        // ── Border around canvas ──────────────────────────────────────────────
        painter.rect_stroke(
            ow_rect,
            Rounding::ZERO,
            Stroke::new(2.0, Color32::from_white_alpha(140)),
        );

        // ── Grid ─────────────────────────────────────────────────────────────
        if self.show_grid || ui.input(|i| i.modifiers.shift_only()) {
            let stroke = Stroke::new(0.5, Color32::from_white_alpha(25));
            // vertical lines
            let start_col = ((view_rect.min.x - origin.x) / tile_sz).floor() as i32;
            let end_col   = ((view_rect.max.x - origin.x) / tile_sz).ceil()  as i32;
            for c in start_col..=end_col {
                let px = origin.x + c as f32 * tile_sz;
                painter.vline(px, view_rect.y_range(), stroke);
            }
            // horizontal lines
            let start_row = ((view_rect.min.y - origin.y) / tile_sz).floor() as i32;
            let end_row   = ((view_rect.max.y - origin.y) / tile_sz).ceil()  as i32;
            for r in start_row..=end_row {
                let py = origin.y + r as f32 * tile_sz;
                painter.hline(view_rect.x_range(), py, stroke);
            }
        }

        // ── Hover / click ─────────────────────────────────────────────────────
        if let Some(cursor) = resp.hover_pos() {
            let rel = (cursor - origin) / tile_sz;
            let tx  = rel.x.floor() as i32;
            let ty  = rel.y.floor() as i32;
            if (0..OW_COLS as i32).contains(&tx) && (0..OW_ROWS as i32).contains(&ty) {
                let x = tx as u32;
                let y = ty as u32;
                let t = self.cpu.mem.load_u16(ow_l1_addr(x, y, self.submap));
                let tile_rect = Rect::from_min_size(
                    origin + vec2(x as f32 * tile_sz, y as f32 * tile_sz),
                    Vec2::splat(tile_sz),
                );
                painter.rect_stroke(tile_rect, Rounding::ZERO, Stroke::new(1.0, Color32::WHITE));

                if resp.clicked_by(egui::PointerButton::Primary) {
                    self.selected_tile = Some((x, y));
                }

                // Status bar
                painter.text(
                    view_rect.right_bottom() - vec2(6.0, 6.0),
                    egui::Align2::RIGHT_BOTTOM,
                    format!("({tx},{ty})  L1=0x{t:04X}  {:.0}%", z * 100.0),
                    egui::FontId::monospace(10.0),
                    Color32::from_white_alpha(170),
                );
            }
        }

        // ── Selected tile highlight ───────────────────────────────────────────
        if let Some((x, y)) = self.selected_tile {
            let r = Rect::from_min_size(
                origin + vec2(x as f32 * tile_sz, y as f32 * tile_sz),
                Vec2::splat(tile_sz),
            );
            painter.rect_stroke(r, Rounding::ZERO, Stroke::new(2.0, Color32::from_rgb(255, 220, 0)));
        }
    }
}

// ── Tile list builders ────────────────────────────────────────────────────────

/// Build draw list for Layer 1 (the SNES 4-quadrant packed tilemap).
fn build_l1_tiles(cpu: &mut Cpu, submap: u8) -> Vec<Tile> {
    let mut tiles = Vec::with_capacity((OW_COLS * OW_ROWS) as usize * 4);
    for row in 0..OW_ROWS {
        for col in 0..OW_COLS {
            let addr = ow_l1_addr(col, row, submap);
            let t    = cpu.mem.load_u16(addr);
            // Each Map16 entry is a 16×16 block built from 4 VRAM 8×8 tiles.
            // OWL1CharData layout: 8 bytes per block → 4 × u16 sub-tiles.
            // The Map16Pointers table (filled by CODE_04DC09) points into OWL1CharData.
            // The actual 8×8 sub-tiles are accessed via the pointer table the
            // game built into WRAM.  We read the four sub-tiles directly from
            // the OWL1CharData via the Map16Pointers table the emulator set up.
            let block_id = (t & 0x1FF) as u32;
            let map16_ptr_base = cpu.mem.cart.resolve("Map16Pointers")
                .unwrap_or(0x7E0000 + 0x0FBE); // fallback
            // Map16Pointers[block_id] = word-pointer into OWL1CharData (bank $05)
            let char_ptr = cpu.mem.load_u16(map16_ptr_base + block_id * 2) as u32;
            // OWL1CharData lives in ROM bank $05; the pointer is a 16-bit offset
            // within that bank.
            let char_base: u32 = cpu.mem.cart.resolve("OWL1CharData").unwrap_or(0x050000);
            let char_bank = char_base & 0xFF0000;
            let gfx_addr  = char_bank | char_ptr;

            let px = col * 16;
            let py = row * 16;
            // 4 sub-tiles: UL, LL, UR, LR (same layout as regular Map16)
            for (si, (ox, oy)) in [(0u32, 0u32), (0, 8), (8, 0), (8, 8)].iter().enumerate() {
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
            let t    = cpu.mem.load_u16(addr);
            // L2 is BG mode 1 — each tile is a single 8×8 VRAM tile entry.
            tiles.push(ow_tile(col * 8, row * 8, t));
        }
    }
    tiles
}

/// Convert a raw u16 SNES tile attribute word into a renderer Tile.
fn ow_tile(x: u32, y: u32, t: u16) -> Tile {
    let t32   = t as u32;
    let tile  = t32 & 0x3FF;
    let pal   = (t32 >> 10) & 0x7;
    let scale = 8u32;
    let params = scale | (pal << 8) | (t32 & 0xC000);
    Tile([x, y, tile, params])
}
