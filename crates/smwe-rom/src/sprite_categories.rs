//! Sprite ID categories, mirroring the level sprite-load dispatch in SMWDisX
//! `bank_02.asm` (`CODE_02A88C`):
//!
//! - `< $C9` falls through to `LoadNormalSprite` (a regular 12-slot sprite).
//! - `$C9-$CA` → `LoadShooter` (shooter number = id - $C8).
//! - `$CB-$D9` → `CurrentGenerator = id - $CA` (generators, incl. the two
//!   "turn off" commands $D2/$D9).
//! - `$DA-$DD`, `$DF` load a normal slot with status 9 (stationary): the
//!   sprite number becomes `id - $DA + 4` (the four shells, and $DF → sprite
//!   09). `$DE` spawns 5 Eeries, `$E0` spawns 3 chain platforms.
//! - `$E1-$E6` activate cluster sprites (`CODE_02AAC0`: Boo ceiling, Boo
//!   rings, Swooper bats, Boo cloud, candle flames).
//! - `$E7+` have no dispatch entry at all in vanilla.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpriteCategory {
    /// Regular sprite occupying one of the 12 sprite slots ($00-$C8).
    Normal,
    /// Bullet Bill / Torpedo Ted shooter ($C9-$CA); uses the shooter tables,
    /// not a sprite slot.
    Shooter,
    /// Sprite generator ($CB-$D9); sets `CurrentGenerator`, occupies no slot.
    Generator,
    /// Special spawn command ($DA-$E0): stationary shells, 5 Eeries, 3 chain
    /// platforms.
    Special,
    /// Cluster-sprite activator ($E1-$E6); spawns into the separate 20-slot
    /// cluster tables.
    Cluster,
    /// No vanilla dispatch entry ($E7+); behavior is undefined without
    /// custom-sprite tools.
    Undefined,
}

impl SpriteCategory {
    pub fn of(sprite_id: u8) -> Self {
        match sprite_id {
            0x00..=0xC8 => Self::Normal,
            0xC9..=0xCA => Self::Shooter,
            0xCB..=0xD9 => Self::Generator,
            0xDA..=0xE0 => Self::Special,
            0xE1..=0xE6 => Self::Cluster,
            _ => Self::Undefined,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Sprite",
            Self::Shooter => "Shooter",
            Self::Generator => "Generator",
            Self::Special => "Special command",
            Self::Cluster => "Cluster",
            Self::Undefined => "Undefined",
        }
    }

    /// Whether `exec_sprite_id`-style previews (running the INIT routine in a
    /// normal sprite slot and reading OAM) are meaningful for this ID. Only
    /// normal sprites use the 12-slot tables the preview relies on.
    pub fn has_slot_preview(self) -> bool {
        self == Self::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_boundaries_match_dispatch() {
        assert_eq!(SpriteCategory::of(0x00), SpriteCategory::Normal);
        assert_eq!(SpriteCategory::of(0xC8), SpriteCategory::Normal);
        assert_eq!(SpriteCategory::of(0xC9), SpriteCategory::Shooter);
        assert_eq!(SpriteCategory::of(0xCA), SpriteCategory::Shooter);
        assert_eq!(SpriteCategory::of(0xCB), SpriteCategory::Generator);
        assert_eq!(SpriteCategory::of(0xD9), SpriteCategory::Generator);
        assert_eq!(SpriteCategory::of(0xDA), SpriteCategory::Special);
        assert_eq!(SpriteCategory::of(0xE0), SpriteCategory::Special);
        assert_eq!(SpriteCategory::of(0xE1), SpriteCategory::Cluster);
        assert_eq!(SpriteCategory::of(0xE6), SpriteCategory::Cluster);
        assert_eq!(SpriteCategory::of(0xE7), SpriteCategory::Undefined);
        assert_eq!(SpriteCategory::of(0xFF), SpriteCategory::Undefined);
    }
}
