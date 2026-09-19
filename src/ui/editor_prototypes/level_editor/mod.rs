mod auto_screens;
mod background_layer;
mod bg_tilemap_editor;
mod boss_text_editor;
mod central_panel;
mod custom_collections_ui;
mod custom_tooltips_ui;
mod dm16_editor;
mod edit_manual_dialog;
mod editing;
mod exgfx_manager;
mod gfx_bypass;
mod gfx_editor;
mod gfx_slot_browser;
mod layer3_settings;
mod message_editor;

mod left_panel;
mod level_renderer;
mod map16_editor;
mod map16_file;
mod mwl;
mod object_layer;
mod palette_editor;
mod properties;
mod secondary_entrance_editor;
mod sprite_catalog;
mod sprite_header_editor;
mod sprite_layer;
mod sprite_tweaker_editor;
mod tile_editor;
mod tile_picker;
mod title_credits_editor;
mod toolbar;
mod xref_search;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::Context;
use egui::{CentralPanel, Frame, SidePanel, Ui, WidgetText, *};
use smwe_emu::{
    emu::{CheckedMem, SpriteOamTile},
    rom::Rom as EmuRom,
    Cpu,
};
use smwe_rom::{
    compression::lc_lz2,
    graphics::gfx_file,
    level::{Layer2Data, Level, LAYER2_HEADER_SIZE, PRIMARY_HEADER_SIZE},
    snes_utils::addr::{AddrPc, AddrSnes},
    SmwRom,
};

use self::{
    background_layer::EditableBackgroundLayer,
    dm16_editor::Dm16Placement,
    level_renderer::LevelRenderer,
    object_layer::{EditableDirectMap16, EditableObjectLayer},
    properties::LevelProperties,
    sprite_layer::EditableSpriteLayer,
    tile_picker::{BgTilePicker, TilePicker},
    xref_search::XrefSearchState,
};
use crate::{
    rom_freespace::{find_free_space, find_free_space_in},
    ui::{editing_mode::EditingMode, tool::DockableEditorTool},
    undo::{Undo, UndoableData},
};

/// Main-entrance ("M" marker) position in absolute tile coordinates.
///
/// Lunar Magic v2.20 made the level entrance selectable/draggable/copyable in
/// sprite editing mode; the position lives here as its own undoable value so
/// entrance edits (drag, cut, paste-move, delete-reset) each undo in one
/// step. Undo ordering across the sprite/object stacks is kept LIFO by the
/// `spawn_undo_pending` / `spawn_redo_pending` flags on [`UiLevelEditor`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SpawnPos {
    pub x: u32,
    pub y: u32,
}

impl Undo for SpawnPos {
    fn from_bytes(bytes: Vec<u8>) -> Self {
        // Guarded per-field decode: short or truncated buffers decode as 0
        // instead of panicking (undo snapshots are always 8 bytes; this is
        // belt-and-braces for hand-fed data).
        let x = bytes.get(0..4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).unwrap_or(0);
        let y = bytes.get(4..8).map(|s| u32::from_le_bytes(s.try_into().unwrap())).unwrap_or(0);
        Self { x, y }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8);
        out.extend_from_slice(&self.x.to_le_bytes());
        out.extend_from_slice(&self.y.to_le_bytes());
        out
    }

    fn size_bytes(&self) -> usize {
        8
    }
}

/// Vanilla default main-entrance position (absolute full-res tile coords).
/// Verified against the real ROM: 374 of 512 levels carry half-res (0, 11) on
/// screen 0 in the secondary-header entrance bytes — Nintendo's factory
/// default for unused levels, and Lunar Magic's delete-reset target. With
/// screen 0 this maps to absolute tile (0, 22) for both horizontal and
/// vertical layouts.
pub(super) const DEFAULT_SPAWN_X: u32 = 0;
pub(super) const DEFAULT_SPAWN_Y: u32 = 22;

pub struct UiLevelEditor {
    gl:             Arc<glow::Context>,
    rom:            Arc<SmwRom>,
    cpu:            Cpu,
    level_renderer: Arc<Mutex<LevelRenderer>>,

    level_num:           u16,
    offset:              Vec2,
    zoom:                f32,
    always_show_grid:    bool,
    show_object_overlay: bool,
    show_sprite_overlay: bool,
    show_object_labels:  bool,
    mark_exit_tiles:     bool, // LM v3.31 view option: mark exit-enabled tiles
    selected_tile:       Option<(u32, u32)>,

    level_properties:        LevelProperties,
    layer1:                  UndoableData<EditableObjectLayer>,
    layer2_objects:          Option<UndoableData<EditableObjectLayer>>,
    layer2_background:       Option<UndoableData<EditableBackgroundLayer>>,
    sprites:                 UndoableData<EditableSpriteLayer>,
    tile_picker:             TilePicker,
    bg_tile_picker:          BgTilePicker,
    sprite_search:           String,
    sprite_preview_textures: HashMap<u8, egui::TextureHandle>,
    sprite_oam_cache:        HashMap<u8, Vec<SpriteOamTile>>,
    preview_texture:         Option<egui::TextureHandle>,
    preview_for:             Option<(u32, u32)>,

    // Animation
    last_anim_tick: Instant,
    /// Editor animation tick counter (one per ~133ms animated-tile tick);
    /// drives ExAnimation preview stepping.
    anim_tick:      u64,

    // Editing state
    editing_mode:               EditingMode,
    selected_object_indices:    HashSet<usize>,
    selected_sprite_indices:    HashSet<usize>,
    /// Pointer-over-window flags (one frame stale is fine) so the level
    /// canvas's global Ctrl+C / Ctrl+V stand down when a floating editor
    /// window has copy intent.
    map16_window_hovered:       bool,
    tile_editor_window_hovered: bool,
    /// Absolute tile coords of the last copy's selection top-left; used as
    /// the paste anchor when the pointer isn't over the canvas (with a +1,+1
    /// cascade so repeated pastes don't stack exactly).
    clipboard_copy_origin:      Option<(u32, u32)>,
    /// In-progress Lunar Magic-style object drag (body move or handle resize).
    /// Transient: not part of the undoable layer; committed once on release.
    object_drag:                Option<editing::ObjectDrag>,
    /// Set for one frame when an object drag ends with a change, so the
    /// release click doesn't also trigger click-select/tile-inspect.
    suppress_click_select:      bool,
    draw_object_id:             u8,
    draw_object_settings:       u8,
    draw_block_id:              u16,
    draw_sprite_id:             u8,
    draw_sprite_extra_bits:     u8,
    edit_layer:                 u8, // 1 or 2
    edit_sprites:               bool,

    // Spawn point marker ("M"): the level's main entrance, selectable and
    // draggable in sprite editing mode (Lunar Magic v2.20).
    spawn:              UndoableData<SpawnPos>,
    initial_spawn_x:    u32,
    initial_spawn_y:    u32,
    dragging_spawn:     bool,
    /// The M marker is selected (exclusive with sprite selection).
    entrance_selected:  bool,
    /// LIFO cross-stack undo: true while the spawn stack's head step is the
    /// most recent undoable mutation overall. Every non-spawn mutation clears
    /// these via [`Self::mark_edited`]; spawn mutations set them.
    spawn_undo_pending: bool,
    spawn_redo_pending: bool,
    /// Pre-drag snapshot for gesture-style entrance drags (one undo step per
    /// drag, committed on release).
    spawn_drag_before:  Option<SpawnPos>,

    // Unsaved changes tracking
    show_unsaved_dialog: bool,
    pending_level_num:   Option<u16>,
    has_edits:           bool,
    request_rom_save:    bool,
    pending_close:       bool,

    // Editor windows
    show_level_header:        bool,
    show_secondary_entrances: bool,
    show_palette_editor:      bool,
    show_map16_editor:        bool,
    // Background tile map editor (Lunar Magic-style dedicated window)
    show_bg_tilemap_editor:   bool,
    bg_tool:                  bg_tilemap_editor::BgTileTool,
    /// Brush: Map16 block number within the current background page.
    bg_selected_tile:         u8,
    /// Background Map16 bank ("page") 0 or 1.
    bg_page:                  u8,
    bg_zoom:                  f32,
    bg_show_grid:             bool,
    /// Selected rectangle in tilemap cells: (x, y, w, h).
    bg_selection:             Option<(u32, u32, u32, u32)>,
    bg_canvas_tex:            Option<egui::TextureHandle>,
    bg_canvas_dirty:          bool,
    bg_selector_tex:          Option<egui::TextureHandle>,
    /// Cached BG Map16 blocks: 512 entries x 4 tile words (pages 0+1).
    bg_block_words:           Vec<[u16; 4]>,
    bg_status:                Option<String>,
    bg_offset_open:           bool,
    bg_offset_val:            i32,
    bg_bank_open:             bool,
    bg_bank_choice:           u8,
    /// Page the tiles were last rematched from; set by "Change Background
    /// Map16 Bank" so "Remap Background Tiles" can restore the graphics.
    bg_prev_page:             Option<u8>,
    bg_drag:                  Option<bg_tilemap_editor::BgDrag>,

    // Secondary entrance data (local mutable copy, 512 entries × 4 bytes)
    secondary_entrance_data:     Vec<[u8; 4]>,
    secondary_entrance_search:   String,
    // LM v3.00 extended secondary-exit options (local mutable copy of the
    // editor-owned RATS block; indices 0..0x2000 options, 0x200..0x2000
    // extended entries, 0x100 OW teleport table).
    secondary_exit_ext:          smwe_rom::level::secondary_entrance::SecondaryExitExtData,
    secondary_exit_ext_dirty:    bool,
    // LM v3.00 per-level main-entrance extras (SMWENTR1 RATS block). The
    // values live on `level_properties`; this flag tracks whether the RATS
    // block needs rewriting on save.
    entrance_extras_dirty:       bool,
    // Entrance selected for the extended-options panel (0x000..0x1FFF;
    // LM v2.50 type-full-values index combo).
    selected_secondary_entrance: u16,
    // Hex text of the "go to entrance" field.
    se_goto_text:                String,

    // Palette editor (12 ABGR1555 colors per group, stored as raw u16).
    // Undoable so Ctrl+Z / Ctrl+Y and the Undo/Redo buttons work like
    // Lunar Magic v1.80's palette editors.
    palettes:               UndoableData<palette_editor::EditablePalettes>,
    palette_dirty:          bool,
    selected_palette_group: u8,
    selected_palette_idx:   usize,
    // Pre-drag snapshot for gesture-style color edits (see
    // palette_editor_window); one undo step is committed per drag.
    palette_gesture_before: Option<palette_editor::EditablePalettes>,

    // Map16 editor. Undoable so Ctrl+Z / Ctrl+Y work like Lunar Magic
    // v1.91's Map16 editor. Serialized sorted-by-key, so undo deltas are
    // deterministic (a HashMap's order is not).
    map16_edits:                   UndoableData<map16_editor::EditableMap16Edits>,
    // Pre-drag snapshot for gesture-style tile-word edits; one undo step
    // is committed when the gesture ends.
    map16_gesture_before:          Option<map16_editor::EditableMap16Edits>,
    // SNES address of each block's data. Vanilla entries are populated at
    // level load; Lunar Magic extended entries are resolved on demand.
    map16_block_ptrs:              Vec<u32>,
    selected_map16_block_for_edit: Option<u16>,
    // Pending "acts like" (act-as) edits: FG tile -> act-as tile.
    map16_acts_edits:              HashMap<u16, u16>,

    // Sprite tweaker byte editor (global, per-sprite-ID behavior; shared across
    // every placement of that sprite, matching Lunar Magic's Sprite Header Editor)
    sprite_tweakers:            smwe_rom::sprite_tweakers::SpriteTweakers,
    sprite_tweakers_dirty:      bool,
    show_sprite_tweaker_editor: bool,
    tweaker_editor_sprite_id:   u8,

    // GFX (ExGFX) editor: pending raw (uncompressed) tile bytes per file
    // number, applied to the ROM on save.
    gfx_edits:           HashMap<usize, Vec<u8>>,
    show_gfx_editor:     bool,
    gfx_editor_file_num: usize,

    // Per-level GFX slot browser (FG1/FG2/FG3/BG1 + SP1-SP4) cross-linked to
    // the 8x8 tile editor.
    show_gfx_slots: bool,

    // "Change Layer 3 Settings" dialog (Lunar Magic parity): edits the
    // secondary header Layer 3 field + primary header priority bit, with a
    // WYSIWYG stripe preview and the per-level GFX bypass table.
    show_layer3_settings: bool,
    layer3_dialog:        Option<layer3_settings::Layer3SettingsDialog>,
    layer3_bypass:        smwe_rom::layer3::Layer3GfxBypass,

    // True ExGFX support (LM v1.10/v1.60 parity): extra graphics files
    // (0x80+) plus the per-level Super GFX Bypass table. The in-memory
    // copies are authoritative during the session and are written back to
    // ROM by `save_to_rom` when dirty.
    exgfx_data:           smwe_rom::exgfx::ExGfxData,
    bypass_data:          smwe_rom::exgfx::BypassData,
    exgfx_dirty:          bool,
    bypass_dirty:         bool,
    show_exgfx_manager:   bool,
    show_gfx_bypass:      bool,
    /// Super GFX Bypass dialog working copy for the current level
    /// (FG1/FG2/FG3/BG1/SP1/SP2/SP3/SP4 slot values).
    bypass_edit_slots:    [u16; smwe_rom::exgfx::BYPASS_SLOT_COUNT],
    /// Level `bypass_edit_slots` was synced from; resync when it differs.
    bypass_edit_level:    u16,
    /// Pending ExGFX insert: raw .bin bytes + chosen file index.
    exgfx_insert_pending: Option<(Vec<u8>, u16)>,
    exgfx_manager_status: Option<String>,

    // 8x8 tile (pixel) editor: staged per-file working copies of the decoded
    // tiles (applied to the ROM on save via `gfx_edits`), plus the pixel
    // editor's working state.
    show_tile_editor:         bool,
    tile_editor_file_num:     usize,
    tile_editor_palette:      usize,
    tile_editor_selected:     usize,
    tile_editor_pixels:       [u8; 64],
    tile_editor_paint_color:  u8,
    tile_editor_dirty:        bool,
    tile_editor_staged:       HashMap<usize, Vec<smwe_rom::graphics::gfx_file::Tile>>,
    tile_editor_grid_tex:     Option<egui::TextureHandle>,
    tile_editor_grid_key:     (usize, usize, u64),
    tile_editor_revision:     u64,
    tile_editor_handoff_note: Option<String>,

    // Message box (dialog text) editor: global, raw tile-index bytes.
    message_boxes:           smwe_rom::message_boxes::MessageBoxes,
    message_boxes_dirty:     bool,
    show_message_editor:     bool,
    message_editor_selected: usize,
    /// Cached `CODE_05B1BC` stripe capture for the selected message, run on a
    /// scratch CPU clone. Preview; the text uses `FontMap::real()`.
    message_preview:         Option<smwe_emu::emu::MessageStripe>,
    /// (message index, byte-hash) the stripe capture was built for.
    message_preview_for:     Option<(usize, u64)>,
    /// Cached decompressed GFX2A message font (128 2bpp tiles) for raster preview.
    message_font:            Option<Vec<Box<[u8]>>>,
    /// Cached raster texture for the selected message's 8×18 grid.
    message_raster_texture:  Option<egui::TextureHandle>,
    /// (message index, byte-hash) the raster texture was built for.
    message_raster_for:      Option<(usize, u64)>,
    /// Per-message byte budgets for the editable text field: each message's
    /// vanilla length, captured at load. The 22-message blob isn't
    /// repointable, so no message may grow past its original span.
    message_budgets:         Vec<usize>,
    /// Editable-text buffer for the selected message (decoded via
    /// `font_map::decode_editable_text`; `\n` = line break).
    message_text_edit:       String,
    /// Which message `message_text_edit` is synced to.
    message_text_for:        Option<usize>,

    // Boss sequence text editor (global, fixed-size stripe blobs in bank $0C).
    boss_text:                smwe_rom::boss_text::BossText,
    boss_text_dirty:          bool,
    show_boss_text_editor:    bool,
    boss_text_boss:           usize,
    boss_text_msg:            usize,
    boss_text_edit:           String,
    /// (boss, msg, tile-hash) the text buffer is synced to.
    boss_text_edit_for:       Option<(usize, usize, u64)>,
    boss_text_error:          Option<String>,
    /// Cached raster texture for the selected message's tile strip.
    boss_text_raster_texture: Option<egui::TextureHandle>,
    /// (boss, msg, tile-hash) the raster texture was built for.
    boss_text_raster_for:     Option<(usize, usize, u64)>,
    /// Hash of the message bytes the text buffer was last synced from (or
    /// successfully applied to); a mismatch means the raw byte grid changed
    /// the bytes and the text must be re-decoded.
    message_text_bytes_hash:  u64,
    /// Last text→bytes encode failure, shown under the text field.
    message_text_error:       Option<String>,
    /// ROM-wide cross-reference search ("find all references") window.
    show_xref_search:         bool,
    xref_search:              XrefSearchState,

    // Edit Manual dialog (Lunar Magic v1.91: raw object/sprite byte editing).
    show_edit_manual:   bool,
    edit_manual_target: Option<edit_manual_dialog::EditManualTarget>,
    edit_manual_bytes:  [String; 3],
    edit_manual_error:  Option<String>,

    // Custom Collections of Objects (Lunar Magic v3.60): per-user named
    // groups of custom extended objects. Editor configuration, not ROM
    // data — persisted to the platform config dir as JSON on every change.
    show_custom_collections: bool,
    custom_collections:      crate::custom_collections::CustomCollections,
    /// Selected collection in the manager window.
    cc_selected:             Option<usize>,
    cc_new_collection_name:  String,
    cc_rename_mode:          bool,
    cc_rename_buf:           String,
    cc_new_entry_name:       String,
    cc_new_entry_id:         String,
    /// (collection, entry) being edited inline, with its text buffers.
    cc_edit_entry:           Option<(usize, usize)>,
    cc_edit_name:            String,
    cc_edit_id:              String,
    cc_status:               Option<String>,
    /// (collection, entry) armed for canvas placement in Draw mode; the next
    /// click places the custom extended object instead of the paint block.
    draw_custom_entry:       Option<(usize, usize)>,

    // Custom object tooltips (Lunar Magic v3.60: user-settable tooltip text
    // for objects). Per-user store — never written to the ROM.
    custom_tooltips:      crate::custom_tooltips::CustomTooltips,
    show_custom_tooltips: bool,
    tooltip_kind:         crate::custom_tooltips::ObjectKind,
    tooltip_selected_id:  u8,
    tooltip_edit_text:    String,
    tooltip_search:       String,

    // ExAnimation (custom per-level tile/palette animation) editor.
    exanimation:             smwe_rom::exanimation::ExAnimationData,
    exanimation_dirty:       bool,
    show_exanimation_editor: bool,

    // Sprite header editor ("Change Properties in Sprite Header", LM v3.00).
    show_sprite_header_editor: bool,
    /// Working copy of the 1-byte sprite header (sprite memory / buoyancy /
    /// Layer 2 interaction).
    sprite_header_edit:        smwe_rom::level::headers::SpriteHeader,
    /// Session-authoritative header bytes for levels edited in the dialog.
    /// `self.rom` is never refreshed after a save, so `load_level` prefers
    /// this map over the stale parse — otherwise a second edit after a save
    /// would silently revert the first.
    sprite_header_edits:       HashMap<u16, smwe_rom::level::headers::SpriteHeader>,
    /// Working copy of the LM 3.00 per-level options (editor-native
    /// `sprite_header_ext` RATS block). Session-authoritative like the
    /// ExAnimation working copy: synced once from the ROM at construction,
    /// never re-read from the (post-save stale) `self.rom`.
    sprite_header_ext:         smwe_rom::level::sprite_header_ext::SpriteHeaderExtData,
    sprite_header_dirty:       bool,
    /// Whether the user touched any LM 3.40 scroll-extension control
    /// (Separate H/V, H/V scroll, Auto-Set Screens) since the level was
    /// loaded. The `$06FA00` table is only installed on ROMs that lack it when
    /// this is set — vanilla ROMs keep `$FF` and are never auto-resized
    /// behind the user's back.
    scroll_ext_dirty:          bool,

    /// Dialog state for the shared "ExAnimated Frames" window.
    exanim_dialog:         crate::ui::exanimation_dialog::ExAnimDialog,
    /// Clean post-load VRAM snapshot the tile browser decodes from.
    exanimation_base_vram: Vec<u8>,

    // Direct Map16 (Lunar Magic v1.70-v1.90 "Add Objects / Direct Map16"
    // parity): per-level objects, undoable, stamped into the WRAM block map
    // on load and after every edit. Distinct from vanilla objects — saved
    // to the editor-native RATS block, never the object stream.
    direct_map16:          UndoableData<EditableDirectMap16>,
    dm16_dirty:            bool,
    /// Vanilla (DM16-free) layer-1 block map snapshot: `dm16_base_lo/hi[i]`
    /// mirror the WRAM bytes at 0x7EC800/0x7FC800 taken right after level
    /// decompression, kept in sync with every vanilla `set_block_id_at`
    /// write. `rerasterize_dm16` restores from it, so vanilla tiles hidden
    /// under DM16 objects are never lost on DM16 edit/undo/redo/delete.
    dm16_base_lo:          Vec<u8>,
    dm16_base_hi:          Vec<u8>,
    selected_dm16_indices: HashSet<usize>,
    /// Redo routing: true when the last undo targeted the DM16 layer (any
    /// selection change or edit elsewhere disarms it).
    last_undo_was_dm16:    bool,
    /// "Add Objects / Direct Map16" window visibility.
    dm16_add_open:         bool,
    /// Selected rectangle in the Map16 picker: (block_x, block_y, w, h).
    dm16_selection:        Option<(u32, u32, u32, u32)>,
    /// In-progress drag on the Add Objects Map16 grid (start block).
    dm16_sel_drag_start:   Option<(u32, u32)>,
    /// Armed placement: next canvas click drops this pattern as one object.
    dm16_placing:          Option<Dm16Placement>,
    /// Conditional Direct Map16 dialog: index of the object being edited.
    dm16_cond_open:        Option<usize>,
    /// Conditional dialog fields: RAM address + bit (8 = nonzero byte).
    dm16_cond_addr:        u32,
    dm16_cond_bit:         i32,
    /// Remap Direct Map16 dialog visibility + rows of (old, new) tile IDs.
    dm16_remap_open:       bool,
    dm16_remap_rows:       Vec<(u32, u32)>,
    /// Status line for Direct Map16 actions (surfaced via the MWL status line).
    dm16_status:           Option<String>,

    // Title screen / ending credits fixed-location data.
    title_credits:             smwe_rom::title_credits::TitleCreditsData,
    title_credits_dirty:       bool,
    show_title_credits_editor: bool,
    credits_editor_selected:   usize,
    // Credits WYSIWYG editor: per-scene L3 text grid, preserved non-L3
    // commands, and render cache. The grid is 64x64 but only rows
    // CREDITS_L3_FIRST_ROW..=CREDITS_L3_LAST_ROW are editable.
    credits_grid:              Option<smwe_rom::title_stripe::TitleTileGrid>,
    credits_non_l3:            Vec<smwe_rom::title_stripe::TitleStripeCommand>,
    credits_grid_tex:          Option<egui::TextureHandle>,
    credits_grid_for_scene:    Option<usize>,
    credits_grid_error:        Option<String>,
    credits_gfx2f_tiles:       Option<Vec<smwe_rom::graphics::gfx_file::Tile>>,
    // Title screen WYSIWYG stripe editor: parsed 64x64 tile grid, its
    // rendered preview, and the VRAM/CGRAM snapshots the preview is drawn
    // from (captured from a scratch CPU running the real title init).
    title_grid:                Option<smwe_rom::title_stripe::TitleTileGrid>,
    title_grid_tex:            Option<egui::TextureHandle>,
    title_grid_vram:           Option<Vec<u8>>,
    title_grid_cgram:          Option<Vec<u8>>,
    title_grid_for_stripe_len: Option<usize>,
    title_selected_cell:       Option<(usize, usize)>,
    title_paint_word:          u16,
    title_grid_error:          Option<String>,
    // Player-select menu stripe (composited over the logo grid). The menu
    // owns rows MENU_FIRST_ROW..=MENU_LAST_ROW; its RLE clear commands are
    // preserved verbatim and not editable.
    menu_grid:                 Option<smwe_rom::title_stripe::TitleTileGrid>,
    menu_clears:               Vec<smwe_rom::title_stripe::TitleStripeCommand>,
    menu_grid_for_stripe_len:  Option<usize>,

    // Lunar Magic `.mwl` level import/export.
    rom_path:   PathBuf,
    mwl_status: Option<String>,

    // Map16 page import/export.
    map16_file_status:   Option<String>,
    // Page selector: FG or BG, page 0x00-0x7F (0x00/0x01 vanilla, 0x02+
    // Lunar Magic expanded pages).
    map16_page_fg:       bool,
    map16_page:          u8,
    map16_tileset_idx:   usize,
    // "Remap act-as…" dialog state.
    map16_remap_open:    bool,
    map16_remap_mode_g:  bool, // true = remap references (G), false = assign range from base (R)
    map16_remap_src:     String,
    map16_remap_ref:     String,
    map16_remap_preview: Option<String>,
}

impl UiLevelEditor {
    pub fn new(gl: Arc<glow::Context>, rom: Arc<SmwRom>, rom_path: PathBuf) -> anyhow::Result<Self> {
        let level_renderer = Arc::new(Mutex::new(LevelRenderer::new(&gl)));

        let raw = std::fs::read(&rom_path)
            .map_err(|e| anyhow::anyhow!("Cannot read ROM for emulator at {}: {e}", rom_path.display()))?;
        let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
        // Per-level Layer 3 GFX bypass table (RATS L3BP block), if the ROM has one.
        let layer3_bypass = smwe_rom::layer3::Layer3GfxBypass::load(&rom_bytes);
        let mut emu_rom = EmuRom::new(rom_bytes);
        emu_rom.load_symbols(include_str!("../../../../symbols/SMW_U.sym"));
        let cpu = smwe_emu::Cpu::new(CheckedMem::new(Arc::new(emu_rom)));
        let sprite_tweakers = rom.sprite_tweakers.clone();
        let message_boxes = rom.message_boxes.clone();
        // Per-message byte budgets for the editable text field: each
        // message's vanilla length at load. The blob isn't repointable, so
        // text edits may not grow a message past its original span.
        let message_budgets: Vec<usize> = message_boxes.messages.iter().map(Vec::len).collect();
        let title_credits = rom.title_credits.clone();
        let boss_text = rom.boss_text.clone();
        let exanimation = rom.exanimation.clone();
        let sprite_header_ext = rom.sprite_header_ext.clone();
        // Clone before `rom` moves into the struct literal below.
        let exgfx_data = rom.exgfx.clone();
        let bypass_data = rom.gfx_bypass.clone();

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
            mark_exit_tiles: false,
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
            anim_tick: 0,
            editing_mode: EditingMode::Select,
            selected_object_indices: HashSet::new(),
            selected_sprite_indices: HashSet::new(),
            map16_window_hovered: false,
            tile_editor_window_hovered: false,
            clipboard_copy_origin: None,
            object_drag: None,
            suppress_click_select: false,
            draw_object_id: 0x00,
            draw_object_settings: 0x00,
            draw_block_id: 0x25,
            draw_sprite_id: 0x00,
            draw_sprite_extra_bits: 0x00,
            edit_layer: 1,
            edit_sprites: false,
            spawn: UndoableData::new(SpawnPos::default()),
            initial_spawn_x: 0,
            initial_spawn_y: 0,
            dragging_spawn: false,
            entrance_selected: false,
            spawn_undo_pending: false,
            spawn_redo_pending: false,
            spawn_drag_before: None,
            show_unsaved_dialog: false,
            pending_level_num: None,
            has_edits: false,
            request_rom_save: false,
            pending_close: false,
            show_level_header: false,
            show_secondary_entrances: false,
            show_palette_editor: false,
            show_map16_editor: false,
            show_bg_tilemap_editor: false,
            bg_tool: bg_tilemap_editor::BgTileTool::Paint,
            bg_selected_tile: 0,
            bg_page: 0,
            bg_zoom: 2.0,
            bg_show_grid: true,
            bg_selection: None,
            bg_canvas_tex: None,
            bg_canvas_dirty: true,
            bg_selector_tex: None,
            bg_block_words: Vec::new(),
            bg_status: None,
            bg_offset_open: false,
            bg_offset_val: 16,
            bg_bank_open: false,
            bg_bank_choice: 0,
            bg_prev_page: None,
            bg_drag: None,
            secondary_entrance_data: Vec::new(),
            secondary_entrance_search: String::new(),
            secondary_exit_ext: smwe_rom::level::secondary_entrance::SecondaryExitExtData::default(),
            secondary_exit_ext_dirty: false,
            entrance_extras_dirty: false,
            selected_secondary_entrance: 0,
            se_goto_text: String::new(),
            palettes: UndoableData::new(palette_editor::EditablePalettes::default()),
            palette_dirty: false,
            selected_palette_group: 3, // none
            selected_palette_idx: 0,
            palette_gesture_before: None,
            map16_edits: UndoableData::new(map16_editor::EditableMap16Edits::default()),
            map16_acts_edits: HashMap::new(),
            map16_gesture_before: None,
            map16_block_ptrs: Vec::new(),
            selected_map16_block_for_edit: None,
            sprite_tweakers,
            sprite_tweakers_dirty: false,
            show_sprite_tweaker_editor: false,
            tweaker_editor_sprite_id: 0,
            gfx_edits: HashMap::new(),
            show_gfx_editor: false,
            show_gfx_slots: false,
            show_layer3_settings: false,
            layer3_dialog: None,
            layer3_bypass,
            gfx_editor_file_num: 0,
            exgfx_data,
            bypass_data,
            exgfx_dirty: false,
            bypass_dirty: false,
            show_exgfx_manager: false,
            show_gfx_bypass: false,
            bypass_edit_slots: [smwe_rom::exgfx::BYPASS_DEFAULT; smwe_rom::exgfx::BYPASS_SLOT_COUNT],
            bypass_edit_level: 0xFFFF,
            exgfx_insert_pending: None,
            exgfx_manager_status: None,
            show_tile_editor: false,
            tile_editor_file_num: 0,
            tile_editor_palette: 0,
            tile_editor_selected: 0,
            tile_editor_pixels: [0u8; 64],
            tile_editor_paint_color: 1,
            tile_editor_dirty: false,
            tile_editor_staged: HashMap::new(),
            tile_editor_grid_tex: None,
            tile_editor_grid_key: (usize::MAX, usize::MAX, u64::MAX),
            tile_editor_revision: 0,
            tile_editor_handoff_note: None,
            message_boxes,
            message_boxes_dirty: false,
            show_message_editor: false,
            message_editor_selected: 0,
            message_preview: None,
            message_preview_for: None,
            message_font: None,
            message_raster_texture: None,
            message_raster_for: None,
            message_budgets,
            message_text_edit: String::new(),
            message_text_for: None,
            message_text_bytes_hash: 0,
            message_text_error: None,
            boss_text,
            boss_text_dirty: false,
            show_boss_text_editor: false,
            boss_text_boss: 0,
            boss_text_msg: 0,
            boss_text_edit: String::new(),
            boss_text_edit_for: None,
            boss_text_error: None,
            boss_text_raster_texture: None,
            boss_text_raster_for: None,
            show_xref_search: false,
            xref_search: XrefSearchState::default(),
            show_edit_manual: false,
            edit_manual_target: None,
            edit_manual_bytes: [String::new(), String::new(), String::new()],
            edit_manual_error: None,
            // Custom Collections of Objects (Lunar Magic v3.60).
            show_custom_collections: false,
            custom_collections: crate::custom_collections::CustomCollections::load(),
            cc_selected: None,
            cc_new_collection_name: String::new(),
            cc_rename_mode: false,
            cc_rename_buf: String::new(),
            cc_new_entry_name: String::new(),
            cc_new_entry_id: String::new(),
            cc_edit_entry: None,
            cc_edit_name: String::new(),
            cc_edit_id: String::new(),
            cc_status: None,
            draw_custom_entry: None,
            // Custom object tooltips (Lunar Magic v3.60).
            custom_tooltips: crate::custom_tooltips::CustomTooltips::load(),
            show_custom_tooltips: false,
            tooltip_kind: crate::custom_tooltips::ObjectKind::Standard,
            tooltip_selected_id: 0x2B,
            tooltip_edit_text: String::new(),
            tooltip_search: String::new(),
            exanimation,
            exanimation_dirty: false,
            show_exanimation_editor: false,
            show_sprite_header_editor: false,
            sprite_header_edit: smwe_rom::level::headers::SpriteHeader::new(0),
            sprite_header_edits: HashMap::new(),
            sprite_header_ext,
            sprite_header_dirty: false,
            scroll_ext_dirty: false,
            exanim_dialog: crate::ui::exanimation_dialog::ExAnimDialog::new(
                crate::ui::exanimation_dialog::ExAnimList::Level,
            ),
            exanimation_base_vram: Vec::new(),
            direct_map16: UndoableData::new(EditableDirectMap16::default()),
            dm16_dirty: false,
            dm16_base_lo: Vec::new(),
            dm16_base_hi: Vec::new(),
            selected_dm16_indices: HashSet::new(),
            last_undo_was_dm16: false,
            dm16_add_open: false,
            dm16_selection: None,
            dm16_sel_drag_start: None,
            dm16_placing: None,
            dm16_cond_open: None,
            dm16_cond_addr: 0x13CE,
            dm16_cond_bit: 0,
            dm16_remap_open: false,
            dm16_remap_rows: Vec::new(),
            dm16_status: None,
            title_credits,
            title_credits_dirty: false,
            show_title_credits_editor: false,
            credits_editor_selected: 0,
            credits_grid: None,
            credits_non_l3: Vec::new(),
            credits_grid_tex: None,
            credits_grid_for_scene: None,
            credits_grid_error: None,
            credits_gfx2f_tiles: None,
            title_grid: None,
            title_grid_tex: None,
            title_grid_vram: None,
            title_grid_cgram: None,
            title_grid_for_stripe_len: None,
            title_selected_cell: None,
            title_paint_word: 0x2C58,
            title_grid_error: None,
            menu_grid: None,
            menu_clears: Vec::new(),
            menu_grid_for_stripe_len: None,
            rom_path: rom_path.clone(),
            mwl_status: None,
            map16_file_status: None,
            map16_page_fg: true,
            map16_page: 0x00,
            map16_remap_open: false,
            map16_remap_mode_g: true,
            map16_remap_src: String::new(),
            map16_remap_ref: String::new(),
            map16_remap_preview: None,
            map16_tileset_idx: 0,
        };
        editor.load_level();
        Ok(editor)
    }

    /// LM 3.40 "Auto-Set Number of Screens": the header length byte to write
    /// on save, or `None` when auto-set does not apply.
    ///
    /// The per-level C bit of the `$06FA00` extension byte only takes effect
    /// once the table is installed (or this save installs it because the
    /// user touched the scroll-extension controls), so a vanilla ROM is never
    /// resized by an untouched default.
    fn auto_set_screens_len(&self, vertical: bool) -> Option<u8> {
        let p = &self.level_properties;
        if !auto_screens::auto_set_applies(p.layer2_scroll_ext_raw, self.scroll_ext_dirty, p.layer2_auto_set_screens) {
            return None;
        }
        let used = self.layer1.read(|l1| {
            self.sprites.read(|sprites| match &self.layer2_objects {
                Some(l2) => l2.read(|l2l| auto_screens::screens_used(l1, Some(l2l), sprites, vertical)),
                None => auto_screens::screens_used(l1, None, sprites, vertical),
            })
        });
        Some((used - 1) as u8)
    }

    /// The `$06FA00` byte to write on save: the table is (re)installed when it
    /// was already installed, separate H/V mode is on, or the user touched
    /// any scroll-extension control since load; otherwise vanilla ROMs keep
    /// `$FF` (never installed behind the user's back).
    fn scroll_ext_save_byte(&self) -> u8 {
        use smwe_rom::level::scroll::{Layer2ScrollExt, SCROLL_EXT_UNINSTALLED};
        let p = &self.level_properties;
        let install =
            auto_screens::scroll_ext_installs(p.layer2_scroll_ext_raw, p.layer2_scroll_separate, self.scroll_ext_dirty);
        if install {
            Layer2ScrollExt {
                separate:         p.layer2_scroll_separate,
                h_auto:           p.layer2_hscroll_auto,
                auto_set_screens: p.layer2_auto_set_screens,
                vscroll:          p.layer2_vscroll,
            }
            .encode()
        } else {
            SCROLL_EXT_UNINSTALLED
        }
    }
}

// UI
impl DockableEditorTool for UiLevelEditor {
    fn update(&mut self, ui: &mut Ui) {
        // Floating editor windows rendered before panels so they draw on top
        let ctx = ui.ctx().clone();
        self.level_header_panel_window(&ctx);
        self.secondary_entrance_editor_window(&ctx);
        self.palette_editor_window(&ctx);
        self.map16_editor_window(&ctx);
        self.map16_remap_window(&ctx);
        self.dm16_windows(ui);
        self.sprite_tweaker_editor_window(&ctx);
        self.sprite_header_editor_window(&ctx);
        self.gfx_editor_window(&ctx);
        self.gfx_slot_browser_window(&ctx);
        self.layer3_settings_window(&ctx);
        self.tile_editor_window(&ctx);
        self.exgfx_manager_window(&ctx);
        self.gfx_bypass_window(&ctx);
        self.message_editor_window(&ctx);
        self.boss_text_editor_window(&ctx);
        if self.show_exanimation_editor {
            let mut open = self.show_exanimation_editor;
            let changed = self.exanim_dialog.show(
                &ctx,
                &mut open,
                &mut self.exanimation,
                &self.exanimation_base_vram,
                &self.cpu.mem.cgram,
                self.level_num as u64,
                Some(self.level_num),
            );
            self.show_exanimation_editor = open;
            if changed {
                self.exanimation_dirty = true;
                self.has_edits = true;
            }
        }
        self.xref_search_window(&ctx);
        self.edit_manual_window(&ctx);
        self.custom_collections_window(&ctx);
        self.custom_tooltips_window(&ctx);
        self.title_credits_editor_window(&ctx);
        self.bg_tilemap_editor_window(&ctx);
        // Lunar Magic-style top toolbar + bottom status bar wrap the editor.
        TopBottomPanel::top("level_editor.toolbar").show_inside(ui, |ui| self.toolbar(ui));
        TopBottomPanel::bottom("level_editor.status").show_inside(ui, |ui| self.status_bar(ui));
        SidePanel::left("level_editor.left_panel").resizable(false).show_inside(ui, |ui| self.left_panel(ui));
        CentralPanel::default().frame(Frame::NONE.inner_margin(0.)).show_inside(ui, |ui| self.central_panel(ui));
    }

    fn title(&self) -> WidgetText {
        "Level Editor".into()
    }

    fn on_closed(&mut self) {
        self.level_renderer.lock().unwrap().destroy(&self.gl);
    }

    fn on_close_attempt_blocked(&mut self) {
        self.pending_close = true;
    }

    fn level_number(&self) -> Option<u16> {
        Some(self.level_num)
    }

    /// "Change Layer 3 Settings" dialog window.

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
        //
        // LM 3.40 "Auto-Set Number of Screens": when the per-level C bit of
        // the `$06FA00` extension byte is set (and the table is installed),
        // the header's Number of Screens is rewritten on save to the screens
        // actually occupied by the level's objects and sprites.
        let p = &self.level_properties;
        let auto_len = self.auto_set_screens_len(vertical);
        if let Some(auto_len) = auto_len {
            if auto_len != p.level_length {
                log::info!(
                    "Level {:03X}: Auto-Set Number of Screens {} -> {} screens",
                    self.level_num,
                    p.level_length as u32 + 1,
                    auto_len as u32 + 1
                );
            }
        }
        let level_length = auto_len.unwrap_or(p.level_length);
        let new_primary_header: [u8; PRIMARY_HEADER_SIZE] = [
            (p.palette_bg << 5) | level_length,
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
            let old_snes =
                read_u24(rom_bytes, ptr_off).ok_or_else(|| anyhow::anyhow!("L1 pointer table out of range"))?;
            let old_file = AddrPc::try_from_lorom(AddrSnes(old_snes))?.as_index() + header_offset;

            let old_block = PRIMARY_HEADER_SIZE + level.layer1.as_bytes().len();
            let new_block = PRIMARY_HEADER_SIZE + new_l1.len();

            let dest = if new_block <= old_block {
                old_file
            } else {
                let pc = find_free_space(rom_bytes, new_block, 0x008000, header_offset).ok_or_else(|| {
                    anyhow::anyhow!("No free space for level {:03X} layer 1 ({} bytes)", self.level_num, new_block)
                })?;
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
            let old_offset =
                read_u16(rom_bytes, ptr_off).ok_or_else(|| anyhow::anyhow!("Sprite pointer table out of range"))?;
            let old_snes = AddrSnes(old_offset as u32 | 0x070000);
            let old_file = AddrPc::try_from_lorom(old_snes)?.as_index() + header_offset;

            // Read sprite-header byte before any possible erasure.
            let sprite_hdr =
                *rom_bytes.get(old_file).ok_or_else(|| anyhow::anyhow!("Sprite header byte out of range"))?;
            // "Change Properties in Sprite Header" (LM v3.00): when the
            // dialog edited the header, write the edited byte instead of the
            // original.
            let sprite_hdr = if self.sprite_header_dirty { self.sprite_header_edit.as_byte() } else { sprite_hdr };
            let old_block = 1 + level.sprite_layer.as_bytes().len();
            let new_block = 1 + new_sprites.len();

            let dest = if new_block <= old_block {
                old_file
            } else {
                // Must stay in bank $07: SNES $078000-$07FFFF = PC $038000-$03FFFF.
                let bank7_start = AddrPc::try_from_lorom(AddrSnes(0x078000))?.as_index();
                let bank7_end = bank7_start + 0x8000;
                let pc = find_free_space_in(rom_bytes, new_block, bank7_start, bank7_end, header_offset).ok_or_else(
                    || {
                        anyhow::anyhow!(
                            "No free space in bank $07 for level {:03X} sprite data ({} bytes)",
                            self.level_num,
                            new_block
                        )
                    },
                )?;
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

            // ── LM 3.00 sprite-header options ("Change Properties in Sprite
            // Header") ── Merge the session-authoritative working copy into
            // the editor-native RATS block. Entries removed from the working
            // copy (both options back to default) are cleared from the ROM
            // block too, so the block disappears again when empty.
            if self.sprite_header_dirty {
                use smwe_rom::level::sprite_header_ext::{SpriteHeaderExtData, SpriteHeaderExtError};
                let mut merged = match SpriteHeaderExtData::parse(rom_bytes) {
                    Ok(data) => data,
                    Err(SpriteHeaderExtError::NotFound) => SpriteHeaderExtData::default(),
                    Err(e) => return Err(anyhow::anyhow!("Sprite header options: {e}")),
                };
                let map_err = |e: SpriteHeaderExtError| anyhow::anyhow!("Sprite header options: {e}");
                for (level, _) in merged.iter().collect::<Vec<_>>() {
                    if !self.sprite_header_ext.is_custom(level) {
                        merged.clear(level);
                    }
                }
                for (level, ext) in self.sprite_header_ext.iter() {
                    merged.set(level, ext).map_err(map_err)?;
                }
                merged
                    .write_to_rom(rom_bytes, header_offset)
                    .map_err(|e| anyhow::anyhow!("Sprite header options: {e}"))?;
            }
        }

        // ── Layer 2  (pointer table $05E600, 3-byte value each) ────────────────
        // If the pointer's bank byte == $FF the data is background (LC-RLE1) at
        // SNES bank $0C with the same 16-bit offset.  Otherwise it is object
        // data with the same block layout as layer 1.
        {
            let tbl_pc = AddrPc::try_from_lorom(AddrSnes(0x05E600))?.as_index();
            let ptr_off = tbl_pc + header_offset + level_idx * 3;
            let l2_raw =
                read_u24(rom_bytes, ptr_off).ok_or_else(|| anyhow::anyhow!("L2 pointer table out of range"))?;

            match (&level.layer2, &self.layer2_objects, &self.layer2_background) {
                (Layer2Data::Objects { objects, .. }, Some(layer2), _) => {
                    let new_l2 = layer2.read(|l| l.serialize_layer1_bytes(vertical))?;
                    let old_file = AddrPc::try_from_lorom(AddrSnes(l2_raw))?.as_index() + header_offset;

                    // The 5-byte L2 header is user-editable (Level Header
                    // window); write the edited bytes instead of copying the
                    // old ones verbatim. The game skips them, so in-place
                    // edits only change the stored bytes.
                    let new_l2_header = self.level_properties.layer2_header;
                    let old_block = LAYER2_HEADER_SIZE + objects.as_bytes().len();
                    let new_block = LAYER2_HEADER_SIZE + new_l2.len();

                    let dest = if new_block <= old_block {
                        old_file
                    } else {
                        let pc = find_free_space(rom_bytes, new_block, 0x008000, header_offset).ok_or_else(|| {
                            anyhow::anyhow!(
                                "No free space for level {:03X} layer 2 ({} bytes)",
                                self.level_num,
                                new_block
                            )
                        })?;
                        rom_bytes[old_file..old_file + old_block].fill(0xFF);
                        let b = AddrSnes::try_from_lorom(AddrPc(pc as u32))?.0.to_le_bytes();
                        rom_bytes[ptr_off..ptr_off + 3].copy_from_slice(&b[..3]);
                        pc + header_offset
                    };

                    rom_bytes[dest..dest + LAYER2_HEADER_SIZE].copy_from_slice(&new_l2_header);
                    let data_dest = dest + LAYER2_HEADER_SIZE;
                    rom_bytes[data_dest..data_dest + new_l2.len()].copy_from_slice(&new_l2);
                    if dest == old_file && new_block < old_block {
                        rom_bytes[dest + new_block..dest + old_block].fill(0xFF);
                    }
                }
                (Layer2Data::Background(_background), _, Some(layer2)) => {
                    let new_bg = layer2.read(|bg| bg.tile_ids.clone());
                    // Bank-aware write: reuses the old location when the data
                    // fits and the bank is unchanged; otherwise repoints into
                    // free space on the correct side of the $E8FE boundary so
                    // the game fills the matching tilemap high byte.
                    smwe_rom::level::background::write_background_to_rom(
                        rom_bytes,
                        level_idx as u32,
                        &new_bg,
                        self.bg_page,
                        header_offset,
                    )
                    .map_err(|e| anyhow::anyhow!("Layer 2 background: {e}"))?;
                }
                _ => {}
            }
        }

        // ── Secondary header byte tables ($05F000/$05F200/$05F400/$05F600) ─────
        // Fully reconstruct all four bytes from level_properties + spawn position.
        {
            let p = &self.level_properties;
            let (spawn_x, spawn_y) = self.spawn_pos();
            let (entrance_screen, local_x, local_y) = if p.is_vertical {
                let sx = spawn_x / 16;
                let sy = spawn_y / 32;
                let screen = ((sy * 2 + sx) as u8).min(31);
                let x = (spawn_x % 16) as u8;
                let y = (spawn_y % 32) as u8;
                (screen, x, y)
            } else {
                let screen = ((spawn_x / 16) as u8).min(31);
                let x = (spawn_x % 16) as u8;
                let y = spawn_y as u8;
                (screen, x, y)
            };
            let entrance_x_half = (local_x / 2).min(7);
            let entrance_y_half = (local_y / 2).min(15);

            // Byte 0: SSSSSYYY Y  (layer2_scroll[3:0] | entrance_y[3:0])
            let t0 = AddrPc::try_from_lorom(AddrSnes(0x05F000))?.as_index() + header_offset + level_idx;
            if let Some(b) = rom_bytes.get_mut(t0) {
                *b = (p.layer2_scroll << 4) | (entrance_y_half & 0x0F);
            }
            // Byte 1: LLAAAXXX  (layer3[1:0] | action[2:0] | entrance_x[2:0])
            let t1 = AddrPc::try_from_lorom(AddrSnes(0x05F200))?.as_index() + header_offset + level_idx;
            if let Some(b) = rom_bytes.get_mut(t1) {
                *b = ((p.layer3 & 0x3) << 6) | ((p.main_entrance_action & 0x7) << 3) | (entrance_x_half & 0x07);
            }
            // Byte 2: SSSSFFBB  (midway_screen[3:0] | fg_initial_pos[1:0] | bg_initial_pos[1:0])
            let t2 = AddrPc::try_from_lorom(AddrSnes(0x05F400))?.as_index() + header_offset + level_idx;
            if let Some(b) = rom_bytes.get_mut(t2) {
                *b = ((p.midway_entrance_screen & 0xF) << 4)
                    | ((p.fg_initial_pos & 0x3) << 2)
                    | (p.bg_initial_pos & 0x3);
            }
            // Byte 3: YUVREEEE  (no_yoshi | unknown_vert | vertical | entrance_screen[4:0])
            let t3 = AddrPc::try_from_lorom(AddrSnes(0x05F600))?.as_index() + header_offset + level_idx;
            if let Some(b) = rom_bytes.get_mut(t3) {
                *b = ((p.no_yoshi_level as u8) << 7)
                    | ((p.unknown_vertical_pos_level as u8) << 6)
                    | ((p.is_vertical as u8) << 5)
                    | (entrance_screen & 0x1F);
            }
        }

        // ── LM 3.40+ Layer 2 scroll extension ($06FA00, SHCvvvvv) ──────────────
        // Preserve the raw byte when the table was never installed ($FF) and
        // the user did not touch any scroll-extension control; otherwise
        // encode the new settings. Touching the Auto-Set Screens checkbox
        // alone installs the table, so the per-level setting actually
        // persists (and takes effect) on vanilla ROMs.
        {
            let new_byte = self.scroll_ext_save_byte();
            let t = AddrPc::try_from_lorom(AddrSnes(0x06FA00))?.as_index() + header_offset + level_idx;
            if let Some(b) = rom_bytes.get_mut(t) {
                *b = new_byte;
            }
        }

        // ── Secondary entrance tables ($05F800 / $05FA00 / $05FC00 / $05FE00) ──
        // Four separate 512-byte tables, one per byte-lane of each entrance.
        if self.secondary_entrance_data.len() == 512 {
            let table_bases = [0x05F800u32, 0x05FA00, 0x05FC00, 0x05FE00];
            for (byte_i, &snes_base) in table_bases.iter().enumerate() {
                let pc_base = AddrPc::try_from_lorom(AddrSnes(snes_base))?.as_index() + header_offset;
                for (idx, bytes) in self.secondary_entrance_data.iter().enumerate() {
                    if let Some(b) = rom_bytes.get_mut(pc_base + idx) {
                        *b = bytes[byte_i];
                    }
                }
            }
        }

        // ── Secondary-exit extended options (LM v3.00, editor RATS block) ──
        // Single RATS-tagged free-space block, shared with the world-map
        // editor's teleport-table edits. Merge on save: re-read the block and
        // replace only this tab's pieces (per-entrance options + extended
        // entries) so a stale teleport-table copy is never clobbered.
        if self.secondary_exit_ext_dirty {
            use smwe_rom::level::secondary_entrance::{SecExitExtError, SecondaryExitExtData};
            let mut merged = match SecondaryExitExtData::parse(rom_bytes) {
                Ok(data) => data,
                Err(SecExitExtError::NotFound) => SecondaryExitExtData::default(),
                Err(e) => anyhow::bail!("Secondary-exit extended-data read failed: {e}"),
            };
            merged.options = self.secondary_exit_ext.options.clone();
            merged.extended_entries = self.secondary_exit_ext.extended_entries.clone();
            merged
                .write_to_rom(rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("Secondary-exit extended-data write failed: {e}"))?;
        }

        // ── LM v3.00 per-level entrance extras (SMWENTR1 RATS block) ─────────
        // The values live on `level_properties`; merge on save: re-read the
        // block and replace only this level's entry so other levels' extras
        // are never clobbered.
        if self.entrance_extras_dirty {
            use smwe_rom::level::entrance_extras::{EntranceExtrasError, LevelEntranceExtras, LevelEntranceExtrasData};
            let mut merged = match LevelEntranceExtrasData::parse(rom_bytes) {
                Ok(data) => data,
                Err(EntranceExtrasError::NotFound) => LevelEntranceExtrasData::default(),
                Err(e) => anyhow::bail!("Entrance-extras read failed: {e}"),
            };
            let p = &self.level_properties;
            merged.set(self.level_num, LevelEntranceExtras {
                face_left:              p.face_left,
                new_fg_bg_init:         p.new_fg_bg_init,
                bg_relative_to_fg_only: p.bg_relative_to_fg_only,
                bg_height:              p.bg_height,
            });
            merged
                .write_to_rom(rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("Entrance-extras write failed: {e}"))?;
        }

        // ── Sprite tweaker bytes (global, $07F26C/$07F335/$07F3FE/$07F4C7/$07F590/$07F659) ──
        if self.sprite_tweakers_dirty {
            let tables = [
                (smwe_rom::sprite_tweakers::SPRITE_TWEAKER_A_SNES, &self.sprite_tweakers.tweaker_a),
                (smwe_rom::sprite_tweakers::SPRITE_TWEAKER_B_SNES, &self.sprite_tweakers.tweaker_b),
                (smwe_rom::sprite_tweakers::SPRITE_TWEAKER_C_SNES, &self.sprite_tweakers.tweaker_c),
                (smwe_rom::sprite_tweakers::SPRITE_TWEAKER_D_SNES, &self.sprite_tweakers.tweaker_d),
                (smwe_rom::sprite_tweakers::SPRITE_TWEAKER_E_SNES, &self.sprite_tweakers.tweaker_e),
                (smwe_rom::sprite_tweakers::SPRITE_TWEAKER_F_SNES, &self.sprite_tweakers.tweaker_f),
            ];
            for (snes_addr, table) in tables {
                let pc_base = AddrPc::try_from_lorom(snes_addr)?.as_index() + header_offset;
                for (idx, &byte) in table.iter().enumerate() {
                    if let Some(b) = rom_bytes.get_mut(pc_base + idx) {
                        *b = byte;
                    }
                }
            }
        }

        // ── GFX (ExGFX) edits: LC_LZ2-compress raw tile bytes, repoint if needed ──
        for (&file_num, raw_bytes) in &self.gfx_edits {
            let compressed = lc_lz2::compress(raw_bytes);
            let is_repointable = file_num < gfx_file::GFX_POINTER_TABLE_LEN;

            let ptr_pcs: Option<(usize, usize, usize)> = if is_repointable {
                Some((
                    AddrPc::try_from_lorom(gfx_file::GFX_POINTER_TABLE_LOW + file_num)?.as_index() + header_offset,
                    AddrPc::try_from_lorom(gfx_file::GFX_POINTER_TABLE_HIGH + file_num)?.as_index() + header_offset,
                    AddrPc::try_from_lorom(gfx_file::GFX_POINTER_TABLE_BANK + file_num)?.as_index() + header_offset,
                ))
            } else {
                None
            };

            let cur_addr = if let Some((low_pc, high_pc, bank_pc)) = ptr_pcs {
                let low = *rom_bytes.get(low_pc).ok_or_else(|| anyhow::anyhow!("GFX pointer table out of range"))?;
                let high = *rom_bytes.get(high_pc).ok_or_else(|| anyhow::anyhow!("GFX pointer table out of range"))?;
                let bank = *rom_bytes.get(bank_pc).ok_or_else(|| anyhow::anyhow!("GFX pointer table out of range"))?;
                AddrSnes(((bank as u32) << 16) | ((high as u32) << 8) | (low as u32))
            } else {
                gfx_file::declared_addr(file_num)
            };
            let old_pc = AddrPc::try_from_lorom(cur_addr)?.as_index() + header_offset;
            let old_len = if old_pc < rom_bytes.len() {
                lc_lz2::decompress_with_len(&rom_bytes[old_pc..], false).map(|(_, len)| len).unwrap_or(0)
            } else {
                0
            };

            let dest = if compressed.len() <= old_len {
                old_pc
            } else if is_repointable {
                let pc = find_free_space(rom_bytes, compressed.len(), 0x008000, header_offset).ok_or_else(|| {
                    anyhow::anyhow!("No free space for GFX file {file_num:02X} ({} bytes)", compressed.len())
                })?;
                if old_pc + old_len <= rom_bytes.len() {
                    rom_bytes[old_pc..old_pc + old_len].fill(0xFF);
                }
                let new_snes = AddrSnes::try_from_lorom(AddrPc(pc as u32))?;
                let (low_pc, high_pc, bank_pc) = ptr_pcs.unwrap();
                rom_bytes[low_pc] = (new_snes.0 & 0xFF) as u8;
                rom_bytes[high_pc] = ((new_snes.0 >> 8) & 0xFF) as u8;
                rom_bytes[bank_pc] = ((new_snes.0 >> 16) & 0xFF) as u8;
                pc + header_offset
            } else {
                anyhow::bail!(
                    "GFX file {file_num:02X} isn't repointable and the new data ({} bytes) doesn't fit in the existing {} bytes",
                    compressed.len(),
                    old_len
                );
            };

            rom_bytes[dest..dest + compressed.len()].copy_from_slice(&compressed);
            if dest == old_pc && compressed.len() < old_len {
                rom_bytes[dest + compressed.len()..dest + old_len].fill(0xFF);
            }
        }

        // ── ExGFX files (LM v1.10/v1.60 parity): staged 8x8-tile pixel edits
        // (via `tile_editor_staged`) plus manager insert/delete. Applied onto
        // a clone so the in-memory `exgfx_data` keeps the authoritative tiles.
        // The tile editor stages ExGFX edits without setting `exgfx_dirty`
        // (so Revert naturally un-dirties them); check both.
        let exgfx_tiles_staged =
            self.tile_editor_staged.keys().any(|&f| f >= smwe_rom::exgfx::EXGFX_FIRST_INDEX as usize);
        if self.exgfx_dirty || exgfx_tiles_staged {
            let mut data = self.exgfx_data.clone();
            for (&file_num, tiles) in &self.tile_editor_staged {
                if file_num >= smwe_rom::exgfx::EXGFX_FIRST_INDEX as usize {
                    if let Some(f) = data.get_mut(file_num as u16) {
                        f.set_tiles(tiles.clone())
                            .with_context(|| format!("ExGFX {file_num:03X}: bad staged tiles"))?;
                    }
                }
            }
            data.write_to_rom(rom_bytes, header_offset).context("ExGFX save")?;
        }

        // ── Super GFX Bypass table (LM v1.60 parity): per-level slot
        // assignments, one RATS block.
        if self.bypass_dirty {
            self.bypass_data.write_to_rom(rom_bytes, header_offset).context("Super GFX Bypass save")?;
        }

        // ── Message box text (global, $05A5D9 blob + $05A5A7 pointer table) ──
        if self.message_boxes_dirty {
            let (blob, pointers) = self.message_boxes.to_blob_and_pointers()?;

            let blob_pc =
                AddrPc::try_from_lorom(smwe_rom::message_boxes::MESSAGE_BOXES_SNES)?.as_index() + header_offset;
            rom_bytes[blob_pc..blob_pc + blob.len()].copy_from_slice(&blob);
            // Zero-fill any shrunk tail so it doesn't look like stray data.
            let max_end = blob_pc + smwe_rom::message_boxes::MESSAGE_BOXES_MAX_SIZE;
            if blob_pc + blob.len() < max_end {
                rom_bytes[blob_pc + blob.len()..max_end].fill(0xFF);
            }

            let ptr_pc =
                AddrPc::try_from_lorom(smwe_rom::message_boxes::MESSAGE_POINTER_TABLE_SNES)?.as_index() + header_offset;
            for (i, &offset) in pointers.iter().enumerate() {
                let [lo, hi] = offset.to_le_bytes();
                rom_bytes[ptr_pc + i * 2] = lo;
                rom_bytes[ptr_pc + i * 2 + 1] = hi;
            }
        }

        // ── Boss sequence text (global, 53 fixed-location stripe blobs in bank $0C) ──
        if self.boss_text_dirty {
            for boss_msgs in &self.boss_text.messages {
                for msg in boss_msgs {
                    let pc = AddrPc::try_from_lorom(msg.snes)?.as_index() + header_offset;
                    let bytes = msg.to_bytes();
                    rom_bytes
                        .get_mut(pc..pc + bytes.len())
                        .ok_or_else(|| anyhow::anyhow!("Boss text blob ${:06X} out of range", msg.snes.0))?
                        .copy_from_slice(&bytes);
                }
            }
        }

        // ── Title screen / credits fixed-location data ───────────────────────
        if self.title_credits_dirty {
            let title_submap_pc =
                AddrPc::try_from_lorom(smwe_rom::title_credits::TITLE_SUBMAP_OPERAND_SNES)?.as_index() + header_offset;
            *rom_bytes
                .get_mut(title_submap_pc)
                .ok_or_else(|| anyhow::anyhow!("Title submap operand out of range"))? = self.title_credits.title_submap;

            let input_pc =
                AddrPc::try_from_lorom(smwe_rom::title_credits::TITLE_INPUT_SEQ_SNES)?.as_index() + header_offset;
            let input_bytes = self.title_credits.title_input_bytes()?;
            let input_end = input_pc + smwe_rom::title_credits::TITLE_INPUT_SEQ_MAX_SIZE;
            rom_bytes
                .get_mut(input_pc..input_end)
                .ok_or_else(|| anyhow::anyhow!("Title input sequence out of range"))?
                .fill(0xFF);
            rom_bytes[input_pc..input_pc + input_bytes.len()].copy_from_slice(&input_bytes);

            self.title_credits.validate_title_screen_stripe()?;
            let title_stripe_pc =
                AddrPc::try_from_lorom(smwe_rom::title_credits::TITLE_SCREEN_STRIPE_SNES)?.as_index() + header_offset;
            let title_stripe_end = title_stripe_pc + smwe_rom::title_credits::TITLE_SCREEN_STRIPE_MAX_SIZE;
            rom_bytes
                .get_mut(title_stripe_pc..title_stripe_end)
                .ok_or_else(|| anyhow::anyhow!("Title screen stripe image out of range"))?
                .fill(0xFF);
            rom_bytes[title_stripe_pc..title_stripe_pc + self.title_credits.title_screen_stripe.len()]
                .copy_from_slice(&self.title_credits.title_screen_stripe);

            self.title_credits.validate_player_select_stripe()?;
            let menu_stripe_pc =
                AddrPc::try_from_lorom(smwe_rom::title_credits::PLAYER_SELECT_STRIPE_SNES)?.as_index() + header_offset;
            let menu_stripe_end = menu_stripe_pc + smwe_rom::title_credits::PLAYER_SELECT_STRIPE_MAX_SIZE;
            rom_bytes
                .get_mut(menu_stripe_pc..menu_stripe_end)
                .ok_or_else(|| anyhow::anyhow!("Player select stripe image out of range"))?
                .fill(0xFF);
            rom_bytes[menu_stripe_pc..menu_stripe_pc + self.title_credits.player_select_stripe.len()]
                .copy_from_slice(&self.title_credits.player_select_stripe);

            for (i, stripe) in self.title_credits.enemy_name_stripes.iter().enumerate() {
                let slot_size = smwe_rom::title_credits::TitleCreditsData::enemy_name_slot_size(i);
                smwe_rom::title_credits::TitleCreditsData::validate_enemy_name_stripe(i, stripe)?;
                let pc = AddrPc::try_from_lorom(smwe_rom::title_credits::ENEMY_NAME_STRIPE_STARTS[i])?.as_index()
                    + header_offset;
                let end = pc + slot_size;
                rom_bytes
                    .get_mut(pc..end)
                    .ok_or_else(|| anyhow::anyhow!("Credits enemy stripe {i:02X} out of range"))?
                    .fill(0xFF);
                rom_bytes[pc..pc + stripe.len()].copy_from_slice(stripe);
            }
        }

        // ── Palette tables ────────────────────────────────────────────────────
        if self.palette_dirty {
            let p = &self.level_properties;
            let write_palette = |rom_bytes: &mut [u8], snes_addr: u32, colors: &[u16; 12]| -> anyhow::Result<()> {
                let pc = AddrPc::try_from_lorom(AddrSnes(snes_addr))?.as_index() + header_offset;
                for (i, &c) in colors.iter().enumerate() {
                    let off = pc + i * 2;
                    if off + 1 < rom_bytes.len() {
                        rom_bytes[off] = (c & 0xFF) as u8;
                        rom_bytes[off + 1] = (c >> 8) as u8;
                    }
                }
                Ok(())
            };
            let (bg, fg, sprite) = self.palettes.read(|pal| (pal.bg, pal.fg, pal.sprite));
            write_palette(rom_bytes, 0x00B0B0 + p.palette_bg as u32 * 0x18, &bg)?;
            write_palette(rom_bytes, 0x00B190 + p.palette_fg as u32 * 0x18, &fg)?;
            write_palette(rom_bytes, 0x00B318 + p.palette_sprite as u32 * 0x18, &sprite)?;
        }

        // ── Map16 block edits ─────────────────────────────────────────────────
        self.map16_edits.read(|edits| {
            for (&block_id, &tile_words) in &edits.edits {
                if let Some(&snes_addr) = self.map16_block_ptrs.get(block_id as usize) {
                    if snes_addr != 0 {
                        if let Ok(pc) = AddrPc::try_from_lorom(AddrSnes(snes_addr)) {
                            let file_off = pc.as_index() + header_offset;
                            for (sub_i, &tw) in tile_words.iter().enumerate() {
                                let off = file_off + sub_i * 2;
                                if off + 1 < rom_bytes.len() {
                                    rom_bytes[off] = (tw & 0xFF) as u8;
                                    rom_bytes[off + 1] = (tw >> 8) as u8;
                                }
                            }
                        }
                    }
                }
            }
        });

        // ── ExAnimation data (per-level custom tile/palette animation) ──────
        // Single RATS-tagged free-space block; erased and reallocated on
        // every save that touched it.
        // Merge on save: the world editor owns the overworld list and may
        // have saved a newer one since this tab loaded, so re-read the block
        // and replace only the level/global lists instead of writing this
        // tab's (possibly stale) copy of the overworld list.
        if self.exanimation_dirty {
            use smwe_rom::exanimation::{ExAnimError, ExAnimationData};
            let mut merged = match ExAnimationData::parse(rom_bytes) {
                Ok(data) => data,
                Err(ExAnimError::NotFound) => ExAnimationData::default(),
                Err(e) => anyhow::bail!("ExAnimation read failed: {e}"),
            };
            merged.levels = self.exanimation.levels.clone();
            merged.global = self.exanimation.global.clone();
            merged
                .write_to_rom(rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("ExAnimation write failed: {e}"))?;
        }

        // Persist the per-level Layer 3 GFX bypass table (RATS L3BP block).
        if !self.layer3_bypass.save_to_rom(rom_bytes, header_offset) {
            anyhow::bail!("No free space for the Layer 3 GFX bypass table");
        }

        // ── Map16 expanded-page edits without a resolved pointer ─────────────
        // Edits to blocks 0x200+ on pages the LM table doesn't own yet have no
        // SNES pointer; read-modify-write whole pages through the
        // expanded-page model (LM locations when its table resolves, else the
        // editor's RATS block).
        {
            let mut by_page: HashMap<u8, Vec<(u16, [u16; 4])>> = HashMap::new();
            let pending: Vec<(u16, [u16; 4])> =
                self.map16_edits.read(|e| e.edits.iter().map(|(&id, &tw)| (id, tw)).collect());
            for (block_id, tile_words) in pending {
                let ptr = self.map16_block_ptrs.get(block_id as usize).copied().unwrap_or(0);
                if ptr == 0 && block_id >= 0x200 {
                    by_page.entry((block_id >> 8) as u8).or_default().push((block_id, tile_words));
                }
            }
            for (page, edits) in by_page {
                let mut page_bytes = smwe_rom::map16_expanded::read_expanded_fg_page(rom_bytes, header_offset, page)
                    .map_err(|e| anyhow::anyhow!("expanded Map16 page {page:02X}: {e}"))?
                    .unwrap_or([0u8; smwe_rom::map16_file::MAP16_PAGE_BYTES]);
                for (block_id, tile_words) in edits {
                    let base = (block_id as usize & 0xFF) * 8;
                    for (i, &tw) in tile_words.iter().enumerate() {
                        page_bytes[base + i * 2] = (tw & 0xFF) as u8;
                        page_bytes[base + i * 2 + 1] = (tw >> 8) as u8;
                    }
                }
                smwe_rom::map16_expanded::write_expanded_fg_page(rom_bytes, header_offset, page, &page_bytes)
                    .map_err(|e| anyhow::anyhow!("expanded Map16 page {page:02X}: {e}"))?;
            }
        }

        // ── Map16 "acts like" edits ──────────────────────────────────────────
        if !self.map16_acts_edits.is_empty() {
            let mut table = smwe_rom::map16_expanded::read_acts_table(rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("act-as table: {e}"))?;
            for (&tile, &act) in &self.map16_acts_edits {
                table.insert(tile, act);
            }
            smwe_rom::map16_expanded::write_acts_table(rom_bytes, header_offset, &table)
                .map_err(|e| anyhow::anyhow!("act-as table: {e}"))?;
        }

        // ── Direct Map16 objects ────────────────────────────────────────────
        // Single RATS-tagged free-space block (`SMWDM161`); erased and
        // reallocated on every save that touched the DM16 objects. Only this
        // level's entry is replaced — other levels' objects are preserved.
        if self.dm16_dirty {
            use smwe_rom::direct_map16::{DirectMap16Data, Dm16Error};
            let mut merged = match DirectMap16Data::parse(rom_bytes) {
                Ok(data) => data,
                Err(Dm16Error::NotFound) => DirectMap16Data::default(),
                Err(e) => anyhow::bail!("Direct Map16 read failed: {e}"),
            };
            let objects: Vec<smwe_rom::direct_map16::DirectMap16Object> =
                self.direct_map16.read(|d| d.objects.iter().map(|o| o.to_rom_obj()).collect());
            if objects.is_empty() {
                merged.levels.remove(&self.level_num);
            } else {
                merged.levels.insert(self.level_num, objects);
            }
            merged
                .write_to_rom(rom_bytes, header_offset)
                .map_err(|e| anyhow::anyhow!("Direct Map16 write failed: {e}"))?;
        }
        // ── Dynamic level height (LM v3.00) ──────────────────────────────────
        // Single RATS-tagged free-space block ("SMWLVLH1"). Merge on save:
        // re-read the block from the bytes being saved and replace only this
        // level's entry, so other tabs' heights are never clobbered. Skip the
        // write entirely when the stored height already matches — the RATS
        // block is reallocated on every write, so an unconditional write
        // would churn free space on every save.
        {
            use smwe_rom::level::dimensions::{LevelHeightError, LevelHeights};
            let mut merged = match LevelHeights::parse(rom_bytes) {
                Ok(data) => data,
                Err(LevelHeightError::NotFound) => LevelHeights::default(),
                Err(e) => anyhow::bail!("Level height read failed: {e}"),
            };
            if merged.get(self.level_num) != self.level_properties.level_height_tiles {
                let screens = self.level_properties.num_screens();
                merged
                    .set(self.level_num, self.level_properties.level_height_tiles, screens)
                    .map_err(|e| anyhow::anyhow!("Level height invalid: {e}"))?;
                merged
                    .write_to_rom(rom_bytes, header_offset)
                    .map_err(|e| anyhow::anyhow!("Level height write failed: {e}"))?;
            }
        }

        Ok(())
    }

    fn has_unsaved_changes(&self) -> bool {
        self.has_edits
    }

    fn take_save_request(&mut self) -> bool {
        std::mem::take(&mut self.request_rom_save)
    }

    fn on_save_succeeded(&mut self) {
        self.has_edits = false;
        self.palette_dirty = false;
        self.title_credits_dirty = false;
        self.exanimation_dirty = false;
        self.secondary_exit_ext_dirty = false;
        self.sprite_header_dirty = false;
        self.dm16_dirty = false;
        self.entrance_extras_dirty = false;
        // ── LM 3.40 Auto-Set Number of Screens ────────────────────────────
        // Sync the in-memory header state with what save_to_rom just wrote,
        // so the Level Length slider shows the recomputed value and the
        // auto-set gate sees the installed table on subsequent saves. The
        // recompute is deterministic (same layers, same controls), so this
        // matches the bytes already on the ROM.
        let vertical = self.level_properties.is_vertical;
        if let Some(auto_len) = self.auto_set_screens_len(vertical) {
            self.level_properties.level_length = auto_len;
        }
        self.level_properties.layer2_scroll_ext_raw = self.scroll_ext_save_byte();
        self.scroll_ext_dirty = false;
        let (spawn_x, spawn_y) = self.spawn_pos();
        self.initial_spawn_x = spawn_x;
        self.initial_spawn_y = spawn_y;
    }
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

/// Pure WRAM block-map index math shared by [`UiLevelEditor::block_map_index`]
/// and the Direct Map16 stamp path. `vertical` selects the level orientation;
/// `edit_layer`/`has_layer2` reproduce the layer-2-background wrap. Kept as a
/// free function so the horizontal/vertical mapping is unit-testable without
/// a UI context.
fn block_map_index_math(block_x: u32, block_y: u32, vertical: bool, edit_layer: u8, has_layer2: bool) -> u32 {
    let scr_size: u32 = if vertical { 16 * 32 } else { 16 * 27 };

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

    let idx = screen * scr_size + sidx;
    if edit_layer == 2 && !has_layer2 {
        idx % (16 * 27 * 2)
    } else {
        idx
    }
}

impl UiLevelEditor {
    fn layer3_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_layer3_settings {
            return;
        }
        let mut open = self.show_layer3_settings;
        // Raw ROM bytes for the Layer3 tables (header-stripped, like the emu ROM).
        // We re-read the file here; the dialog only needs the tables.
        let rom_bytes = match std::fs::read(&self.rom_path) {
            Ok(raw) => {
                if raw.len() % 0x400 == 0x200 {
                    raw[0x200..].to_vec()
                } else {
                    raw
                }
            }
            Err(_) => Vec::new(),
        };
        let tileset = self.level_properties.fg_bg_gfx;
        let vram = self.cpu.mem.vram.clone();
        let cgram = self.cpu.mem.cgram.clone();
        let level_num = self.level_num;
        let changed = if let Some(dlg) = self.layer3_dialog.as_mut() {
            dlg.show(
                ctx,
                &mut open,
                tileset,
                &rom_bytes,
                &vram,
                &cgram,
                &mut self.layer3_bypass,
                level_num,
                &mut self.level_properties.layer3,
                &mut self.level_properties.layer3_priority,
            )
        } else {
            false
        };
        if changed {
            self.mark_edited();
        }
        self.show_layer3_settings = open;
    }

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
            // Dynamic level dimensions (LM v3.00): the height travels in a
            // RATS block, not the vanilla header, so load it alongside.
            let height_tiles = self.rom.level_heights.get(self.level_num);
            // Entrance extras (LM v3.00): editor-owned SMWENTR1 block.
            let extras = self.rom.level_entrance_extras.extras_for(self.level_num);
            self.level_properties = LevelProperties::from_level(level, height_tiles, &extras);
            self.entrance_extras_dirty = false;
            let layer1 = EditableObjectLayer::from_level(level);
            self.layer1 = UndoableData::new(layer1);
            self.direct_map16 =
                UndoableData::new(EditableDirectMap16::from_rom_data(&self.rom.direct_map16, self.level_num));
            self.sprites = UndoableData::new(EditableSpriteLayer::from_level(level));
            // Sprite header dialog working copy: the 1-byte vanilla header.
            // Prefer the session-authoritative edit map: `self.rom` is not
            // refreshed after a save, so re-reading the stale parse would
            // revert an already-saved edit on the next level switch.
            // The LM 3.00 options live in the session-authoritative
            // `sprite_header_ext` working copy (synced at construction) and
            // are intentionally not re-read here.
            self.sprite_header_edit =
                self.sprite_header_edits.get(&self.level_num).cloned().unwrap_or_else(|| level.sprite_header.clone());
            self.sprite_header_dirty = false;
            self.scroll_ext_dirty = false;
            let bg_bank = match &level.layer2 {
                Layer2Data::Objects { objects, .. } => {
                    self.layer2_objects = Some(UndoableData::new(EditableObjectLayer::from_object_layer(
                        objects,
                        level.secondary_header.vertical_level(),
                    )));
                    self.layer2_background = None;
                    None
                }
                Layer2Data::Background(bg) => {
                    self.layer2_objects = None;
                    let bank = bg.high_byte();
                    self.layer2_background =
                        Some(UndoableData::new(EditableBackgroundLayer::new(bg.tile_ids().to_vec())));
                    Some(bank)
                }
            };
            // The background tile map editor follows the parsed Map16 bank and
            // starts with a clean selection/status each level.
            if let Some(bank) = bg_bank {
                self.bg_page = bank;
                self.bg_selection = None;
                self.bg_drag = None;
                self.bg_status = None;
                self.bg_offset_open = false;
                self.bg_bank_open = false;
                self.bg_prev_page = None;
                self.bg_canvas_dirty = true;
                self.bg_selector_tex = None;
                self.bg_block_words = Self::bg_map16_block_words(&mut self.cpu);
            } else {
                self.show_bg_tilemap_editor = false;
                self.bg_block_words.clear();
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
        self.selected_dm16_indices.clear();
        self.last_undo_was_dm16 = false;
        self.dm16_placing = None;
        self.dm16_cond_open = None;
        self.dm16_remap_open = false;
        self.dm16_selection = None;
        self.dm16_sel_drag_start = None;
        self.dm16_status = None;
        self.dm16_dirty = false;
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
        // Super GFX Bypass (LM v1.60 parity): overwrite explicitly-assigned
        // slots' VRAM ranges, mirroring what LM's ExGFX ASM hack does at
        // level load. Vanilla files go through the game's own UploadGFXFile
        // (bit-exact); ExGFX files are memcpied. Everything downstream
        // (renderer upload, tile picker rebuild) then sees the bypassed GFX.
        self.apply_bypass_to_vram();

        // Snapshot the vanilla layer-1 block map before any DM16 stamping,
        // so rerasterization can always restore tiles hidden under DM16.
        self.dm16_snapshot_base();

        // Stamp Direct Map16 objects into the WRAM block map (the vanilla
        // game would render them via the Direct Map16 ASM; the editor
        // reproduces that rendering itself).
        self.stamp_dm16_tiles();

        // Snapshot clean VRAM for the ExAnimation tile browser (it decodes
        // source tiles from the pre-animation graphics), and restart the
        // ExAnimation tick counter.
        self.exanimation_base_vram = self.cpu.mem.vram.clone();
        self.anim_tick = 0;
        self.exanim_dialog.reset_atlas();

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

        // Mario spawn point is now rendered as text "M", not a sprite

        self.sprite_oam_cache = oam_map.clone();

        // Upload palette + GFX from the clean post-decompress state, then tiles.
        let mut renderer = self.level_renderer.lock().expect("Cannot lock level_renderer");
        renderer.upload_palette(&self.gl, &self.cpu.mem.cgram);
        renderer.upload_gfx(&self.gl, &self.cpu.mem.vram);
        renderer.upload_level(&self.gl, &mut self.cpu, &self.rom, self.level_properties.fg_bg_gfx);
        let sprite_list = self.sprites.read(|sprites| sprites.sprites.clone());
        renderer.upload_editable_sprites(&self.gl, &sprite_list, &oam_map, is_vertical);
        drop(renderer);

        // Rebuild the tile picker from the loaded level's tileset.
        self.tile_picker.rebuild(&mut self.cpu);
        self.bg_tile_picker.rebuild(&mut self.cpu);

        // ── Secondary entrance data ──────────────────────────────────────────
        self.secondary_entrance_data = self.rom.secondary_entrances.iter().map(|se| se.bytes()).collect();
        // ── Secondary-exit extended options (LM v3.00, editor RATS block) ──
        self.secondary_exit_ext = self.rom.secondary_exit_ext.clone();
        self.secondary_exit_ext_dirty = false;

        // ── Palette data ─────────────────────────────────────────────────────
        {
            let rom_bytes = self.rom.rom_bytes();
            let p = &self.rom.levels[level_idx].primary_header;
            let read_palette = |snes_addr: u32| -> [u16; 12] {
                let mut colors = [0u16; 12];
                if let Ok(pc) = AddrPc::try_from_lorom(AddrSnes(snes_addr)) {
                    let base = pc.as_index();
                    for (i, c) in colors.iter_mut().enumerate() {
                        let off = base + i * 2;
                        if off + 1 < rom_bytes.len() {
                            *c = rom_bytes[off] as u16 | ((rom_bytes[off + 1] as u16) << 8);
                        }
                    }
                }
                colors
            };
            self.palettes = UndoableData::new(palette_editor::EditablePalettes {
                bg:     read_palette(0x00B0B0 + p.palette_bg() as u32 * 0x18),
                fg:     read_palette(0x00B190 + p.palette_fg() as u32 * 0x18),
                sprite: read_palette(0x00B318 + p.palette_sprite() as u32 * 0x18),
            });
            self.palette_dirty = false;
            self.palette_gesture_before = None;
        }

        // ── Map16 block pointers ─────────────────────────────────────────────
        {
            let map16_bank = self.cpu.mem.cart.resolve("Map16Common").unwrap_or(0) & 0xFF0000;
            let mut ptrs = vec![0u32; 0x4000];
            for (block_id, ptr) in ptrs.iter_mut().take(0x200).enumerate() {
                let ptr_lo_addr = 0x0FBE + block_id as u32 * 2;
                if ptr_lo_addr + 1 < 0x10000 {
                    let offset = self.cpu.mem.load_u16(ptr_lo_addr) as u32;
                    if offset != 0 {
                        *ptr = map16_bank | offset;
                    }
                }
            }
            self.map16_block_ptrs = ptrs;
        }

        // Mark as clean (no unsaved edits)
        self.has_edits = false;
    }

    /// Resolve a Lunar Magic extended Map16 block only when the editor needs
    /// it. The ROM routine has level-specific state, so this must happen after
    /// the level was loaded into `self.cpu`.
    pub(super) fn ensure_map16_block_ptr(&mut self, block_id: u16) {
        if block_id < 0x200 || block_id as usize >= self.map16_block_ptrs.len() {
            return;
        }
        let slot = &mut self.map16_block_ptrs[block_id as usize];
        if *slot != 0 {
            return;
        }
        let mut scratch = self.cpu.clone();
        *slot = smwe_emu::emu::lm_ext_map16_data_addr(&mut scratch, block_id).unwrap_or(0);
        if *slot == 0 {
            // No LM resolver hit: point at this editor's expanded-page
            // storage when the page already exists there, so edits to the
            // block save in place. (Pages not stored anywhere yet are
            // handled by the read-modify-write path in `save_to_rom`.)
            let bytes = self.rom.rom_bytes();
            *slot = smwe_rom::map16_expanded::expanded_fg_tile_snes(bytes, 0, block_id).unwrap_or(0);
        }
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
        let mut unique_ids: Vec<u8> =
            self.sprites.read(|sprites| sprites.sprites.iter().map(|s| s.sprite_id).collect());
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
        // Re-apply the bypass: upload_sprite_tileset just overwrote the
        // sprite slots with the tileset tables' files.
        self.apply_bypass_to_vram();
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
        self.block_map_index_for_layer(block_x, block_y, self.edit_layer)
    }

    /// Layer-explicit variant of [`Self::block_map_index`]. Direct Map16
    /// stamping always targets layer 1, regardless of the editing layer.
    fn block_map_index_for_layer(&self, block_x: u32, block_y: u32, edit_layer: u8) -> u32 {
        block_map_index_math(
            block_x,
            block_y,
            self.level_properties.is_vertical,
            edit_layer,
            self.level_properties.has_layer2,
        )
    }

    /// Get the WRAM base addresses for the currently edited layer.
    fn block_map_base(&self) -> (u32, u32) {
        self.block_map_base_for_layer(self.edit_layer)
    }

    /// Layer-explicit variant of [`Self::block_map_base`].
    fn block_map_base_for_layer(&self, edit_layer: u8) -> (u32, u32) {
        if edit_layer == 2 {
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
    ///
    /// Vanilla (non-DM16) writes to layer 1 are mirrored into the DM16-free
    /// base snapshot (`dm16_base_lo/hi`), so [`Self::rerasterize_dm16`] can
    /// restore vanilla tiles hidden under Direct Map16 objects.
    fn set_block_id_at(&mut self, block_x: u32, block_y: u32, block_id: u16) {
        let idx = self.block_map_index(block_x, block_y);
        let (lo_base, hi_base) = self.block_map_base();
        self.cpu.mem.store_u8(lo_base + idx, (block_id & 0xFF) as u8);
        self.cpu.mem.store_u8(hi_base + idx, ((block_id >> 8) & 0x01) as u8);
        if self.edit_layer != 2 {
            let i = idx as usize;
            if i < self.dm16_base_lo.len() && i < self.dm16_base_hi.len() {
                self.dm16_base_lo[i] = (block_id & 0xFF) as u8;
                self.dm16_base_hi[i] = ((block_id >> 8) & 0x01) as u8;
            }
            // A vanilla edit under a Direct Map16 object updates the base
            // tile (revealed if the object is later deleted) but must not
            // punch a hole in the overlay: re-stamp the topmost covering
            // DM16 object at this cell so it stays visually on top.
            let stamp = self.direct_map16.read(|d| {
                d.objects.iter().rev().find_map(|o| {
                    (block_x >= o.x && block_x < o.x + o.w && block_y >= o.y && block_y < o.y + o.h)
                        .then(|| o.tile_at(block_x - o.x, block_y - o.y))
                })
            });
            if let Some(tile) = stamp {
                self.dm16_stamp_at(block_x, block_y, tile);
            }
        }
    }

    /// Layer-1 block write for Direct Map16 stamping. Bypasses the DM16-free
    /// base snapshot: the snapshot is, by definition, the map without DM16
    /// stamps, and [`Self::rerasterize_dm16`] re-applies them afterwards.
    fn dm16_stamp_at(&mut self, block_x: u32, block_y: u32, block_id: u16) {
        let idx = self.block_map_index_for_layer(block_x, block_y, 1);
        let (lo_base, hi_base) = self.block_map_base_for_layer(1);
        self.cpu.mem.store_u8(lo_base + idx, (block_id & 0xFF) as u8);
        self.cpu.mem.store_u8(hi_base + idx, ((block_id >> 8) & 0x01) as u8);
    }

    /// Layer-1 block read for Direct Map16 hit-testing (flood fill).
    fn dm16_block_id_at(&mut self, block_x: u32, block_y: u32) -> Option<u16> {
        let idx = self.block_map_index_for_layer(block_x, block_y, 1);
        let (lo_base, hi_base) = self.block_map_base_for_layer(1);
        Some(self.cpu.mem.load_u8(lo_base + idx) as u16 | (((self.cpu.mem.load_u8(hi_base + idx) as u16) & 0x01) << 8))
    }

    /// Snapshot the vanilla (DM16-free) layer-1 block map. Called from
    /// `load_level` after decompression, before any DM16 stamping.
    fn dm16_snapshot_base(&mut self) {
        let vertical = self.level_properties.is_vertical;
        let has_l2 = self.level_properties.has_layer2;
        let scr_len: u32 = match (vertical, has_l2) {
            (false, false) => 0x20,
            (true, false) => 0x1C,
            (false, true) => 0x10,
            (true, true) => 0x0E,
        };
        let scr_size: u32 = if vertical { 16 * 32 } else { 16 * 27 };
        let len = (scr_len * scr_size) as usize;
        let (lo_base, hi_base) = self.block_map_base_for_layer(1);
        self.dm16_base_lo = (0..len).map(|i| self.cpu.mem.load_u8(lo_base + i as u32)).collect();
        self.dm16_base_hi = (0..len).map(|i| self.cpu.mem.load_u8(hi_base + i as u32)).collect();
    }

    /// Restore the vanilla layer-1 block map, re-stamp every Direct Map16
    /// object in order, and re-render. The single choke point after any
    /// DM16 model change (place / edit / delete / remap / fill / undo /
    /// redo): vanilla tiles hidden under DM16 objects are restored instead
    /// of being blanked to 0x25.
    pub(super) fn rerasterize_dm16(&mut self) {
        if !self.dm16_base_lo.is_empty() && self.dm16_base_lo.len() == self.dm16_base_hi.len() {
            let (lo_base, hi_base) = self.block_map_base_for_layer(1);
            for (i, b) in self.dm16_base_lo.iter().enumerate() {
                self.cpu.mem.store_u8(lo_base + i as u32, *b);
            }
            for (i, b) in self.dm16_base_hi.iter().enumerate() {
                self.cpu.mem.store_u8(hi_base + i as u32, *b);
            }
        }
        self.stamp_dm16_tiles();
        self.rebuild_tiles();
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

    /// Look up the block ID at the given block coordinates on the WRAM
    /// block map starting at (`lo_base`, `hi_base`), independent of the
    /// current edit layer. The exit-enabled-tile view option uses this to
    /// evaluate Layer 1 always (bases `$7EC800`/`$7FC800`), plus the
    /// object-backed Layer 2 in level mode 0x01. High byte uses the
    /// renderer's `$3F` mask (full 12-bit Map16 ID space) rather than
    /// [`Self::block_id_at`]'s vanilla `$01` mask.
    pub(super) fn raw_block_id_at(&mut self, block_x: u32, block_y: u32, lo_base: u32, hi_base: u32) -> u16 {
        let vertical = self.level_properties.is_vertical;
        let scr_size: u32 = if vertical { 16 * 32 } else { 16 * 27 };
        let block_x_px = block_x * 16;
        let block_y_px = block_y * 16;
        // Same screen/tile math as block_map_index, with explicit bases so
        // this works regardless of the current edit layer.
        let (screen, sidx) = if vertical {
            let sub_y = block_y_px / 512;
            let sub_x = block_x_px / 256;
            let screen = sub_y * 2 + sub_x;
            let col = (block_x_px / 16) % 16;
            let row = (block_y_px / 16) % 32;
            (screen, row * 16 + col)
        } else {
            let screen = block_x / 16;
            (screen, block_y * 16 + (block_x % 16))
        };
        let idx = screen * scr_size + sidx;
        self.cpu.mem.load_u8(lo_base + idx) as u16 | (((self.cpu.mem.load_u8(hi_base + idx) as u16) & 0x3F) << 8)
    }

    /// Update spawn point from absolute tile coordinates
    fn update_spawn_from_tiles(&mut self, abs_x: u32, abs_y: u32, is_vertical: bool) {
        // Convert absolute tile coords back to entrance screen + local coords
        let (_entrance_screen, _entrance_x, _entrance_y) = if is_vertical {
            let sx = abs_x / 16;
            let sy = abs_y / 32;
            let screen = (sy * 2 + sx) as u8;
            let x = (abs_x % 16) as u8;
            let y = (abs_y % 32) as u8;
            (screen, x, y)
        } else {
            let screen = (abs_x / 16) as u8;
            let x = (abs_x % 16) as u8;
            let y = abs_y as u8;
            (screen, x, y)
        };

        // Divide by 2 to get half-resolution entrance coords (for future ROM persistence)
        let _entrance_x = (_entrance_x / 2).min(7);
        let _entrance_y = (_entrance_y / 2).min(15);

        // Update the local spawn position (transient mid-drag write; the
        // drag start/end sites own the undo step)
        let pos = self.spawn.data_mut();
        pos.x = abs_x;
        pos.y = abs_y;
        self.mark_edited();
    }

    /// Mark the level as having edits.
    ///
    /// This is the choke point every undoable mutation funnels through, so
    /// it also clears the spawn cross-stack undo priority: any non-spawn
    /// mutation makes the spawn stack's head no longer the newest undoable
    /// action. Spawn mutations set the flags back afterwards (see
    /// [`Self::set_spawn`] / [`Self::end_spawn_drag`]).
    pub(super) fn mark_edited(&mut self) {
        self.has_edits = true;
        self.spawn_undo_pending = false;
        self.spawn_redo_pending = false;
    }

    /// Main-entrance position in absolute tile coordinates.
    pub(super) fn spawn_pos(&self) -> (u32, u32) {
        self.spawn.read(|s| (s.x, s.y))
    }

    /// Set the main-entrance position as one undoable step (no-op when
    /// unchanged). Restores spawn-stack undo priority afterwards.
    pub(super) fn set_spawn(&mut self, x: u32, y: u32) {
        if self.spawn_pos() == (x, y) {
            return;
        }
        self.spawn.write(|s| {
            s.x = x;
            s.y = y;
        });
        self.mark_edited();
        self.spawn_undo_pending = true;
        self.spawn_redo_pending = false;
    }

    /// Lunar Magic delete behavior for the entrance: reset to the vanilla
    /// default position instead of removing it (a level always has one).
    pub(super) fn reset_spawn_to_default(&mut self) {
        self.set_spawn(DEFAULT_SPAWN_X, DEFAULT_SPAWN_Y);
    }

    /// Begin a gesture-style entrance drag: snapshot so the whole drag
    /// commits as one undo step on release.
    pub(super) fn begin_spawn_drag(&mut self) {
        self.spawn_drag_before = Some(self.spawn.read(|s| s.clone()));
    }

    /// Move the entrance mid-drag (transient: no undo step per frame).
    pub(super) fn drag_spawn_to(&mut self, x: u32, y: u32) {
        let pos = self.spawn.data_mut();
        pos.x = x;
        pos.y = y;
        self.mark_edited();
    }

    /// End a gesture-style entrance drag: commit one undo step, unless the
    /// pointer never actually moved (a click, not a drag).
    pub(super) fn end_spawn_drag(&mut self) {
        if let Some(before) = self.spawn_drag_before.take() {
            let after = self.spawn.read(|s| s.clone());
            if before != after {
                self.spawn.commit_change(&before);
                self.mark_edited();
                self.spawn_undo_pending = true;
                self.spawn_redo_pending = false;
            }
        }
    }

    /// Screen rect of the red "M" entrance marker (shared by the overlay
    /// draw code and hit-testing).
    pub(super) fn entrance_rect(&self, origin: Pos2, tile_sz: f32) -> Rect {
        let (sx, sy) = self.spawn_pos();
        let spawn_pos = origin + vec2(sx as f32 * tile_sz, sy as f32 * tile_sz);
        Rect::from_center_size(spawn_pos + vec2(tile_sz / 2.0, tile_sz / 2.0), Vec2::splat(tile_sz))
    }

    /// Check if there are unsaved changes
    pub(super) fn has_unsaved_changes(&self) -> bool {
        self.has_edits
    }

    /// Position Mario (editor-only visual marker) at the level's main entrance.
    /// Uses sprite ID 0xFE as an editor-only placeholder (not saved to ROM).
    fn position_mario_at_entrance(&mut self, is_vertical: bool, level: &Level) {
        let (entrance_x_half, entrance_y_half) = level.secondary_header.main_entrance_xy_pos();
        let entrance_screen = level.secondary_header.main_entrance_screen();

        // Entrance coordinates are stored at half-resolution, multiply by 2
        let entrance_x = entrance_x_half as u32 * 2;
        let entrance_y = entrance_y_half as u32 * 2;

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

        // Store spawn coordinates for rendering the "M" marker; a fresh
        // undo stack per level load so undo never crosses level switches.
        self.spawn = UndoableData::new(SpawnPos { x: abs_x, y: abs_y });
        self.spawn_undo_pending = false;
        self.spawn_redo_pending = false;
        self.spawn_drag_before = None;
        self.entrance_selected = false;
        // Store initial state for unsaved changes tracking
        self.initial_spawn_x = abs_x;
        self.initial_spawn_y = abs_y;

        // Spawn coordinates stored for rendering as "M" text overlay
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::undo::Undo;

    #[test]
    fn spawn_pos_bytes_round_trip() {
        let p = SpawnPos { x: 5, y: 18 };
        let q = SpawnPos::from_bytes(p.to_bytes());
        assert_eq!((q.x, q.y), (5, 18));
    }

    #[test]
    fn spawn_pos_from_bytes_short_or_empty_decodes_zero() {
        // Empty / field-truncated buffers decode missing fields as 0
        // instead of panicking; a complete 4-byte field still decodes.
        let q = SpawnPos::from_bytes(vec![]);
        assert_eq!((q.x, q.y), (0, 0));
        let q = SpawnPos::from_bytes(vec![1, 2, 3]);
        assert_eq!((q.x, q.y), (0, 0));
        let q = SpawnPos::from_bytes(vec![9; 7]);
        assert_eq!((q.x, q.y), (0x0909_0909, 0));
    }
}

#[cfg(test)]
mod block_map_tests {
    use super::block_map_index_math;

    #[test]
    fn horizontal_index_math() {
        // idx = screen * (16*27) + row * 16 + col; block_x = col + screen*16.
        let h = |x, y| block_map_index_math(x, y, false, 1, false);
        assert_eq!(h(0, 0), 0);
        assert_eq!(h(15, 0), 15);
        assert_eq!(h(0, 26), 26 * 16);
        assert_eq!(h(15, 26), 26 * 16 + 15);
        assert_eq!(h(16, 0), 16 * 27);
        assert_eq!(h(16, 26), 16 * 27 + 26 * 16);
        assert_eq!(h(31, 13), 16 * 27 + 13 * 16 + 15);
    }

    #[test]
    fn vertical_index_math() {
        // Vertical screens are 16x32; screen = sub_y * 2 + sub_x with
        // 256px-wide / 512px-tall subscreens.
        let v = |x, y| block_map_index_math(x, y, true, 1, false);
        assert_eq!(v(0, 0), 0);
        assert_eq!(v(15, 0), 15);
        assert_eq!(v(0, 31), 31 * 16);
        assert_eq!(v(16, 0), 16 * 32); // second subscreen column
        assert_eq!(v(0, 32), 2 * 16 * 32); // second subscreen row
        assert_eq!(v(16, 32), 3 * 16 * 32);
        assert_eq!(v(31, 63), 3 * 16 * 32 + 31 * 16 + 15);
    }

    #[test]
    fn layer2_background_wrap() {
        // edit_layer == 2 without Layer 2 wraps into the 2-screen BG map.
        let wrap = |x, y| block_map_index_math(x, y, false, 2, false);
        assert_eq!(wrap(0, 0), 0);
        assert_eq!(wrap(32, 0), (2 * 16 * 27) % (16 * 27 * 2));
        // Layer 1 is unaffected by the wrap.
        assert_eq!(block_map_index_math(32, 0, false, 1, false), 2 * 16 * 27);
    }

    #[test]
    fn dm16_layer_is_always_layer1() {
        // The Direct Map16 stamp path forces layer 1: same index as an
        // explicit layer-1 write, never the layer-2 wrap.
        for &(x, y) in &[(0, 0), (15, 26), (48, 13)] {
            assert_eq!(block_map_index_math(x, y, false, 1, false), block_map_index_math(x, y, false, 1, true));
        }
    }
}
