mod central_panel;
mod editing;
mod left_panel;
mod level_renderer;
mod object_layer;
mod properties;
mod sprite_editor;
mod tile_picker;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use egui::{CentralPanel, Frame, SidePanel, Ui, WidgetText, *};
use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};
use smwe_render::{atlas_renderer::AtlasRenderer, tile_atlas::TileAtlas};
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

    // Pre-rendered tile atlas (decoded once on level load)
    tile_atlas: TileAtlas,
    atlas_renderer: Arc<Mutex<AtlasRenderer>>,
    atlas_quads: Vec<smwe_render::atlas_renderer::QuadVertex>,

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
    preview_texture: Option<egui::TextureHandle>,
    preview_for: Option<(u32, u32)>,

    // Editing state
    editing_mode: EditingMode,
    selected_object_indices: HashSet<usize>,
    draw_object_id: u8,
    draw_object_settings: u8,
    draw_block_id: u16,
    edit_layer: u8, // 1 or 2
    show_sprites: bool,
    selected_sprite: Option<usize>,
    place_sprite_id: u8,
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
            gl: gl.clone(),
            rom,
            cpu,
            level_renderer,
            tile_atlas: TileAtlas::new(&gl),
            atlas_renderer: Arc::new(Mutex::new(AtlasRenderer::new(&gl))),
            atlas_quads: Vec::new(),
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
            preview_texture: None,
            preview_for: None,
            editing_mode: EditingMode::Select,
            selected_object_indices: HashSet::new(),
            draw_object_id: 0x00,
            draw_object_settings: 0x00,
            draw_block_id: 0x25,
            edit_layer: 1,
            show_sprites: true,
            selected_sprite: None,
            place_sprite_id: 0x00,
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
        self.tile_atlas.destroy(&self.gl);
        self.atlas_renderer.lock().unwrap().destroy(&self.gl);
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

        // Upload palette + GFX from the clean post-decompress state, then tiles.
        // Sprite tiles are NOT generated via the emulator here — that would run
        // exec_sprites per unique ID (very slow).  The sprite overlay in the
        // central panel already renders colored rectangles at correct positions.
        let oam_map: HashMap<u8, Vec<smwe_emu::emu::SpriteOamTile>> = HashMap::new();
        let mut renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
        renderer.upload_palette(&self.gl, &self.cpu.mem.cgram);
        renderer.upload_gfx(&self.gl, &self.cpu.mem.vram);
        renderer.upload_level(&self.gl, &mut self.cpu);
        renderer.upload_sprites(&self.gl, &sprite_layer, &oam_map, is_vertical);
        drop(renderer);

        // Rebuild the tile picker from the loaded level's tileset.
        self.tile_picker.rebuild(&mut self.cpu);

        // Build the pre-rendered tile atlas for fast display.
        self.rebuild_atlas();
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
            // Each horizontal screen is 16 tiles wide (256 px / 16 px per tile).
            // load_layer indexes as: idx = screen * (16 * 27) + row * 16 + col
            //   where block_x = col + screen * 16,  block_y = row
            let screen = block_x / 16;
            let col = block_x % 16;
            let row = block_y;
            let sidx = row * 16 + col;
            (screen, sidx)
        };

        screen * scr_size as u32 + sidx
    }

    /// Get the WRAM base addresses for the currently edited layer.
    fn block_map_base(&self) -> (u32, u32) {
        if self.edit_layer == 2 && self.level_properties.has_layer2 {
            let vertical = self.level_properties.is_vertical;
            let scr_len: u32 = if vertical { 0x0E } else { 0x10 };
            let scr_size: u32 = if vertical { 16 * 32 } else { 16 * 27 };
            let offset = scr_len * scr_size;
            (0x7EC800 + offset, 0x7FC800 + offset)
        } else {
            (0x7EC800, 0x7FC800)
        }
    }

    /// Write a block ID at the given block coordinates into the WRAM block map.
    fn set_block_id_at(&mut self, block_x: u32, block_y: u32, block_id: u16) {
        let idx = self.block_map_index(block_x, block_y);
        let (lo_base, hi_base) = self.block_map_base();
        self.cpu.mem.store_u8(lo_base + idx, (block_id & 0xFF) as u8);
        self.cpu.mem.store_u8(hi_base + idx, ((block_id >> 8) & 0x01) as u8);
    }

    /// Re-render the GL tiles from the current WRAM block map.
    fn rebuild_tiles(&mut self) {
        let mut renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
        renderer.upload_level(&self.gl, &mut self.cpu);
        drop(renderer);
        self.rebuild_atlas();
    }

    /// Re-upload sprite tiles after editing the sprite list.
    /// Uses an empty oam_map (no emulator tile generation) — the sprite
    /// overlay in the central panel handles visual representation.
    fn upload_sprites_for_level(&mut self) {
        let is_vertical = self.level_properties.is_vertical;
        let sprites = sprite_editor::read_sprites_from_wram(&self.cpu.mem.wram);
        let sprite_layer = smwe_rom::level::SpriteLayer { sprites };
        let oam_map: std::collections::HashMap<u8, Vec<smwe_emu::emu::SpriteOamTile>> =
            std::collections::HashMap::new();

        let mut renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
        renderer.upload_sprites(&self.gl, &sprite_layer, &oam_map, is_vertical);
    }

    /// Look up the block ID at the given block coordinates by reading
    /// the WRAM block map for the current edit layer.
    fn block_id_at(&mut self, block_x: u32, block_y: u32) -> Option<u16> {
        let idx = self.block_map_index(block_x, block_y);
        let (lo_base, hi_base) = self.block_map_base();
        Some(self.cpu.mem.load_u8(lo_base + idx) as u16 | (((self.cpu.mem.load_u8(hi_base + idx) as u16) & 0x01) << 8))
    }

    /// Rebuild the texture atlas and quad vertices from the current WRAM block map.
    /// This is called after level load and after block edits.
    fn rebuild_atlas(&mut self) {
        use smwe_render::atlas_renderer::QuadVertex;
        use smwe_render::tile_atlas::{TileAtlas, ATLAS_TILES};

        // Phase 1: iterate all blocks in both layers, collect unique tile words.
        let mut all_tile_words: Vec<u16> = Vec::with_capacity(2048);
        let mut layer_data: Vec<(u32, u32, [u16; 4])> = Vec::with_capacity(8192); // (px_x, px_y, [4 tile words])

        for bg in [false, true] {
            let map16_bank = self.cpu.mem.cart.resolve("Map16Common").unwrap_or(0) & 0xFF0000;
            let map16_bg = self.cpu.mem.cart.resolve("Map16BGTiles").unwrap_or(0);

            let vertical = self.level_properties.is_vertical;
            let has_layer2 = self.level_properties.has_layer2;
            let scr_len: u32 = if vertical {
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
            let scr_size: u32 = if vertical { 16 * 32 } else { 16 * 27 };

            let (blocks_lo_addr, blocks_hi_addr) = if bg && has_layer2 {
                let o = scr_len * scr_size;
                (0x7EC800 + o, 0x7FC800 + o)
            } else if bg {
                (0x7EB900, 0x7EBD00)
            } else {
                (0x7EC800, 0x7FC800)
            };

            let len: u32 = if has_layer2 { 256 * 27 } else { 512 * 27 };

            for idx in 0..len {
                let (block_x_px, block_y_px) = if vertical {
                    let (screen, sidx) = (idx / (16 * 16), idx % (16 * 16));
                    let (row, col) = (sidx / 16, sidx % 16);
                    let (sub_y, sub_x) = (screen / 2, screen % 2);
                    (col * 8 + sub_x * 256, row * 8 + sub_y * 256)
                } else {
                    let (screen, sidx) = (idx / (16 * 27), idx % (16 * 27));
                    let (row, col) = (sidx / 16, sidx % 16);
                    (col * 8 + screen * 256, row * 8)
                };

                let idx_adj = if bg && !has_layer2 { idx % (16 * 27 * 2) } else { idx };
                let block_id = self.cpu.mem.load_u8(blocks_lo_addr + idx_adj) as u16
                    | (((self.cpu.mem.load_u8(blocks_hi_addr + idx_adj) as u16) & 0x01) << 8);

                let block_ptr = if bg && !has_layer2 {
                    block_id as u32 * 8 + map16_bg
                } else {
                    self.cpu.mem.load_u16(0x0FBE + block_id as u32 * 2) as u32 + map16_bank
                };

                let mut tile_words = [0u16; 4];
                for (sub_i, (_off_x, _off_y)) in [(0u32, 0u32), (8, 0), (0, 8), (8, 8)].iter().enumerate() {
                    let tw = self.cpu.mem.load_u16(block_ptr + sub_i as u32 * 2);
                    tile_words[sub_i] = tw;
                    all_tile_words.push(tw);
                }
                layer_data.push((block_x_px, block_y_px, tile_words));
            }
        }

        // Phase 2: decode unique tiles into the atlas.
        all_tile_words.sort_unstable();
        all_tile_words.dedup();
        // Limit to atlas capacity
        if all_tile_words.len() > ATLAS_TILES {
            all_tile_words.truncate(ATLAS_TILES);
        }
        let slot_map = self.tile_atlas.rebuild(&self.gl, &self.cpu.mem.vram, &self.cpu.mem.cgram, &all_tile_words);

        // Phase 3: generate quad vertices for all tiles in both layers.
        let mut quads = Vec::with_capacity(layer_data.len() * 6);
        for &(px_x, px_y, ref tile_words) in &layer_data {
            let sub_positions = [(0u32, 0u32), (8, 0), (0, 8), (8, 8)];
            for (sub_i, &(sx, sy)) in sub_positions.iter().enumerate() {
                let tw = tile_words[sub_i];
                if let Some(&slot) = slot_map.get(&tw) {
                    let (u0, v0, u1, v1) = TileAtlas::slot_uv(slot);
                    let x0 = (px_x + sx) as f32;
                    let y0 = (px_y + sy) as f32;
                    let x1 = x0 + 8.0;
                    let y1 = y0 + 8.0;
                    // Two triangles forming a quad
                    quads.push(QuadVertex { x: x0, y: y0, u: u0, v: v0 });
                    quads.push(QuadVertex { x: x1, y: y0, u: u1, v: v0 });
                    quads.push(QuadVertex { x: x0, y: y1, u: u0, v: v1 });
                    quads.push(QuadVertex { x: x1, y: y0, u: u1, v: v0 });
                    quads.push(QuadVertex { x: x1, y: y1, u: u1, v: v1 });
                    quads.push(QuadVertex { x: x0, y: y1, u: u0, v: v1 });
                }
            }
        }

        self.atlas_quads = quads;
        self.atlas_renderer.lock().expect("Cannot lock atlas_renderer").set_quads(&self.gl, &self.atlas_quads);
    }
}
