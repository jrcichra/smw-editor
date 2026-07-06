//! Overworld map data parsed from the ROM.
//!
//! 7 submaps: 0=Main, 1=Yoshi's Island, 2=Vanilla Dome, 3=Forest of Illusion,
//!            4=Valley of Bowser, 5=Special World, 6=Star World.
//!
//! Layer 1 (interactive tiles) lives uncompressed at ROM $0CF7DF → WRAM $7EC800.
//! Layer 2 (background) is RLE-compressed at $04A533/$04C02B → WRAM $7F4000.

use crate::snes_utils::{addr::{AddrPc, AddrSnes}, rom::Rom};

pub const SUBMAP_COUNT: usize = 7;

pub const SUBMAP_NAMES: [&str; SUBMAP_COUNT] = [
    "Main Map",
    "Yoshi's Island",
    "Vanilla Dome",
    "Forest of Illusion",
    "Valley of Bowser",
    "Special World",
    "Star World",
];

/// OW Layer-1 uncompressed tilemap in the ROM (SNES $0CF7DF).
/// Full map: 64 columns × 32 rows of 8×8 tiles = 0x800 bytes.
pub const OWL1_TILE_DATA_SNES: AddrSnes = AddrSnes(0x0CF7DF);
pub const OWL1_TILE_DATA_SIZE: usize = 0x0800;

/// Width/height of the full packed overworld tilemap in tiles.
pub const OW_WIDTH_TILES: u32 = 64;
pub const OW_HEIGHT_TILES: u32 = 32;
pub const OW_WIDTH_PX: u32 = OW_WIDTH_TILES * 8;
pub const OW_HEIGHT_PX: u32 = OW_HEIGHT_TILES * 8;

/// Layer-1 tile IDs in this inclusive range are "level tiles" (the game scans
/// the tilemap in order and assigns each one a sequential "translevel" number).
/// Confirmed in SMWDisX `bank_04.asm` (`CODE_04D832`): `CMP #$56 BCC +` / `CMP #$81 BCS +`.
pub const OW_LEVEL_TILE_RANGE: std::ops::RangeInclusive<u8> = 0x56..=0x80;

#[derive(Debug)]
pub struct OverworldData {
    /// Raw layer-1 tile bytes (0x800), index = row*64 + col.
    pub layer1_tiles: Vec<u8>,
}

impl OverworldData {
    pub fn parse(rom: &Rom) -> anyhow::Result<Self> {
        let pc = AddrPc::try_from_lorom(OWL1_TILE_DATA_SNES)
            .map_err(|e| anyhow::anyhow!("OWL1TileData addr conversion: {e}"))?;
        let start = pc.0 as usize;
        let end = start + OWL1_TILE_DATA_SIZE;
        if end > rom.0.len() {
            anyhow::bail!("OWL1TileData extends past end of ROM");
        }
        Ok(Self { layer1_tiles: rom.0[start..end].to_vec() })
    }

    pub fn tile_at(&self, col: u32, row: u32) -> u8 {
        let idx = (row * OW_WIDTH_TILES + col) as usize;
        self.layer1_tiles.get(idx).copied().unwrap_or(0)
    }

    /// The vanilla game's "translevel" number for the tile at `(col, row)`, if
    /// it is a level tile (byte in `OW_LEVEL_TILE_RANGE`).
    ///
    /// This is NOT a free per-tile assignment: the real game scans
    /// `layer1_tiles` in index order (row-major) and assigns each level tile
    /// the next sequential number, starting at 0. Moving/inserting level tiles
    /// elsewhere on the map changes every subsequent tile's translevel number.
    /// Confirmed in SMWDisX `bank_04.asm` (`CODE_04D832`, building `OWLayer1Translevel`
    /// at WRAM `$7ED000`).
    pub fn translevel_at(&self, col: u32, row: u32) -> Option<u8> {
        translevel_for_index(&self.layer1_tiles, (row * OW_WIDTH_TILES + col) as usize)
    }

    /// The vanilla in-game level number for the tile at `(col, row)`, derived
    /// from its translevel number via the real game's remap: translevel < 0x25
    /// maps directly; translevel >= 0x25 has 0x24 subtracted. Confirmed in
    /// SMWDisX `bank_05.asm` (`CODE_05D8A2`, right after `OWLayer1Translevel` is
    /// loaded into `TranslevelNo`).
    pub fn level_number_at(&self, col: u32, row: u32) -> Option<u8> {
        self.translevel_at(col, row).map(translevel_to_level_number)
    }
}

/// Free-function form of [`OverworldData::translevel_at`], usable directly on
/// any layer-1 tile buffer (e.g. an in-progress editor buffer that may include
/// unsaved edits) by its flat `row * OW_WIDTH_TILES + col` index.
pub fn translevel_for_index(tiles: &[u8], idx: usize) -> Option<u8> {
    if idx >= tiles.len() || !OW_LEVEL_TILE_RANGE.contains(&tiles[idx]) {
        return None;
    }
    let count = tiles[..idx].iter().filter(|&&b| OW_LEVEL_TILE_RANGE.contains(&b)).count();
    Some(count as u8)
}

/// Free-function form of [`OverworldData::level_number_at`]; see
/// [`translevel_for_index`].
pub fn level_number_for_index(tiles: &[u8], idx: usize) -> Option<u8> {
    translevel_for_index(tiles, idx).map(translevel_to_level_number)
}

fn translevel_to_level_number(translevel: u8) -> u8 {
    if translevel < 0x25 { translevel } else { translevel - 0x24 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiles_with_level_ids_at(positions: &[usize]) -> Vec<u8> {
        let mut tiles = vec![0x00u8; OWL1_TILE_DATA_SIZE];
        for &idx in positions {
            tiles[idx] = 0x60; // arbitrary value inside OW_LEVEL_TILE_RANGE
        }
        tiles
    }

    #[test]
    fn translevel_numbers_assigned_in_scan_order() {
        let data = OverworldData { layer1_tiles: tiles_with_level_ids_at(&[5, 70, 130]) };
        assert_eq!(data.translevel_at(5, 0), Some(0));
        assert_eq!(data.translevel_at(70 % OW_WIDTH_TILES as usize as u32, 70 / OW_WIDTH_TILES as usize as u32), Some(1));
        assert_eq!(data.translevel_at(130 % OW_WIDTH_TILES as usize as u32, 130 / OW_WIDTH_TILES as usize as u32), Some(2));
        // Not a level tile.
        assert_eq!(data.translevel_at(0, 0), None);
    }

    #[test]
    fn level_number_remaps_past_0x24() {
        let mut positions = Vec::new();
        for i in 0..0x30usize {
            positions.push(i);
        }
        let data = OverworldData { layer1_tiles: tiles_with_level_ids_at(&positions) };
        // 0x24th tile (translevel 0x24, 0-indexed) is still < 0x25 -> unchanged.
        assert_eq!(data.level_number_at(0x24, 0), Some(0x24));
        // 0x25th tile (translevel 0x25) gets remapped: 0x25 - 0x24 = 0x01.
        assert_eq!(data.level_number_at(0x25, 0), Some(0x01));
    }
}
