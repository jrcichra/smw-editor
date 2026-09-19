#![allow(dead_code)]

use smwe_rom::level::{entrance_extras::LevelEntranceExtras, scroll::Layer2ScrollExt, Layer2Data, Level};

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct LevelProperties {
    // Primary header
    pub palette_bg:      u8,
    pub level_length:    u8,
    pub back_area_color: u8,
    pub level_mode:      u8,
    pub layer3_priority: bool,
    pub music:           u8,
    pub sprite_gfx:      u8,
    pub timer:           u8,
    pub palette_sprite:  u8,
    pub palette_fg:      u8,
    pub item_memory:     u8,
    pub vertical_scroll: u8,
    pub fg_bg_gfx:       u8,

    // Secondary header
    pub is_vertical:                bool,
    pub has_layer2:                 bool,
    pub layer2_scroll:              u8,
    pub layer3:                     u8,
    pub main_entrance_action:       u8,
    pub midway_entrance_screen:     u8,
    pub fg_initial_pos:             u8,
    pub bg_initial_pos:             u8,
    pub no_yoshi_level:             bool,
    pub unknown_vertical_pos_level: bool,

    // LM v3.00 entrance extras (main entrance; "Change Other Properties"
    // data). No vanilla storage — persisted in the editor's SMWENTR1 RATS
    // block; in-game playback needs Lunar Magic's ASM hacks.
    pub face_left:              bool,
    pub new_fg_bg_init:         bool,
    pub bg_relative_to_fg_only: bool,
    pub bg_height:              u8,

    // LM 3.40+ Layer 2 scroll extension ($06FA00, SHCvvvvv). `layer2_scroll`
    // above is the paired preset (or the horizontal setting when separate).
    pub layer2_scroll_separate:  bool,
    pub layer2_hscroll_auto:     bool,
    pub layer2_vscroll:          u8,
    pub layer2_auto_set_screens: bool,
    /// Raw $06FA00 byte as loaded; `$FF` means LM never installed the table.
    pub layer2_scroll_ext_raw:   u8,

    // Layer 2 object-data header (5 bytes at the Layer 2 pointer, game-ignored).
    // Only meaningful when `has_layer2` is true.
    pub layer2_header: [u8; 5],

    /// Level height in tiles for horizontal levels (LM v3.00 dynamic
    /// dimensions; vanilla 27). Vertical levels ignore this.
    pub level_height_tiles: u16,
}

impl LevelProperties {
    pub fn from_level(level: &Level, level_height_tiles: u16, extras: &LevelEntranceExtras) -> Self {
        let h = &level.primary_header;
        let s = &level.secondary_header;
        let is_vertical = s.vertical_level();
        let has_layer2 = matches!(level.layer2, Layer2Data::Objects { .. });
        let layer2_header = match &level.layer2 {
            Layer2Data::Objects { header, .. } => *header,
            Layer2Data::Background(_) => [0u8; 5],
        };
        let (_, _) = s.main_entrance_xy_pos();
        // LM 3.40+ scroll extension ($06FA00). A vanilla ROM has $FF here
        // (table not installed); treat that as paired mode, not as S=1.
        // Default auto_set_screens to true so a fresh install matches LM's
        // $20 initial value (auto-set screens on).
        let ext_raw = s.scroll_ext;
        let ext = if Layer2ScrollExt::is_installed(ext_raw) {
            Layer2ScrollExt::decode(ext_raw)
        } else {
            Layer2ScrollExt {
                separate:         false,
                h_auto:           false,
                auto_set_screens: true,
                vscroll:          0,
            }
        };
        Self {
            palette_bg: h.palette_bg(),
            level_length: h.level_length(),
            back_area_color: h.back_area_color(),
            level_mode: h.level_mode(),
            layer3_priority: h.layer3_priority(),
            music: h.music(),
            sprite_gfx: h.sprite_gfx(),
            timer: h.timer(),
            palette_sprite: h.palette_sprite(),
            palette_fg: h.palette_fg(),
            item_memory: h.item_memory(),
            vertical_scroll: h.vertical_scroll(),
            fg_bg_gfx: h.fg_bg_gfx(),
            is_vertical,
            has_layer2,
            layer2_scroll: s.layer2_scroll(),
            layer3: s.layer3(),
            main_entrance_action: s.main_entrance_mario_action(),
            midway_entrance_screen: s.midway_entrance_screen(),
            fg_initial_pos: s.fg_initial_pos(),
            bg_initial_pos: s.bg_initial_pos(),
            no_yoshi_level: s.no_yoshi_level(),
            unknown_vertical_pos_level: s.unknown_vertical_pos_level(),
            face_left: extras.face_left,
            new_fg_bg_init: extras.new_fg_bg_init,
            bg_relative_to_fg_only: extras.bg_relative_to_fg_only,
            bg_height: extras.bg_height,
            layer2_scroll_separate: ext.separate,
            layer2_hscroll_auto: ext.h_auto,
            layer2_vscroll: ext.vscroll,
            layer2_auto_set_screens: ext.auto_set_screens,
            layer2_scroll_ext_raw: ext_raw,
            layer2_header,
            level_height_tiles,
        }
    }

    /// (width, height)
    pub fn level_dimensions_in_tiles(&self) -> (u32, u32) {
        let (screen_width, screen_height) = self.screen_dimensions_in_tiles();
        let screens = self.num_screens();
        if self.is_vertical {
            (screen_width, screen_height * screens)
        } else {
            (screen_width * screens, screen_height)
        }
    }

    /// (width, height)
    pub fn screen_dimensions_in_tiles(&self) -> (u32, u32) {
        if self.is_vertical {
            (32, 16)
        } else {
            (16, self.level_height_tiles.max(1) as u32)
        }
    }

    pub fn num_screens(&self) -> u32 {
        (self.level_length as u32) + 1
    }
}
