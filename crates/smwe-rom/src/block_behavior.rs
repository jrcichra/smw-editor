//! Vanilla Map16 block ID ranges and what collision/interaction behavior each
//! one gets. This is NOT a per-block data table the game reads — SMW
//! dispatches block interaction by comparing the block ID against these
//! hardcoded ranges in ASM, scattered across many routines. There is no
//! generic "acts like" byte for arbitrary reassignment in vanilla SMW.
//!
//! Practically, this means: a custom block can already "act like" any of
//! these categories today, for free, by using a Map16 ID from the
//! corresponding range with custom graphics assigned to it (already fully
//! supported by the Map16 editor) — no ASM insertion required. Only a
//! genuinely *novel* behavior (not matching any of these) would need new code.
//!
//! Source: SMW Central Data Repository, "Detailed explanation of interaction
//! of each tile" by MarioFanGamer (SMW ASM Moderator), 17 Oct 2024. Ranges are
//! inclusive.

/// General interaction category, determined purely by which range a block ID
/// falls into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockCategory {
    NonSolid,
    Ledge,
    Solid,
    Slope,
    SlopeAssist,
}

impl BlockCategory {
    pub fn label(self) -> &'static str {
        match self {
            BlockCategory::NonSolid => "Non-solid",
            BlockCategory::Ledge => "Ledge (solid on top only)",
            BlockCategory::Solid => "Solid (impassable)",
            BlockCategory::Slope => "Slope",
            BlockCategory::SlopeAssist => "Slope assist",
        }
    }
}

/// The general interaction category for a Map16 block ID (0x000-0x1FF).
pub fn category_of(block_id: u16) -> BlockCategory {
    match block_id {
        0x000..=0x0FF => BlockCategory::NonSolid,
        0x100..=0x110 => BlockCategory::Ledge,
        0x111..=0x16D => BlockCategory::Solid,
        0x16E..=0x1D7 => BlockCategory::Slope,
        _ => BlockCategory::SlopeAssist, // 0x1D8-0x1FF
    }
}

/// A specific, narrower documented behavior for a block ID (beyond its
/// general category), if any. Not exhaustive — only the most commonly
/// relevant special tiles are listed.
pub fn specific_behavior(block_id: u16) -> Option<&'static str> {
    match block_id {
        0x000..=0x003 => Some("Liquid (water-state)"),
        0x004 => Some("Lava surface (kills most sprites on contact)"),
        0x005 => Some("Lava subsurface (kills Mario if feet touch it)"),
        0x006..=0x01C => Some("Climbable"),
        0x01F | 0x020 | 0x027 | 0x028 => Some("Door"),
        0x021..=0x024 => Some("Invisible bounce block (appears when hit from below)"),
        0x02A..=0x02C => Some("Coin"),
        0x02D | 0x02E => Some("Dragon Coin"),
        0x038 => Some("Midway point"),
        0x111 | 0x12D => Some("Bounce block"),
        0x11E => Some("Yellow turn block (smashable with spin jump from above)"),
        0x12E => Some("Throw/ice block (pick up with Run)"),
        0x132 => Some("Brown block (activates block snakes when stepped on)"),
        0x137 | 0x138 | 0x13F => Some("Exit-enabled pipe"),
        0x1B4 | 0x1B5 => Some("Purple triangle (slide)"),
        0x1CE..=0x1D2 => Some("Conveyor slope"),
        0x1FB..=0x1FF => Some("Magma inside (kills Mario from above)"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_match_documented_ranges() {
        assert_eq!(category_of(0x000), BlockCategory::NonSolid);
        assert_eq!(category_of(0x0FF), BlockCategory::NonSolid);
        assert_eq!(category_of(0x100), BlockCategory::Ledge);
        assert_eq!(category_of(0x110), BlockCategory::Ledge);
        assert_eq!(category_of(0x111), BlockCategory::Solid);
        assert_eq!(category_of(0x16D), BlockCategory::Solid);
        assert_eq!(category_of(0x16E), BlockCategory::Slope);
        assert_eq!(category_of(0x1D7), BlockCategory::Slope);
        assert_eq!(category_of(0x1D8), BlockCategory::SlopeAssist);
        assert_eq!(category_of(0x1FF), BlockCategory::SlopeAssist);
    }

    #[test]
    fn every_id_has_exactly_one_category() {
        // Sanity check the ranges partition 0x000..=0x1FF with no gaps/overlaps
        // (each id must resolve, which category_of guarantees by construction,
        // but this also documents the full space is covered).
        for id in 0x000..=0x1FFu16 {
            let _ = category_of(id); // must not panic for any id in range
        }
    }

    #[test]
    fn specific_behaviors_spot_check() {
        assert_eq!(specific_behavior(0x038), Some("Midway point"));
        assert_eq!(specific_behavior(0x132), Some("Brown block (activates block snakes when stepped on)"));
        assert_eq!(specific_behavior(0x050), None);
    }
}
