mod central_panel;
mod left_panel;
mod level_renderer;
mod object_layer;
mod properties;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use egui::{CentralPanel, Frame, SidePanel, Ui, WidgetText, *};
use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};
use smwe_rom::SmwRom;

use self::{level_renderer::LevelRenderer, object_layer::EditableObjectLayer, properties::LevelProperties};
use crate::ui::tool::DockableEditorTool;

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

    level_properties: LevelProperties,
    layer1: EditableObjectLayer,
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
            self.layer1 = EditableObjectLayer::from_level(level);
            (level.sprite_layer.clone(), level.secondary_header.vertical_level())
        };
        self.offset = Vec2::ZERO;

        // Decompress level — fills WRAM block maps, VRAM, and CGRAM.
        smwe_emu::emu::decompress_sublevel(&mut self.cpu, self.level_num);

        // Build a map of sprite_id -> (tile_word, is_16x16) by running
        // exec_sprite_id for each unique sprite ID in the level. This gives
        // us the correct tile/palette for each sprite type without relying
        // on live OAM positions (which are emulator-fake).
        let mut oam_map: HashMap<u8, (u16, bool)> = HashMap::new();
        {
            let mut unique_ids: Vec<u8> = sprite_layer.sprites.iter()
                .map(|s| s.sprite_id())
                .collect();
            unique_ids.sort_unstable();
            unique_ids.dedup();

            for id in unique_ids {
                if let Some(info) = smwe_emu::emu::sprite_oam_info(&mut self.cpu, id) {
                    oam_map.insert(id, info);
                }
            }

            // Re-run decompress after exec_sprite_id calls to restore clean VRAM/CGRAM
            // (exec_sprite_id may disturb emulator state).
            smwe_emu::emu::decompress_sublevel(&mut self.cpu, self.level_num);
        }

        // Upload CGRAM and VRAM from the clean post-decompress state.
        {
            let mut renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
            renderer.upload_palette(&self.gl, &self.cpu.mem.cgram);
            renderer.upload_gfx(&self.gl, &self.cpu.mem.vram);
            renderer.upload_level(&self.gl, &mut self.cpu);
            renderer.upload_sprites(&self.gl, &sprite_layer, &oam_map, is_vertical);
        }
    }

    fn upload_gfx_palette(&self) {
        let level_idx = self.level_num as usize;
        if level_idx >= self.rom.levels.len() {
            return;
        }
        let mut renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
        renderer.upload_palette(&self.gl, &self.cpu.mem.cgram);
        renderer.upload_gfx(&self.gl, &self.cpu.mem.vram);
    }
}
