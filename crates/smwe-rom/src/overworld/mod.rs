/// SMW Overworld tilemap parser.
///
/// # Data Sources (LoROM, after stripping SMC header)
///
/// Layer-2 (terrain) tilemap — LC_RLE2 compressed:
///   Tile numbers at $04A533
///   YXPCCCTT attributes at $04C02B
///
/// The decompressed data is 40 tiles wide and contains both the main map
/// and all 6 submaps in a single flat buffer.
///
/// Scroll positions for each submap (in pixels):
///   X scroll table at $04D89A (7 × 16-bit)
///   Y scroll table at $04D8A1 (7 × 16-bit)
///
/// Submap order: 0=Main, 1=Yoshi's Island, 2=Vanilla Dome, 3=Forest of Illusion,
///               4=Valley of Bowser, 5=Special, 6=Star World
///
/// To find a tile at submap (col, row):
///   buffer_col = (scroll_x / 8 + col) % 40
///   buffer_row = scroll_y / 8 + row
///   index = buffer_row * 40 + buffer_col
///
/// # GFX / VRAM mapping
/// All submaps use the same 4 GFX files: GFX1C, GFX1D, GFX08, GFX1E
/// (indices 0x1C, 0x1D, 0x08, 0x1E)
///
/// # Tile-entry bit layout
/// ```
///   15  14  13  12 11 10   9 …  0
///    Y   X   P  C2 C1 C0  T9 … T0
/// ```
/// Y/X = vertical/horizontal flip, P = BG priority, CCC = sub-palette (0-7),
/// T = 10-bit CHR tile index.
use thiserror::Error;

use crate::compression::lc_rle2::decompress_rle2;
use crate::disassembler::RomDisassembly;

// ── Constants ─────────────────────────────────────────────────────────────────

pub const OW_SUBMAP_COUNT: usize = 7;
pub const OW_BUFFER_WIDTH: usize = 40;
pub const OW_TILEMAP_COLS: usize = 32;
pub const OW_TILEMAP_ROWS: usize = 27;
pub const OW_VISIBLE_ROWS: usize = 27;

pub const OW_TILEMAP_SIZE: usize = OW_TILEMAP_COLS * OW_TILEMAP_ROWS;

pub const OW_GFX_FILES: [usize; 4] = [0x1C, 0x1D, 0x08, 0x1E];

const TILE_DATA_PC: u32 = 0x04A533;
const ATTR_DATA_PC: u32 = 0x04C02B;
const SCROLL_X_PC: u32 = 0x04D89A;
const SCROLL_Y_PC: u32 = 0x04D8A1;

const BUFFER_HEIGHT: usize = 80;

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum OverworldError {
    #[error("Failed to read OW tilemap data")]
    TilemapRead,
    #[error("Failed to read OW scroll positions")]
    ScrollRead,
}

// ── BgTile ───────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct BgTile(pub u16);

impl BgTile {
    #[inline]
    pub fn tile_index(self) -> u16 {
        self.0 & 0x3FF
    }
    #[inline]
    pub fn palette(self) -> u8 {
        ((self.0 >> 10) & 7) as u8
    }
    #[inline]
    pub fn priority(self) -> bool {
        (self.0 >> 13) & 1 != 0
    }
    #[inline]
    pub fn flip_x(self) -> bool {
        (self.0 >> 14) & 1 != 0
    }
    #[inline]
    pub fn flip_y(self) -> bool {
        (self.0 >> 15) & 1 != 0
    }

    pub fn new(tile_index: u16, palette: u8, priority: bool, flip_x: bool, flip_y: bool) -> Self {
        let mut v: u16 = tile_index & 0x3FF;
        v |= ((palette as u16) & 7) << 10;
        if priority {
            v |= 1 << 13;
        }
        if flip_x {
            v |= 1 << 14;
        }
        if flip_y {
            v |= 1 << 15;
        }
        Self(v)
    }
}

// ── SubmapInfo ──────────────────────────────────────────────────────────────

#[derive(Copy, Clone, Debug)]
pub struct SubmapInfo {
    pub name: &'static str,
    pub scroll_x: u16,
    pub scroll_y: u16,
}

impl SubmapInfo {
    pub fn tile_origin(&self) -> (usize, usize) {
        let col = ((self.scroll_x / 8) as usize) % OW_BUFFER_WIDTH;
        let row = (self.scroll_y / 8) as usize;
        (col, row)
    }
}

// ── OwTilemap ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct OwTilemap {
    pub tiles: Vec<BgTile>,
    pub submap_index: usize,
    pub submap_info: SubmapInfo,
}

impl Default for OwTilemap {
    fn default() -> Self {
        Self {
            tiles: vec![BgTile::default(); OW_TILEMAP_SIZE],
            submap_index: 0,
            submap_info: SubmapInfo { name: "Main", scroll_x: 0, scroll_y: 0 },
        }
    }
}

impl OwTilemap {
    pub fn get(&self, col: usize, row: usize) -> BgTile {
        self.tiles.get(row * OW_TILEMAP_COLS + col).copied().unwrap_or_default()
    }

    pub fn set(&mut self, col: usize, row: usize, tile: BgTile) {
        let idx = row * OW_TILEMAP_COLS + col;
        if let Some(slot) = self.tiles.get_mut(idx) {
            *slot = tile;
        }
    }
}

// ── OverworldMaps ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct OverworldMaps {
    pub layer2: Vec<OwTilemap>,
}

impl OverworldMaps {
    pub fn empty() -> Self {
        Self { layer2: vec![OwTilemap::default(); OW_SUBMAP_COUNT] }
    }

    pub fn parse(disasm: &mut RomDisassembly) -> Result<Self, OverworldError> {
        let rom = disasm.rom.0.as_ref();

        let tile_pc = lorom_pc(TILE_DATA_PC).ok_or(OverworldError::TilemapRead)?;
        let attr_pc = lorom_pc(ATTR_DATA_PC).ok_or(OverworldError::TilemapRead)?;
        let scroll_x_pc = lorom_pc(SCROLL_X_PC).ok_or(OverworldError::ScrollRead)?;
        let scroll_y_pc = lorom_pc(SCROLL_Y_PC).ok_or(OverworldError::ScrollRead)?;

        let total_tiles = OW_BUFFER_WIDTH * BUFFER_HEIGHT;
        let tiles = decompress_rle2(&rom[tile_pc..], &rom[attr_pc..], total_tiles * 2);

        let submap_names = [
            "Main",
            "Yoshi's Island",
            "Vanilla Dome",
            "Forest of Illusion",
            "Valley of Bowser",
            "Special",
            "Star World",
        ];

        let mut layer2 = Vec::with_capacity(OW_SUBMAP_COUNT);

        for sm in 0..OW_SUBMAP_COUNT {
            let scroll_x = u16::from_le_bytes([rom[scroll_x_pc + sm * 2], rom[scroll_x_pc + sm * 2 + 1]]);
            let scroll_y = u16::from_le_bytes([rom[scroll_y_pc + sm * 2], rom[scroll_y_pc + sm * 2 + 1]]);

            let (origin_col, origin_row) = {
                let col = ((scroll_x / 8) as usize) % OW_BUFFER_WIDTH;
                let row = (scroll_y / 8) as usize;
                (col, row)
            };

            let mut tilemap_tiles = Vec::with_capacity(OW_TILEMAP_SIZE);

            for row in 0..OW_TILEMAP_ROWS {
                for col in 0..OW_TILEMAP_COLS {
                    // No offset - matches the working example
                    let buffer_col = (origin_col + col) % OW_BUFFER_WIDTH;
                    let buffer_row = origin_row + row;
                    let idx = buffer_row * OW_BUFFER_WIDTH + buffer_col;
                    let tile = if idx < tiles.len() { BgTile(tiles[idx]) } else { BgTile::default() };
                    tilemap_tiles.push(tile);
                }
            }

            layer2.push(OwTilemap {
                tiles: tilemap_tiles,
                submap_index: sm,
                submap_info: SubmapInfo { name: submap_names[sm], scroll_x, scroll_y },
            });
        }

        Ok(Self { layer2 })
    }

    pub fn write_to_rom_bytes(&self, _rom: &mut Vec<u8>) {}
}

fn lorom_pc(snes: u32) -> Option<usize> {
    if snes & 0x8000 == 0 {
        return None;
    }
    Some((((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize)
}
