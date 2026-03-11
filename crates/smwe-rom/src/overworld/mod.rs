//! Super Mario World Overworld Tilemap Parser
//!
//! The Layer 2 tilemap is stored compressed (LC_RLE2) at $04A533 (tile numbers)
//! and $04C02B (YXPCCCTT attributes). It decompresses to a single flat buffer
//! that is 40 tiles wide × 27 tiles tall = 1080 tiles total.
//!
//! All submaps (main map + 6 sub-areas) share this **same** buffer — only one
//! is ever loaded into VRAM at a time. The main map is displayed from buffer
//! origin (0, 0). The six sub-areas (Yoshi's Island etc.) are displayed from
//! buffer origin (2, 1) — i.e. their display tile (0,0) lives at buffer (2,1).
//!
//! Reference: https://smwspeedruns.com/Overworld_Data_Format
//!   "To locate a specific tile: index with ((Y * 40) + X) * 2.
//!    On submaps, subtract 2 from X and 1 from Y."

use thiserror::Error;

use crate::compression::lc_rle2::decompress_rle2;
use crate::disassembler::RomDisassembly;

pub const OW_SUBMAP_COUNT: usize = 7;

/// The decompressed buffer is always 40 tiles wide.
pub const OW_BUFFER_WIDTH: usize = 40;
/// The decompressed buffer is always 27 tiles tall.
pub const OW_BUFFER_HEIGHT: usize = 27;

/// Visible display size: full 40×27 tile buffer.
pub const OW_TILEMAP_COLS: usize = OW_BUFFER_WIDTH; // 40
pub const OW_TILEMAP_ROWS: usize = OW_BUFFER_HEIGHT; // 27
pub const OW_VISIBLE_ROWS: usize = OW_BUFFER_HEIGHT; // 27

pub const OW_TILEMAP_SIZE: usize = OW_TILEMAP_COLS * OW_TILEMAP_ROWS;

pub const OW_GFX_FILES: [usize; 4] = [0x1C, 0x1D, 0x08, 0x1E];

const TILE_DATA_SN: u32 = 0x04A533;
const ATTR_DATA_SN: u32 = 0x04C02B;
const SCROLL_X_SN: u32 = 0x04D89A;
const SCROLL_Y_SN: u32 = 0x04D8A1;

/// On submaps (sm > 0), the display origin is at buffer position (2, 1).
/// Main map (sm == 0) starts at buffer (0, 0).
pub const SUBMAP_ORIGIN_COL: usize = 2;
pub const SUBMAP_ORIGIN_ROW: usize = 1;

#[derive(Debug, Error)]
pub enum OverworldError {
    #[error("Tilemap read failed")]
    TilemapRead,

    #[error("Scroll read failed")]
    ScrollRead,

    #[error("Tile decompression failed")]
    Decompress,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct BgTile(pub u16);

impl BgTile {
    pub fn tile_index(self) -> u16 {
        self.0 & 0x3FF
    }

    pub fn palette(self) -> u8 {
        ((self.0 >> 10) & 7) as u8
    }

    pub fn priority(self) -> bool {
        (self.0 >> 13) != 0
    }

    pub fn flip_x(self) -> bool {
        (self.0 >> 14) != 0
    }

    pub fn flip_y(self) -> bool {
        (self.0 >> 15) != 0
    }

    pub fn page(self) -> usize {
        (self.tile_index() as usize) / 256
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

#[derive(Clone, Debug)]
pub struct SubmapInfo {
    pub name: &'static str,
    pub scroll_x: u16,
    pub scroll_y: u16,
}

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

#[derive(Clone, Debug)]
pub struct OverworldMaps {
    pub layer2: Vec<OwTilemap>,
    /// The raw decompressed buffer (40×27 tiles), shared by all submaps.
    pub raw_buffer: Vec<BgTile>,
}

impl OverworldMaps {
    pub fn empty() -> Self {
        Self {
            layer2: vec![OwTilemap::default(); OW_SUBMAP_COUNT],
            raw_buffer: vec![BgTile::default(); OW_TILEMAP_SIZE],
        }
    }

    pub fn parse(disasm: &mut RomDisassembly) -> Result<Self, OverworldError> {
        let rom = disasm.rom.0.as_ref();

        let tile_pc = lorom_pc(TILE_DATA_SN)?;
        let attr_pc = lorom_pc(ATTR_DATA_SN)?;
        let scroll_x_pc = lorom_pc(SCROLL_X_SN)?;
        let scroll_y_pc = lorom_pc(SCROLL_Y_SN)?;

        // Decompress exactly the real buffer: 40 × 27 × 2 bytes.
        let buffer_bytes = OW_BUFFER_WIDTH * OW_BUFFER_HEIGHT * 2;
        let tiles_u16 = decompress_rle2(&rom[tile_pc..], &rom[attr_pc..], buffer_bytes);

        log::info!("Overworld::parse: sample buffer dump (6x6) starting at (0,0):");
        for r in 0..6 {
            let mut line = String::new();
            for c in 0..6 {
                let idx = r * OW_BUFFER_WIDTH + c;
                let w = tiles_u16.get(idx).copied().unwrap_or(0);
                line.push_str(&format!("{:04X} ", w));
            }
            log::info!("  row {:02}: {}", r, line);
        }

        // also print a 6x6 box starting at the declared submap origin (2,1)
        let oc = SUBMAP_ORIGIN_COL;
        let orow = SUBMAP_ORIGIN_ROW;
        log::info!("Overworld::parse: sample buffer dump (6x6) at SUBMAP_ORIGIN (col={},row={}):", oc, orow);
        for r in orow..(orow + 6) {
            let mut line = String::new();
            for c in oc..(oc + 6) {
                let idx = r * OW_BUFFER_WIDTH + c;
                let w = tiles_u16.get(idx).copied().unwrap_or(0);
                line.push_str(&format!("{:04X} ", w));
            }
            log::info!("  raw[{}]: {}", r, line);
        }

        // Debug: log summary of decompressed buffer
        log::debug!("Overworld::parse: decompressed tiles_u16.len() = {}", tiles_u16.len());

        // Log first N entries to inspect layout (N small to avoid huge logs)
        for (i, &v) in tiles_u16.iter().enumerate().take(20) {
            let t = BgTile(v);
            log::debug!(
                "  raw[{}] = {:#06X} -> tile_index={:#05X} page={} pal={} flipX={} flipY={}",
                i,
                v,
                t.tile_index(),
                t.page(),
                t.palette(),
                t.flip_x(),
                t.flip_y()
            );
        }

        let raw_buffer: Vec<BgTile> = tiles_u16.iter().map(|&w| BgTile(w)).collect();

        let names = [
            "Main Map",
            "Yoshi's Island",
            "Vanilla Dome",
            "Forest of Illusion",
            "Valley of Bowser",
            "Special",
            "Star World",
        ];

        let mut layer2 = Vec::with_capacity(OW_SUBMAP_COUNT);

        for sm in 0..OW_SUBMAP_COUNT {
            // All submaps share the same buffer. Main map starts at (0,0);
            // submaps start at (SUBMAP_ORIGIN_COL, SUBMAP_ORIGIN_ROW) = (2,1).
            let (origin_col, origin_row) =
                if sm == 0 { (0usize, 0usize) } else { (SUBMAP_ORIGIN_COL, SUBMAP_ORIGIN_ROW) };

            let sx_off = scroll_x_pc + sm * 2;
            let sy_off = scroll_y_pc + sm * 2;
            if sx_off + 1 >= rom.len() || sy_off + 1 >= rom.len() {
                return Err(OverworldError::ScrollRead);
            }
            let scroll_x = u16::from_le_bytes([rom[sx_off], rom[sx_off + 1]]);
            let scroll_y = u16::from_le_bytes([rom[sy_off], rom[sy_off + 1]]);

            log::debug!(
                "Overworld::parse: submap {} origin=({},{}), scroll_x={:#06x}, scroll_y={:#06x}",
                sm,
                origin_col,
                origin_row,
                scroll_x,
                scroll_y
            );

            // Extract the visible 40×27 view for this submap from the shared buffer.
            // Columns that would go past the right edge wrap to sky (0).
            let mut tiles = Vec::with_capacity(OW_TILEMAP_SIZE);
            for row in 0..OW_TILEMAP_ROWS {
                for col in 0..OW_TILEMAP_COLS {
                    let bc = origin_col + col;
                    let br = origin_row + row;
                    let tile = if bc < OW_BUFFER_WIDTH && br < OW_BUFFER_HEIGHT {
                        tiles_u16[br * OW_BUFFER_WIDTH + bc]
                    } else {
                        0
                    };
                    // If this looks suspicious, you'll see it here
                    if tile != 0 && (row < 2 && col < 5) {
                        let t = BgTile(tile);
                        log::debug!(
                            "  submap {} tile at view({},{}) -> raw_buf[{},{}] = {:#06x} idx={:#05x} page={} pal={}",
                            sm,
                            col,
                            row,
                            br,
                            bc,
                            tile,
                            t.tile_index(),
                            t.page(),
                            t.palette()
                        );
                    }
                    tiles.push(BgTile(tile));
                }
            }

            layer2.push(OwTilemap {
                tiles,
                submap_index: sm,
                submap_info: SubmapInfo { name: names[sm], scroll_x, scroll_y },
            });
        }

        Ok(Self { layer2, raw_buffer })
    }

    pub fn write_to_rom_bytes(&self, _rom: &mut Vec<u8>) {}
}

fn lorom_pc(addr: u32) -> Result<usize, OverworldError> {
    let bank = ((addr >> 16) & 0xFF) as usize;
    let offset = (addr & 0xFFFF) as usize;

    if offset < 0x8000 {
        return Err(OverworldError::TilemapRead);
    }

    Ok(((bank & 0x7F) << 15) | (offset & 0x7FFF))
}
