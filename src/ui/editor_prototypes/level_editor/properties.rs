#![allow(dead_code)]

use smwe_rom::level::Level;

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

    // Other
    pub is_vertical: bool,
    pub has_layer2: bool,
}

impl LevelProperties {
    pub fn from_level(level: &Level) -> Self {
        let h = &level.primary_header;
        let is_vertical = level.secondary_header.vertical_level();
        let has_layer2 = matches!(level.layer2, smwe_rom::level::Layer2Data::Objects(_));
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
        }
    }

    /// (width, height)
    pub fn level_dimensions_in_tiles(&self) -> (u32, u32) {
        let (screen_width, screen_height) = self.screen_dimensions_in_tiles();
        if self.is_vertical {
            (screen_width, screen_height * self.num_screens())
        } else {
            (screen_width * self.num_screens(), screen_height)
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
        match (self.is_vertical, self.has_layer2) {
            (false, false) => 0x20,
            (true, false) => 0x1C,
            (false, true) => 0x10,
            (true, true) => 0x0E,
        }
    }
}
