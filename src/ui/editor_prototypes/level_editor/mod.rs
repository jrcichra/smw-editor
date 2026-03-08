mod central_panel;
mod left_panel;
mod level_renderer;
mod object_layer;
mod properties;

use std::sync::{Arc, Mutex};

use egui::{CentralPanel, SidePanel, Ui, WidgetText, *};
use smwe_rom::{
    graphics::palette::ColorPalette,
    SmwRom,
};

use self::{level_renderer::LevelRenderer, object_layer::EditableObjectLayer, properties::LevelProperties};
use crate::ui::tool::DockableEditorTool;

pub struct UiLevelEditor {
    gl:             Arc<glow::Context>,
    rom:            Arc<SmwRom>,
    level_renderer: Arc<Mutex<LevelRenderer>>,

    level_num:        u16,
    offset:           Vec2,
    zoom:             f32,
    tile_size_px:     f32,
    pixels_per_point: f32,
    always_show_grid: bool,

    level_properties: LevelProperties,
    layer1:           EditableObjectLayer,
}

impl UiLevelEditor {
    pub fn new(gl: Arc<glow::Context>, rom: Arc<SmwRom>) -> Self {
        let level_renderer = Arc::new(Mutex::new(LevelRenderer::new(&gl)));
        let mut editor = Self {
            gl,
            rom,
            level_renderer,
            level_num: 0x105,
            offset: Vec2::ZERO,
            zoom: 2.,
            tile_size_px: 16.,
            pixels_per_point: 1.,
            always_show_grid: false,
            level_properties: LevelProperties::default(),
            layer1: EditableObjectLayer::default(),
        };
        editor.load_level();
        editor
    }
}

// UI
impl DockableEditorTool for UiLevelEditor {
    fn update(&mut self, ui: &mut Ui) {
        self.pixels_per_point = ui.ctx().pixels_per_point();
        SidePanel::left("level_editor.left_panel").resizable(false).show_inside(ui, |ui| self.left_panel(ui));
        CentralPanel::default()
            .frame(Frame::none().inner_margin(0.).fill(Color32::GRAY))
            .show_inside(ui, |ui| self.central_panel(ui));
    }

    fn title(&self) -> WidgetText {
        "Level Editor".into()
    }

    fn on_closed(&mut self) {
        self.level_renderer.lock().unwrap().destroy(&self.gl);
    }
}

// Internals
impl UiLevelEditor {
    pub(super) fn load_level(&mut self) {
        let level_idx = self.level_num as usize;
        if level_idx >= self.rom.levels.len() {
            log::warn!("Level {:#X} out of range", self.level_num);
            return;
        }
        let level = &self.rom.levels[level_idx];
        self.level_properties = LevelProperties::from_level(level);
        self.layer1 = EditableObjectLayer::from_level(level);
        self.upload_gfx_palette();
        self.upload_level_tiles();
    }

    fn upload_gfx_palette(&self) {
        let level_idx = self.level_num as usize;
        if level_idx >= self.rom.levels.len() {
            return;
        }
        let level = &self.rom.levels[level_idx];
        let palette = match self.rom.gfx.color_palettes.get_level_palette(&level.primary_header) {
            Ok(p) => p,
            Err(e) => { log::warn!("Palette error: {e}"); return; }
        };

        // Build a 256-color CGRAM buffer (16 rows × 16 cols × 2 bytes/color = 512 bytes)
        let mut cgram = vec![0u8; 512];
        for row in 0..=0xF_usize {
            for col in 0..=0xF_usize {
                let color = palette.get_color_at(row, col)
                    .unwrap_or(smwe_rom::graphics::palette::ColorPalettes::TRANSPARENT);
                let idx = (row * 16 + col) * 2;
                let le = color.0.to_le_bytes();
                cgram[idx] = le[0];
                cgram[idx + 1] = le[1];
            }
        }
        let renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
        renderer.upload_palette(&self.gl, &cgram);

        // Build VRAM from GFX files — 4bpp, 0x2000 bytes (128 tiles × 32 bytes each)
        // Use the first 128 tiles from the relevant GFX files for this level's tileset.
        let mut vram = vec![0u8; 0x10000];
        let tileset = level.primary_header.fg_bg_gfx() as usize % smwe_rom::objects::tilesets::TILESETS_COUNT;
        for (file_slot, gfx_file) in self.rom.gfx.files.iter().enumerate().take(4) {
            let base = file_slot * 0x80 * 32; // 0x80 tiles per slot, 32 bytes per 4bpp tile
            for (tile_idx, tile) in gfx_file.tiles.iter().enumerate().take(0x80) {
                let tile_base = base + tile_idx * 32;
                if tile_base + 32 > vram.len() { break; }
                // Convert color_indices back to 4bpp planar format
                for row in 0..8_usize {
                    let mut p0 = 0u8; let mut p1 = 0u8;
                    let mut p2 = 0u8; let mut p3 = 0u8;
                    for col in 0..8_usize {
                        let ci = tile.color_indices[row * 8 + col];
                        let bit = 7 - col;
                        p0 |= ((ci >> 0) & 1) << bit;
                        p1 |= ((ci >> 1) & 1) << bit;
                        p2 |= ((ci >> 2) & 1) << bit;
                        p3 |= ((ci >> 3) & 1) << bit;
                    }
                    vram[tile_base + row * 2 + 0]  = p0;
                    vram[tile_base + row * 2 + 1]  = p1;
                    vram[tile_base + row * 2 + 16] = p2;
                    vram[tile_base + row * 2 + 17] = p3;
                }
            }
        }
        let _ = tileset; // will be used for tileset-specific GFX selection in future
        renderer.upload_gfx(&self.gl, &vram);
    }

    fn upload_level_tiles(&mut self) {
        self.level_renderer.lock().expect("Cannot lock level_renderer")
            .upload_level_from_rom(&self.gl, &self.rom, self.level_num);
    }
}
