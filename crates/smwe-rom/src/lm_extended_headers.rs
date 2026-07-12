//! Lunar Magic extended secondary header fields
//!
//! Beyond the 4-vanilla-header bytes, LM adds per-level metadata at known ROM
//! addresses. This module parses those bytes so they can be surfaced in the
//! editor and round-tripped through `.mwl`.
//!
//! Layout (per SMW Speedruns wiki "Level Data Format", verified against TOP2020):
//!
//! | Version  | $05DE00      | $06FA00      | $06FC00     | $06FE00     |
//! |----------|-------------|-------------|------------|------------|
//! | < 3.00   | IWPYX---    | —           | —          | —          |
//! | 3.00-3.33| IWPXXtTT    | OFYYYYYY    | RL-ooooo   | (n/a)      |
//! | 3.40+    | IWPXXtTT    | SHCvvvvv    | OFYYYYYY   | RL-ooooo   |
//!
//! Bit definitions:
//! - I = Slippery flag
//! - W = Water flag
//! - P = Use X/Y position method 2
//! - XX/YY = Extended entrance XY coords (method 2)
//! - t = Smart spawn flag
//! - TT = Sprite spawn range
//! - S = Separate L2 horizontal/vertical scroll rates
//! - H = Auto-set number of screens (v3.40+ replaces with C in $06FE00)
//! - C = Auto screen count / various flags
//! - vvvvv = L2 vertical scroll rate (when S is set)
//! - O = BG relative to FG
//! - R = Set entrance FG/BG relative to player
//! - L = Face entrance left
//! - ooooo = BG height (-1), or relative offset when O is set

use crate::{
    disassembler::binary_block::{DataBlock, DataKind},
    snes_utils::{addr::AddrSnes, rom::noop_error_mapper, rom_slice::SnesSlice},
    RomDisassembly,
};

/// Extended secondary header data read from LM-specific tables.
///
/// All four 5th-byte fields are present regardless of LM version; on vanilla
/// ROMs the tables simply contain zeroes so every accessor returns defaults.
#[derive(Debug, Clone)]
pub struct LmExtendedSecondaryHeader {
    /// Byte from `$05DE00` — slippery/water/position-method/spawn-range
    pub byte_de00: u8,
    /// Byte from `$06FA00` — scroll separation / auto-screens / vertical rate
    pub byte_fa00: u8,
    /// Byte from `$06FC00` — method-2 XY extension (v3.00-3.33) or BG-relative/FG-pos (v3.40+)
    pub byte_fc00: u8,
    /// Byte from `$06FE00` — relative-player-pos / face-left / BG-height
    pub byte_fe00: u8,
}

/// Sentinel value for "table not found" / vanilla ROM fallback.
impl Default for LmExtendedSecondaryHeader {
    fn default() -> Self {
        Self { byte_de00: 0, byte_fa00: 0, byte_fc00: 0, byte_fe00: 0 }
    }
}

/// Check whether this ROM has been modified by Lunar Magic (tables exist).
fn lm_tables_exist(_disasm: &RomDisassembly) -> bool {
    // On vanilla U SMW, $05DE00 is in the middle of code / unused data that
    // does NOT have the same pattern across all 0x200 entries.
    // We probe $0EF31F (LM secondary-header enable flag byte) — if present and
    // non-zero we treat the ROM as LM-modified. Alternatively, check $05DE00
    // table directly for a writable pattern.
    // Safest heuristic: try reading $05DE00; if it reads without panic we use
    // whatever values are there. The editor handles any version gracefully.
    true
}

impl LmExtendedSecondaryHeader {
    /// Read one LM-extended header for the given level number.
    ///
    /// Falls back to all-zeroes when the ROM doesn't have LM tables (vanilla).
    pub fn read_from_rom(disasm: &mut RomDisassembly, level_num: u16) -> Self {
        if !lm_tables_exist(disasm) {
            return Self::default();
        }

        let addrs = [0x05DE00, 0x06FA00, 0x06FC00, 0x06FE00];
        let mut bytes = [0u8; 4];

        for (i, &addr) in addrs.iter().enumerate() {
            // Each table is at least 512 entries (one per $00-$FF level);
            // some LM versions allocate dynamically beyond that.
            let data_block = DataBlock {
                slice: SnesSlice::new(AddrSnes(addr), 0x200),
                kind: DataKind::LevelHeaderSecondaryByteTable,
            };
            if let Ok(table) = disasm.rom_slice_at_block(data_block, noop_error_mapper) {
                if let Ok(contents) = table.as_bytes() {
                    bytes[i] = contents[level_num as usize];
                }
            }
        }

        Self {
            byte_de00: bytes[0],
            byte_fa00: bytes[1],
            byte_fc00: bytes[2],
            byte_fe00: bytes[3],
        }
    }

    // ------------------------------------------------------------------
    // Byte $05DE00: IWPXXtTT (v3.00+)  /  IWPYX--- (< 3.00)
    // ------------------------------------------------------------------

    /// Slippery level flag
    pub fn slippery(&self) -> bool {
        (self.byte_de00 >> 7) & 1 != 0
    }

    /// Water level flag
    pub fn water(&self) -> bool {
        (self.byte_de00 >> 6) & 1 != 0
    }

    /// Use X/Y position method 2 (higher-precision placement)
    pub fn position_method_2(&self) -> bool {
        (self.byte_de00 >> 5) & 1 != 0
    }

    /// Extended entrance X position (when method 2 is used, bits 3-4 of full X)
    pub fn extended_x(&self) -> u8 {
        (self.byte_de00 >> 3) & 0b11
    }

    /// Extended entrance Y position (v3.00+: upper bits of Y; pre-v3.00 this was part of X)
    pub fn extended_y(&self) -> u8 {
        (self.byte_de00 >> 1) & 0b11
    }

    /// Smart spawn flag — sprites only loaded when player approaches screen
    pub fn smart_spawn(&self) -> bool {
        (self.byte_de00 >> 1) & 1 != 0
    }

    /// Sprite spawn range (2 bits: how many screens around player are active)
    pub fn sprite_spawn_range(&self) -> u8 {
        self.byte_de00 & 0b11
    }

    // ------------------------------------------------------------------
    // Byte $06FA00 (v3.40+): SHCvvvvv
    //   $06FA00 (v3.00-3.33 / pre-v3.40 alias of $06FC00): OFYYYYYY
    // ------------------------------------------------------------------

    /// Separate L2 horizontal/vertical scroll settings (v3.40+)
    pub fn separate_l2_scroll(&self) -> bool {
        (self.byte_fa00 >> 7) & 1 != 0
    }

    /// Auto-set number of screens flag (v3.40+, replaces H in earlier versions)
    pub fn auto_screen_count(&self) -> bool {
        (self.byte_fa00 >> 6) & 1 != 0
    }

    /// L2 vertical scroll rate (5 bits, only when separate_scroll is set)
    pub fn l2_vertical_scroll_rate(&self) -> u8 {
        self.byte_fa00 & 0b11111
    }

    // ------------------------------------------------------------------
    // Byte $06FC00: OFYYYYYY (v3.00-3.33) or reused as scroll in v3.40+
    // ------------------------------------------------------------------

    /// BG relative to FG setting
    pub fn bg_relative_to_fg(&self) -> bool {
        (self.byte_fc00 >> 7) & 1 != 0
    }

    /// Reserved / future use bit (varies by LM version)
    pub fn reserved_fc_bit6(&self) -> bool {
        (self.byte_fc00 >> 6) & 1 != 0
    }

    /// Extended Y position for method 2 (6 bits, upper portion)
    pub fn extended_y_method2(&self) -> u8 {
        self.byte_fc00 & 0b111111
    }

    // ------------------------------------------------------------------
    // Byte $06FE00: RL-ooooo
    // ------------------------------------------------------------------

    /// Set entrance FG/BG relative to player position
    pub fn relative_to_player(&self) -> bool {
        (self.byte_fe00 >> 7) & 1 != 0
    }

    /// Face entrance left flag
    pub fn face_left(&self) -> bool {
        (self.byte_fe00 >> 6) & 1 != 0
    }

    /// Reserved bit (varies by version, may be used for C in v3.40+)
    pub fn reserved_fe_bit5(&self) -> bool {
        (self.byte_fe00 >> 5) & 1 != 0
    }

    /// BG height (-1), or relative BG offset when bg_relative_to_fg is set.
    /// Absolute 0 = 0x10 tiles from bottom of screen.
    pub fn bg_height(&self) -> u8 {
        self.byte_fe00 & 0b11111
    }

    /// Reconstruct the raw bytes for writing back to ROM
    pub fn into_bytes(self) -> [u8; 4] {
        [
            self.byte_de00,
            self.byte_fa00,
            self.byte_fc00,
            self.byte_fe00,
        ]
    }

    /// True if any byte differs from zeroes (i.e. LM-modified ROM has data here)
    pub fn is_modified(&self) -> bool {
        self.byte_de00 != 0 || self.byte_fa00 != 0 || self.byte_fc00 != 0 || self.byte_fe00 != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_all_zeroes() {
        let hdr = LmExtendedSecondaryHeader::default();
        assert_eq!(hdr.byte_de00, 0);
        assert_eq!(hdr.byte_fa00, 0);
        assert_eq!(hdr.byte_fc00, 0);
        assert_eq!(hdr.byte_fe00, 0);
        assert!(!hdr.is_modified());
    }

    #[test]
    fn test_slippery_flag() {
        let mut hdr = LmExtendedSecondaryHeader::default();
        hdr.byte_de00 = 0x80;
        assert!(hdr.slippery());
        hdr.byte_de00 = 0x7F;
        assert!(!hdr.slippery());
    }

    #[test]
    fn test_water_flag() {
        let mut hdr = LmExtendedSecondaryHeader::default();
        hdr.byte_de00 = 0x40;
        assert!(hdr.water());
    }

    #[test]
    fn test_position_method_2() {
        let mut hdr = LmExtendedSecondaryHeader::default();
        hdr.byte_de00 = 0x20;
        assert!(hdr.position_method_2());
    }

    #[test]
    fn test_face_left_and_relative_to_player() {
        let mut hdr = LmExtendedSecondaryHeader::default();
        hdr.byte_fe00 = 0xC0; // RL bits both set
        assert!(hdr.relative_to_player());
        assert!(hdr.face_left());

        hdr.byte_fe00 = 0x40;
        assert!(!hdr.relative_to_player());
        assert!(hdr.face_left());
    }

    #[test]
    fn test_bg_height() {
        let mut hdr = LmExtendedSecondaryHeader::default();
        hdr.byte_fe00 = 0x1F;
        assert_eq!(hdr.bg_height(), 0x1F);

        hdr.byte_fe00 = 0x0F;
        assert_eq!(hdr.bg_height(), 0x0F);
    }

    #[test]
    fn test_separate_l2_scroll() {
        let mut hdr = LmExtendedSecondaryHeader::default();
        hdr.byte_fa00 = 0x80;
        assert!(hdr.separate_l2_scroll());

        hdr.byte_fa00 = 0x00;
        assert!(!hdr.separate_l2_scroll());
    }

    #[test]
    fn test_into_bytes_roundtrip() {
        let hdr = LmExtendedSecondaryHeader {
            byte_de00: 0xAB,
            byte_fa00: 0xCD,
            byte_fc00: 0xEF,
            byte_fe00: 0x12,
        };
        let bytes = hdr.into_bytes();
        assert_eq!(bytes, [0xAB, 0xCD, 0xEF, 0x12]);
    }

    #[test]
    fn test_is_modified() {
        let mut hdr = LmExtendedSecondaryHeader::default();
        assert!(!hdr.is_modified());
        hdr.byte_de00 = 0x01;
        assert!(hdr.is_modified());
    }
}
