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
    vec2, CentralPanel, Color32, CornerRadius, Frame, PaintCallback, Rect, Sense, SidePanel, Stroke, StrokeKind, Ui, Vec2,
    WidgetText,
};
use egui_glow::CallbackFn;
use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};
use smwe_render::{
    gfx_buffers::GfxBuffers,
    tile_renderer::{Tile, TileRenderer, TileUniforms},
};
use smwe_rom::compression::lc_rle2;
use smwe_rom::{
    overworld::{OWL1_TILE_DATA_SNES, OWL1_TILE_DATA_SIZE, SUBMAP_NAMES},
    snes_utils::addr::AddrPc,
    SmwRom,
};

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
pub(super) const VRAM_TILE_ROWS: u32 = 64;
pub(super) const VRAM_L1_TILEMAP_BASE: usize = 0x2000 * 2;
pub(super) const VRAM_L2_TILEMAP_BASE: usize = 0x3000 * 2;

// ── SNES overworld tile-index helpers ─────────────────────────────────────────

pub(super) const OW_L2_COLS: u32 = 64;

pub(super) fn tilemap_vram_addr(base: usize, col: u32, row: u32) -> usize {
    let quadrant = ((row / 32) * 2) + (col / 32);
    let sub_row = row % 32;
    let sub_col = col % 32;
    let quadrant_offset = quadrant * 32 * 32 * 2;
    let idx = quadrant_offset + ((sub_row * 32 + sub_col) * 2);
    base + idx as usize
}

pub(super) fn visible_map_size(submap: u8) -> (u32, u32) {
    if submap == 0 {
        (512, 512)
    } else {
        (SUBMAP_VIEW_W, SUBMAP_VIEW_H)
    }
}

pub(super) fn visible_map_crop(submap: u8) -> (u32, u32) {
    if submap == 0 {
        (0, 0)
    } else {
        (SUBMAP_VIEW_X as u32, SUBMAP_VIEW_Y as u32)
    }
}

pub(super) fn l1_vram_addr_for_map16(submap: u8, map16_x: u32, map16_y: u32) -> usize {
    let (crop_x, crop_y) = visible_map_crop(submap);
    let tile_x = (map16_x * 16 + crop_x) / 8;
    let tile_y = (map16_y * 16 + crop_y) / 8;
    tilemap_vram_addr(VRAM_L1_TILEMAP_BASE, tile_x, tile_y)
}

// ── OpenGL renderer ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub(super) struct OverworldRenderer {
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

    pub(super) fn set_tiles(&mut self, gl: &glow::Context, l1: Vec<Tile>, l2: Vec<Tile>) {
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
    pub(super) gl: Arc<glow::Context>,
    #[allow(dead_code)]
    rom: Arc<SmwRom>,
    pub(super) cpu: Cpu,
    pub(super) renderer: Arc<Mutex<OverworldRenderer>>,

    pub(super) submap: u8,

    offset: Vec2,
    zoom: f32,
    show_grid: bool,
    show_layer1: bool,
    show_layer2: bool,
    pub(super) selected_tile: Option<(u32, u32)>,
    needs_center: bool,

    // Editing state
    pub(super) editing_mode: crate::ui::editing_mode::EditingMode,
    pub(super) draw_tile_num: u8,
    pub(super) draw_palette: u8,
    pub(super) draw_tile_attr: u8,
    pub(super) tile_picker: crate::ui::ow_tile_picker::OwTilePicker,
    pub(super) l1_tile_picker: crate::ui::ow_tile_picker::OwL1TilePicker,
    pub(super) edit_layer: u8, // 1 or 2
    preview_texture: Option<egui::TextureHandle>,
    preview_for: Option<(u32, u32)>,
    pub(super) has_unsavable_changes: bool,
    pub(super) source_layer1_tiles: Vec<u8>,
    pub(super) source_layer2_words: Vec<u16>,
}

impl UiWorldEditor {
    pub fn new(gl: Arc<glow::Context>, rom: Arc<SmwRom>, rom_path: PathBuf) -> Self {
        let renderer = Arc::new(Mutex::new(OverworldRenderer::new(&gl)));

        let raw = std::fs::read(&rom_path).expect("cannot read ROM for emulator");
        let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
        let mut emu_rom = EmuRom::new(rom_bytes);
        emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
        let cpu = Cpu::new(CheckedMem::new(Arc::new(emu_rom)));

        let source_layer1_tiles = rom.overworld.layer1_tiles.clone();
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
            editing_mode: crate::ui::editing_mode::EditingMode::Select,
            draw_tile_num: 0x00,
            draw_palette: 0,
            draw_tile_attr: 0x00,
            tile_picker: crate::ui::ow_tile_picker::OwTilePicker::new(),
            l1_tile_picker: crate::ui::ow_tile_picker::OwL1TilePicker::new(),
            edit_layer: 1,
            preview_texture: None,
            preview_for: None,
            has_unsavable_changes: false,
            source_layer1_tiles,
            source_layer2_words: Vec::new(),
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

        // Rebuild the tile pickers from current VRAM
        self.tile_picker.rebuild(&self.cpu.mem.vram, &self.cpu.mem.cgram, VRAM_L1_TILEMAP_BASE, VRAM_L2_TILEMAP_BASE);
        self.l1_tile_picker.rebuild(&mut self.cpu);

        self.offset = Vec2::ZERO;
        self.selected_tile = None;
        self.needs_center = true;
        self.has_unsavable_changes = false;
        self.source_layer2_words = read_overworld_l2_words(&self.cpu);
    }
}

impl DockableEditorTool for UiWorldEditor {
    fn title(&self) -> WidgetText {
        "World Map Editor".into()
    }

    fn update(&mut self, ui: &mut Ui) {
        SidePanel::left("world_editor.left_panel").resizable(false).show_inside(ui, |ui| self.left_panel(ui));
        CentralPanel::default().frame(Frame::NONE.inner_margin(0.)).show_inside(ui, |ui| self.central_panel(ui));
    }

    fn on_closed(&mut self) {
        self.renderer.lock().expect("Cannot lock overworld renderer").destroy(&self.gl);
    }

    fn save_to_rom(&self, rom_bytes: &mut [u8], has_smc_header: bool) -> anyhow::Result<()> {
        if self.has_unsavable_changes {
            anyhow::bail!("Overworld edits currently only modify composed VRAM and cannot be serialized to ROM yet");
        }
        let header_offset = usize::from(has_smc_header) * 0x200;
        let start = AddrPc::try_from_lorom(OWL1_TILE_DATA_SNES)?.as_index() + header_offset;
        let end = start + OWL1_TILE_DATA_SIZE;
        let dst = rom_bytes
            .get_mut(start..end)
            .ok_or_else(|| anyhow::anyhow!("Overworld layer 1 ROM write range out of bounds"))?;
        dst.copy_from_slice(&self.source_layer1_tiles);

        let tile_stream: Vec<u8> = self.source_layer2_words.iter().map(|w| (*w & 0x00FF) as u8).collect();
        let attr_stream: Vec<u8> = self.source_layer2_words.iter().map(|w| (*w >> 8) as u8).collect();
        let tile_compressed = lc_rle2::compress_pass(&tile_stream);
        let attr_compressed = lc_rle2::compress_pass(&attr_stream);

        write_overworld_l2_stream(
            rom_bytes,
            has_smc_header,
            AddrPc::try_from_lorom(smwe_rom::snes_utils::addr::AddrSnes(0x04A533))?.as_index(),
            self.source_layer2_words.len(),
            &tile_compressed,
            "OWTileNumbers",
        )?;
        write_overworld_l2_stream(
            rom_bytes,
            has_smc_header,
            AddrPc::try_from_lorom(smwe_rom::snes_utils::addr::AddrSnes(0x04C02B))?.as_index(),
            self.source_layer2_words.len(),
            &attr_compressed,
            "OWTilemap",
        )?;
        Ok(())
    }
}

// ── UI ────────────────────────────────────────────────────────────────────────

impl UiWorldEditor {
    fn source_l1_offset(&self) -> usize {
        if self.submap == 0 { 0 } else { 0x400 }
    }

    fn source_l1_index_for_view(&self, map16_x: u32, map16_y: u32) -> Option<usize> {
        let (crop_x, crop_y) = visible_map_crop(self.submap);
        let src_col = ((map16_x * 16 + crop_x) / 16) as usize;
        let src_row = ((map16_y * 16 + crop_y) / 16) as usize;
        if src_col >= 32 || src_row >= 32 {
            return None;
        }
        Some(self.source_l1_offset() + ow_l1_addr(src_col as u32, src_row as u32))
    }

    fn source_l1_tile_at_view(&self, map16_x: u32, map16_y: u32) -> Option<u8> {
        let idx = self.source_l1_index_for_view(map16_x, map16_y)?;
        self.source_layer1_tiles.get(idx).copied()
    }

    pub(super) fn set_source_l1_tile_at_view(&mut self, map16_x: u32, map16_y: u32, tile_id: u8) {
        let Some(idx) = self.source_l1_index_for_view(map16_x, map16_y) else {
            return;
        };
        if let Some(slot) = self.source_layer1_tiles.get_mut(idx) {
            *slot = tile_id;
        }
        self.apply_source_l1_tile_to_vram(map16_x, map16_y, tile_id);
    }

    fn apply_source_l1_tile_to_vram(&mut self, map16_x: u32, map16_y: u32, tile_id: u8) {
        let (crop_x, crop_y) = visible_map_crop(self.submap);
        let src_col = (map16_x * 16 + crop_x) / 16;
        let src_row = (map16_y * 16 + crop_y) / 16;
        self.write_source_l1_block_words(src_col, src_row, tile_id);
    }

    fn write_source_l1_block_words(&mut self, src_col: u32, src_row: u32, tile_id: u8) {
        let sub_tiles = source_l1_subtiles(&mut self.cpu, tile_id);
        let base_tile_x = src_col * 2;
        let base_tile_y = src_row * 2;
        let offsets = [(0u32, 0u32), (1u32, 0u32), (0u32, 1u32), (1u32, 1u32)];
        for (word, (dx, dy)) in sub_tiles.into_iter().zip(offsets) {
            let addr = tilemap_vram_addr(VRAM_L1_TILEMAP_BASE, base_tile_x + dx, base_tile_y + dy);
            if addr + 1 < self.cpu.mem.vram.len() {
                let [lo, hi] = word.to_le_bytes();
                self.cpu.mem.vram[addr] = lo;
                self.cpu.mem.vram[addr + 1] = hi;
            }
        }
    }

    fn left_panel(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Overworld");
            ui.add_space(4.0);

            // Submap selector
            ui.horizontal(|ui| {
                ui.label("Submap");
                let prev = self.submap;
                egui::ComboBox::from_id_salt("world_editor.submap")
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

            // ── Editing mode toolbar ────────────────────────────────
            ui.separator();
            ui.label("Mode:");
            ui.horizontal(|ui| {
                let modes = [
                    ("Select [1]", crate::ui::editing_mode::EditingMode::Select),
                    ("Draw [2]", crate::ui::editing_mode::EditingMode::Draw),
                    ("Erase [3]", crate::ui::editing_mode::EditingMode::Erase),
                ];
                for (label, mode) in modes {
                    let active = self.editing_mode == mode;
                    let fill = if active { Some(Color32::from_rgb(70, 130, 200)) } else { None };
                    let btn = egui::Button::new(label);
                    let btn = if let Some(f) = fill { btn.fill(f) } else { btn };
                    if ui.add(btn).clicked() {
                        self.editing_mode = mode;
                    }
                }
            });

            // ── Layer selector ────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("Layer:");
                let layers = [("L1", 1u8), ("L2", 2u8)];
                for (label, layer) in layers {
                    let active = self.edit_layer == layer;
                    let fill = if active { Some(Color32::from_rgb(70, 130, 200)) } else { None };
                    let btn = egui::Button::new(label);
                    let btn = if let Some(f) = fill { btn.fill(f) } else { btn };
                    if ui.add(btn).clicked() {
                        self.edit_layer = layer;
                        self.preview_texture = None; // Force preview refresh
                    }
                }
            });

            // ── Draw mode tile picker ───────────────────────────────
            if self.editing_mode == crate::ui::editing_mode::EditingMode::Draw {
                ui.separator();
                ui.label("Paint tile:");
                ui.horizontal(|ui| {
                    let label = if self.edit_layer == 1 { "Tile ID" } else { "Tile" };
                    ui.label(format!("{label}: {:#04X}", self.draw_tile_num));
                    let mut t = self.draw_tile_num as u16;
                    if ui
                        .add(egui::Slider::new(&mut t, 0..=0xFF).show_value(false).hexadecimal(2, false, false))
                        .changed()
                    {
                        self.draw_tile_num = t as u8;
                    }
                });
                if self.edit_layer == 2 {
                    ui.horizontal(|ui| {
                        ui.label("Palette:");
                        let mut p = self.draw_palette as u16;
                        if ui.add(egui::Slider::new(&mut p, 0..=7)).changed() {
                            self.draw_palette = p as u8;
                        }
                    });

                    // VRAM tile picker grid
                    let tex = self.tile_picker.texture(ui.ctx());
                    let tex_size = tex.size();
                    let max_w = ui.available_width().min(300.0);
                    let display_w = max_w;
                    let display_h = display_w;
                    let (rect, resp) = ui.allocate_exact_size(vec2(display_w, display_h), egui::Sense::click());
                    ui.painter().image(
                        tex.id(),
                        rect,
                        Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(1.0, 1.0)),
                        Color32::WHITE,
                    );

                    if resp.clicked_by(egui::PointerButton::Primary) {
                        if let Some(pos) = resp.interact_pointer_pos() {
                            let rel = pos - rect.min;
                            let px = rel.x / display_w * tex_size[0] as f32;
                            let py = rel.y / display_h * tex_size[1] as f32;
                            if let Some((tile_num, pal)) = self.tile_picker.tile_at_pixel(px, py) {
                                self.draw_tile_num = tile_num;
                                self.draw_palette = pal;
                            }
                        }
                    }

                    if let Some((col, row)) = self.tile_picker.tile_grid_pos(self.draw_tile_num, self.draw_palette) {
                        let tile_screen = display_w / (tex_size[0] as f32 / 16.0);
                        let sel_rect = Rect::from_min_size(
                            rect.min + vec2(col as f32 * tile_screen, row as f32 * tile_screen),
                            vec2(tile_screen, tile_screen),
                        );
                        ui.painter()
                            .rect_stroke(
                                sel_rect,
                                egui::CornerRadius::ZERO,
                                egui::Stroke::new(2.0, Color32::YELLOW),
                                egui::StrokeKind::Outside,
                            );
                    }
                } else {
                    // Visual L1 tile picker grid
                    let tex = self.l1_tile_picker.texture(ui.ctx());
                    let tex_size = tex.size();
                    let max_w = ui.available_width().min(300.0);
                    let (rect, resp) = ui.allocate_exact_size(vec2(max_w, max_w), egui::Sense::click());
                    ui.painter().image(
                        tex.id(),
                        rect,
                        Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    if resp.clicked_by(egui::PointerButton::Primary) {
                        if let Some(pos) = resp.interact_pointer_pos() {
                            let rel = pos - rect.min;
                            let px = rel.x / max_w * tex_size[0] as f32;
                            let py = rel.y / max_w * tex_size[1] as f32;
                            if let Some(tile_id) = self.l1_tile_picker.block_at_pixel(px, py) {
                                self.draw_tile_num = tile_id;
                                self.preview_texture = None; // Invalidate preview cache
                            }
                        }
                    }
                    // Selection highlight
                    let (col, row) = self.l1_tile_picker.block_grid_pos(self.draw_tile_num);
                    let tile_screen = max_w / crate::ui::ow_tile_picker::L1_COLS as f32;
                    let sel_rect = Rect::from_min_size(
                        rect.min + vec2(col as f32 * tile_screen, row as f32 * tile_screen),
                        vec2(tile_screen, tile_screen),
                    );
                    ui.painter()
                        .rect_stroke(
                            sel_rect,
                            egui::CornerRadius::ZERO,
                            egui::Stroke::new(2.0, Color32::YELLOW),
                            egui::StrokeKind::Outside,
                        );
                }
            }

            ui.separator();

            // ── Tile preview ────────────────────────────────────
            // In draw mode, show the draw tile. Otherwise, show the selected tile.
            let draw_mode = self.editing_mode == crate::ui::editing_mode::EditingMode::Draw;
            if draw_mode {
                if self.edit_layer == 1 {
                    ui.label(format!("Paint tile ID: {:#04X}", self.draw_tile_num));
                } else {
                    ui.label(format!("Paint: {:#04X} pal {}", self.draw_tile_num, self.draw_palette));
                }
                let cache_key = (self.draw_tile_num as u32 | 0x100, self.draw_palette as u32);
                if self.preview_for != Some(cache_key) {
                    let image = if self.edit_layer == 1 {
                        render_source_l1_tile_preview(&mut self.cpu, self.draw_tile_num)
                    } else {
                        render_single_tile_preview(
                            &self.cpu.mem.vram,
                            &self.cpu.mem.cgram,
                            self.draw_tile_num,
                            self.draw_palette,
                        )
                    };
                    let handle = ui.ctx().load_texture(
                        format!("ow_draw_preview_{}", self.draw_tile_num),
                        image,
                        egui::TextureOptions::NEAREST,
                    );
                    self.preview_texture = Some(handle);
                    self.preview_for = Some(cache_key);
                }
                if let Some(ref tex) = self.preview_texture {
                    let display_size = 64.0;
                    let (rect, _) = ui.allocate_exact_size(vec2(display_size, display_size), egui::Sense::hover());
                    ui.painter().image(
                        tex.id(),
                        rect,
                        Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            } else if let Some((x, y)) = self.selected_tile {
                ui.label(format!("Selected: ({x}, {y}) [L{}]", self.edit_layer));
                let tilemap_base = if self.edit_layer == 2 { VRAM_L2_TILEMAP_BASE } else { VRAM_L1_TILEMAP_BASE };
                if self.edit_layer == 1 {
                    if let Some(tile_id) = self.source_l1_tile_at_view(x, y) {
                        ui.monospace(format!("  Source tile ID: {tile_id:#04X}"));
                    }
                } else {
                    let (crop_x, crop_y) = visible_map_crop(self.submap);
                    let tile_x = (x * 16 + crop_x) / 8;
                    let tile_y = (y * 16 + crop_y) / 8;
                    let addr = tilemap_vram_addr(tilemap_base, tile_x, tile_y);
                    let sub0 = u16::from_le_bytes([self.cpu.mem.vram[addr], self.cpu.mem.vram[addr + 1]]);
                    let tile_num = (sub0 & 0x3FF) as u32;
                    let pal = ((sub0 >> 10) & 0x7) as u32;
                    let flip_x = (sub0 & 0x4000) != 0;
                    let flip_y = (sub0 & 0x8000) != 0;
                    ui.monospace(format!("  TL vram #{tile_num:03X}  pal {pal}"));
                    if flip_x || flip_y {
                        ui.monospace(format!("  flip x={flip_x} y={flip_y}"));
                    }
                }

                let cache_key = ((x & 0xFFFF) | ((y & 0xFFFF) << 16), 0u32);
                if self.preview_for != Some(cache_key) {
                    let image = if self.edit_layer == 1 {
                        let tile_id = self.source_l1_tile_at_view(x, y).unwrap_or(0);
                        render_source_l1_tile_preview(&mut self.cpu, tile_id)
                    } else {
                        render_ow_block_preview(
                            &self.cpu.mem.vram,
                            &self.cpu.mem.cgram,
                            self.submap,
                            x,
                            y,
                            tilemap_base,
                        )
                    };
                    let handle =
                        ui.ctx().load_texture(format!("ow_preview_{x}_{y}"), image, egui::TextureOptions::NEAREST);
                    self.preview_texture = Some(handle);
                    self.preview_for = Some(cache_key);
                }
                if let Some(ref tex) = self.preview_texture {
                    let display_size = 64.0;
                    let (rect, _) = ui.allocate_exact_size(vec2(display_size, display_size), egui::Sense::hover());
                    ui.painter().image(
                        tex.id(),
                        rect,
                        Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            } else {
                ui.label("Selected: (none)");
            }
        });
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
        painter.rect_filled(view_rect, CornerRadius::ZERO, Color32::from_rgb(16, 16, 20));

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
                    r.paint(painter.gl().as_ref(), screen_sz, gl_zoom, gl_offset, draw_l1, draw_l2);
                })),
            });
        }

        // ── Border around canvas ──────────────────────────────────────────────
        painter.rect_stroke(ow_rect, CornerRadius::ZERO, Stroke::new(2.0, Color32::from_white_alpha(140)), StrokeKind::Outside);

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
                painter.rect_stroke(tile_rect, CornerRadius::ZERO, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);

                if resp.clicked_by(egui::PointerButton::Primary)
                    && (self.editing_mode == crate::ui::editing_mode::EditingMode::Select
                        || ui.input(|i| i.modifiers.alt))
                {
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

        // ── Editing interaction ─────────────────────────────────────
        self.handle_editing_interaction(&resp, origin, map16_sz);

        // ── Keyboard shortcuts ──────────────────────────────────────
        ui.input_mut(|input| {
            if input.key_pressed(egui::Key::Num1) {
                self.editing_mode = crate::ui::editing_mode::EditingMode::Select;
            }
            if input.key_pressed(egui::Key::Num2) {
                self.editing_mode = crate::ui::editing_mode::EditingMode::Draw;
            }
            if input.key_pressed(egui::Key::Num3) {
                self.editing_mode = crate::ui::editing_mode::EditingMode::Erase;
            }
        });

        // ── Selected tile highlight ───────────────────────────────────────────
        if let Some((x, y)) = self.selected_tile {
            let r = Rect::from_min_size(origin + vec2(x as f32 * map16_sz, y as f32 * map16_sz), Vec2::splat(map16_sz));
            painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(2.0, Color32::from_rgb(255, 220, 0)), StrokeKind::Outside);
        }
    }
}

// ── Tile list builders ────────────────────────────────────────────────────────

/// Build draw list from the composed BG tilemap already uploaded to VRAM.
pub(super) fn build_bg_tiles(vram: &[u8], tilemap_base: usize, submap: u8, scroll_x: i32, scroll_y: i32) -> Vec<Tile> {
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

fn read_overworld_l2_words(cpu: &Cpu) -> Vec<u16> {
    let base = (0x7F4000 - 0x7E0000) as usize;
    let bytes = &cpu.mem.wram[base..base + 0x4000];
    bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

fn write_overworld_l2_stream(
    rom_bytes: &mut [u8], has_smc_header: bool, start_pc_no_header: usize, output_len: usize, compressed: &[u8],
    label: &str,
) -> anyhow::Result<()> {
    let header_offset = usize::from(has_smc_header) * 0x200;
    let start = start_pc_no_header + header_offset;
    let old_size = lc_rle2::compressed_size_for_output(
        rom_bytes
            .get(start..)
            .ok_or_else(|| anyhow::anyhow!("{label} ROM source start out of bounds"))?,
        output_len,
    );
    if compressed.len() > old_size {
        anyhow::bail!("{label} compressed data grew from {} to {} bytes; repointing is not implemented yet", old_size, compressed.len());
    }
    let dst = rom_bytes
        .get_mut(start..start + old_size)
        .ok_or_else(|| anyhow::anyhow!("{label} ROM write range out of bounds"))?;
    dst[..compressed.len()].copy_from_slice(compressed);
    dst[compressed.len()..].fill(0);
    Ok(())
}

fn ow_l1_addr(col: u32, row: u32) -> usize {
    let x_part = (col & 0x0F) | ((col & 0x10) << 4);
    let y_part = ((row & 0x0F) << 4) | ((row & 0x10) << 5);
    (x_part + y_part) as usize
}

fn source_l1_subtiles(cpu: &mut Cpu, tile_id: u8) -> [u16; 4] {
    let ptr_base = 0x7E0FBEu32;
    let char_bank = 0x05_0000u32;
    let char_ptr = cpu.mem.load_u16(ptr_base + tile_id as u32 * 2) as u32;
    let gfx_addr = char_bank | char_ptr;
    [
        cpu.mem.load_u16(gfx_addr),
        cpu.mem.load_u16(gfx_addr + 2),
        cpu.mem.load_u16(gfx_addr + 4),
        cpu.mem.load_u16(gfx_addr + 6),
    ]
}

fn render_source_l1_tile_preview(cpu: &mut Cpu, tile_id: u8) -> egui::ColorImage {
    let sub_tiles = source_l1_subtiles(cpu, tile_id);
    let mut pixels = vec![0u8; 16 * 16 * 4];
    let offsets = [(0u32, 0u32), (8u32, 0u32), (0u32, 8u32), (8u32, 8u32)];
    for (sub_tile, (x0, y0)) in sub_tiles.into_iter().zip(offsets) {
        let tile_num = (sub_tile & 0x03FF) as usize;
        let pal = ((sub_tile >> 10) & 0x7) as usize;
        let flip_x = (sub_tile & 0x4000) != 0;
        let flip_y = (sub_tile & 0x8000) != 0;
        render_preview_tile(
            &cpu.mem.vram,
            &cpu.mem.cgram,
            tile_num,
            pal,
            flip_x,
            flip_y,
            x0,
            y0,
            16,
            &mut pixels,
        );
    }
    egui::ColorImage::from_rgba_unmultiplied([16, 16], &pixels)
}

#[allow(clippy::too_many_arguments)]
fn render_preview_tile(
    vram: &[u8], cgram: &[u8], tile_num: usize, pal: usize, flip_x: bool, flip_y: bool, x0: u32, y0: u32, width: usize,
    pixels: &mut [u8],
) {
    let tile_base = tile_num * 32;
    for ty_px in 0..8u32 {
        for tx_px in 0..8u32 {
            let px = if flip_x { 7 - tx_px } else { tx_px };
            let py = if flip_y { 7 - ty_px } else { ty_px };
            let row_off = tile_base + (py as usize) * 2;
            if row_off + 17 >= vram.len() {
                continue;
            }
            let b0 = vram[row_off];
            let b1 = vram[row_off + 1];
            let b2 = vram[row_off + 16];
            let b3 = vram[row_off + 17];
            let bit = 7 - px as usize;
            let color_idx = (((b0 >> bit) & 1)
                | (((b1 >> bit) & 1) << 1)
                | (((b2 >> bit) & 1) << 2)
                | (((b3 >> bit) & 1) << 3)) as usize;
            if color_idx == 0 {
                continue;
            }
            let pal_idx = pal * 16 + color_idx;
            let off_color = pal_idx * 2;
            if off_color + 1 >= cgram.len() {
                continue;
            }
            let lo = cgram[off_color] as u16;
            let hi = cgram[off_color + 1] as u16;
            let rgb = lo | (hi << 8);
            let r = ((rgb & 0x1F) << 3) as u8;
            let g = (((rgb >> 5) & 0x1F) << 3) as u8;
            let b = (((rgb >> 10) & 0x1F) << 3) as u8;
            let px_abs = x0 + tx_px;
            let py_abs = y0 + ty_px;
            let off = ((py_abs as usize) * width + px_abs as usize) * 4;
            if off + 3 < pixels.len() {
                pixels[off] = r;
                pixels[off + 1] = g;
                pixels[off + 2] = b;
                pixels[off + 3] = 255;
            }
        }
    }
}

/// Render a 16×16 preview of the Map16 block at (map16_x, map16_y) by reading
/// its 4 sub-tiles from the given VRAM tilemap.
fn render_ow_block_preview(
    vram: &[u8], cgram: &[u8], submap: u8, map16_x: u32, map16_y: u32, tilemap_base: usize,
) -> egui::ColorImage {
    let (crop_x, crop_y) = visible_map_crop(submap);
    let base_tile_x = (map16_x * 16 + crop_x) / 8;
    let base_tile_y = (map16_y * 16 + crop_y) / 8;
    let mut pixels = vec![0u8; 16 * 16 * 4];

    let sub_positions = [(0u32, 0u32), (1, 0), (0, 1), (1, 1)];
    for (dx, dy) in sub_positions {
        let tx = base_tile_x + dx;
        let ty = base_tile_y + dy;
        let addr = tilemap_vram_addr(tilemap_base, tx, ty);
        if addr + 1 >= vram.len() {
            continue;
        }
        let t0 = vram[addr] as u16;
        let t1 = vram[addr + 1] as u16;
        let tile_num = (t0 | ((t1 & 3) << 8)) as usize;
        let pal = ((t1 >> 2) & 7) as usize;
        let flip_x = (t1 & 0x40) != 0;
        let flip_y = (t1 & 0x80) != 0;

        let tile_base = tile_num * 32;
        let x0 = dx * 8;
        let y0 = dy * 8;
        for ty_px in 0..8u32 {
            for tx_px in 0..8u32 {
                let px = if flip_x { 7 - tx_px } else { tx_px };
                let py = if flip_y { 7 - ty_px } else { ty_px };
                let row_off = tile_base + (py as usize) * 2;
                if row_off + 17 >= vram.len() {
                    continue;
                }
                let b0 = vram[row_off];
                let b1 = vram[row_off + 1];
                let b2 = vram[row_off + 16];
                let b3 = vram[row_off + 17];
                let bit = 7 - px as usize;
                let color_idx = (((b0 >> bit) & 1)
                    | (((b1 >> bit) & 1) << 1)
                    | (((b2 >> bit) & 1) << 2)
                    | (((b3 >> bit) & 1) << 3)) as usize;
                if color_idx == 0 {
                    continue;
                }
                let pal_idx = pal * 16 + color_idx;
                let off_color = pal_idx * 2;
                if off_color + 1 >= cgram.len() {
                    continue;
                }
                let lo = cgram[off_color] as u16;
                let hi = cgram[off_color + 1] as u16;
                let rgb = lo | (hi << 8);
                let r = ((rgb & 0x1F) << 3) as u8;
                let g = (((rgb >> 5) & 0x1F) << 3) as u8;
                let b = (((rgb >> 10) & 0x1F) << 3) as u8;

                let px_abs = x0 + tx_px;
                let py_abs = y0 + ty_px;
                let off = ((py_abs as usize) * 16 + px_abs as usize) * 4;
                if off + 3 < pixels.len() {
                    pixels[off] = r;
                    pixels[off + 1] = g;
                    pixels[off + 2] = b;
                    pixels[off + 3] = 255;
                }
            }
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([16, 16], &pixels)
}

/// Render a single 8×8 VRAM tile as a 16×16 preview (2× nearest neighbor).
fn render_single_tile_preview(vram: &[u8], cgram: &[u8], tile_num: u8, pal: u8) -> egui::ColorImage {
    let mut pixels = vec![0u8; 16 * 16 * 4];
    let tile_base = (tile_num as usize) * 32;
    for ty in 0..8u32 {
        for tx in 0..8u32 {
            let row_off = tile_base + (ty as usize) * 2;
            if row_off + 17 >= vram.len() {
                continue;
            }
            let b0 = vram[row_off];
            let b1 = vram[row_off + 1];
            let b2 = vram[row_off + 16];
            let b3 = vram[row_off + 17];
            let bit = 7 - tx as usize;
            let color_idx =
                (((b0 >> bit) & 1) | (((b1 >> bit) & 1) << 1) | (((b2 >> bit) & 1) << 2) | (((b3 >> bit) & 1) << 3))
                    as usize;
            if color_idx == 0 {
                continue;
            }
            let pal_idx = (pal as usize) * 16 + color_idx;
            let off_color = pal_idx * 2;
            if off_color + 1 >= cgram.len() {
                continue;
            }
            let lo = cgram[off_color] as u16;
            let hi = cgram[off_color + 1] as u16;
            let rgb = lo | (hi << 8);
            let r = ((rgb & 0x1F) << 3) as u8;
            let g = (((rgb >> 5) & 0x1F) << 3) as u8;
            let b = (((rgb >> 10) & 0x1F) << 3) as u8;
            // 2× nearest neighbor
            for dy in 0..2u32 {
                for dx in 0..2u32 {
                    let px = tx * 2 + dx;
                    let py = ty * 2 + dy;
                    let off = ((py as usize) * 16 + px as usize) * 4;
                    if off + 3 < pixels.len() {
                        pixels[off] = r;
                        pixels[off + 1] = g;
                        pixels[off + 2] = b;
                        pixels[off + 3] = 255;
                    }
                }
            }
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([16, 16], &pixels)
}
