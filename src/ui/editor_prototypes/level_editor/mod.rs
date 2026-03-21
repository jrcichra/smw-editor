mod central_panel;
mod editing;
mod left_panel;
mod level_renderer;
mod object_layer;
mod properties;
mod tile_picker;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use egui::{CentralPanel, Frame, SidePanel, Ui, WidgetText, *};
use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};
use smwe_rom::SmwRom;

use self::{
    level_renderer::LevelRenderer, object_layer::EditableObjectLayer, properties::LevelProperties,
    tile_picker::TilePicker,
};
use crate::{
    ui::{editing_mode::EditingMode, tool::DockableEditorTool},
    undo::UndoableData,
};

pub struct UiLevelEditor {
    gl: Arc<glow::Context>,
    rom: Arc<SmwRom>,
    cpu: Cpu,
    level_renderer: Arc<Mutex<LevelRenderer>>,

    level_num: u16,
    offset: Vec2,
    zoom: f32,
    always_show_grid: bool,
    show_object_overlay: bool,
    show_object_labels: bool,
    selected_tile: Option<(u32, u32)>,

    level_properties: LevelProperties,
    layer1: UndoableData<EditableObjectLayer>,
    tile_picker: TilePicker,

    // Editing state
    editing_mode: EditingMode,
    selected_object_indices: HashSet<usize>,
    draw_object_id: u8,
    draw_object_settings: u8,
    draw_block_id: u16,
}

impl UiLevelEditor {
    pub fn new(gl: Arc<glow::Context>, rom: Arc<SmwRom>, rom_path: PathBuf) -> Self {
        let level_renderer = Arc::new(Mutex::new(LevelRenderer::new(&gl)));

        let raw = std::fs::read(&rom_path).expect("cannot read ROM for emulator");
        let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
        let mut emu_rom = EmuRom::new(rom_bytes);
        emu_rom.load_symbols(include_str!("../../../../symbols/SMW_U.sym"));
        let cpu = smwe_emu::Cpu::new(CheckedMem::new(Arc::new(emu_rom)));

        let mut editor = Self {
            gl,
            rom,
            cpu,
            level_renderer,
            level_num: 0x105,
            offset: Vec2::ZERO,
            zoom: 1.0,
            always_show_grid: false,
            show_object_overlay: false,
            show_object_labels: true,
            selected_tile: None,
            level_properties: LevelProperties::default(),
            layer1: UndoableData::new(EditableObjectLayer::default()),
            tile_picker: TilePicker::new(),
            editing_mode: EditingMode::Select,
            selected_object_indices: HashSet::new(),
            draw_object_id: 0x00,
            draw_object_settings: 0x00,
            draw_block_id: 0x25,
        };
        editor.load_level();
        editor
    }
}

// UI
impl DockableEditorTool for UiLevelEditor {
    fn update(&mut self, ui: &mut Ui) {
        SidePanel::left("level_editor.left_panel").resizable(false).show_inside(ui, |ui| self.left_panel(ui));
        CentralPanel::default().frame(Frame::none().inner_margin(0.)).show_inside(ui, |ui| self.central_panel(ui));
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

        let (sprite_layer, is_vertical) = {
            let level = &self.rom.levels[level_idx];
            self.level_properties = LevelProperties::from_level(level);
            let layer1 = EditableObjectLayer::from_level(level);
            self.layer1 = UndoableData::new(layer1);
            (level.sprite_layer.clone(), level.secondary_header.vertical_level())
        };
        self.offset = Vec2::ZERO;
        self.selected_tile = None;
        self.selected_object_indices.clear();

        // Reset emulator RAM before loading the new level so no state leaks
        // from the previously loaded level (stale sprite tables, VRAM, etc.).
        self.cpu.mem.wram.fill(0);
        self.cpu.mem.vram.fill(0);
        self.cpu.mem.cgram.fill(0);
        self.cpu.mem.regs.fill(0);

        // Decompress level: fills WRAM block maps, VRAM tile graphics, CGRAM palette.
        smwe_emu::emu::decompress_sublevel(&mut self.cpu, self.level_num);

        // For each unique sprite ID, clone the clean post-decompress CPU state,
        // run exec_sprite_id on the clone (so state never accumulates between IDs),
        // and collect the OAM tiles the sprite emits relative to the anchor point.
        let mut oam_map: HashMap<u8, Vec<smwe_emu::emu::SpriteOamTile>> = HashMap::new();
        {
            let mut unique_ids: Vec<u8> = sprite_layer.sprites.iter().map(|s| s.sprite_id()).collect();
            unique_ids.sort_unstable();
            unique_ids.dedup();

            for id in unique_ids {
                // Clone gives each ID a pristine post-decompress environment.
                let mut cpu_clone = self.cpu.clone();
                let tiles = smwe_emu::emu::sprite_oam_tiles(&mut cpu_clone, id);
                if !tiles.is_empty() {
                    oam_map.insert(id, tiles);
                }
            }
        }

        // Upload palette + GFX from the clean post-decompress state, then tiles.
        let mut renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
        renderer.upload_palette(&self.gl, &self.cpu.mem.cgram);
        renderer.upload_gfx(&self.gl, &self.cpu.mem.vram);
        renderer.upload_level(&self.gl, &mut self.cpu);
        renderer.upload_sprites(&self.gl, &sprite_layer, &oam_map, is_vertical);
        drop(renderer);

        // Rebuild the tile picker from the loaded level's tileset.
        self.tile_picker.rebuild(&mut self.cpu);
    }

    #[allow(dead_code)]
    fn upload_gfx_palette(&self) {
        let level_idx = self.level_num as usize;
        if level_idx >= self.rom.levels.len() {
            return;
        }
        let renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
        renderer.upload_palette(&self.gl, &self.cpu.mem.cgram);
        renderer.upload_gfx(&self.gl, &self.cpu.mem.vram);
    }

    /// Compute the WRAM block map index from block (tile) coordinates.
    /// Must produce the same index as `load_layer`'s reverse mapping.
    fn block_map_index(&self, block_x: u32, block_y: u32) -> u32 {
        let vertical = self.level_properties.is_vertical;
        let has_layer2 = self.level_properties.has_layer2;

        let scr_len = if vertical {
            if has_layer2 {
                0x0E
            } else {
                0x1C
            }
        } else {
            if has_layer2 {
                0x10
            } else {
                0x20
            }
        };
        let scr_size = if vertical { 16 * 32 } else { 16 * 27 };

        // Convert block coords to pixel coords matching load_layer's format:
        //   block_x_pixels = column * 16 + screen * 256
        //   block_y_pixels = row * 16  (+ screen offset for vertical)
        // The "screen column" used by load_layer is block_x_pixels / 16.
        let block_x_px = block_x * 16;
        let block_y_px = block_y * 16;

        let (screen, sidx) = if vertical {
            let sub_y = block_y_px / 512;
            let sub_x = block_x_px / 256;
            let screen = sub_y * 2 + sub_x;
            let col = (block_x_px / 16) % 16;
            let row = (block_y_px / 16) % 32;
            (screen, row * 16 + col)
        } else {
            let screen_col = block_x_px / 16; // screen * scr_len + local_col
            let screen = screen_col / scr_len as u32;
            let col = screen_col % scr_len as u32;
            let row = block_y;
            (screen, row * scr_len as u32 + col)
        };

        screen * scr_size as u32 + sidx
    }

    /// Write a block ID at the given block coordinates into the WRAM block map.
    fn set_block_id_at(&mut self, block_x: u32, block_y: u32, block_id: u16) {
        let idx = self.block_map_index(block_x, block_y);
        self.cpu.mem.store_u8(0x7EC800 + idx, (block_id & 0xFF) as u8);
        self.cpu.mem.store_u8(0x7FC800 + idx, ((block_id >> 8) & 0x01) as u8);
    }

    /// Re-render the GL tiles from the current WRAM block map.
    fn rebuild_tiles(&mut self) {
        let mut renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
        renderer.upload_level(&self.gl, &mut self.cpu);
    }

    /// Look up the L1 block ID at the given block coordinates by reading
    /// the WRAM block map populated during `decompress_sublevel`.
    fn block_id_at(&mut self, block_x: u32, block_y: u32) -> Option<u16> {
        let idx = self.block_map_index(block_x, block_y);
        let lo = 0x7EC800u32 + idx;
        let hi = 0x7FC800u32 + idx;
        Some(self.cpu.mem.load_u8(lo) as u16 | (((self.cpu.mem.load_u8(hi) as u16) & 0x01) << 8))
    }
}
