#![allow(dead_code)]

use smwe_rom::level::{Layer2Data, Level};

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct LevelProperties {
    // Primary header
    pub palette_bg: u8,
    pub level_length: u8,
    pub back_area_color: u8,
    pub level_mode: u8,
    pub layer3_priority: bool,
    pub music: u8,
    pub sprite_gfx: u8,
    pub timer: u8,
    pub palette_sprite: u8,
    pub palette_fg: u8,
    pub item_memory: u8,
    pub vertical_scroll: u8,
    pub fg_bg_gfx: u8,

    // Secondary header
    pub is_vertical: bool,
    pub has_layer2: bool,
    pub layer2_scroll: u8,
    pub layer3: u8,
    pub main_entrance_action: u8,
    pub midway_entrance_screen: u8,
    pub fg_initial_pos: u8,
    pub bg_initial_pos: u8,
    pub no_yoshi_level: bool,
    pub unknown_vertical_pos_level: bool,
}

impl LevelProperties {
    pub fn from_level(level: &Level) -> Self {
        let h = &level.primary_header;
        let s = &level.secondary_header;
        let is_vertical = s.vertical_level();
        let has_layer2 = matches!(level.layer2, Layer2Data::Objects(_));
        let (_, _) = s.main_entrance_xy_pos();
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
            (16, 27)
        }
    }

    pub fn num_screens(&self) -> u32 {
        (self.level_length as u32) + 1
    }
}
