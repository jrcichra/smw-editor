mod background_layer;
mod central_panel;
mod editing;
mod left_panel;
mod level_renderer;
mod object_layer;
mod properties;
mod sprite_catalog;
mod sprite_layer;
mod tile_picker;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use egui::{CentralPanel, Frame, SidePanel, Ui, WidgetText, *};
use smwe_emu::{
    emu::{CheckedMem, SpriteOamTile},
    rom::Rom as EmuRom,
    Cpu,
};
use smwe_rom::{
    compression::lc_rle1,
    level::{Layer2Data, PRIMARY_HEADER_SIZE},
    snes_utils::addr::{AddrPc, AddrSnes},
    SmwRom,
};

use self::{
    background_layer::EditableBackgroundLayer,
    level_renderer::LevelRenderer, object_layer::EditableObjectLayer, properties::LevelProperties,
    sprite_layer::EditableSpriteLayer,
    tile_picker::{BgTilePicker, TilePicker},
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
    show_sprite_overlay: bool,
    show_object_labels: bool,
    selected_tile: Option<(u32, u32)>,

    level_properties: LevelProperties,
    layer1: UndoableData<EditableObjectLayer>,
    layer2_objects: Option<UndoableData<EditableObjectLayer>>,
    layer2_background: Option<UndoableData<EditableBackgroundLayer>>,
    sprites: UndoableData<EditableSpriteLayer>,
    tile_picker: TilePicker,
    bg_tile_picker: BgTilePicker,
    sprite_search: String,
    sprite_preview_textures: HashMap<u8, egui::TextureHandle>,
    sprite_oam_cache: HashMap<u8, Vec<SpriteOamTile>>,
    preview_texture: Option<egui::TextureHandle>,
    preview_for: Option<(u32, u32)>,

    // Editing state
    editing_mode: EditingMode,
    selected_object_indices: HashSet<usize>,
    selected_sprite_indices: HashSet<usize>,
    draw_object_id: u8,
    draw_object_settings: u8,
    draw_block_id: u16,
    draw_sprite_id: u8,
    draw_sprite_extra_bits: u8,
    edit_layer: u8, // 1 or 2
    edit_sprites: bool,
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
            show_sprite_overlay: true,
            show_object_labels: true,
            selected_tile: None,
            level_properties: LevelProperties::default(),
            layer1: UndoableData::new(EditableObjectLayer::default()),
            layer2_objects: None,
            layer2_background: None,
            sprites: UndoableData::new(EditableSpriteLayer::default()),
            tile_picker: TilePicker::new(),
            bg_tile_picker: BgTilePicker::new(),
            sprite_search: String::new(),
            sprite_preview_textures: HashMap::new(),
            sprite_oam_cache: HashMap::new(),
            preview_texture: None,
            preview_for: None,
            editing_mode: EditingMode::Select,
            selected_object_indices: HashSet::new(),
            selected_sprite_indices: HashSet::new(),
            draw_object_id: 0x00,
            draw_object_settings: 0x00,
            draw_block_id: 0x25,
            draw_sprite_id: 0x00,
            draw_sprite_extra_bits: 0x00,
            edit_layer: 1,
            edit_sprites: false,
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

    fn save_to_rom(&self, rom_bytes: &mut [u8], has_smc_header: bool) -> anyhow::Result<()> {
        let level = self
            .rom
            .levels
            .get(self.level_num as usize)
            .ok_or_else(|| anyhow::anyhow!("Level {:03X} out of range", self.level_num))?;

        let new_layer1 = self.layer1.read(|layer| layer.serialize_layer1_bytes(level.secondary_header.vertical_level()))?;
        let old_layer1 = level.layer1.as_bytes();
        if new_layer1.len() > old_layer1.len() {
            anyhow::bail!(
                "Level {:03X} layer 1 data grew from {} to {} bytes; repointing is not implemented yet",
                self.level_num,
                old_layer1.len(),
                new_layer1.len()
            );
        }

        let header_offset = usize::from(has_smc_header) * 0x200;
        let pointer_table_pc = AddrPc::try_from_lorom(AddrSnes(0x05E000))?.as_index() + header_offset;
        let pointer_off = pointer_table_pc + self.level_num as usize * 3;
        let ptr_bytes = rom_bytes
            .get(pointer_off..pointer_off + 3)
            .ok_or_else(|| anyhow::anyhow!("Level {:03X} pointer table offset out of range", self.level_num))?;
        let level_addr = u32::from_le_bytes([ptr_bytes[0], ptr_bytes[1], ptr_bytes[2], 0]);
        let level_pc = AddrPc::try_from_lorom(AddrSnes(level_addr))?.as_index() + header_offset;
        let layer1_start = level_pc + PRIMARY_HEADER_SIZE;
        let layer1_end = layer1_start + old_layer1.len();
        let layer1_dst = rom_bytes
            .get_mut(layer1_start..layer1_end)
            .ok_or_else(|| anyhow::anyhow!("Level {:03X} layer 1 write range out of bounds", self.level_num))?;

        layer1_dst[..new_layer1.len()].copy_from_slice(&new_layer1);
        layer1_dst[new_layer1.len()..].fill(0);

        let sprite_data_old = level.sprite_layer.as_bytes();
        let sprite_new = self.sprites.read(|sprites| sprites.serialize_bytes(level.secondary_header.vertical_level()))?;
        if sprite_new.len() > sprite_data_old.len() {
            anyhow::bail!(
                "Level {:03X} sprite data grew from {} to {} bytes; repointing is not implemented yet",
                self.level_num,
                sprite_data_old.len(),
                sprite_new.len()
            );
        }
        let sprite_ptr_table_pc = AddrPc::try_from_lorom(AddrSnes(0x05EC00))?.as_index() + header_offset;
        let sprite_ptr_off = sprite_ptr_table_pc + self.level_num as usize * 2;
        let sprite_ptr = rom_bytes
            .get(sprite_ptr_off..sprite_ptr_off + 2)
            .ok_or_else(|| anyhow::anyhow!("Level {:03X} sprite pointer table offset out of range", self.level_num))?;
        let sprite_addr = u16::from_le_bytes([sprite_ptr[0], sprite_ptr[1]]) as u32 | 0x070000;
        let sprite_pc = AddrPc::try_from_lorom(AddrSnes(sprite_addr))?.as_index() + header_offset;
        let sprite_data_start = sprite_pc + 1;
        let sprite_data_end = sprite_data_start + sprite_data_old.len();
        let sprite_dst = rom_bytes
            .get_mut(sprite_data_start..sprite_data_end)
            .ok_or_else(|| anyhow::anyhow!("Level {:03X} sprite write range out of bounds", self.level_num))?;
        sprite_dst[..sprite_new.len()].copy_from_slice(&sprite_new);
        sprite_dst[sprite_new.len()..].fill(0);

        let layer2_ptr_table_pc = AddrPc::try_from_lorom(AddrSnes(0x05E600))?.as_index() + header_offset;
        let layer2_ptr_off = layer2_ptr_table_pc + self.level_num as usize * 3;
        let layer2_ptr_bytes = rom_bytes
            .get(layer2_ptr_off..layer2_ptr_off + 3)
            .ok_or_else(|| anyhow::anyhow!("Level {:03X} layer 2 pointer table offset out of range", self.level_num))?;
        let layer2_addr_raw = u32::from_le_bytes([layer2_ptr_bytes[0], layer2_ptr_bytes[1], layer2_ptr_bytes[2], 0]);

        match (&level.layer2, &self.layer2_objects, &self.layer2_background) {
            (Layer2Data::Objects(objects), Some(layer2), _) => {
                let new_layer2 = layer2.read(|layer| layer.serialize_layer1_bytes(level.secondary_header.vertical_level()))?;
                let old_layer2 = objects.as_bytes();
                if new_layer2.len() > old_layer2.len() {
                    anyhow::bail!(
                        "Level {:03X} layer 2 object data grew from {} to {} bytes; repointing is not implemented yet",
                        self.level_num,
                        old_layer2.len(),
                        new_layer2.len()
                    );
                }
                let layer2_pc = AddrPc::try_from_lorom(AddrSnes(layer2_addr_raw))?.as_index() + header_offset;
                let start = layer2_pc + PRIMARY_HEADER_SIZE;
                let end = start + old_layer2.len();
                let dst = rom_bytes
                    .get_mut(start..end)
                    .ok_or_else(|| anyhow::anyhow!("Level {:03X} layer 2 object write range out of bounds", self.level_num))?;
                dst[..new_layer2.len()].copy_from_slice(&new_layer2);
                dst[new_layer2.len()..].fill(0);
            }
            (Layer2Data::Background(background), _, Some(layer2)) => {
                let new_bg = layer2.read(|bg| bg.tile_ids.clone());
                let compressed = lc_rle1::compress(&new_bg);
                let old_size = background.compressed_size();
                if compressed.len() > old_size {
                    anyhow::bail!(
                        "Level {:03X} layer 2 background data grew from {} to {} bytes; repointing is not implemented yet",
                        self.level_num,
                        old_size,
                        compressed.len()
                    );
                }
                let layer2_pc = AddrPc::try_from_lorom(AddrSnes((layer2_addr_raw & 0x00FFFF) | 0x0C0000))?.as_index()
                    + header_offset;
                let dst = rom_bytes
                    .get_mut(layer2_pc..layer2_pc + old_size)
                    .ok_or_else(|| anyhow::anyhow!("Level {:03X} layer 2 background write range out of bounds", self.level_num))?;
                dst[..compressed.len()].copy_from_slice(&compressed);
                dst[compressed.len()..].fill(0);
            }
            _ => {}
        }
        Ok(())
    }
}

// Internals
impl UiLevelEditor {
    pub(super) fn editing_objects(&self) -> Option<&UndoableData<EditableObjectLayer>> {
        if self.edit_layer == 2 {
            self.layer2_objects.as_ref()
        } else {
            Some(&self.layer1)
        }
    }

    pub(super) fn editing_objects_mut(&mut self) -> Option<&mut UndoableData<EditableObjectLayer>> {
        if self.edit_layer == 2 {
            self.layer2_objects.as_mut()
        } else {
            Some(&mut self.layer1)
        }
    }

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
            self.sprites = UndoableData::new(EditableSpriteLayer::from_level(level));
            match &level.layer2 {
                Layer2Data::Objects(objects) => {
                    self.layer2_objects = Some(UndoableData::new(EditableObjectLayer::from_object_layer(
                        objects,
                        level.secondary_header.vertical_level(),
                    )));
                    self.layer2_background = None;
                }
                Layer2Data::Background(bg) => {
                    self.layer2_objects = None;
                    self.layer2_background = Some(UndoableData::new(EditableBackgroundLayer::new(bg.tile_ids().to_vec())));
                }
            }
            (level.sprite_layer.clone(), level.secondary_header.vertical_level())
        };
        self.offset = Vec2::ZERO;
        self.selected_tile = None;
        self.selected_object_indices.clear();
        self.selected_sprite_indices.clear();
        self.sprite_preview_textures.clear();
        self.sprite_oam_cache.clear();

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
        let mut oam_map: HashMap<u8, Vec<SpriteOamTile>> = HashMap::new();
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
        self.sprite_oam_cache = oam_map.clone();

        // Upload palette + GFX from the clean post-decompress state, then tiles.
        let mut renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
        renderer.upload_palette(&self.gl, &self.cpu.mem.cgram);
        renderer.upload_gfx(&self.gl, &self.cpu.mem.vram);
        renderer.upload_level(&self.gl, &mut self.cpu);
        renderer.upload_editable_sprites(
            &self.gl,
            &self.sprites.read(|sprites| sprites.sprites.clone()),
            &oam_map,
            is_vertical,
        );
        drop(renderer);

        // Rebuild the tile picker from the loaded level's tileset.
        self.tile_picker.rebuild(&mut self.cpu);
        self.bg_tile_picker.rebuild(&mut self.cpu);
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

    pub(super) fn rebuild_sprite_tiles(&mut self) {
        let level_idx = self.level_num as usize;
        if level_idx >= self.rom.levels.len() {
            return;
        }
        let is_vertical = self.rom.levels[level_idx].secondary_header.vertical_level();
        let mut unique_ids: Vec<u8> = self.sprites.read(|sprites| sprites.sprites.iter().map(|s| s.sprite_id).collect());
        unique_ids.sort_unstable();
        unique_ids.dedup();

        let mut oam_map: HashMap<u8, Vec<SpriteOamTile>> = HashMap::new();
        for id in unique_ids {
            let mut cpu_clone = self.cpu.clone();
            let tiles = smwe_emu::emu::sprite_oam_tiles(&mut cpu_clone, id);
            if !tiles.is_empty() {
                oam_map.insert(id, tiles);
            }
        }
        self.sprite_oam_cache = oam_map.clone();

        let sprite_entries = self.sprites.read(|sprites| sprites.sprites.clone());
        let mut renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
        renderer.upload_editable_sprites(&self.gl, &sprite_entries, &oam_map, is_vertical);
    }

    pub(super) fn sprite_oam_tiles(&mut self, sprite_id: u8) -> Vec<SpriteOamTile> {
        if let Some(tiles) = self.sprite_oam_cache.get(&sprite_id) {
            return tiles.clone();
        }
        let mut cpu_clone = self.cpu.clone();
        let tiles = smwe_emu::emu::sprite_oam_tiles(&mut cpu_clone, sprite_id);
        self.sprite_oam_cache.insert(sprite_id, tiles.clone());
        tiles
    }

    pub(super) fn sprite_pixel_bounds(&mut self, sprite_id: u8) -> Option<(i32, i32, i32, i32)> {
        let tiles = self.sprite_oam_tiles(sprite_id);
        if tiles.is_empty() {
            return None;
        }

        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        for tile in &tiles {
            let size = if tile.is_16x16 { 16 } else { 8 };
            min_x = min_x.min(tile.dx);
            min_y = min_y.min(tile.dy);
            max_x = max_x.max(tile.dx + size);
            max_y = max_y.max(tile.dy + size);
        }
        Some((min_x, min_y, max_x, max_y))
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

        let idx = screen * scr_size as u32 + sidx;
        if self.edit_layer == 2 && !self.level_properties.has_layer2 {
            idx % (16 * 27 * 2)
        } else {
            idx
        }
    }

    /// Get the WRAM base addresses for the currently edited layer.
    fn block_map_base(&self) -> (u32, u32) {
        if self.edit_layer == 2 {
            if self.level_properties.has_layer2 {
                let vertical = self.level_properties.is_vertical;
                let scr_len: u32 = if vertical { 0x0E } else { 0x10 };
                let scr_size: u32 = if vertical { 16 * 32 } else { 16 * 27 };
                let offset = scr_len * scr_size;
                (0x7EC800 + offset, 0x7FC800 + offset)
            } else {
                (0x7EB900, 0x7EBD00)
            }
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
    }

    /// Look up the block ID at the given block coordinates by reading
    /// the WRAM block map for the current edit layer.
    fn block_id_at(&mut self, block_x: u32, block_y: u32) -> Option<u16> {
        let idx = self.block_map_index(block_x, block_y);
        let (lo_base, hi_base) = self.block_map_base();
        Some(self.cpu.mem.load_u8(lo_base + idx) as u16 | (((self.cpu.mem.load_u8(hi_base + idx) as u16) & 0x01) << 8))
    }
}
