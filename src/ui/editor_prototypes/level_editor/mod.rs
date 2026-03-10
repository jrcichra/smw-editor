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
            // offset is in canvas-pixel units at zoom=1; (0,0) puts the level top-left
            // at the viewport top-left.  load_level() resets this on each level load.
            offset: Vec2::ZERO,
            zoom: 1.0,
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
        self.offset = Vec2::ZERO; // reset pan so level top-left is visible
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
        // Slot N → tiles 0x80*N .. 0x80*N+0x7F, using gfx_file[object_gfx_list[tileset*4 + N]]
        let tileset = (level.primary_header.fg_bg_gfx() as usize)
            % smwe_rom::objects::tilesets::TILESETS_COUNT;

        // VRAM buffer layout (matches tile.fs.glsl):
        //   layout(std140) uniform Graphics { uvec4 graphics[0x1000]; };
        //   Tile T occupies graphics[T*2] and graphics[T*2+1]  (32 bytes).
        //
        //   Shader unpacking per row Y (0-7):
        //     lpart1 = graphics[T*2  ][Y/2]   (bitplanes 0+1 for rows 0-7)
        //     lpart2 = graphics[T*2+1][Y/2]   (bitplanes 2+3 for rows 0-7)
        //     line   = lpart >> ((Y%2) * 16)  → low 16 bits = p_low | (p_high<<8)
        //     bit[X] = (line >> (7-X)) & 1    → p_low  for lpart1 → color bit 0
        //     bit[X] = (line >> (15-X)) & 1   → p_high for lpart1 → color bit 1
        //   i.e. graphics[T*2][row/2] = row_p0|(row_p1<<8) | (next_row_p0|(next_row_p1<<8)) << 16
        //
        //   We upload 4 GFX slots × 0x80 tiles = 0x200 tiles.
        //   Buffer = 0x200 × 32 bytes = 0x4000 bytes.
        let mut vram = vec![0u8; 0x4000];
        for slot in 0..4_usize {
            let dummy_tile = smwe_rom::objects::map16::Tile8x8((slot as u16) << 7);
            let file_num = self.rom.gfx.object_gfx_list
                .gfx_file_for_object_tile(dummy_tile, tileset);
            let gfx_file = match self.rom.gfx.files.get(file_num) {
                Some(f) => f,
                None => continue,
            };
            for (tile_idx, tile) in gfx_file.tiles.iter().enumerate().take(0x80) {
                let vram_tile_id = slot * 0x80 + tile_idx;
                let base = vram_tile_id * 32;
                if base + 32 > vram.len() { break; }

                for row in 0..8_usize {
                    let (mut p0, mut p1, mut p2, mut p3) = (0u8, 0u8, 0u8, 0u8);
                    for col in 0..8_usize {
                        let ci = tile.color_indices[row * 8 + col];
                        let bit = 7 - col;
                        p0 |= ((ci     ) & 1) << bit;
                        p1 |= ((ci >> 1) & 1) << bit;
                        p2 |= ((ci >> 2) & 1) << bit;
                        p3 |= ((ci >> 3) & 1) << bit;
                    }
                    // Each u32 word packs two rows: low16 = even row, high16 = odd row
                    // graphics[T*2  ][row/2] = row_p01 → bitplanes 0+1
                    // graphics[T*2+1][row/2] = row_p23 → bitplanes 2+3
                    let word_idx = row / 2;
                    let shift    = (row % 2) * 16;
                    let val01 = (p0 as u32) | ((p1 as u32) << 8);
                    let val23 = (p2 as u32) | ((p3 as u32) << 8);
                    let w_off_01 = base      + word_idx * 4;
                    let w_off_23 = base + 16 + word_idx * 4;
                    let cur01 = u32::from_le_bytes(vram[w_off_01..w_off_01+4].try_into().unwrap());
                    let cur23 = u32::from_le_bytes(vram[w_off_23..w_off_23+4].try_into().unwrap());
                    let new01 = (cur01 & !(0xFFFFu32 << shift)) | (val01 << shift);
                    let new23 = (cur23 & !(0xFFFFu32 << shift)) | (val23 << shift);
                    vram[w_off_01..w_off_01+4].copy_from_slice(&new01.to_le_bytes());
                    vram[w_off_23..w_off_23+4].copy_from_slice(&new23.to_le_bytes());
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
