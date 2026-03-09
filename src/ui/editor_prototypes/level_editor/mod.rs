mod central_panel;
mod left_panel;
mod level_renderer;
mod object_layer;
mod properties;

use std::sync::{Arc, Mutex};

use egui::{CentralPanel, Frame, SidePanel, Ui, WidgetText, *};
use smwe_rom::{graphics::palette::ColorPalette, SmwRom};

use self::{level_renderer::LevelRenderer, object_layer::EditableObjectLayer, properties::LevelProperties};
use crate::ui::tool::DockableEditorTool;

pub struct UiLevelEditor {
    gl:             Arc<glow::Context>,
    rom:            Arc<SmwRom>,
    level_renderer: Arc<Mutex<LevelRenderer>>,

    level_num:        u16,
    offset:           Vec2,
    zoom:             f32,
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
            zoom: 2.0,
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
        SidePanel::left("level_editor.left_panel").resizable(false).show_inside(ui, |ui| self.left_panel(ui));
        CentralPanel::default()
            .frame(Frame::none().inner_margin(0.))
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

        // ── CGRAM: build a 256-color palette buffer (512 bytes, ABGR1555 LE)
        let palette = match self.rom.gfx.color_palettes.get_level_palette(&level.primary_header) {
            Ok(p) => p,
            Err(e) => { log::warn!("Palette error: {e}"); return; }
        };
        let mut cgram = vec![0u8; 512];
        for row in 0..16_usize {
            for col in 0..16_usize {
                let color = palette
                    .get_color_at(row, col)
                    .unwrap_or(smwe_rom::graphics::palette::ColorPalettes::TRANSPARENT);
                let idx = (row * 16 + col) * 2;
                let le = color.0.to_le_bytes();
                cgram[idx]     = le[0];
                cgram[idx + 1] = le[1];
            }
        }

        // ── VRAM: pack 4 GFX files selected by the level's tileset into 0x200 tile slots
        // SNES VRAM layout for layer1 objects:
        //   Slot 0: tile  0x000..0x07F → gfx_file[object_gfx_list[tileset*4 + 0]]
        //   Slot 1: tile  0x080..0x0FF → gfx_file[object_gfx_list[tileset*4 + 1]]
        //   Slot 2: tile  0x100..0x17F → gfx_file[object_gfx_list[tileset*4 + 2]]
        //   Slot 3: tile  0x180..0x1FF → gfx_file[object_gfx_list[tileset*4 + 3]]
        // Each 8x8 4bpp tile = 32 bytes planar.
        // Total: 0x200 tiles × 32 bytes = 0x4000 bytes, packed into the 0x10000-byte VRAM buffer.
        let tileset = (level.primary_header.fg_bg_gfx() as usize)
            % smwe_rom::objects::tilesets::TILESETS_COUNT;
        let mut vram = vec![0u8; 0x10000];
        for slot in 0..4_usize {
            // Look up which GFX file goes into this slot for the current tileset
            // object_gfx_list encodes 4 file numbers per tileset: [tileset*4 + slot]
            // We reconstruct the lookup by creating a dummy Tile8x8 with the right layer.
            // Layer = slot, tile_within_file = 0 ⇒ tile_number = slot * 0x80
            let dummy_tile = smwe_rom::objects::map16::Tile8x8((slot as u16) << 7);
            let file_num = self.rom.gfx.object_gfx_list
                .gfx_file_for_object_tile(dummy_tile, tileset);
            let gfx_file = match self.rom.gfx.files.get(file_num) {
                Some(f) => f,
                None => continue,
            };
            let base = slot * 0x80 * 32; // byte offset in vram buffer
            for (tile_idx, tile) in gfx_file.tiles.iter().enumerate().take(0x80) {
                let tile_base = base + tile_idx * 32;
                if tile_base + 32 > vram.len() { break; }
                // Convert color_indices to 4bpp SNES planar: bitplanes 0/1 interleaved then 2/3
                for row in 0..8_usize {
                    let (mut p0, mut p1, mut p2, mut p3) = (0u8, 0u8, 0u8, 0u8);
                    for col in 0..8_usize {
                        let ci = tile.color_indices[row * 8 + col];
                        let bit = 7 - col;
                        p0 |= ((ci >> 0) & 1) << bit;
                        p1 |= ((ci >> 1) & 1) << bit;
                        p2 |= ((ci >> 2) & 1) << bit;
                        p3 |= ((ci >> 3) & 1) << bit;
                    }
                    vram[tile_base + row * 2 +  0] = p0;
                    vram[tile_base + row * 2 +  1] = p1;
                    vram[tile_base + row * 2 + 16] = p2;
                    vram[tile_base + row * 2 + 17] = p3;
                }
            }
        }

        let renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
        renderer.upload_palette(&self.gl, &cgram);
        renderer.upload_gfx(&self.gl, &vram);
    }

    fn upload_level_tiles(&mut self) {
        self.level_renderer.lock().expect("Cannot lock level_renderer").upload_level_from_rom(
            &self.gl,
            &self.rom,
            self.level_num,
        );
    }
}
