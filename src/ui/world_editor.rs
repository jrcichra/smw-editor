//! World Map (Overworld) Editor UI.
//!
//! The overworld tilemap is stored in WRAM at $7EC800 (Map16TilesLow) after the
//! game's init routines run. `CODE_04DC09` copies `OWL1TileData` with `MVN`,
//! so the 0x800-byte buffer stays in its packed ROM layout: 64 columns × 32 rows
//! of u8 Map16 tile IDs in row-major order. The game selects each submap by
//! changing the camera position, not by swapping to a separate L1 buffer.
//!
//! Layer 2 ($7F4000 / OWLayer2Tilemap): a 64×64 8×8-tile map stored as four
//! 32×32 screens (2 across × 2 down). Each entry is [tile_num_u8, YXPCCCTT_u8].

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

/// Visible viewport size used by the editor: 32×32 Map16 blocks = 512×512 pixels.
const SUBMAP_VIEW_X: i32 = 16;
const SUBMAP_VIEW_Y: i32 = 40;
const SUBMAP_VIEW_W: u32 = 224;
const SUBMAP_VIEW_H: u32 = 168;

/// Full BG tilemap size after the game composes the active overworld into VRAM.
const VRAM_TILE_ROWS: u32 = 64;
const VRAM_L1_TILEMAP_BASE: usize = 0x2000 * 2;
const VRAM_L2_TILEMAP_BASE: usize = 0x3000 * 2;

// ── SNES overworld tile-index helpers ─────────────────────────────────────────

const OW_L2_COLS: u32 = 64;

fn tilemap_vram_addr(base: usize, col: u32, row: u32) -> usize {
    let quadrant = ((row / 32) * 2) + (col / 32);
    let sub_row = row % 32;
    let sub_col = col % 32;
    let quadrant_offset = quadrant * 32 * 32 * 2;
    let idx = quadrant_offset + ((sub_row * 32 + sub_col) * 2);
    base + idx as usize
}

fn visible_map_size(submap: u8) -> (u32, u32) {
    if submap == 0 {
        (512, 512)
    } else {
        (SUBMAP_VIEW_W, SUBMAP_VIEW_H)
    }
}

fn visible_map_crop(submap: u8) -> (u32, u32) {
    if submap == 0 {
        (0, 0)
    } else {
        (SUBMAP_VIEW_X as u32, SUBMAP_VIEW_Y as u32)
    }
}

fn l1_vram_addr_for_map16(submap: u8, map16_x: u32, map16_y: u32) -> usize {
    let (crop_x, crop_y) = visible_map_crop(submap);
    let tile_x = (map16_x * 16 + crop_x) / 8;
    let tile_y = (map16_y * 16 + crop_y) / 8;
    tilemap_vram_addr(VRAM_L1_TILEMAP_BASE, tile_x, tile_y)
}

// ── OpenGL renderer ───────────────────────────────────────────────────────────

#[derive(Debug)]
struct OverworldRenderer {
    layer1: TileRenderer,
    layer2: TileRenderer,
    gfx_bufs: GfxBuffers,
    #[allow(dead_code)]
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
    needs_center: bool,
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
            needs_center: false,
        };
        editor.load_submap();
        editor
    }

    fn load_submap(&mut self) {
        activate_all_overworld_events(&mut self.cpu);
        smwe_emu::emu::load_overworld(&mut self.cpu, self.submap);

        let mut r = self.renderer.lock().expect("Cannot lock overworld renderer");
        r.upload_palette(&self.gl, &self.cpu.mem.cgram);
        r.upload_gfx(&self.gl, &self.cpu.mem.vram);

        let l2_scroll_x = i16::from_le_bytes(self.cpu.mem.load_u16(0x001E).to_le_bytes()) as i32;
        let l2_scroll_y = i16::from_le_bytes(self.cpu.mem.load_u16(0x0020).to_le_bytes()) as i32;

        let l1 = build_bg_tiles(&self.cpu.mem.vram, VRAM_L1_TILEMAP_BASE, self.submap, l2_scroll_x, l2_scroll_y);
        let l2 = build_bg_tiles(&self.cpu.mem.vram, VRAM_L2_TILEMAP_BASE, self.submap, l2_scroll_x, l2_scroll_y);

        // Debug: Log tile counts
        log::info!("Loaded submap {}: L1={} tiles, L2={} tiles", self.submap, l1.len(), l2.len());

        r.set_tiles(&self.gl, l1, l2);

        self.offset = Vec2::ZERO;
        self.selected_tile = None;
        self.needs_center = true;
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
            let addr = l1_vram_addr_for_map16(self.submap, x, y);
            let sub0 = u16::from_le_bytes([self.cpu.mem.vram[addr], self.cpu.mem.vram[addr + 1]]);
            let tile_num = (sub0 & 0x3FF) as u32;
            let pal = ((sub0 >> 10) & 0x7) as u32;
            let flip_x = (sub0 & 0x4000) != 0;
            let flip_y = (sub0 & 0x8000) != 0;
            ui.monospace(format!("  TL vram #{tile_num:03X}  pal {pal}"));
            if flip_x || flip_y {
                ui.monospace(format!("  flip x={flip_x} y={flip_y}"));
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

        // ── Auto-center on submap load ─────────────────────────────────────
        if self.needs_center {
            self.needs_center = false;
            let z = self.zoom;
            let (map_px_w, map_px_h) = visible_map_size(self.submap);
            self.offset =
                vec2((view_rect.width() / z - map_px_w as f32) * 0.5, (view_rect.height() / z - map_px_h as f32) * 0.5);
        }

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
        let (map_px_w, map_px_h) = visible_map_size(self.submap);
        let map16_cols = map_px_w.div_ceil(16);
        let map16_rows = map_px_h.div_ceil(16);
        // L1 Map16 blocks are 16×16 game pixels.  The canvas border and all
        // hover/grid/selection overlays use map16_sz so they align with L1.
        let map16_sz = MAP16_PX * z;
        let canvas_w = map_px_w as f32 * z;
        let canvas_h = map_px_h as f32 * z;
        let origin = view_rect.min + self.offset * z;
        let ow_rect = Rect::from_min_size(origin, vec2(canvas_w, canvas_h));

        // ── GL render ────────────────────────────────────────────────────────
        {
            let renderer = Arc::clone(&self.renderer);
            let draw_l1 = self.show_layer1;
            let draw_l2 = self.show_layer2;
            let ppp = ui.ctx().pixels_per_point();
            let screen_sz = view_rect.size() * ppp;
            // The paint callback renders in view-local coordinates, so the GL
            // tile origin must match the egui overlay origin exactly.
            let gl_offset = self.offset;
            let gl_zoom = z * ppp;

            ui.painter().add(PaintCallback {
                rect: view_rect,
                callback: Arc::new(CallbackFn::new(move |_info, painter| {
                    let r = renderer.lock().expect("Cannot lock overworld renderer");
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
            if (0..map16_cols as i32).contains(&tx) && (0..map16_rows as i32).contains(&ty) {
                let x = tx as u32;
                let y = ty as u32;
                let addr = l1_vram_addr_for_map16(self.submap, x, y);
                let tile_id = u16::from_le_bytes([self.cpu.mem.vram[addr], self.cpu.mem.vram[addr + 1]]) & 0x03FF;
                let tile_rect =
                    Rect::from_min_size(origin + vec2(x as f32 * map16_sz, y as f32 * map16_sz), Vec2::splat(map16_sz));
                painter.rect_stroke(tile_rect, Rounding::ZERO, Stroke::new(1.0, Color32::WHITE));

                if resp.clicked_by(egui::PointerButton::Primary) {
                    self.selected_tile = Some((x, y));
                }

                painter.text(
                    view_rect.right_bottom() - vec2(6.0, 6.0),
                    egui::Align2::RIGHT_BOTTOM,
                    format!("({tx},{ty})  L1={tile_id:#05x}  {:.0}%", z * 100.0),
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

/// Build draw list from the composed BG tilemap already uploaded to VRAM.
fn build_bg_tiles(vram: &[u8], tilemap_base: usize, submap: u8, scroll_x: i32, scroll_y: i32) -> Vec<Tile> {
    let mut tiles = Vec::with_capacity((OW_L2_COLS * VRAM_TILE_ROWS) as usize);
    let (crop_x, crop_y, view_w, view_h) = if submap == 0 {
        (0, 0, 512, 512)
    } else {
        (SUBMAP_VIEW_X, SUBMAP_VIEW_Y, SUBMAP_VIEW_W as i32, SUBMAP_VIEW_H as i32)
    };

    for row in 0..VRAM_TILE_ROWS {
        for col in 0..OW_L2_COLS {
            let addr = tilemap_vram_addr(tilemap_base, col, row);
            let t0 = vram[addr] as u16;
            let t1 = vram[addr + 1] as u16;
            let tile_num = t0 | ((t1 & 3) << 8);
            let palette = (t1 >> 2) & 7;
            let flip_x = (t1 & 0x40) != 0;
            let flip_y = (t1 & 0x80) != 0;
            let px = (col * 8) as i32 - scroll_x - crop_x;
            let py = (row * 8) as i32 - scroll_y - crop_y;
            if px <= -8 || py <= -8 || px >= view_w || py >= view_h {
                continue;
            }

            let t = tile_num | (palette << 10) | ((flip_x as u16) << 14) | ((flip_y as u16) << 15);
            tiles.push(ow_tile(px.max(0) as u32, py.max(0) as u32, t));
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

fn activate_all_overworld_events(cpu: &mut Cpu) {
    for addr in 0x1F02u32..=0x1F60 {
        cpu.mem.store_u8(addr, 0xFF);
    }
}
