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

mod editing;
mod events_passed;
mod ow_tile_picker;
mod reveal_list_editor;
mod se_teleport_editor;
mod secret_exits;
mod sprite_tool;
mod submap_music;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use egui::{
    vec2,
    CentralPanel,
    Color32,
    CornerRadius,
    Frame,
    Key,
    PaintCallback,
    Pos2,
    Rect,
    Sense,
    SidePanel,
    Stroke,
    StrokeKind,
    TopBottomPanel,
    Ui,
    Vec2,
    WidgetText,
};
use egui_glow::CallbackFn;
use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};
use smwe_render::{
    gfx_buffers::GfxBuffers,
    tile_renderer::{Tile, TileRenderer, TileUniforms},
};
use smwe_rom::{
    compression::lc_rle2,
    overworld::{
        secret_exits as ow_secret_exits,
        sprites as ow_sprites,
        L2EventEntry,
        L2EventKind,
        OWL1_TILE_DATA_SIZE,
        OWL1_TILE_DATA_SNES,
        OW_EVENT_COUNT,
        SUBMAP_NAMES,
    },
    snes_utils::addr::{AddrPc, AddrSnes},
    SmwRom,
};

use crate::{
    rom_freespace::find_free_space,
    ui::{
        editing_mode::EditingMode,
        exanimation_dialog::{ExAnimDialog, ExAnimList},
        style::toggle_button,
        tool::DockableEditorTool,
    },
    undo::{Undo, UndoableData},
};

// ── Layout constants ──────────────────────────────────────────────────────────

/// Game pixels per Map16 block (L1 tiles are 16×16 game pixels each).
const MAP16_PX: f32 = 16.0;

/// Visible viewport size used by the editor: 32×32 Map16 blocks = 512×512 pixels.
const SUBMAP_VIEW_X: i32 = 16;
const SUBMAP_VIEW_Y: i32 = 40;
const SUBMAP_VIEW_W: u32 = 224;
const SUBMAP_VIEW_H: u32 = 168;

/// Full BG tilemap size after the game composes the active overworld into VRAM.
const VRAM_TILE_ROWS: u32 = 64;
const VRAM_L1_TILEMAP_BASE: usize = 0x2000 * 2;
const VRAM_L2_TILEMAP_BASE: usize = 0x3000 * 2;

// ── SNES overworld tile-index helpers ─────────────────────────────────────────

const OW_L2_COLS: u32 = 64;

fn tilemap_vram_addr(base: usize, col: u32, row: u32) -> usize {
    let quadrant = ((row / 32) * 2) + (col / 32);
    let sub_row = row % 32;
    let sub_col = col % 32;
    let quadrant_offset = quadrant * 32 * 32 * 2;
    let idx = quadrant_offset + ((sub_row * 32 + sub_col) * 2);
    base + idx as usize
}

fn visible_map_size(submap: u8) -> (u32, u32) {
    if submap == 0 {
        (512, 512)
    } else {
        (SUBMAP_VIEW_W, SUBMAP_VIEW_H)
    }
}

fn visible_map_crop(submap: u8) -> (u32, u32) {
    if submap == 0 {
        (0, 0)
    } else {
        (SUBMAP_VIEW_X as u32, SUBMAP_VIEW_Y as u32)
    }
}

fn l1_vram_addr_for_map16(submap: u8, map16_x: u32, map16_y: u32) -> usize {
    let (crop_x, crop_y) = visible_map_crop(submap);
    let tile_x = (map16_x * 16 + crop_x) / 8;
    let tile_y = (map16_y * 16 + crop_y) / 8;
    tilemap_vram_addr(VRAM_L1_TILEMAP_BASE, tile_x, tile_y)
}

// ── OpenGL renderer ───────────────────────────────────────────────────────────

#[derive(Debug)]
struct OverworldRenderer {
    layer1:    TileRenderer,
    layer2:    TileRenderer,
    gfx_bufs:  GfxBuffers,
    destroyed: bool,
}

impl OverworldRenderer {
    fn new(gl: &glow::Context) -> Self {
        Self {
            layer1:    TileRenderer::new(gl),
            layer2:    TileRenderer::new(gl),
            gfx_bufs:  GfxBuffers::new(gl),
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

    fn set_tiles(&mut self, gl: &glow::Context, l1: Vec<Tile>, l2: Vec<Tile>) {
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

// ── Undoable overworld edit state ─────────────────────────────────────────────

/// The serialization layout is:
/// `[L1 tiles (OWL1_TILE_DATA_SIZE bytes)]`
/// `[u32 LE layer-2 word count][L2 words as LE u16 pairs]`
/// `[13 vanilla sprite records × 5 bytes][11 visibility bytes]`
/// `[u8 foreign-custom-table flag]`
/// `[u32 LE custom payload length][custom sprite RATS payload]`
/// `[128 extra-byte counts]`
/// `[u32 LE secret-exit entry count][entries × 4 bytes: level u16 LE, exit2, exit3]`
/// `[22 reveal-list before bytes][22 reveal-list after bytes]`
/// `[22 start-position table bytes]`
///
/// L1 is always exactly OWL1_TILE_DATA_SIZE bytes so `from_bytes` can split
/// correctly; everything after it is length-prefixed.
#[derive(Clone)]
pub(super) struct OverworldEditState {
    pub layer1_tiles:         Vec<u8>,
    pub layer2_words:         Vec<u16>,
    pub vanilla_sprites:      smwe_rom::overworld::sprites::VanillaOwSprites,
    pub custom_sprites:       smwe_rom::overworld::sprites::CustomSpriteTable,
    /// The ROM's custom-sprite pointer aimed at a table smw-editor did not
    /// author (e.g. LM's). Custom sprites then can't be saved safely.
    pub foreign_custom_table: bool,
    /// Extra-byte counts in force (derived from the sprite record-size
    /// table when one exists, otherwise [`DEFAULT_EXTRA_BYTES`]).
    ///
    /// Invariant (maintained by the sprite tool): every custom sprite's
    /// `extra.len()` equals `custom_extra_counts[number]`. The undo payload
    /// encodes/decodes extra bytes with these counts, so violating it would
    /// corrupt extra bytes across undo/redo. Changing the size table (via
    /// the size-table dialog) recomputes the counts and resizes every
    /// sprite's extra bytes to match, as one undo step.
    pub custom_extra_counts:  [u8; 128],
    /// The ROM's custom overworld sprite record-size table (LM v3.51+),
    /// `None` when the ROM has none. Edited via the size-table dialog;
    /// created on save when the user applies sizes to a ROM without one.
    pub sprite_size_table:    Option<ow_sprites::SpriteSizeTable>,
    /// LM v3.00 Secret Exit 2/3 direction-to-enable settings (editor-owned
    /// `SMWSEXIT` RATS block; see `smwe_rom::overworld::secret_exits`).
    pub secret_exits:         smwe_rom::overworld::secret_exits::SecretExitSettings,
    /// The 22 before/after reveal-tile pairs (`$04DA1D`/`$04DA33`, LM v2.30
    /// "Edit Reveal Tile List"). Undoable like everything else in this state.
    pub reveal_list:          smwe_rom::overworld::reveal_list::RevealTileList,
    /// Mario's and Luigi's overworld starting positions (`$009EF0`, LM
    /// v1.60/v1.90). Undoable like everything else in this state.
    pub start_positions:      smwe_rom::overworld::start_positions::OverworldStartPositions,
    /// Per-submap overworld music (`$048D8A` + mirror `$04DBC8`, LM v1.30
    /// "Change Overworld Music"). Undoable like everything else in this state.
    pub submap_music:         smwe_rom::overworld::submap_music::SubmapMusic,
}

impl Undo for OverworldEditState {
    fn from_bytes(bytes: Vec<u8>) -> Self {
        let mut pos = 0usize;
        let take = |pos: &mut usize, n: usize| -> Vec<u8> {
            let end = (*pos + n).min(bytes.len());
            let chunk = bytes[*pos..end].to_vec();
            *pos = end;
            chunk
        };
        let l1 = take(&mut pos, OWL1_TILE_DATA_SIZE);
        let l2_count = u32::from_le_bytes(take(&mut pos, 4).try_into().unwrap_or([0; 4])) as usize;
        let l2_bytes = take(&mut pos, l2_count * 2);
        let layer2_words = l2_bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();

        let sprite_bytes = take(&mut pos, ow_sprites::VANILLA_SPRITE_COUNT * ow_sprites::VANILLA_SPRITE_RECORD_LEN);
        let mut sprites = [ow_sprites::VanillaOwSprite { number: 0, x: 0, y: 0 }; ow_sprites::VANILLA_SPRITE_COUNT];
        for (i, slot) in sprites.iter_mut().enumerate() {
            let off = i * ow_sprites::VANILLA_SPRITE_RECORD_LEN;
            if let Some(rec) = sprite_bytes.get(off..off + ow_sprites::VANILLA_SPRITE_RECORD_LEN) {
                slot.number = rec[0];
                slot.x = u16::from_le_bytes([rec[1], rec[2]]);
                slot.y = u16::from_le_bytes([rec[3], rec[4]]);
            }
        }
        let vis_bytes = take(&mut pos, ow_sprites::VISIBILITY_COUNT);
        let mut visibility = [0u8; ow_sprites::VISIBILITY_COUNT];
        for (i, b) in visibility.iter_mut().enumerate() {
            *b = vis_bytes.get(i).copied().unwrap_or(0);
        }
        let foreign_custom_table = take(&mut pos, 1).first().copied().unwrap_or(0) != 0;
        let payload_len = u32::from_le_bytes(take(&mut pos, 4).try_into().unwrap_or([0; 4])) as usize;
        let payload = take(&mut pos, payload_len);
        let counts_bytes = take(&mut pos, 128);
        let mut custom_extra_counts = [ow_sprites::DEFAULT_EXTRA_BYTES as u8; 128];
        for (i, b) in custom_extra_counts.iter_mut().enumerate() {
            *b = counts_bytes.get(i).copied().unwrap_or(ow_sprites::DEFAULT_EXTRA_BYTES as u8);
        }
        let custom_sprites =
            ow_sprites::CustomSpriteTable::decode_payload(&payload, &custom_extra_counts).unwrap_or_default();
        let size_flag = take(&mut pos, 1).first().copied().unwrap_or(0);
        let sprite_size_table = if size_flag != 0 {
            let size_bytes = take(&mut pos, ow_sprites::SIZE_TABLE_LEN);
            let mut sizes = [ow_sprites::DEFAULT_SPRITE_RECORD_SIZE; ow_sprites::SIZE_TABLE_LEN];
            for (i, b) in sizes.iter_mut().enumerate() {
                *b = size_bytes.get(i).copied().unwrap_or(ow_sprites::DEFAULT_SPRITE_RECORD_SIZE);
            }
            Some(ow_sprites::SpriteSizeTable { sizes })
        } else {
            None
        };
        let se_count = u32::from_le_bytes(take(&mut pos, 4).try_into().unwrap_or([0; 4])) as usize;
        let mut secret_exits = ow_secret_exits::SecretExitSettings::default();
        for _ in 0..se_count {
            if let Some(rec) = take(&mut pos, 4).get(..4) {
                secret_exits.set(ow_secret_exits::SecretExitEntry {
                    level: u16::from_le_bytes([rec[0], rec[1]]),
                    exit2: rec[2],
                    exit3: rec[3],
                });
            } else {
                break;
            }
        }
        let reveal_before = take(&mut pos, smwe_rom::overworld::reveal_list::REVEAL_COUNT);
        let reveal_after = take(&mut pos, smwe_rom::overworld::reveal_list::REVEAL_COUNT);
        let reveal_list = if reveal_before.len() == smwe_rom::overworld::reveal_list::REVEAL_COUNT
            && reveal_after.len() == smwe_rom::overworld::reveal_list::REVEAL_COUNT
        {
            smwe_rom::overworld::reveal_list::RevealTileList { before: reveal_before, after: reveal_after }
        } else {
            // Payloads written before the reveal list was undoable predate
            // this tail; fall back to empty lists rather than failing the
            // undo (can't happen for payloads this build writes).
            smwe_rom::overworld::reveal_list::RevealTileList { before: Vec::new(), after: Vec::new() }
        };
        let start_bytes = take(&mut pos, smwe_rom::overworld::start_positions::START_POSITIONS_LEN);
        let start_positions = if start_bytes.len() == smwe_rom::overworld::start_positions::START_POSITIONS_LEN {
            smwe_rom::overworld::start_positions::OverworldStartPositions::decode(&start_bytes)
        } else {
            smwe_rom::overworld::start_positions::OverworldStartPositions::decode(
                &[0u8; smwe_rom::overworld::start_positions::START_POSITIONS_LEN],
            )
        };
        let music_bytes = take(&mut pos, smwe_rom::overworld::submap_music::SUBMAP_MUSIC_LEN);
        let submap_music = if music_bytes.len() == smwe_rom::overworld::submap_music::SUBMAP_MUSIC_LEN {
            let mut tracks = [0u8; smwe_rom::overworld::submap_music::SUBMAP_MUSIC_LEN];
            tracks.copy_from_slice(&music_bytes);
            smwe_rom::overworld::submap_music::SubmapMusic { tracks }
        } else {
            // Payloads written before submap music was undoable predate this
            // tail; fall back to the vanilla table (can't happen for payloads
            // this build writes).
            smwe_rom::overworld::submap_music::SubmapMusic {
                tracks: smwe_rom::overworld::submap_music::SubmapMusic::VANILLA,
            }
        };
        Self {
            layer1_tiles: l1,
            layer2_words,
            vanilla_sprites: ow_sprites::VanillaOwSprites { sprites, visibility },
            custom_sprites,
            foreign_custom_table,
            custom_extra_counts,
            sprite_size_table,
            secret_exits,
            reveal_list,
            start_positions,
            submap_music,
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.size_bytes());
        out.extend_from_slice(&self.layer1_tiles);
        out.extend_from_slice(&(self.layer2_words.len() as u32).to_le_bytes());
        for &w in &self.layer2_words {
            out.extend_from_slice(&w.to_le_bytes());
        }
        for sprite in &self.vanilla_sprites.sprites {
            out.push(sprite.number);
            out.extend_from_slice(&sprite.x.to_le_bytes());
            out.extend_from_slice(&sprite.y.to_le_bytes());
        }
        out.extend_from_slice(&self.vanilla_sprites.visibility);
        out.push(u8::from(self.foreign_custom_table));
        let payload = self.custom_sprites.encode_payload(&self.custom_extra_counts);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&payload);
        out.extend_from_slice(&self.custom_extra_counts);
        match &self.sprite_size_table {
            Some(table) => {
                out.push(1);
                out.extend_from_slice(&table.sizes);
            }
            None => out.push(0),
        }
        out.extend_from_slice(&(self.secret_exits.entries.len() as u32).to_le_bytes());
        for e in &self.secret_exits.entries {
            out.extend_from_slice(&e.level.to_le_bytes());
            out.push(e.exit2);
            out.push(e.exit3);
        }
        out.extend_from_slice(&self.reveal_list.before);
        out.extend_from_slice(&self.reveal_list.after);
        out.extend_from_slice(&self.start_positions.encode());
        out.extend_from_slice(&self.submap_music.encode());
        out
    }

    fn size_bytes(&self) -> usize {
        self.layer1_tiles.len()
            + 4
            + self.layer2_words.len() * 2
            + ow_sprites::VANILLA_SPRITE_COUNT * ow_sprites::VANILLA_SPRITE_RECORD_LEN
            + ow_sprites::VISIBILITY_COUNT
            + 1
            + 4
            + self.custom_sprites.encode_payload(&self.custom_extra_counts).len()
            + 128
            + 1
            + self.sprite_size_table.map(|_| ow_sprites::SIZE_TABLE_LEN).unwrap_or(0)
            + 4
            + self.secret_exits.entries.len() * 4
            + smwe_rom::overworld::reveal_list::REVEAL_COUNT * 2
            + smwe_rom::overworld::start_positions::START_POSITIONS_LEN
            + smwe_rom::overworld::submap_music::SUBMAP_MUSIC_LEN
    }
}

// ── Overworld sprite tool ─────────────────────────────────────────────────────

/// Reference to a sprite editable in the overworld sprite tool: either a
/// fixed vanilla table slot or a custom sprite on a submap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OwSpriteRef {
    Vanilla(usize),
    Custom { submap: usize, index: usize },
}

// ── Editor ────────────────────────────────────────────────────────────────────

pub struct UiWorldEditor {
    gl:       Arc<glow::Context>,
    #[allow(dead_code)]
    rom:      Arc<SmwRom>,
    cpu:      Cpu,
    renderer: Arc<Mutex<OverworldRenderer>>,

    submap: u8,

    offset:         Vec2,
    zoom:           f32,
    show_grid:      bool,
    show_layer1:    bool,
    show_layer2:    bool,
    selected_tile:  Option<(u32, u32)>,
    /// Clipboard region selection (x0, y0, x1, y1 inclusive, map16-tile
    /// coords) for layer-1 copy/paste — LM v2.30 overworld clipboard flow.
    /// Set by Shift+drag in Select mode on layer 1.
    ow_sel_rect:    Option<(u32, u32, u32, u32)>,
    /// Shift+drag anchor while a region selection is being drawn.
    ow_drag_anchor: Option<(u32, u32)>,
    /// Copy origin for pastes when the pointer isn't over the canvas.
    ow_copy_origin: Option<(u32, u32)>,
    needs_center:   bool,

    // Editing state
    editing_mode:           EditingMode,
    draw_tile_num:          u8,
    draw_palette:           u8,
    draw_tile_attr:         u8,
    tile_picker:            ow_tile_picker::OwTilePicker,
    l1_tile_picker:         ow_tile_picker::OwL1TilePicker,
    edit_layer:             u8, // 1 or 2
    preview_texture:        Option<egui::TextureHandle>,
    preview_for:            Option<(u32, u32)>,
    has_edits:              bool,
    has_unsavable_changes:  bool,
    /// Lunar Magic v3.00 "Insert all GFX and ExGFX then reload" toolbar
    /// button: set by the toolbar, consumed by the app via
    /// `take_insert_all_gfx_request`. The overworld editor authors no GFX
    /// itself; the app merges every tab's staged GFX/ExGFX edits into the
    /// ROM image and this tab re-uploads the overworld graphics from it.
    request_insert_all_gfx: bool,
    /// Status line from the last insert-all-GFX run, shown in the toolbar.
    insert_gfx_status:      Option<String>,
    pub(super) edit_state:  UndoableData<OverworldEditState>,

    /// Per-event (0..smwe_rom::overworld::OW_EVENT_COUNT) preview toggle: whether
    /// this "destruction" event (castle/fortress/switch palace beaten, etc.) is
    /// considered active for preview purposes. Defaults to all-on, matching the
    /// previous blanket "activate everything" behavior.
    active_events:             Vec<bool>,
    /// Whether the Layer 2 event target markers are drawn over the map.
    show_l2_event_markers:     bool,
    /// "Change Events Passed…" dialog (Lunar Magic overworld dialog: Edit menu
    /// since v3.61, toolbar button added in v3.70): open flag. Preview-only —
    /// nothing in the dialog is ever written to the ROM.
    show_change_events_passed: bool,
    /// Current event number for the "Change Events Passed" preview dialog
    /// (0..OW_EVENT_COUNT). Dialog bookkeeping from LM's testing-only dialog:
    /// changing it jumps the dialog's event list to that event; the
    /// passed-events checkboxes (shared with the Events panel) are what drive
    /// the preview, through the game's $1F02–$1F60 passed-events bits.
    preview_current_event:     u8,
    /// Pending jump-to-event for the dialog's checklist, set when the current
    /// event spinner changes; consumed by `events_passed_window`.
    events_passed_scroll_to:   Option<usize>,
    /// "Special World Passed" view (Lunar Magic v1.10 View-menu item):
    /// preview flag. When set, the game's beaten-Special-World bits are
    /// written to WRAM before `load_overworld` runs, so the real
    /// `CODE_00AD25` / `UploadGFXFile` code produces the autumn overworld
    /// palettes and post-Special-World koopa graphics exactly as the game
    /// does. Preview-only — nothing is ever written to the ROM.
    special_world_passed:      bool,

    /// Per-tile (index into `layer1_tiles`) level-number overrides. Absent
    /// entries use the vanilla scan-order-derived level number unchanged.
    /// Applying these requires patching a single ROM instruction operand to
    /// read a custom table instead of the WRAM-computed one — see
    /// `smwe_rom::overworld::LEVEL_NUMBER_PATCH_OPERAND_SNES` for why this
    /// doesn't need new ASM code, just different data.
    custom_level_numbers:   HashMap<usize, u8>,
    level_numbers_dirty:    bool,
    /// Overworld sprite editing tool (LM overworld sprite mode parity):
    /// when true, the canvas shows sprite markers and click/drag edits
    /// sprites instead of tiles.
    ow_sprite_tool:         bool,
    /// Currently selected sprite in the sprite tool.
    ow_sprite_selection:    Option<OwSpriteRef>,
    /// Active sprite drag: the grabbed sprite plus the grab offset in map
    /// pixels (pointer pos minus sprite pos at grab time).
    ow_sprite_drag:         Option<(OwSpriteRef, Vec2)>,
    /// Hex text buffer for the selected custom sprite's extra bytes, synced
    /// on selection change.
    ow_extra_hex:           String,
    /// Last validation error from a sprite field edit, if any.
    ow_sprite_error:        Option<String>,
    /// "Custom Overworld Sprite Record Sizes" dialog (LM v3.51 parity):
    /// open flag plus the draft per-sprite record sizes (127 entries for
    /// sprites 01..7F), synced from the ROM's table on open (defaults when
    /// the ROM has none).
    ow_size_table_open:     bool,
    ow_size_table_draft:    [u8; ow_sprites::SIZE_TABLE_LEN],
    /// Sprite state as parsed at ROM load; compared on save so untouched
    /// sprite data is never rewritten (avoids orphaning RATS blocks). The
    /// third element is the size table as parsed at load.
    sprites_at_load: (ow_sprites::VanillaOwSprites, ow_sprites::CustomSpriteTable, Option<ow_sprites::SpriteSizeTable>),
    /// Whether the Secret Exits 2/3 window is open.
    show_secret_exits:      bool,
    /// Level number selected in the Secret Exits 2/3 window.
    secret_exit_level:      u16,
    /// Secret-exit settings as parsed at ROM load; compared on save so an
    /// untouched ROM keeps no `SMWSEXIT` block.
    secret_exits_at_load:   ow_secret_exits::SecretExitSettings,
    /// Vanilla level names decoded from the ROM (93 entries, index =
    /// translevel). Used as the base for custom name edits.
    vanilla_level_names:    Vec<String>,
    /// Custom level names by translevel. Absent entries use the vanilla name.
    custom_level_names:     HashMap<u8, String>,
    /// True if any level name has been customized (requires the name-table
    /// relocation patch on save).
    level_names_dirty:      bool,
    /// Level-name text field buffer, synced to `level_name_for`.
    level_name_edit:        String,
    /// Translevel the name field (and error) currently belong to.
    level_name_for:         Option<u8>,
    /// Validation error from the last rejected name edit, if any.
    level_name_error:       Option<String>,
    /// Lunar Magic v3.40 custom table file (.lmtbl) for level names;
    /// `None` = the built-in overworld-name tile map.
    level_name_table:       Option<smwe_rom::table_file::Table>,
    /// File name of the loaded level-name table, for display.
    level_name_table_name:  Option<String>,
    /// Last level-name table load status / parse warnings, shown under the
    /// Load button.
    level_name_table_error: Option<String>,
    /// Event-ownership table (`$05D608` events-by-translevel): raw byte per
    /// translevel (`0x00`–`0x5C`), `$FF` = no event. Edited via the
    /// event-ownership panel; written back in place on save.
    event_ownership:        Vec<u8>,
    /// True if any event-ownership assignment has been changed.
    event_ownership_dirty:  bool,
    /// Last time the overworld animated tiles were ticked.
    last_anim_tick:         std::time::Instant,

    // ExAnimation (custom overworld tile/palette animation, LM v2.40 parity).
    exanimation:             smwe_rom::exanimation::ExAnimationData,
    exanimation_dirty:       bool,
    show_exanimation_editor: bool,
    /// Dialog state for the shared "ExAnimated Frames" window.
    exanim_dialog:           ExAnimDialog,
    /// Editor animation tick counter (one per ~133ms animated-tile tick);
    /// drives the overworld ExAnimation preview stepping.
    exanim_tick:             u64,
    // Secondary-exit Star/Pipe teleport table (LM v3.00 parity): 0x100
    // overworld destinations for secondary exits that exit to the overworld.
    // Stored in the editor's RATS block (`SMWESEX2`); the level editor owns
    // the per-entrance options half of that block.
    se_teleports: [smwe_rom::level::secondary_entrance::OwTeleportEntry;
        smwe_rom::level::secondary_entrance::OW_TELEPORT_TABLE_LEN],
    se_teleports_dirty:      bool,
    show_se_teleport_editor: bool,
    se_teleport_search:      String,
    /// LM v2.30 "Edit Reveal Tile List" dialog visibility.
    show_reveal_list_editor: bool,
    /// True once the reveal list has been edited (in-place save on Ctrl+S).
    reveal_list_dirty:       bool,
    /// True once a start position has been edited (in-place save on Ctrl+S).
    start_positions_dirty:   bool,
    /// True once a submap's music has been edited (in-place save on Ctrl+S).
    submap_music_dirty:      bool,
    /// LM v1.30 "Change Overworld Music" dialog visibility.
    show_submap_music:       bool,
    /// Click-to-place arming for the Mario/Luigi start markers on the main
    /// map: while set, the next canvas click in Select mode moves that
    /// player's starting position instead of selecting a tile.
    place_start_target:      Option<StartPlayer>,
    /// Clean post-load VRAM snapshot the ExAnimation tile browser decodes from.
    exanimation_base_vram:   Vec<u8>,
    /// Bumped on every submap load so the tile-browser atlas rebuilds.
    exanim_vram_gen:         u64,
}

/// Which player's overworld starting position an action targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StartPlayer {
    Mario,
    Luigi,
}

impl UiWorldEditor {
    pub fn new(gl: Arc<glow::Context>, rom: Arc<SmwRom>, rom_path: PathBuf) -> Self {
        let renderer = Arc::new(Mutex::new(OverworldRenderer::new(&gl)));

        let raw = std::fs::read(&rom_path).expect("cannot read ROM for emulator");
        let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
        let mut emu_rom = EmuRom::new(rom_bytes);
        emu_rom.load_symbols(include_str!("../../../symbols/SMW_U.sym"));
        let cpu = Cpu::new(CheckedMem::new(Arc::new(emu_rom)));

        let source_layer1_tiles = rom.overworld.layer1_tiles.clone();
        // Decode the vanilla overworld sprite table ($04F625) and the custom
        // sprite table ($0EF55D pointer) before `rom` is moved into the struct.
        let (vanilla_sprites, custom_sprites, foreign_custom_table) = {
            let bytes = rom.rom_bytes();
            let vanilla = ow_sprites::VanillaOwSprites::parse(bytes, 0).unwrap_or_else(|e| {
                log::warn!("Could not parse overworld sprites: {e}");
                ow_sprites::VanillaOwSprites {
                    sprites:    [ow_sprites::VanillaOwSprite { number: 0, x: 0, y: 0 };
                        ow_sprites::VANILLA_SPRITE_COUNT],
                    visibility: [0; ow_sprites::VISIBILITY_COUNT],
                }
            });
            match ow_sprites::parse_custom_table(bytes, 0) {
                Ok(table) => (vanilla, table.unwrap_or_default(), false),
                Err(ow_sprites::SpriteError::ForeignTable) => {
                    log::warn!("ROM has a custom overworld sprite table not authored by smw-editor; leaving it alone");
                    (vanilla, ow_sprites::CustomSpriteTable::default(), true)
                }
                Err(e) => {
                    log::warn!("Could not parse custom overworld sprites: {e}");
                    (vanilla, ow_sprites::CustomSpriteTable::default(), false)
                }
            }
        };
        let custom_extra_counts = ow_sprites::extra_byte_counts(rom.rom_bytes(), 0);
        let sprite_size_table = match ow_sprites::parse_size_table(rom.rom_bytes(), 0) {
            Ok(table) => table,
            Err(e) => {
                log::warn!("Could not parse custom overworld sprite size table: {e}");
                None
            }
        };
        let sprites_at_load = (vanilla_sprites.clone(), custom_sprites.clone(), sprite_size_table);
        // LM v3.00 Secret Exit 2/3 direction settings live in our own RATS
        // block; absence is the normal case (vanilla ROMs have none).
        let secret_exits = ow_secret_exits::parse_secret_exits(rom.rom_bytes(), 0);
        let secret_exits_at_load = secret_exits.clone();
        // Decode the reveal-tile list ($04DA1D/$04DA33) and the Mario/Luigi
        // starting positions ($009EF0) the same way; both are edited in
        // place, so the parsed copies ride in the undoable edit state and
        // are only written back on save when dirty.
        let reveal_list =
            smwe_rom::overworld::reveal_list::RevealTileList::parse(rom.rom_bytes(), 0).unwrap_or_else(|e| {
                log::warn!("Could not parse overworld reveal list: {e}");
                smwe_rom::overworld::reveal_list::RevealTileList { before: Vec::new(), after: Vec::new() }
            });
        let start_positions = smwe_rom::overworld::start_positions::OverworldStartPositions::parse(rom.rom_bytes(), 0)
            .unwrap_or_else(|e| {
                log::warn!("Could not parse overworld start positions: {e}");
                smwe_rom::overworld::start_positions::OverworldStartPositions::decode(
                    &[0u8; smwe_rom::overworld::start_positions::START_POSITIONS_LEN],
                )
            });
        // Per-submap overworld music ($048D8A/$04DBC8, LM v1.30 "Change
        // Overworld Music"); edited in place like the tables above, written
        // back on save only when dirty.
        let submap_music =
            smwe_rom::overworld::submap_music::SubmapMusic::parse(rom.rom_bytes(), 0).unwrap_or_else(|e| {
                log::warn!("Could not parse overworld submap music: {e}");
                smwe_rom::overworld::submap_music::SubmapMusic {
                    tracks: smwe_rom::overworld::submap_music::SubmapMusic::VANILLA,
                }
            });
        let edit_state = UndoableData::new(OverworldEditState {
            layer1_tiles: source_layer1_tiles,
            layer2_words: Vec::new(),
            vanilla_sprites,
            custom_sprites,
            foreign_custom_table,
            custom_extra_counts,
            sprite_size_table,
            secret_exits,
            reveal_list,
            start_positions,
            submap_music,
        });
        // Decode vanilla level names before `rom` is moved into the struct.
        // The MultiChar option controls whether squished tiles decode to
        // their strings ("LL") or to `\XX` hex escapes.
        let use_multichar = crate::editor_options::EditorOptions::load().use_multichar_tiles;
        let vanilla_level_names =
            smwe_rom::overworld::level_names::decode_all(rom.rom_bytes(), 0, false, use_multichar).unwrap_or_default();
        // Decode the vanilla event-ownership table ($05D608) the same way.
        let event_ownership = smwe_rom::overworld::event_ownership::EventOwnership::parse(rom.rom_bytes(), 0)
            .map(|eo| eo.table)
            .unwrap_or_else(|_| {
                vec![
                    smwe_rom::overworld::event_ownership::NO_EVENT;
                    smwe_rom::overworld::event_ownership::EVENT_OWNERSHIP_COUNT
                ]
            });
        // Overworld ExAnimation data rides along on the ROM (RATS block).
        let exanimation = rom.exanimation.clone();
        // Secondary-exit Star/Pipe teleport table rides along on the ROM
        // (same editor RATS block the level editor's exit options use).
        let se_teleports = rom.secondary_exit_ext.teleport_table;
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
            ow_sel_rect: None,
            ow_drag_anchor: None,
            ow_copy_origin: None,
            needs_center: false,
            editing_mode: EditingMode::Select,
            draw_tile_num: 0x00,
            draw_palette: 0,
            draw_tile_attr: 0x00,
            tile_picker: ow_tile_picker::OwTilePicker::new(),
            l1_tile_picker: ow_tile_picker::OwL1TilePicker::new(),
            edit_layer: 1,
            preview_texture: None,
            preview_for: None,
            has_edits: false,
            has_unsavable_changes: false,
            request_insert_all_gfx: false,
            insert_gfx_status: None,
            edit_state,
            active_events: vec![true; smwe_rom::overworld::OW_EVENT_COUNT],
            show_l2_event_markers: true,
            show_change_events_passed: false,
            preview_current_event: 0,
            events_passed_scroll_to: None,
            special_world_passed: false,
            custom_level_numbers: HashMap::new(),
            level_numbers_dirty: false,
            ow_sprite_tool: false,
            ow_sprite_selection: None,
            ow_sprite_drag: None,
            ow_extra_hex: String::new(),
            ow_sprite_error: None,
            ow_size_table_open: false,
            ow_size_table_draft: [ow_sprites::DEFAULT_SPRITE_RECORD_SIZE; ow_sprites::SIZE_TABLE_LEN],
            sprites_at_load,
            show_secret_exits: false,
            secret_exit_level: 0,
            secret_exits_at_load,
            vanilla_level_names,
            custom_level_names: HashMap::new(),
            level_names_dirty: false,
            level_name_edit: String::new(),
            level_name_for: None,
            level_name_error: None,
            level_name_table: None,
            level_name_table_name: None,
            level_name_table_error: None,
            event_ownership,
            event_ownership_dirty: false,
            last_anim_tick: std::time::Instant::now(),
            exanimation,
            exanimation_dirty: false,
            show_exanimation_editor: false,
            exanim_dialog: ExAnimDialog::new(ExAnimList::Overworld),
            exanim_tick: 0,
            exanimation_base_vram: Vec::new(),
            exanim_vram_gen: 0,
            se_teleports,
            se_teleports_dirty: false,
            show_se_teleport_editor: false,
            se_teleport_search: String::new(),
            show_reveal_list_editor: false,
            reveal_list_dirty: false,
            start_positions_dirty: false,
            submap_music_dirty: false,
            show_submap_music: false,
            place_start_target: None,
        };
        editor.load_submap();
        editor
    }

    fn load_submap(&mut self) {
        apply_active_events_to_wram(&mut self.cpu, &self.active_events);
        // "Special World Passed" view (LM v1.10): set the game's beaten-
        // Special-World bits before the init routines run, so the real
        // CODE_00AD25 / UploadGFXFile code produces the autumn palettes and
        // post-Special-World koopa graphics exactly as the game does.
        smwe_emu::emu::special_world::set_special_world_passed(&mut self.cpu, self.special_world_passed);
        smwe_emu::emu::load_overworld(&mut self.cpu, self.submap);
        let mut r = self.renderer.lock().expect("Cannot lock overworld renderer");
        r.upload_palette(&self.gl, &self.cpu.mem.cgram);
        r.upload_gfx(&self.gl, &self.cpu.mem.vram);

        let l2_scroll_x = i16::from_le_bytes(self.cpu.mem.load_u16(0x001E).to_le_bytes()) as i32;
        let l2_scroll_y = i16::from_le_bytes(self.cpu.mem.load_u16(0x0020).to_le_bytes()) as i32;

        let l1 = build_bg_tiles(&self.cpu.mem.vram, VRAM_L1_TILEMAP_BASE, self.submap, l2_scroll_x, l2_scroll_y);
        let l2 = build_bg_tiles(&self.cpu.mem.vram, VRAM_L2_TILEMAP_BASE, self.submap, l2_scroll_x, l2_scroll_y);

        log::info!("Loaded submap {}: L1={} tiles, L2={} tiles", self.submap, l1.len(), l2.len());

        r.set_tiles(&self.gl, l1, l2);
        drop(r); // release the renderer guard before touching the CPU/VRAM again below

        self.tile_picker.rebuild(&self.cpu.mem.vram, &self.cpu.mem.cgram, VRAM_L1_TILEMAP_BASE, VRAM_L2_TILEMAP_BASE);
        self.l1_tile_picker.rebuild(&mut self.cpu);

        self.offset = Vec2::ZERO;
        self.selected_tile = None;
        self.needs_center = true;
        self.has_edits = false;
        self.has_unsavable_changes = false;
        let layer2_words = read_overworld_l2_words(&self.cpu);
        self.edit_state.write(|s| {
            s.layer2_words = layer2_words;
        });
        self.edit_state.clear_stack();

        // Snapshot clean VRAM for the ExAnimation tile browser (it decodes
        // source tiles from the pre-animation graphics), and restart the
        // overworld ExAnimation tick counter.
        self.exanimation_base_vram = self.cpu.mem.vram.clone();
        self.exanim_vram_gen += 1;
        self.exanim_tick = 0;
        self.exanim_dialog.reset_atlas();

        // Re-apply the destruction events with the reveal list currently in
        // the edit state. The emulated init above used the ROM bytes the
        // emulator was constructed with; when the reveal list has unsaved
        // edits, this Rust-side pass is what makes the preview show them.
        // With an unedited list it reproduces the emulated result exactly.
        self.refresh_event_preview();
    }

    /// "Special World Passed" view (LM v1.10) toggle: set the game's beaten-
    /// Special-World bits and re-render through the real game code without
    /// disturbing unsaved edits. Only VRAM's sprite-GFX region and CGRAM can
    /// change (the overworld tilemap, event preview, and all edit state are
    /// untouched), so this re-runs just `UploadSpriteGFX` + `CODE_00AD25`
    /// and re-uploads palettes/graphics to the renderer.
    fn refresh_special_world_view(&mut self) {
        smwe_emu::emu::special_world::set_special_world_passed(&mut self.cpu, self.special_world_passed);
        smwe_emu::emu::special_world::refresh_overworld_special_world(&mut self.cpu);

        let r = self.renderer.lock().expect("Cannot lock overworld renderer");
        r.upload_palette(&self.gl, &self.cpu.mem.cgram);
        r.upload_gfx(&self.gl, &self.cpu.mem.vram);
        drop(r);

        self.tile_picker.rebuild(&self.cpu.mem.vram, &self.cpu.mem.cgram, VRAM_L1_TILEMAP_BASE, VRAM_L2_TILEMAP_BASE);
        self.l1_tile_picker.rebuild(&mut self.cpu);
        self.exanimation_base_vram = self.cpu.mem.vram.clone();
        self.exanim_vram_gen += 1;
        self.exanim_dialog.reset_atlas();
    }
}

impl DockableEditorTool for UiWorldEditor {
    fn title(&self) -> WidgetText {
        "World Map Editor".into()
    }

    fn update(&mut self, ui: &mut Ui) {
        // Slim top toolbar (LM v3.70 added a "Change Events Passed" button to
        // the overworld editor's toolbar; this tab has no other toolbar, so
        // the strip holds just that button for now).
        TopBottomPanel::top("world_editor.toolbar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Change Events Passed…").clicked() {
                    self.show_change_events_passed = true;
                }
                // Lunar Magic v3.00: toolbar button that inserts all GFX and
                // ExGFX then reloads the graphics. The app merges every tab's
                // staged GFX/ExGFX edits into the ROM image and each tab
                // re-uploads its graphics; nothing is written to disk (the
                // next save persists the staged edits as usual).
                if ui
                    .button("Insert all GFX and ExGFX then reload")
                    .on_hover_text("Insert all GFX and ExGFX then reload graphics (LM v3.00)")
                    .clicked()
                {
                    self.request_insert_all_gfx = true;
                }
                // LM v1.10 View-menu item: preview the overworld as it looks
                // after Special World is beaten (autumn palettes + koopa GFX).
                if ui.checkbox(&mut self.special_world_passed, "Special World Passed").changed() {
                    self.refresh_special_world_view();
                }
                if let Some(status) = &self.insert_gfx_status {
                    ui.separator();
                    ui.label(egui::RichText::new(status).small().color(egui::Color32::LIGHT_GREEN));
                }
            });
        });
        SidePanel::left("world_editor.left_panel").resizable(false).show_inside(ui, |ui| self.left_panel(ui));
        CentralPanel::default().frame(Frame::NONE.inner_margin(0.)).show_inside(ui, |ui| self.central_panel(ui));
        if self.show_exanimation_editor {
            let mut open = self.show_exanimation_editor;
            let changed = self.exanim_dialog.show(
                ui.ctx(),
                &mut open,
                &mut self.exanimation,
                &self.exanimation_base_vram,
                &self.cpu.mem.cgram,
                self.exanim_vram_gen,
                None, // single overworld list: no Level/Global tabs
            );
            self.show_exanimation_editor = open;
            if !open {
                self.exanim_dialog.disarm_select();
            }
            if changed {
                self.exanimation_dirty = true;
                self.has_edits = true;
            }
        }
        self.se_teleport_editor_window(ui.ctx());
        self.secret_exits_window(ui.ctx());
        self.reveal_list_editor_window(ui.ctx());
        self.submap_music_window(ui.ctx());
        self.events_passed_window(ui.ctx());
    }

    fn on_closed(&mut self) {
        self.renderer.lock().expect("Cannot lock overworld renderer").destroy(&self.gl);
    }

    fn has_unsaved_changes(&self) -> bool {
        self.has_edits
    }

    fn take_insert_all_gfx_request(&mut self) -> bool {
        std::mem::take(&mut self.request_insert_all_gfx)
    }

    /// Lunar Magic v3.00 "Insert all GFX and ExGFX then reload": the merged
    /// `rom_bytes` already contain every staged GFX/ExGFX edit (written by
    /// the level editor's `save_to_rom` GFX sections during the app's
    /// merge). Swap the emulator cart to the new image and force the game
    /// to re-upload the overworld's graphics slots — the same 8 files
    /// `load_overworld` uploads (verified byte-exact) — then refresh the
    /// renderer + tile pickers. WRAM tilemaps, CGRAM, and all unsaved
    /// overworld edits are untouched.
    fn reload_graphics_from_rom(&mut self, rom_bytes: &[u8]) -> Option<String> {
        let body = if rom_bytes.len() % 0x400 == 0x200 { &rom_bytes[0x200..] } else { rom_bytes };
        let mut emu_rom = EmuRom::new(body.to_vec());
        emu_rom.load_symbols(include_str!("../../../symbols/SMW_U.sym"));
        self.cpu.mem.cart = Arc::new(emu_rom);

        smwe_emu::emu::reload_overworld_graphics(&mut self.cpu);

        {
            let r = self.renderer.lock().expect("Cannot lock overworld renderer");
            r.upload_gfx(&self.gl, &self.cpu.mem.vram);
        }
        self.tile_picker.rebuild(&self.cpu.mem.vram, &self.cpu.mem.cgram, VRAM_L1_TILEMAP_BASE, VRAM_L2_TILEMAP_BASE);
        self.l1_tile_picker.rebuild(&mut self.cpu);
        self.exanimation_base_vram = self.cpu.mem.vram.clone();
        self.exanim_vram_gen += 1;
        self.exanim_dialog.reset_atlas();

        let status =
            "Inserted all GFX and ExGFX into the ROM image; overworld graphics reloaded (LM v3.00).".to_owned();
        self.insert_gfx_status = Some(status.clone());
        log::info!("{status}");
        Some(status)
    }

    fn on_save_succeeded(&mut self) {
        self.has_edits = false;
        self.event_ownership_dirty = false;
        self.exanimation_dirty = false;
        self.se_teleports_dirty = false;
        self.reveal_list_dirty = false;
        self.start_positions_dirty = false;
        self.submap_music_dirty = false;
        // The ROM now matches the edit state; future saves skip rewrites.
        self.sprites_at_load =
            self.edit_state.read(|s| (s.vanilla_sprites.clone(), s.custom_sprites.clone(), s.sprite_size_table));
        self.secret_exits_at_load = self.edit_state.read(|s| s.secret_exits.clone());
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
        self.edit_state.read(|s| dst.copy_from_slice(&s.layer1_tiles));

        let (tile_compressed, attr_compressed, l2_len) = self.edit_state.read(|s| {
            let tile_stream: Vec<u8> = s.layer2_words.iter().map(|w| (*w & 0x00FF) as u8).collect();
            let attr_stream: Vec<u8> = s.layer2_words.iter().map(|w| (*w >> 8) as u8).collect();
            (lc_rle2::compress_pass(&tile_stream), lc_rle2::compress_pass(&attr_stream), s.layer2_words.len())
        });

        write_overworld_l2_stream(
            rom_bytes,
            has_smc_header,
            AddrPc::try_from_lorom(AddrSnes(0x04A533))?.as_index(),
            l2_len,
            &tile_compressed,
            "OWTileNumbers",
        )?;
        write_overworld_l2_stream(
            rom_bytes,
            has_smc_header,
            AddrPc::try_from_lorom(AddrSnes(0x04C02B))?.as_index(),
            l2_len,
            &attr_compressed,
            "OWTilemap",
        )?;

        // ── Custom per-tile level-number assignment ─────────────────────────
        // Only touches the ROM if the user has actually overridden a level
        // number: leaving this alone keeps overworld behavior byte-for-byte
        // vanilla (translevel/scan-order-derived) for hacks that don't use it.
        //
        // Known limitation: each save that has overrides allocates a fresh
        // freespace table rather than reusing/growing a previously-patched
        // one, so repeated saves with active overrides accumulate small
        // (0x800-byte) orphaned regions in ROM. Harmless but wasteful; a
        // follow-up could detect and reuse an already-owned table in place.
        if !self.custom_level_numbers.is_empty() {
            let tiles = self.edit_state.read(|s| s.layer1_tiles.clone());
            let mut table = vec![0u8; tiles.len()];
            for (idx, slot) in table.iter_mut().enumerate() {
                if let Some(vanilla_level_num) = smwe_rom::overworld::level_number_for_index(&tiles, idx) {
                    let effective = self.custom_level_numbers.get(&idx).copied().unwrap_or(vanilla_level_num);
                    *slot = smwe_rom::overworld::encode_custom_level_number(effective).ok_or_else(|| {
                        anyhow::anyhow!(
                            "Level number {effective:#04X} at tile index {idx} exceeds the maximum \
                             assignable value ({:#04X})",
                            smwe_rom::overworld::MAX_ASSIGNABLE_LEVEL_NUMBER
                        )
                    })?;
                }
            }

            let table_pc = find_free_space(rom_bytes, table.len(), 0x008000, header_offset).ok_or_else(|| {
                anyhow::anyhow!("No free space for the custom level-number table ({} bytes)", table.len())
            })?;
            rom_bytes[table_pc + header_offset..table_pc + header_offset + table.len()].copy_from_slice(&table);

            let table_snes = AddrSnes::try_from_lorom(AddrPc(table_pc as u32))?;
            let patch_pc = AddrPc::try_from_lorom(smwe_rom::overworld::LEVEL_NUMBER_PATCH_OPERAND_SNES)?.as_index()
                + header_offset;
            let bytes = table_snes.0.to_le_bytes();
            rom_bytes[patch_pc..patch_pc + 3].copy_from_slice(&bytes[..3]);
        }

        // ── Custom level names ──────────────────────────────────────────────
        // Encodes all 93 names (vanilla + overrides) into the relocated
        // fragment tables and applies the patch. Only touches the ROM if the
        // user has actually customized a name.
        if !self.custom_level_names.is_empty() {
            use smwe_rom::overworld::level_names as ln;
            // Start from vanilla names decoded at load; apply overrides.
            let mut names = self.vanilla_level_names.clone();
            // Ensure 93 entries (in case decode failed at load).
            names.resize(ln::LEVEL_NAMES_COUNT, String::new());
            for (&translevel, custom) in &self.custom_level_names {
                if (translevel as usize) < names.len() {
                    names[translevel as usize] = custom.clone();
                }
            }
            let use_multichar = crate::editor_options::EditorOptions::load().use_multichar_tiles;
            let encoded = match &self.level_name_table {
                // Lunar Magic v3.40 custom table file: the display strings
                // were validated through the table, so encode them with it.
                Some(t) => ln::encode_names_with_table(&names, t),
                None => ln::encode_names(&names, use_multichar),
            }
            .map_err(|e| anyhow::anyhow!("Cannot encode level names: {e}"))?;
            let header_offset = usize::from(has_smc_header) * 0x200;
            ln::apply_to_rom(rom_bytes, header_offset, &encoded)
                .map_err(|e| anyhow::anyhow!("Cannot apply level-name patch: {e}"))?;
        }

        // ── Event ownership (which event each level triggers) ────────────────
        // In-place write of the 93-byte `$05D608` table; only touches the ROM
        // if the user actually changed an assignment.
        if self.event_ownership_dirty {
            use smwe_rom::overworld::event_ownership as eo;
            let ownership = eo::EventOwnership { table: self.event_ownership.clone() };
            ownership
                .apply_to_rom(rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("Cannot apply event ownership edits: {e}"))?;
        }

        // ── Reveal tile list (LM v2.30 "Edit Reveal Tile List") ──────────────
        // In-place write of the 22 before/after byte pairs
        // (`$04DA1D`/`$04DA33`); only touches the ROM if the user actually
        // changed a pair.
        if self.reveal_list_dirty {
            let list = self.edit_state.read(|s| s.reveal_list.clone());
            list.apply_to_rom(rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("Cannot apply reveal list edits: {e}"))?;
        }

        // ── Mario/Luigi starting positions (LM v1.60/v1.90) ─────────────────
        // In-place write of the 22-byte `$009EF0` table; only touches the ROM
        // if the user actually moved a start position.
        if self.start_positions_dirty {
            let pos = self.edit_state.read(|s| s.start_positions);
            pos.apply_to_rom(rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("Cannot apply start position edits: {e}"))?;
        }

        // ── Overworld submap music (LM v1.30 "Change Overworld Music") ────
        // In-place write of the two 7-byte tables `$048D8A` (overworld init)
        // and `$04DBC8` (submap swap), always together so the mirrors can't
        // desync; only touches the ROM if the user actually changed a track.
        if self.submap_music_dirty {
            let music = self.edit_state.read(|s| s.submap_music);
            music
                .apply_to_rom(rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("Cannot apply submap music edits: {e}"))?;
        }

        // ── Overworld ExAnimation (custom tile/palette animation) ────────────
        // Single RATS-tagged free-space block, shared with the level/global
        // lists; erased and reallocated on every save that touched it.
        // Merge on save: the level editor owns the per-level/global lists and
        // may have saved newer ones since this tab loaded, so re-read the
        // block and replace only the overworld list instead of writing this
        // tab's (possibly stale) copies of the other lists.
        if self.exanimation_dirty {
            use smwe_rom::exanimation::{ExAnimError, ExAnimationData};
            let mut merged = match ExAnimationData::parse(rom_bytes) {
                Ok(data) => data,
                Err(ExAnimError::NotFound) => ExAnimationData::default(),
                Err(e) => anyhow::bail!("Overworld ExAnimation read failed: {e}"),
            };
            merged.overworld = self.exanimation.overworld.clone();
            merged
                .write_to_rom(rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("Overworld ExAnimation write failed: {e}"))?;
        }

        // ── Secondary-exit Star/Pipe teleport table (LM v3.00) ───────────────
        // Same editor RATS block the level editor's per-entrance options use.
        // Merge on save: re-read the block and replace only the teleport
        // table so this tab's (possibly stale) copy of the exit options is
        // never clobbered.
        if self.se_teleports_dirty {
            use smwe_rom::level::secondary_entrance::{SecExitExtError, SecondaryExitExtData};
            let mut merged = match SecondaryExitExtData::parse(rom_bytes) {
                Ok(data) => data,
                Err(SecExitExtError::NotFound) => SecondaryExitExtData::default(),
                Err(e) => anyhow::bail!("Secondary-exit teleport-table read failed: {e}"),
            };
            merged.teleport_table = self.se_teleports;
            merged
                .write_to_rom(rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("Secondary-exit teleport-table write failed: {e}"))?;
        }

        // ── Overworld sprites (vanilla + custom) ──────────────────────────────
        // Vanilla records and visibility bytes are fixed-location in-place
        // writes; the custom table is a RATS-tagged free-space block behind
        // the `$0EF55D` pointer. Untouched sprite data is never rewritten, so
        // repeated saves don't orphan RATS blocks.
        let (vanilla, custom, size_table) =
            self.edit_state.read(|s| (s.vanilla_sprites.clone(), s.custom_sprites.clone(), s.sprite_size_table));
        if vanilla != self.sprites_at_load.0 {
            vanilla
                .write(rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("Cannot write overworld sprites: {e}"))?;
        }
        // The size table goes first: `write_custom_table` re-derives the
        // extra-byte counts from the ROM, so the on-disk table must already
        // reflect the edit state's sizes before the custom records are
        // re-encoded.
        if size_table != self.sprites_at_load.2 {
            match size_table {
                Some(table) => match ow_sprites::write_size_table(&table, rom_bytes, header_offset) {
                    Ok(()) => {}
                    Err(ow_sprites::SpriteError::NoSizeTable) => {
                        // New table: allocate a RATS-tagged block, point
                        // $0DE18C at it, set the $42 marker.
                        ow_sprites::create_size_table(&table, rom_bytes, header_offset)
                            .map_err(|e| anyhow::anyhow!("Cannot create sprite size table: {e}"))?;
                    }
                    Err(e) => return Err(anyhow::anyhow!("Cannot write sprite size table: {e}")),
                },
                None => {}
            }
        }
        if custom != self.sprites_at_load.1 {
            ow_sprites::write_custom_table(&custom, rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("Cannot write custom overworld sprites: {e}"))?;
        }

        // ── LM v3.00 Secret Exit 2/3 direction settings ──────────────────
        // Editor-owned `SMWSEXIT` RATS block; untouched ROMs keep no block.
        let secret_exits = self.edit_state.read(|s| s.secret_exits.clone());
        if secret_exits != self.secret_exits_at_load {
            ow_secret_exits::write_secret_exits(&secret_exits, rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("Cannot write secret-exit settings: {e}"))?;
        }

        Ok(())
    }
}

// ── UI ────────────────────────────────────────────────────────────────────────

impl UiWorldEditor {
    fn source_l1_offset(&self) -> usize {
        if self.submap == 0 {
            0
        } else {
            0x400
        }
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

    pub(super) fn source_l1_tile_at_view(&self, map16_x: u32, map16_y: u32) -> Option<u8> {
        let idx = self.source_l1_index_for_view(map16_x, map16_y)?;
        self.edit_state.read(|s| s.layer1_tiles.get(idx).copied())
    }

    pub(super) fn set_source_l1_tile_at_view(&mut self, map16_x: u32, map16_y: u32, tile_id: u8) {
        let Some(idx) = self.source_l1_index_for_view(map16_x, map16_y) else {
            return;
        };
        self.edit_state.write(|s| {
            if let Some(slot) = s.layer1_tiles.get_mut(idx) {
                *slot = tile_id;
            }
        });
        self.has_edits = true;
        self.apply_source_l1_tile_to_vram(map16_x, map16_y, tile_id);
    }

    fn apply_source_l1_tile_to_vram(&mut self, map16_x: u32, map16_y: u32, tile_id: u8) {
        let (crop_x, crop_y) = visible_map_crop(self.submap);
        let src_col = (map16_x * 16 + crop_x) / 16;
        let src_row = (map16_y * 16 + crop_y) / 16;
        self.write_source_l1_block_words(src_col, src_row, tile_id);
    }

    pub(super) fn write_source_l1_block_words(&mut self, src_col: u32, src_row: u32, tile_id: u8) {
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

    /// Per-event ("destruction event": castle/fortress/switch palace beaten,
    /// etc.) preview toggles. Toggling any checkbox reloads the current submap
    /// with the new `OWEventsActivated` bits, so the real emulated game code
    /// applies (or doesn't apply) that event's reveal-tile swap.
    fn events_panel(&mut self, ui: &mut Ui) {
        // Full dialog version of the checklist below (LM Edit-menu dialog;
        // toolbar button added in LM v3.70).
        if ui.button("Change Events Passed…").clicked() {
            self.show_change_events_passed = true;
        }
        ui.small("Current event + passed-events checklist for previewing event tiles.");
        ui.collapsing("Events (preview)", |ui| {
            ui.horizontal(|ui| {
                if ui.button("All on").clicked() {
                    self.active_events.iter_mut().for_each(|e| *e = true);
                    self.load_submap();
                }
                if ui.button("All off").clicked() {
                    self.active_events.iter_mut().for_each(|e| *e = false);
                    self.load_submap();
                }
            });
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                let mut changed = false;
                for (i, active) in self.active_events.iter_mut().enumerate() {
                    let offset = self.rom.overworld_events.tile_offsets.get(i).copied().unwrap_or(0);
                    if offset == 0 {
                        continue; // unused event slot
                    }
                    changed |= ui.checkbox(active, format!("Event {i:3} (tile offset {offset:#06X})")).changed();
                }
                if changed {
                    self.load_submap();
                }
            });
        });

        ui.collapsing("Layer 2 events", |ui| {
            ui.checkbox(&mut self.show_l2_event_markers, "Show target markers on map");
            let l2 = &self.rom.overworld_l2_events;
            let events_with_l2: Vec<usize> = (0..OW_EVENT_COUNT)
                .filter(|&e| {
                    !l2.entries_for_event(e).unwrap_or(0..0).is_empty() || !l2.silent_l2_events_for(e as u8).is_empty()
                })
                .collect();
            ui.label(format!(
                "{} table entries · {} events with L2 data · {} silent L2 rows",
                l2.entry_count(),
                events_with_l2.len(),
                l2.silent_events.iter().filter(|s| s.is_l2).count(),
            ));
            ui.label("Markers follow the event checkboxes above. The animated L2 sequence itself runs in-game;");
            ui.label("this panel shows where each event's Layer 2 tiles land.");
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for event in events_with_l2 {
                    let range = l2.entries_for_event(event).unwrap_or(0..0);
                    let silent = l2.silent_l2_events_for(event as u8);
                    let header = if range.is_empty() {
                        format!("Event {event}: silent row only")
                    } else {
                        format!("Event {event}: entries {}..{}", range.start, range.end)
                    };
                    ui.collapsing(header, |ui| {
                        for idx in range {
                            if let Some(entry) = l2.entries.get(idx) {
                                ui.monospace(format!("[{idx:3}] {}", describe_l2_entry(entry)));
                            }
                        }
                        for s in &silent {
                            ui.monospace(format!("[silent] {}", describe_l2_entry(&s.as_entry())));
                        }
                    });
                }
            });
        });
    }

    /// Event-ownership editor: which event each level (translevel) triggers when
    /// beaten — the `$05D608` events-by-translevel table. The game reads
    /// `DATA_05D608[TranslevelNo]` into `OverworldEvent` on level completion;
    /// Lunar Magic has no UI for choosing these assignments.
    fn event_ownership_panel(&mut self, ui: &mut Ui) {
        use smwe_rom::overworld::event_ownership as eo;
        ui.collapsing("Event ownership (by level)", |ui| {
            ui.label("Which event triggers when each level is beaten ($05D608).");
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                for tl in 0..eo::EVENT_OWNERSHIP_COUNT {
                    let name = self
                        .custom_level_names
                        .get(&(tl as u8))
                        .cloned()
                        .or_else(|| self.vanilla_level_names.get(tl).cloned())
                        .unwrap_or_default();
                    let cur: Option<u8> = match self.event_ownership.get(tl).copied() {
                        Some(b) if b != eo::NO_EVENT => Some(b),
                        _ => None,
                    };
                    let mut new = cur;
                    ui.horizontal(|ui| {
                        ui.label(format!("0x{tl:02X} {name}"));
                        egui::ComboBox::from_id_salt(("world_editor.event_ownership", tl))
                            .selected_text(event_option_label(cur, &self.rom))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut new, None, event_option_label(None, &self.rom));
                                for e in 0..smwe_rom::overworld::OW_EVENT_COUNT as u8 {
                                    ui.selectable_value(&mut new, Some(e), event_option_label(Some(e), &self.rom));
                                }
                            });
                    });
                    if new != cur {
                        if let Some(slot) = self.event_ownership.get_mut(tl) {
                            *slot = new.unwrap_or(eo::NO_EVENT);
                        }
                        self.event_ownership_dirty = true;
                        self.has_edits = true;
                    }
                }
            });
        });
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
            ui.small("Clipboard: Shift+drag on layer 1 selects a region • Ctrl+C copies • Ctrl+V pastes.");

            ui.separator();
            self.events_panel(ui);

            ui.separator();
            self.event_ownership_panel(ui);

            // ── Overworld ExAnimation (LM v2.40 parity) ─────────────────
            ui.separator();
            if ui.button("ExAnimated Frames…").clicked() {
                self.show_exanimation_editor = true;
            }
            ui.small("Custom overworld tile/palette animation, live in the view.");

            // ── Secondary-exit teleport locations (LM v3.00 parity) ──────
            ui.separator();
            if ui.button("Secondary Exit Teleports…").clicked() {
                self.show_se_teleport_editor = true;
            }
            ui.small("Star/Pipe table: where exit-to-overworld secondary exits place the player.");

            // ── Secret Exits 2/3 (LM v3.00 parity) ───────────────────────
            ui.separator();
            if ui.button("Secret Exits 2/3…").clicked() {
                self.show_secret_exits = true;
            }
            ui.small("Per-level direction-to-enable settings for LM v3.00's Secret Exits 2/3.");
            // ── Reveal tile list (LM v2.30 parity) ──────────────────────────
            ui.separator();
            if ui.button("Edit Reveal Tile List…").clicked() {
                self.show_reveal_list_editor = true;
            }
            ui.small("Which layer-1 tiles events reveal into which other tiles.");

            // ── Starting positions (LM v1.60 Mario / v1.90 Luigi parity) ────
            ui.separator();
            self.start_position_panel(ui);

            // ── Submap music (LM v1.30 "Change Overworld Music" parity) ──
            ui.separator();
            if ui.button("Overworld Submap Music…").clicked() {
                self.show_submap_music = true;
            }
            ui.small("Which music track each overworld submap plays.");

            // ── Editing mode toolbar ────────────────────────────────
            ui.separator();
            ui.label("Mode:");
            ui.horizontal(|ui| {
                let modes = [
                    ("Select [1]", EditingMode::Select),
                    ("Draw [2]", EditingMode::Draw),
                    ("Erase [3]", EditingMode::Erase),
                ];
                for (label, mode) in modes {
                    if toggle_button(ui, label, self.editing_mode == mode) {
                        self.editing_mode = mode;
                    }
                }
                self.ow_sprite_tool_toggle(ui);
            });

            // ── Overworld sprite tool (LM overworld sprite mode parity) ──
            if self.ow_sprite_tool {
                ui.separator();
                self.ow_sprite_panel(ui);
            }

            // ── Layer selector ────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("Layer:");
                let layers = [("L1", 1u8), ("L2", 2u8)];
                for (label, layer) in layers {
                    if toggle_button(ui, label, self.edit_layer == layer) {
                        self.edit_layer = layer;
                        self.preview_texture = None; // Force preview refresh
                    }
                }
            });

            // ── Draw mode tile picker ───────────────────────────────
            if self.editing_mode == EditingMode::Draw {
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
                        ui.painter().rect_stroke(
                            sel_rect,
                            egui::CornerRadius::ZERO,
                            egui::Stroke::new(2.0_f32, Color32::YELLOW),
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
                    let tile_screen = max_w / ow_tile_picker::L1_COLS as f32;
                    let sel_rect = Rect::from_min_size(
                        rect.min + vec2(col as f32 * tile_screen, row as f32 * tile_screen),
                        vec2(tile_screen, tile_screen),
                    );
                    ui.painter().rect_stroke(
                        sel_rect,
                        egui::CornerRadius::ZERO,
                        egui::Stroke::new(2.0_f32, Color32::YELLOW),
                        egui::StrokeKind::Outside,
                    );
                }
            }

            ui.separator();

            // ── Tile preview ────────────────────────────────────
            let draw_mode = self.editing_mode == EditingMode::Draw;
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
                    if let Some(idx) = self.source_l1_index_for_view(x, y) {
                        let tiles = self.edit_state.read(|s| s.layer1_tiles.clone());
                        if let Some(vanilla_level_num) = smwe_rom::overworld::level_number_for_index(&tiles, idx) {
                            let translevel = smwe_rom::overworld::translevel_for_index(&tiles, idx).unwrap_or(0);
                            ui.monospace(format!(
                                "  Level tile: #{vanilla_level_num:03X} (translevel {translevel:#04X})"
                            ));
                            ui.small(
                                "  vanilla number is order-derived — moving/inserting level tiles elsewhere \
                                 renumbers it unless overridden below",
                            );

                            let current = self.custom_level_numbers.get(&idx).copied().unwrap_or(vanilla_level_num);
                            let mut new_num = current as i32;
                            ui.horizontal(|ui| {
                                ui.label("  Assign level:");
                                let changed = ui
                                    .add(
                                        egui::Slider::new(
                                            &mut new_num,
                                            0..=smwe_rom::overworld::MAX_ASSIGNABLE_LEVEL_NUMBER as i32,
                                        )
                                        .hexadecimal(2, false, false),
                                    )
                                    .changed();
                                if changed {
                                    if new_num as u8 == vanilla_level_num {
                                        self.custom_level_numbers.remove(&idx);
                                    } else {
                                        self.custom_level_numbers.insert(idx, new_num as u8);
                                    }
                                    self.level_numbers_dirty = true;
                                    self.has_edits = true;
                                }
                            });
                            if self.custom_level_numbers.contains_key(&idx) {
                                ui.colored_label(
                                    egui::Color32::from_rgb(220, 160, 60),
                                    "  Overridden — needs the level-number patch on save",
                                );
                            }

                            // ── Level name editor ───────────────────────────
                            // Translevel indexes into the 93-entry name table.
                            let translevel_u8 = (translevel & 0xFF) as u8;
                            // Active Lunar Magic v3.40 custom table file
                            // (cloned so the handlers below can mutate
                            // `self` without borrow issues).
                            let table = self.level_name_table.clone();
                            let vanilla_name =
                                self.vanilla_level_names.get(translevel as usize).cloned().unwrap_or_default();
                            // Keep the text buffer synced: selecting a
                            // different level tile re-decodes it.
                            if self.level_name_for != Some(translevel_u8) {
                                self.level_name_edit = self
                                    .custom_level_names
                                    .get(&translevel_u8)
                                    .cloned()
                                    .unwrap_or_else(|| vanilla_name.clone());
                                self.level_name_for = Some(translevel_u8);
                                self.level_name_error = None;
                            }
                            ui.horizontal(|ui| {
                                ui.label("  Level name:");
                                let resp = ui.add(
                                    egui::TextEdit::singleline(&mut self.level_name_edit)
                                        .desired_width(200.0)
                                        .hint_text(&vanilla_name),
                                );
                                if resp.changed() {
                                    use smwe_rom::overworld::level_names as ln;
                                    let trimmed = self.level_name_edit.trim().to_string();
                                    // Back to the vanilla name clears the
                                    // override. With a table the comparison
                                    // is exact (the table defines the
                                    // charset); without one it stays
                                    // case-insensitive like before.
                                    let matches_vanilla = match &table {
                                        Some(_) => trimmed == vanilla_name,
                                        None => {
                                            trimmed.is_empty()
                                                || trimmed.to_uppercase() == vanilla_name.to_uppercase()
                                        }
                                    };
                                    if matches_vanilla {
                                        self.custom_level_names.remove(&translevel_u8);
                                        self.level_name_error = None;
                                        self.level_names_dirty = true;
                                        self.has_edits = true;
                                    } else {
                                        let use_multichar =
                                            crate::editor_options::EditorOptions::load().use_multichar_tiles;
                                        let result = match &table {
                                            Some(t) => ln::check_name_with_table(&trimmed, t)
                                                .map(|(normalized, _)| normalized),
                                            None => ln::check_name_with(&trimmed, use_multichar),
                                        };
                                        match result {
                                            Ok(normalized) => {
                                                self.custom_level_names.insert(translevel_u8, normalized);
                                                self.level_name_error = None;
                                                self.level_names_dirty = true;
                                                self.has_edits = true;
                                            }
                                            Err(e) => {
                                                // Refuse over-budget/invalid
                                                // input; the field keeps the
                                                // rejected text so the user
                                                // can fix it.
                                                self.level_name_error = Some(e.to_string());
                                            }
                                        }
                                    }
                                }
                            });
                            // Use MultiChar Tiles option (Lunar Magic v3.40).
                            // When toggled, re-decode the vanilla names so
                            // squished tiles switch between their character
                            // strings and `\XX` hex escapes.
                            {
                                use smwe_rom::overworld::level_names as ln;
                                let mut use_multichar =
                                    crate::editor_options::EditorOptions::load().use_multichar_tiles;
                                if ui
                                    .checkbox(&mut use_multichar, "  Use MultiChar Tiles")
                                    .on_hover_text(
                                        "Display the squished tiles Nintendo used in \"YELLOW SWITCH \
                                         PALACE\" and \"FOREST OF ILLUSION\" as their characters and \
                                         auto-encode \"LL\" to the squished tile (Lunar Magic v3.40). \
                                         When off, squished tiles show as \\XX hex escapes. \
                                         Type \\XX in the name field to insert a specific tile.",
                                    )
                                    .changed()
                                {
                                    let mut opts = crate::editor_options::EditorOptions::load();
                                    opts.use_multichar_tiles = use_multichar;
                                    opts.save();
                                    // Re-decode vanilla names with the new setting.
                                    if let Some(decoded) = ln::decode_all(self.rom.rom_bytes(), 0, false, use_multichar)
                                    {
                                        self.vanilla_level_names = decoded;
                                    }
                                    // Force the text field to re-sync from the
                                    // (possibly re-decoded) vanilla name.
                                    self.level_name_for = None;
                                    self.level_name_error = None;
                                }
                            }
                            // Tile-budget feedback, mirroring the message-box
                            // editor: the game draws at most MAX_NAME_TILES
                            // tiles per name (CODE_049D07's $26-byte stripe).
                            // With MultiChar tiles, characters can outnumber
                            // tiles ("LL" is one tile). With a custom table
                            // file the budget counts encoded tiles.
                            {
                                use smwe_rom::overworld::level_names as ln;
                                let use_multichar = crate::editor_options::EditorOptions::load().use_multichar_tiles;
                                let used = match &table {
                                    Some(t) => t.encode(self.level_name_edit.trim()).len(),
                                    None => ln::count_name_tiles(self.level_name_edit.trim(), use_multichar),
                                };
                                let budget_color = if self.level_name_error.is_some() || used > ln::MAX_NAME_TILES {
                                    egui::Color32::from_rgb(220, 60, 60)
                                } else {
                                    ui.style().visuals.text_color()
                                };
                                ui.colored_label(
                                    budget_color,
                                    format!("  Name encodes to {used} / {} tiles", ln::MAX_NAME_TILES),
                                );
                                if let Some(err) = self.level_name_error.as_ref() {
                                    ui.colored_label(egui::Color32::from_rgb(220, 60, 60), format!("  {err}"));
                                }
                            }
                            // Lunar Magic v3.40 "Custom Table File" (.lmtbl)
                            // support: a table replaces the built-in
                            // overworld-name tile map for this panel, so
                            // names can be displayed/edited in a different
                            // language.
                            ui.horizontal(|ui| {
                                ui.label("  ");
                                match (&self.level_name_table_name, &self.level_name_table) {
                                    (Some(name), Some(t)) => {
                                        ui.label(format!("Table file: {name} ({} entries)", t.len()));
                                    }
                                    _ => {
                                        ui.label("Table file: built-in name tiles");
                                    }
                                }
                                if ui.button("Load Table File...").clicked() {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("Lunar Magic table file", &["lmtbl"])
                                        .pick_file()
                                    {
                                        match smwe_rom::table_file::load_table_file_for_dialog(
                                            &path,
                                            smwe_rom::table_file::TableDialog::LevelNames,
                                        ) {
                                            Ok((new_table, name, warnings)) => {
                                                let had_custom = !self.custom_level_names.is_empty();
                                                // Re-decode the vanilla
                                                // names through the table.
                                                // Custom names were validated
                                                // under the previous mapping
                                                // and are dropped rather than
                                                // re-encoded through a table
                                                // that might skip their
                                                // characters.
                                                self.vanilla_level_names =
                                                    smwe_rom::overworld::level_names::decode_all_with_table(
                                                        self.rom.rom_bytes(),
                                                        0,
                                                        false,
                                                        &new_table,
                                                    )
                                                    .unwrap_or_default();
                                                self.custom_level_names.clear();
                                                self.level_name_table = Some(new_table);
                                                self.level_name_table_name = Some(name);
                                                self.level_name_for = None;
                                                self.level_name_error = None;
                                                let mut notes = warnings;
                                                if had_custom {
                                                    notes.insert(0, "Table loaded — custom names validated under the previous mapping were cleared.".to_string());
                                                }
                                                self.level_name_table_error = if notes.is_empty() {
                                                    None
                                                } else {
                                                    Some(notes.join("\n"))
                                                };
                                            }
                                            Err(e) => {
                                                self.level_name_table_error = Some(e.to_string());
                                            }
                                        }
                                    }
                                }
                                if self.level_name_table.is_some() && ui.button("Clear Table").clicked() {
                                    self.vanilla_level_names = smwe_rom::overworld::level_names::decode_all(
                                        self.rom.rom_bytes(),
                                        0,
                                        false,
                                        crate::editor_options::EditorOptions::load().use_multichar_tiles,
                                    )
                                    .unwrap_or_default();
                                    self.custom_level_names.clear();
                                    self.level_name_table = None;
                                    self.level_name_table_name = None;
                                    self.level_name_table_error = None;
                                    self.level_name_error = None;
                                    self.level_name_for = None;
                                }
                            });
                            if let Some(err) = self.level_name_table_error.as_deref() {
                                ui.colored_label(egui::Color32::from_rgb(220, 160, 60), format!("  {err}"));
                            }
                            if self.custom_level_names.contains_key(&translevel_u8) {
                                ui.colored_label(
                                    egui::Color32::from_rgb(220, 160, 60),
                                    "  Custom name — needs the name-table relocation patch on save",
                                );
                                if table.is_some() {
                                    ui.small("  Table active (LM v3.40): unmapped bytes show as <XX>; unmapped typed characters are skipped");
                                } else {
                                    ui.small("  A–Z 0–9 space # ' supported");
                                }
                            }
                        }
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

        // ── Input handling moved below (needs the canvas origin) ──

        let zoom_delta = ui.input(|i| i.zoom_delta());
        let wheel_delta = ui.input(|i| i.raw_scroll_delta.y);
        if resp.contains_pointer() {
            let factor = if (zoom_delta - 1.0).abs() > f32::EPSILON {
                zoom_delta
            } else if wheel_delta.abs() > f32::EPSILON {
                (wheel_delta * 0.005).exp()
            } else {
                1.0
            };
            if factor != 1.0 {
                self.zoom = (self.zoom * factor).clamp(0.25, 16.0);
            }
        }

        // ── Background ───────────────────────────────────────────────────────
        painter.rect_filled(view_rect, CornerRadius::ZERO, Color32::from_rgb(16, 16, 20));

        let z = self.zoom;
        let (map_px_w, map_px_h) = visible_map_size(self.submap);
        let map16_cols = map_px_w.div_ceil(16);
        let map16_rows = map_px_h.div_ceil(16);
        let map16_sz = MAP16_PX * z;
        let canvas_w = map_px_w as f32 * z;
        let canvas_h = map_px_h as f32 * z;
        let origin = view_rect.min + self.offset * z;
        let ow_rect = Rect::from_min_size(origin, vec2(canvas_w, canvas_h));

        // ── Input ────────────────────────────────────────────────────────────
        // Shift+drag on layer 1 in Select mode draws a clipboard region
        // (LM v2.30 overworld copy) instead of panning.
        let shift = ui.input(|i| i.modifiers.shift);
        let region_dragging = shift
            && self.edit_layer == 1
            && matches!(self.editing_mode, EditingMode::Select | EditingMode::Probe)
            && resp.dragged_by(egui::PointerButton::Primary);
        let is_pan = resp.dragged_by(egui::PointerButton::Middle)
            || (resp.dragged_by(egui::PointerButton::Primary) && !region_dragging && !self.ow_sprite_tool);
        if is_pan {
            self.offset += resp.drag_delta() / self.zoom;
        }
        if region_dragging {
            let tile_at = |pos: Pos2| -> (u32, u32) {
                let rel = (pos - origin) / map16_sz;
                (
                    rel.x.floor().clamp(0.0, map16_cols as f32 - 1.0) as u32,
                    rel.y.floor().clamp(0.0, map16_rows as f32 - 1.0) as u32,
                )
            };
            if resp.drag_started_by(egui::PointerButton::Primary) {
                if let Some(pos) = resp.interact_pointer_pos().or_else(|| resp.hover_pos()) {
                    let (ax, ay) = tile_at(pos);
                    self.ow_drag_anchor = Some((ax, ay));
                    self.ow_sel_rect = Some((ax, ay, ax, ay));
                    self.selected_tile = None;
                }
            } else if let (Some((ax, ay)), Some(pos)) = (self.ow_drag_anchor, resp.hover_pos()) {
                let (cx, cy) = tile_at(pos);
                self.ow_sel_rect = Some((ax.min(cx), ay.min(cy), ax.max(cx), ay.max(cy)));
            }
        } else if resp.drag_stopped_by(egui::PointerButton::Primary) {
            self.ow_drag_anchor = None;
        }

        // ── GL render ────────────────────────────────────────────────────────
        {
            let renderer = Arc::clone(&self.renderer);
            let draw_l1 = self.show_layer1;
            let draw_l2 = self.show_layer2;
            let ppp = ui.ctx().pixels_per_point();
            let screen_sz = view_rect.size() * ppp;
            let gl_offset = self.offset;
            let gl_zoom = z * ppp;

            // ── Overworld animated tiles ─────────────────────────────
            // SMW advances each animated tile slot once every 8 game-frames at
            // 60 fps, so each distinct animation frame shows for ~133ms.  We tick
            // at the same interval to match the real game's visual speed.
            const ANIM_INTERVAL: std::time::Duration = std::time::Duration::from_millis(133);
            if self.last_anim_tick.elapsed() >= ANIM_INTERVAL {
                self.last_anim_tick = std::time::Instant::now();
                self.exanim_tick += 1;
                // Custom overworld ExAnimation frames (LM v2.40 parity) play
                // on the same tick. `disable_original` skips the game's own
                // overworld animated tiles so the custom ones replace them.
                let anim = self.exanimation.for_overworld();
                if !anim.disable_original {
                    smwe_emu::emu::advance_ow_anim_frame(&mut self.cpu);
                }
                let r = self.renderer.lock().expect("Cannot lock overworld renderer");
                if anim.frames.is_empty() {
                    r.upload_gfx(&self.gl, &self.cpu.mem.vram);
                } else {
                    smwe_rom::exanimation::apply_tick(
                        &anim,
                        self.exanim_tick,
                        &mut self.cpu.mem.vram,
                        &mut self.cpu.mem.cgram,
                    );
                    r.upload_gfx(&self.gl, &self.cpu.mem.vram);
                    r.upload_palette(&self.gl, &self.cpu.mem.cgram);
                }
            }
            ui.ctx().request_repaint_after(ANIM_INTERVAL);

            ui.painter().add(PaintCallback {
                rect:     view_rect,
                callback: Arc::new(CallbackFn::new(move |_info, painter| {
                    let r = renderer.lock().expect("Cannot lock overworld renderer");
                    r.paint(painter.gl().as_ref(), screen_sz, gl_zoom, gl_offset, draw_l1, draw_l2);
                })),
            });
        }

        // ── Border around canvas ──────────────────────────────────────────────
        painter.rect_stroke(
            ow_rect,
            CornerRadius::ZERO,
            Stroke::new(2.0_f32, Color32::from_white_alpha(140)),
            StrokeKind::Outside,
        );

        // ── Grid (Map16 block grid, aligned to L1) ───────────────────────────
        if self.show_grid || ui.input(|i| i.modifiers.shift_only()) {
            let stroke = Stroke::new(0.5_f32, Color32::from_white_alpha(25));
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

        // ── Layer 2 event target markers ──────────────────────────────────────
        // Cyan ring = VRAM tile stream, orange ring = tilemap copy. Follows the
        // event checkboxes in the left panel (only active events are drawn).
        if self.show_l2_event_markers {
            let (crop_x, crop_y) = visible_map_crop(self.submap);
            let tile_sz = 8.0 * z;
            let l2 = &self.rom.overworld_l2_events;
            for (event, active) in self.active_events.iter().enumerate() {
                if !active {
                    continue;
                }
                let mark = |entry: &L2EventEntry| {
                    let (col, row) = entry.target_tile();
                    let sx = origin.x + (col as f32 * 8.0 - crop_x as f32) * z;
                    let sy = origin.y + (row as f32 * 8.0 - crop_y as f32) * z;
                    let center = egui::pos2(sx + tile_sz * 0.5, sy + tile_sz * 0.5);
                    if !view_rect.contains(center) {
                        return;
                    }
                    let color = l2_marker_color(entry.kind());
                    painter.circle_stroke(center, tile_sz * 0.45, Stroke::new(1.5_f32, color));
                    painter.circle_filled(center, 1.5, color);
                };
                if let Some(range) = l2.entries_for_event(event) {
                    for idx in range {
                        if let Some(entry) = l2.entries.get(idx) {
                            mark(entry);
                        }
                    }
                }
                for s in l2.silent_l2_events_for(event as u8) {
                    mark(&s.as_entry());
                }
            }
        }

        // ── Mario/Luigi start-position markers ────────────────────────────────
        // Main map only (the `$009EF0` coordinates live in the main-map pixel
        // space); drawn under the sprite markers.
        self.ow_draw_start_markers(&painter, origin, z);

        // ── Overworld sprite markers ──────────────────────────────────────────
        // Drawn above the event markers, using the same canvas basis
        // (`origin`, `z`, `visible_map_crop`) as the GL render.
        if self.ow_sprite_tool {
            self.ow_draw_markers(&painter, &resp, origin, z, view_rect);
        }

        // ── Hover / click (Map16 block granularity) ───────────────────────────
        // Inactive while the sprite tool owns the canvas.
        if !self.ow_sprite_tool {
            if let Some(cursor) = resp.hover_pos() {
                let rel = (cursor - origin) / map16_sz;
                let tx = rel.x.floor() as i32;
                let ty = rel.y.floor() as i32;
                if (0..map16_cols as i32).contains(&tx) && (0..map16_rows as i32).contains(&ty) {
                    let x = tx as u32;
                    let y = ty as u32;
                    let addr = l1_vram_addr_for_map16(self.submap, x, y);
                    let tile_id = u16::from_le_bytes([self.cpu.mem.vram[addr], self.cpu.mem.vram[addr + 1]]) & 0x03FF;
                    let tile_rect = Rect::from_min_size(
                        origin + vec2(x as f32 * map16_sz, y as f32 * map16_sz),
                        Vec2::splat(map16_sz),
                    );
                    painter.rect_stroke(
                        tile_rect,
                        CornerRadius::ZERO,
                        Stroke::new(1.0_f32, Color32::WHITE),
                        StrokeKind::Outside,
                    );

                    if resp.clicked_by(egui::PointerButton::Primary)
                        && (self.editing_mode == EditingMode::Select || ui.input(|i| i.modifiers.alt))
                        && !ui.input(|i| i.modifiers.shift)
                    {
                        // Armed click-to-place for a start marker consumes the
                        // click instead of selecting a tile.
                        if !self.ow_place_start_on_click(x, y) {
                            self.selected_tile = Some((x, y));
                            // A plain click replaces the clipboard region selection.
                            self.ow_sel_rect = None;
                        }
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
        }

        // ── Editing interaction ─────────────────────────────────────
        // The sprite tool owns the canvas while active; otherwise the tile
        // editing path handles the pointer.
        if self.ow_sprite_tool {
            self.ow_handle_canvas(&resp, origin, z);
        } else {
            // Shift suppresses plain-click selection while a clipboard region
            // is being drawn (see the region-select block above).
            self.handle_editing_interaction(&resp, origin, map16_sz, shift);
        }

        // ── Keyboard shortcuts ──────────────────────────────────────
        // Captured before `input_mut`: Delete must not fire while a text
        // field (e.g. the extra-bytes hex box) has keyboard focus.
        let kb_focus = ui.ctx().wants_keyboard_input();
        ui.input_mut(|input| {
            if input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, Key::Z)) {
                self.handle_undo();
            }
            if input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, Key::Y)) {
                self.handle_redo();
            }
            if input.key_pressed(egui::Key::Num1) {
                self.editing_mode = EditingMode::Select;
            }
            if input.key_pressed(egui::Key::Num2) {
                self.editing_mode = EditingMode::Draw;
            }
            if input.key_pressed(egui::Key::Num3) {
                self.editing_mode = EditingMode::Erase;
            }
            if input.key_pressed(egui::Key::Num4) {
                self.ow_sprite_tool = !self.ow_sprite_tool;
                if !self.ow_sprite_tool {
                    self.ow_sprite_selection = None;
                    self.ow_sprite_drag = None;
                    self.ow_extra_hex.clear();
                }
            }
            // Delete removes the selected custom sprite (vanilla slots are
            // fixed). Guarded on text focus so typing in the extra-bytes
            // field doesn't nuke a sprite.
            if self.ow_sprite_tool && input.key_pressed(egui::Key::Delete) && !kb_focus {
                self.ow_delete_selected();
            }
        });

        // ── Selected tile highlight ───────────────────────────────────────────
        if let Some((x, y)) = self.selected_tile {
            let r = Rect::from_min_size(origin + vec2(x as f32 * map16_sz, y as f32 * map16_sz), Vec2::splat(map16_sz));
            painter.rect_stroke(
                r,
                CornerRadius::ZERO,
                Stroke::new(2.0_f32, Color32::from_rgb(255, 220, 0)),
                StrokeKind::Outside,
            );
        }

        // ── Clipboard region selection highlight ────────────────────────────
        if let Some((x0, y0, x1, y1)) = self.ow_sel_rect {
            let r = Rect::from_min_max(
                origin + vec2(x0 as f32 * map16_sz, y0 as f32 * map16_sz),
                origin + vec2((x1 + 1) as f32 * map16_sz, (y1 + 1) as f32 * map16_sz),
            );
            painter.rect_stroke(
                r,
                CornerRadius::ZERO,
                Stroke::new(2.0_f32, Color32::from_rgb(80, 200, 255)),
                StrokeKind::Outside,
            );
        }

        // ── Clipboard: copy/paste overworld layer-1 tiles ───────────────
        // Lunar Magic v2.30 lets you copy/paste between the background
        // editor and the overworld. Ctrl+C copies the Shift+drag region (or
        // the selected tile); paste arrives as Event::Paste directly on
        // Ctrl+V — drain it here (a focused text widget keeps its own paste).
        let ow_widget_focused = ui.ctx().memory(|m| m.focused().is_some());
        if !ow_widget_focused {
            if ui.input(|i| i.events.contains(&egui::Event::Copy)) {
                self.ow_clipboard_copy(ui.ctx());
            }
            if let Some(text) = crate::ui::clipboard::take_paste_text(ui.ctx()) {
                // Paste at the hovered tile, falling back to the copy origin.
                let anchor = resp
                    .hover_pos()
                    .map(|pos| {
                        let rel = (pos - origin) / map16_sz;
                        (rel.x.floor().max(0.0) as u32, rel.y.floor().max(0.0) as u32)
                    })
                    .or(self.ow_copy_origin);
                self.ow_clipboard_paste(&text, anchor);
            }
        }
    }
}

// ── Tile list builders ────────────────────────────────────────────────────────

fn build_bg_tiles(vram: &[u8], tilemap_base: usize, submap: u8, scroll_x: i32, scroll_y: i32) -> Vec<Tile> {
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

fn ow_tile(x: u32, y: u32, t: u16) -> Tile {
    let t32 = t as u32;
    let tile = t32 & 0x3FF;
    let pal = (t32 >> 10) & 0x7;
    let scale = 8u32;
    let params = scale | (pal << 8) | (t32 & 0xC000);
    Tile([x, y, tile, params])
}

/// Label for an event-ownership combo option: the event number plus the
/// overworld tile it reveals (when the event has a reveal-tile entry), so the
/// user can pick events by what they visibly do.
fn event_option_label(event: Option<u8>, rom: &SmwRom) -> String {
    match event {
        None => "None (no event)".to_string(),
        Some(e) => {
            let off = rom.overworld_events.tile_offsets.get(e as usize).copied().unwrap_or(0);
            if off == 0 {
                format!("Event {e}")
            } else {
                format!("Event {e} — reveals tile {off:#06X}")
            }
        }
    }
}

/// Write `active_events` (indices 0..OW_EVENT_COUNT) into the emulated
/// `OWEventsActivated` WRAM table ($1F02-$1F60, 8 events/byte, MSB-first bit
/// order per SMWDisX `bank_04.asm` `DATA_04E44B`), so that when the real game
/// code runs `load_overworld` it applies exactly the reveal-tile swaps
/// (`CODE_04DA49`) for the events the user has toggled on.
fn apply_active_events_to_wram(cpu: &mut Cpu, active_events: &[bool]) {
    for byte_idx in 0..15u32 {
        let mut byte = 0u8;
        for bit in 0..8u32 {
            let event_num = (byte_idx * 8 + bit) as usize;
            if active_events.get(event_num).copied().unwrap_or(false) {
                byte |= 0x80 >> bit;
            }
        }
        cpu.mem.store_u8(0x1F02 + byte_idx, byte);
    }
}

/// One-line human description of a Layer 2 event table entry for the events
/// panel, e.g. `stream 36 tiles -> (12,7)` or `tilemap copy $7F8000+0x0900 -> (6,15)`.
fn describe_l2_entry(entry: &L2EventEntry) -> String {
    let (col, row) = entry.target_tile();
    match entry.kind() {
        L2EventKind::TileStream(n) => format!("stream {n} tiles -> ({col},{row})"),
        L2EventKind::TilemapCopy(off) => format!("tilemap copy WRAM+{off:#06X} -> ({col},{row})"),
    }
}

/// Marker color for a Layer 2 event entry on the map: cyan for VRAM tile
/// streams, orange for tilemap copies.
fn l2_marker_color(kind: L2EventKind) -> Color32 {
    match kind {
        L2EventKind::TileStream(_) => Color32::from_rgb(0, 220, 255),
        L2EventKind::TilemapCopy(_) => Color32::from_rgb(255, 170, 0),
    }
}

fn read_overworld_l2_words(cpu: &Cpu) -> Vec<u16> {
    let base = (0x7F4000 - 0x7E0000) as usize;
    let bytes = &cpu.mem.wram[base..base + 0x2000];
    bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

fn write_overworld_l2_stream(
    rom_bytes: &mut [u8], has_smc_header: bool, start_pc_no_header: usize, output_len: usize, compressed: &[u8],
    label: &str,
) -> anyhow::Result<()> {
    let header_offset = usize::from(has_smc_header) * 0x200;
    let start = start_pc_no_header + header_offset;
    let old_size = lc_rle2::compressed_size_for_output(
        rom_bytes.get(start..).ok_or_else(|| anyhow::anyhow!("{label} ROM source start out of bounds"))?,
        output_len,
    );
    if compressed.len() <= old_size {
        let dst = rom_bytes
            .get_mut(start..start + old_size)
            .ok_or_else(|| anyhow::anyhow!("{label} ROM write range out of bounds"))?;
        dst[..compressed.len()].copy_from_slice(compressed);
        dst[compressed.len()..].fill(0);
    } else {
        let new_pc = find_free_space(rom_bytes, compressed.len(), 0x008000, header_offset)
            .ok_or_else(|| anyhow::anyhow!("{label} no free space found for {} bytes", compressed.len()))?;

        if let Some(dst) = rom_bytes.get_mut(start..start + old_size) {
            dst.fill(0xFF);
        }

        let new_file = new_pc + header_offset;
        rom_bytes
            .get_mut(new_file..new_file + compressed.len())
            .ok_or_else(|| anyhow::anyhow!("{label} new location write out of bounds"))?
            .copy_from_slice(compressed);

        let old_snes = AddrSnes::try_from_lorom(AddrPc(start_pc_no_header as u32))?.0;
        let new_snes = AddrSnes::try_from_lorom(AddrPc(new_pc as u32))?.0;
        patch_snes_pointer(rom_bytes, old_snes, new_snes, label)?;
    }
    Ok(())
}

fn patch_snes_pointer(rom_bytes: &mut [u8], old_snes: u32, new_snes: u32, label: &str) -> anyhow::Result<()> {
    let old_bytes = old_snes.to_le_bytes();
    let new_bytes = new_snes.to_le_bytes();
    let matches: Vec<usize> = rom_bytes
        .windows(3)
        .enumerate()
        .filter_map(|(offset, window)| (window == &old_bytes[..3]).then_some(offset))
        .collect();
    let [offset] = matches.as_slice() else {
        anyhow::bail!(
            "{label} expected exactly one pointer to SNES ${old_snes:06X}, found {}; refusing to repoint",
            matches.len()
        );
    };
    rom_bytes[*offset..*offset + 3].copy_from_slice(&new_bytes[..3]);
    log::info!("{label} repointed from SNES ${old_snes:06X} to ${new_snes:06X}");
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
        render_preview_tile(&cpu.mem.vram, &cpu.mem.cgram, tile_num, pal, flip_x, flip_y, x0, y0, 16, &mut pixels);
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
            let color_idx =
                (((b0 >> bit) & 1) | (((b1 >> bit) & 1) << 1) | (((b2 >> bit) & 1) << 2) | (((b3 >> bit) & 1) << 3))
                    as usize;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The undo serialization must round-trip the sprite tables exactly —
    /// undo/redo runs through `to_bytes`/`from_bytes` on every step.
    #[test]
    fn overworld_edit_state_undo_round_trip() {
        let mut custom = ow_sprites::CustomSpriteTable::default();
        // Invariant the UI maintains: extra.len() == counts[number], so the
        // undo payload (encoded with those counts) round-trips exactly.
        let mut counts = [7u8; 128];
        counts[0x10] = 3;
        custom.submaps[2].push(ow_sprites::CustomOwSprite {
            number: 0x10,
            x:      20,
            y:      44,
            height: 3,
            extra:  vec![0x2A, 0x00, 0x01],
        });
        let mut size_table = ow_sprites::SpriteSizeTable::default();
        size_table.set_size(0x10, 6).unwrap();
        let state = OverworldEditState {
            layer1_tiles:         vec![0x12; OWL1_TILE_DATA_SIZE],
            layer2_words:         vec![0x1234, 0xABCD],
            vanilla_sprites:      ow_sprites::VanillaOwSprites {
                sprites:    [ow_sprites::VanillaOwSprite { number: 0x07, x: 0x38, y: 0x18A };
                    ow_sprites::VANILLA_SPRITE_COUNT],
                visibility: [0x3F; ow_sprites::VISIBILITY_COUNT],
            },
            custom_sprites:       custom,
            foreign_custom_table: true,
            custom_extra_counts:  counts,
            sprite_size_table:    Some(size_table),
            secret_exits:         ow_secret_exits::SecretExitSettings {
                entries: vec![ow_secret_exits::SecretExitEntry {
                    level: 0x101,
                    exit2: ow_secret_exits::DIR_UP,
                    exit3: ow_secret_exits::DIR_LEFT,
                }],
            },
            reveal_list:          smwe_rom::overworld::reveal_list::RevealTileList {
                before: vec![0x00; smwe_rom::overworld::reveal_list::REVEAL_COUNT],
                after:  vec![0x00; smwe_rom::overworld::reveal_list::REVEAL_COUNT],
            },
            start_positions:      smwe_rom::overworld::start_positions::OverworldStartPositions::decode(
                &[0u8; smwe_rom::overworld::start_positions::START_POSITIONS_LEN],
            ),
            submap_music:         smwe_rom::overworld::submap_music::SubmapMusic { tracks: [9, 9, 9, 9, 9, 9, 9] },
        };
        let back = OverworldEditState::from_bytes(state.to_bytes());
        assert_eq!(back.layer1_tiles, state.layer1_tiles);
        assert_eq!(back.layer2_words, state.layer2_words);
        assert_eq!(back.vanilla_sprites, state.vanilla_sprites);
        assert_eq!(back.custom_sprites, state.custom_sprites);
        assert_eq!(back.foreign_custom_table, state.foreign_custom_table);
        assert_eq!(back.custom_extra_counts, state.custom_extra_counts);
        assert_eq!(back.sprite_size_table, state.sprite_size_table);
        assert_eq!(back.secret_exits, state.secret_exits);
        assert_eq!(back.reveal_list.before, state.reveal_list.before);
        assert_eq!(back.reveal_list.after, state.reveal_list.after);
        assert_eq!(back.start_positions.mario.pixel_x, state.start_positions.mario.pixel_x);
        assert_eq!(back.start_positions.luigi.tile_y, state.start_positions.luigi.tile_y);
        assert_eq!(back.submap_music.tracks, state.submap_music.tracks);
    }

    /// Truncated buffers must not panic — `from_bytes` degrades gracefully.
    #[test]
    fn overworld_edit_state_from_bytes_truncated() {
        let back = OverworldEditState::from_bytes(vec![0xAA; 100]);
        assert_eq!(back.layer1_tiles.len(), 100);
        assert!(back.layer2_words.is_empty());
        assert!(back.custom_sprites.submaps.iter().all(|v| v.is_empty()));
    }
}
