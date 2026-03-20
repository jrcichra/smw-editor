mod central_panel;
mod left_panel;
mod level_renderer;
mod object_layer;
mod properties;

use std::{
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

        // Build the emulator ROM from the same file.
        // SMC/SFC files have a 0x200-byte header we must skip (same as old project.rs).
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

        // Parse object layer / properties from smwe-rom (for the overlay / left panel)
        {
            let level = &self.rom.levels[level_idx];
            self.level_properties = LevelProperties::from_level(level);
            self.layer1 = EditableObjectLayer::from_level(level);
        }
        self.offset = Vec2::ZERO;

        // Run the actual SNES level decompressor in the emulator.
        // This populates WRAM 0x7EC800/0x7FC800 with Map16 block IDs, exactly as
        // the real game does — no hand-translated object routines needed.
        smwe_emu::emu::decompress_sublevel(&mut self.cpu, self.level_num);

        // Run sprite init so OAM ($0300) is populated with tile/palette data for
        // every sprite in the level — required before uploading sprite tiles.
        smwe_emu::emu::exec_sprites(&mut self.cpu);

        // Upload GFX/palette (still sourced from smwe-rom, which is fine)
        self.upload_gfx_palette();

        // Upload tile layers and OAM sprite tiles in one lock.
        {
            let mut renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
            renderer.upload_level(&self.gl, &mut self.cpu);
            // Upload OAM sprite tiles so sprites (e.g. Dragon Coin) render with
            // the correct graphics and palette instead of a purple placeholder.
            renderer.upload_sprites(&self.gl, &mut self.cpu);
        }
    }

    fn upload_gfx_palette(&self) {
        let level_idx = self.level_num as usize;
        if level_idx >= self.rom.levels.len() {
            return;
        }
        // ── CGRAM ──────────────────────────────────────────────────────────────
        // The emulator runs LoadPalette + CODE_00922F so self.cpu.mem.cgram is
        // already correct after decompress_sublevel.  Upload it directly.
        self.level_renderer.lock().expect("Cannot lock level_renderer").upload_palette(&self.gl, &self.cpu.mem.cgram);

        // ── VRAM ───────────────────────────────────────────────────────────────
        // The emulator runs UploadSpriteGFX which fills self.cpu.mem.vram with
        // the correct tile graphics for this level's tileset.  Upload directly.
        self.level_renderer.lock().expect("Cannot lock level_renderer").upload_gfx(&self.gl, &self.cpu.mem.vram);
    }
}
