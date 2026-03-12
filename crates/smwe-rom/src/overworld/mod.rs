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
}
