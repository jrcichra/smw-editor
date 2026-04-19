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
    time::Instant,
};

use egui::{CentralPanel, Frame, SidePanel, Ui, WidgetText, *};
use smwe_emu::{
    emu::{CheckedMem, SpriteOamTile},
    rom::Rom as EmuRom,
    Cpu,
};
use smwe_rom::{
    compression::lc_rle1,
    level::{Layer2Data, Level, PRIMARY_HEADER_SIZE},
    snes_utils::addr::{AddrPc, AddrSnes},
    SmwRom,
};

use self::{
    background_layer::EditableBackgroundLayer,
    level_renderer::LevelRenderer, object_layer::EditableObjectLayer, properties::LevelProperties,
    sprite_layer::{EditableSprite, EditableSpriteLayer},
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

    // Animation
    last_anim_tick: Instant,

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
    pub fn new(gl: Arc<glow::Context>, rom: Arc<SmwRom>, rom_path: PathBuf) -> anyhow::Result<Self> {
        let level_renderer = Arc::new(Mutex::new(LevelRenderer::new(&gl)));

        let raw = std::fs::read(&rom_path)
            .map_err(|e| anyhow::anyhow!("Cannot read ROM for emulator at {}: {e}", rom_path.display()))?;
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
            last_anim_tick: Instant::now(),
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
        Ok(editor)
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
        let level_idx = self.level_num as usize;
        let level = self
            .rom
            .levels
            .get(level_idx)
            .ok_or_else(|| anyhow::anyhow!("Level {:03X} out of range", self.level_num))?;
        let vertical = level.secondary_header.vertical_level();
        let header_offset = usize::from(has_smc_header) * 0x200;

        // Serialize current editor state.
        let new_l1 = self.layer1.read(|l| l.serialize_layer1_bytes(vertical))?;
        let new_sprites = self.sprites.read(|s| s.serialize_bytes(vertical))?;

        // Reconstruct all 5 primary-header bytes from LevelProperties.
        let p = &self.level_properties;
        let new_primary_header: [u8; PRIMARY_HEADER_SIZE] = [
            (p.palette_bg << 5) | p.level_length,
            (p.back_area_color << 5) | p.level_mode,
            ((p.layer3_priority as u8) << 7) | (p.music << 4) | (p.sprite_gfx & 0x0F),
            (p.timer << 6) | (p.palette_sprite << 3) | p.palette_fg,
            (p.item_memory << 6) | (p.vertical_scroll << 4) | p.fg_bg_gfx,
        ];

        // ── Layer 1  (pointer table $05E000, 3-byte LoROM SNES addr each) ──────
        // Block layout on ROM: [5-byte primary header][layer-1 object data]
        {
            let tbl_pc = AddrPc::try_from_lorom(AddrSnes(0x05E000))?.as_index();
            let ptr_off = tbl_pc + header_offset + level_idx * 3;
            let old_snes = read_u24(rom_bytes, ptr_off)
                .ok_or_else(|| anyhow::anyhow!("L1 pointer table out of range"))?;
            let old_file = AddrPc::try_from_lorom(AddrSnes(old_snes))?.as_index() + header_offset;

            let old_block = PRIMARY_HEADER_SIZE + level.layer1.as_bytes().len();
            let new_block = PRIMARY_HEADER_SIZE + new_l1.len();

            let dest = if new_block <= old_block {
                old_file
            } else {
                let pc = find_free_space(rom_bytes, new_block, 0x008000, header_offset)
                    .ok_or_else(|| anyhow::anyhow!(
                        "No free space for level {:03X} layer 1 ({} bytes)", self.level_num, new_block))?;
                rom_bytes[old_file..old_file + old_block].fill(0xFF);
                let b = AddrSnes::try_from_lorom(AddrPc(pc as u32))?.0.to_le_bytes();
                rom_bytes[ptr_off..ptr_off + 3].copy_from_slice(&b[..3]);
                pc + header_offset
            };

            rom_bytes[dest..dest + PRIMARY_HEADER_SIZE].copy_from_slice(&new_primary_header);
            let data_dest = dest + PRIMARY_HEADER_SIZE;
            rom_bytes[data_dest..data_dest + new_l1.len()].copy_from_slice(&new_l1);
            // Fill any shrunk tail with 0xFF so it is recognised as free space.
            if dest == old_file && new_block < old_block {
                rom_bytes[dest + new_block..dest + old_block].fill(0xFF);
            }
        }

        // ── Sprites  (pointer table $05EC00, 2-byte offset in bank $07 each) ──
        // Block layout on ROM: [1-byte sprite header][sprite data…0xFF]
        // Sprite data must stay in bank $07 (SNES $078000-$07FFFF).
        {
            let tbl_pc = AddrPc::try_from_lorom(AddrSnes(0x05EC00))?.as_index();
            let ptr_off = tbl_pc + header_offset + level_idx * 2;
            let old_offset = read_u16(rom_bytes, ptr_off)
                .ok_or_else(|| anyhow::anyhow!("Sprite pointer table out of range"))?;
            let old_snes = AddrSnes(old_offset as u32 | 0x070000);
            let old_file = AddrPc::try_from_lorom(old_snes)?.as_index() + header_offset;

            // Read sprite-header byte before any possible erasure.
            let sprite_hdr = *rom_bytes.get(old_file)
                .ok_or_else(|| anyhow::anyhow!("Sprite header byte out of range"))?;
            let old_block = 1 + level.sprite_layer.as_bytes().len();
            let new_block = 1 + new_sprites.len();

            let dest = if new_block <= old_block {
                old_file
            } else {
                // Must stay in bank $07: SNES $078000-$07FFFF = PC $038000-$03FFFF.
                let bank7_start = AddrPc::try_from_lorom(AddrSnes(0x078000))?.as_index();
                let bank7_end = bank7_start + 0x8000;
                let pc = find_free_space_in(rom_bytes, new_block, bank7_start, bank7_end, header_offset)
                    .ok_or_else(|| anyhow::anyhow!(
                        "No free space in bank $07 for level {:03X} sprite data ({} bytes)", self.level_num, new_block))?;
                rom_bytes[old_file..old_file + old_block].fill(0xFF);
                let new_off = AddrSnes::try_from_lorom(AddrPc(pc as u32))?.0 as u16;
                rom_bytes[ptr_off..ptr_off + 2].copy_from_slice(&new_off.to_le_bytes());
                pc + header_offset
            };

            rom_bytes[dest] = sprite_hdr;
            let data_dest = dest + 1;
            rom_bytes[data_dest..data_dest + new_sprites.len()].copy_from_slice(&new_sprites);
            if dest == old_file && new_block < old_block {
                rom_bytes[dest + new_block..dest + old_block].fill(0xFF);
            }
        }

        // ── Layer 2  (pointer table $05E600, 3-byte value each) ────────────────
        // If the pointer's bank byte == $FF the data is background (LC-RLE1) at
        // SNES bank $0C with the same 16-bit offset.  Otherwise it is object
        // data with the same block layout as layer 1.
        {
            let tbl_pc = AddrPc::try_from_lorom(AddrSnes(0x05E600))?.as_index();
            let ptr_off = tbl_pc + header_offset + level_idx * 3;
            let l2_raw = read_u24(rom_bytes, ptr_off)
                .ok_or_else(|| anyhow::anyhow!("L2 pointer table out of range"))?;

            match (&level.layer2, &self.layer2_objects, &self.layer2_background) {
                (Layer2Data::Objects(objects), Some(layer2), _) => {
                    let new_l2 = layer2.read(|l| l.serialize_layer1_bytes(vertical))?;
                    let old_file = AddrPc::try_from_lorom(AddrSnes(l2_raw))?.as_index() + header_offset;

                    // The 5-byte L2 header is not edited; copy it verbatim when repointing.
                    let old_block = PRIMARY_HEADER_SIZE + objects.as_bytes().len();
                    let new_block = PRIMARY_HEADER_SIZE + new_l2.len();

                    let dest = if new_block <= old_block {
                        old_file
                    } else {
                        // Save the existing L2 header before erasing.
                        let mut l2_hdr = [0u8; PRIMARY_HEADER_SIZE];
                        l2_hdr.copy_from_slice(&rom_bytes[old_file..old_file + PRIMARY_HEADER_SIZE]);
                        let pc = find_free_space(rom_bytes, new_block, 0x008000, header_offset)
                            .ok_or_else(|| anyhow::anyhow!(
                                "No free space for level {:03X} layer 2 ({} bytes)", self.level_num, new_block))?;
                        rom_bytes[old_file..old_file + old_block].fill(0xFF);
                        let b = AddrSnes::try_from_lorom(AddrPc(pc as u32))?.0.to_le_bytes();
                        rom_bytes[ptr_off..ptr_off + 3].copy_from_slice(&b[..3]);
                        let dest_file = pc + header_offset;
                        rom_bytes[dest_file..dest_file + PRIMARY_HEADER_SIZE].copy_from_slice(&l2_hdr);
                        dest_file
                    };

                    let data_dest = dest + PRIMARY_HEADER_SIZE;
                    rom_bytes[data_dest..data_dest + new_l2.len()].copy_from_slice(&new_l2);
                    if dest == old_file && new_block < old_block {
                        rom_bytes[dest + new_block..dest + old_block].fill(0xFF);
                    }
                }
                (Layer2Data::Background(background), _, Some(layer2)) => {
                    let new_bg = layer2.read(|bg| bg.tile_ids.clone());
                    let compressed = lc_rle1::compress(&new_bg);
                    // Background lives at bank $0C with the same 16-bit offset.
                    let old_snes_0c = AddrSnes((l2_raw & 0x00FFFF) | 0x0C0000);
                    let old_file = AddrPc::try_from_lorom(old_snes_0c)?.as_index() + header_offset;
                    let old_size = background.compressed_size();

                    let dest = if compressed.len() <= old_size {
                        old_file
                    } else {
                        // Background data lives in bank $0C: SNES $0C8000-$0CFFFF = PC $060000-$067FFF.
                        let bank0c_start = AddrPc::try_from_lorom(AddrSnes(0x0C8000))?.as_index();
                        let bank0c_end = bank0c_start + 0x8000;
                        let pc = find_free_space_in(rom_bytes, compressed.len(), bank0c_start, bank0c_end, header_offset)
                            .ok_or_else(|| anyhow::anyhow!(
                                "No free space in bank $0C for level {:03X} layer 2 bg ({} bytes)", self.level_num, compressed.len()))?;
                        rom_bytes[old_file..old_file + old_size].fill(0xFF);
                        // Pointer stores bank $FF with the same 16-bit offset used in bank $0C.
                        let new_snes_0c = AddrSnes::try_from_lorom(AddrPc(pc as u32))?;
                        let new_ptr = (0xFF0000u32) | (new_snes_0c.0 & 0x00FFFF);
                        let b = new_ptr.to_le_bytes();
                        rom_bytes[ptr_off..ptr_off + 3].copy_from_slice(&b[..3]);
                        pc + header_offset
                    };

                    rom_bytes[dest..dest + compressed.len()].copy_from_slice(&compressed);
                    if dest == old_file && compressed.len() < old_size {
                        rom_bytes[dest + compressed.len()..dest + old_size].fill(0xFF);
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }
}

// ── ROM save helpers ────────────────────────────────────────────────────────

/// Scan `rom_bytes` for `needed` consecutive `0xFF` bytes, starting at
/// `pc_start` (PC address, *without* the SMC header offset), anywhere up to
/// the end of the ROM.  Returns the PC address of the first byte of the run,
/// or `None` if no suitable region is found.
///
/// LoROM banks are 32 KB each; a run may not cross a bank boundary because the
/// SNES mapper cannot address such a block as a single contiguous region.
fn find_free_space(rom_bytes: &[u8], needed: usize, pc_start: usize, header_offset: usize) -> Option<usize> {
    let pc_end = rom_bytes.len().saturating_sub(header_offset);
    find_free_space_in(rom_bytes, needed, pc_start, pc_end, header_offset)
}

/// Like `find_free_space` but restricted to the PC range `[pc_start, pc_end)`.
fn find_free_space_in(
    rom_bytes: &[u8], needed: usize, pc_start: usize, pc_end: usize, header_offset: usize,
) -> Option<usize> {
    const BANK: usize = 0x8000;
    let mut run_start: Option<usize> = None;
    let mut run_len = 0usize;
    for pc in pc_start..pc_end {
        // Never span a LoROM bank boundary.
        if pc % BANK == 0 && pc != pc_start {
            run_start = None;
            run_len = 0;
        }
        let file = pc + header_offset;
        if file >= rom_bytes.len() {
            break;
        }
        if rom_bytes[file] == 0xFF {
            run_start.get_or_insert(pc);
            run_len += 1;
            if run_len >= needed {
                return run_start;
            }
        } else {
            run_start = None;
            run_len = 0;
        }
    }
    None
}

fn read_u16(rom_bytes: &[u8], file_off: usize) -> Option<u16> {
    let b = rom_bytes.get(file_off..file_off + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn read_u24(rom_bytes: &[u8], file_off: usize) -> Option<u32> {
    let b = rom_bytes.get(file_off..file_off + 3)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], 0]))
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

        // Position Mario at the level entrance
        let level = self.rom.levels[level_idx].clone();
        self.position_mario_at_entrance(is_vertical, &level);
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
        // Run one animation frame so animated VRAM tiles (coins, ? blocks) are
        // populated with their correct graphics instead of whatever the initial
        // GFX load left behind.
        smwe_emu::emu::fetch_anim_frame(&mut self.cpu);

        // For each unique sprite ID, clone the clean post-decompress CPU state,
        // run exec_sprite_id on the clone (so state never accumulates between IDs),
        // and collect the OAM tiles the sprite emits relative to the anchor point.
        let mut oam_map: HashMap<u8, Vec<SpriteOamTile>> = HashMap::new();
        {
            let mut unique_ids: Vec<u8> = sprite_layer.sprites.iter().map(|s| s.sprite_id()).collect();
            unique_ids.sort_unstable();
            unique_ids.dedup();

            for id in unique_ids {
                let tiles = self.compute_sprite_oam_tiles(id);
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
        renderer.upload_level(&self.gl, &mut self.cpu, &self.rom, self.level_properties.fg_bg_gfx);
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
            let tiles = self.compute_sprite_oam_tiles(id);
            if !tiles.is_empty() {
                oam_map.insert(id, tiles);
            }
        }
        self.sprite_oam_cache = oam_map.clone();

        let sprite_entries = self.sprites.read(|sprites| sprites.sprites.clone());
        let mut renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
        renderer.upload_editable_sprites(&self.gl, &sprite_entries, &oam_map, is_vertical);
    }

    pub(super) fn refresh_sprite_gfx(&mut self) {
        smwe_emu::emu::upload_sprite_tileset(&mut self.cpu, self.level_properties.sprite_gfx);
        self.sprite_preview_textures.clear();
        self.sprite_oam_cache.clear();
        {
            let renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
            renderer.upload_gfx(&self.gl, &self.cpu.mem.vram);
            renderer.upload_palette(&self.gl, &self.cpu.mem.cgram);
        }
        self.rebuild_sprite_tiles();
    }

    pub(super) fn sprite_oam_tiles(&mut self, sprite_id: u8) -> Vec<SpriteOamTile> {
        if let Some(tiles) = self.sprite_oam_cache.get(&sprite_id) {
            return tiles.clone();
        }
        let tiles = self.compute_sprite_oam_tiles(sprite_id);
        self.sprite_oam_cache.insert(sprite_id, tiles.clone());
        tiles
    }

    fn compute_sprite_oam_tiles(&self, sprite_id: u8) -> Vec<SpriteOamTile> {
        let mut cpu_clone = self.cpu.clone();
        if let Some(tileset) = sprite_catalog::preview_sprite_tileset(sprite_id) {
            smwe_emu::emu::upload_sprite_tileset(&mut cpu_clone, tileset);
        }
        smwe_emu::emu::sprite_oam_tiles(&mut cpu_clone, sprite_id)
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
        renderer.upload_level(&self.gl, &mut self.cpu, &self.rom, self.level_properties.fg_bg_gfx);
    }

    /// Look up the block ID at the given block coordinates by reading
    /// the WRAM block map for the current edit layer.
    fn block_id_at(&mut self, block_x: u32, block_y: u32) -> Option<u16> {
        let idx = self.block_map_index(block_x, block_y);
        let (lo_base, hi_base) = self.block_map_base();
        Some(self.cpu.mem.load_u8(lo_base + idx) as u16 | (((self.cpu.mem.load_u8(hi_base + idx) as u16) & 0x01) << 8))
    }

    /// Position Mario (sprite 0x00) at the level's main entrance.
    fn position_mario_at_entrance(&mut self, is_vertical: bool, level: &Level) {
        let (entrance_x, entrance_y) = level.secondary_header.main_entrance_xy_pos();
        let entrance_screen = level.secondary_header.main_entrance_screen();

        // Entrance coordinates are stored at half-resolution, multiply by 2
        let entrance_x = entrance_x as u32 * 2;
        let entrance_y = entrance_y as u32 * 2;

        // Convert entrance screen + local coords to absolute tile coordinates
        let abs_x = if is_vertical {
            let sx = entrance_screen as u32 % 2;
            sx * 16 + entrance_x
        } else {
            entrance_screen as u32 * 16 + entrance_x
        };

        let abs_y = if is_vertical {
            let sy = entrance_screen as u32 / 2;
            sy * 32 + entrance_y
        } else {
            entrance_y
        };

        // Find or create Mario sprite
        self.sprites.write(|sprites| {
            if let Some(mario) = sprites.sprites.iter_mut().find(|s| s.sprite_id == 0x00) {
                // Move existing Mario to entrance
                mario.x = abs_x;
                mario.y = abs_y;
            } else {
                // Create Mario at entrance if not present
                sprites.sprites.insert(0, EditableSprite {
                    x: abs_x,
                    y: abs_y,
                    sprite_id: 0x00,
                    extra_bits: 0,
                });
            }
        });
    }
}
