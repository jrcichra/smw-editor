//! Super Mario World Overworld Tilemap Parser
//!
//! # Data layout
//!
//! Layer 2 is stored in two separate LC_RLE2-compressed streams:
//!   * `$04A533` – tile numbers (one byte per tile)
//!   * `$04C02B` – YXPCCCTT attribute bytes (one byte per tile)
//!
//! After decompression the two streams are interleaved into a single flat
//! u16 buffer of **40 × 27 = 1080** tiles.  All seven submaps share this
//! same buffer – only one is ever loaded into SNES VRAM at a time.
//!
//! Each u16 word: `YXPCCCTT_tttttttt`
//!   bits 15   = flip Y
//!   bits 14   = flip X
//!   bits 13   = priority
//!   bits 12-10= CCC palette (0-7; OW layer2 uses 4-7)
//!   bits 9-8  = TT  high tile-index bits (→ CHR bits 9-8, but in practice 0)
//!   bits 7-0  = tttttttt low tile-index byte
//!   CHR index = (word & 0x3FF)
//!
//! # Submap origins
//!
//! | Submap | Buffer origin col | Buffer origin row |
//! |--------|-------------------|-------------------|
//! | Main   | 0                 | 0                 |
//! | Others | 2                 | 1                 |
//!
//! Main map editor shows all 40 cols.
//! Submaps show 32 cols × 27 rows (one SNES screen wide).
//!
//! # References
//! https://smwspeedruns.com/Overworld_Data_Format

use crate::compression::lc_rle2::decompress_rle2;
use crate::disassembler::RomDisassembly;
use thiserror::Error;

// ── Public constants ──────────────────────────────────────────────────────────

pub const OW_SUBMAP_COUNT: usize = 7;

/// Width of the shared raw decompressed buffer (tiles).
pub const OW_BUFFER_WIDTH: usize = 40;
/// Height of the shared raw decompressed buffer (tiles).
pub const OW_BUFFER_HEIGHT: usize = 27;

/// Total tiles in the shared buffer.
pub const OW_TILEMAP_SIZE: usize = OW_BUFFER_WIDTH * OW_BUFFER_HEIGHT; // 1080

/// The main map is shown at full buffer width (40 tiles).
pub const OW_MAIN_COLS: usize = OW_BUFFER_WIDTH; // 40
/// Submaps are one SNES screen wide (32 tiles = 256 px).
pub const OW_SUBMAP_COLS: usize = 32;
/// All submaps are 27 tiles tall.
pub const OW_TILEMAP_ROWS: usize = OW_BUFFER_HEIGHT; // 27

// Keep these aliases so world_editor.rs can import them without change.
pub const OW_TILEMAP_COLS: usize = OW_MAIN_COLS; // used by main-map path
pub const OW_VISIBLE_ROWS: usize = OW_TILEMAP_ROWS;

/// Column offset in the raw buffer where all submap (1-6) content begins.
pub const SUBMAP_ORIGIN_COL: usize = 2;
/// Row offset in the raw buffer where all submap (1-6) content begins.
pub const SUBMAP_ORIGIN_ROW: usize = 1;

/// The four GFX files the overworld loads into VRAM (3 bpp each, 128 tiles/file).
/// CHR bit layout:  bits 8-7 = slot (0-3),  bits 6-0 = tile within file.
///   slot 0 → GFX 0x1C  (CHR 0x000-0x07F)
///   slot 1 → GFX 0x1D  (CHR 0x080-0x0FF)
///   slot 2 → GFX 0x08  (CHR 0x100-0x17F)
///   slot 3 → GFX 0x1E  (CHR 0x180-0x1FF)
pub const OW_GFX_FILES: [usize; 4] = [0x1C, 0x1D, 0x08, 0x1E];

// ── SNES address helpers ──────────────────────────────────────────────────────

/// Convert a LoROM SNES address to a PC (file) offset.
fn lorom_to_pc(snes: u32) -> Result<usize, OverworldError> {
    let bank = ((snes >> 16) & 0xFF) as usize;
    let off  = (snes & 0xFFFF) as usize;
    if off < 0x8000 {
        return Err(OverworldError::BadAddress(snes));
    }
    Ok(((bank & 0x7F) << 15) | (off & 0x7FFF))
}

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum OverworldError {
    #[error("ROM address out of range: ${0:06X}")]
    BadAddress(u32),
    #[error("Tilemap read failed")]
    TilemapRead,
    #[error("Scroll data read failed")]
    ScrollRead,
    #[error("Decompression produced too few bytes")]
    Decompress,
}

// ── BgTile ────────────────────────────────────────────────────────────────────

/// One SNES BG2 tile word.  Layout: `YXPCCCTT_tttttttt`
#[derive(Copy, Clone, Debug, Default)]
pub struct BgTile(pub u16);

impl BgTile {
    /// 10-bit CHR index (used to look up the GFX file and tile within it).
    #[inline]
    pub fn tile_index(self) -> u16 { self.0 & 0x3FF }

    /// Sub-palette (0-7).  OW layer2 uses values 4-7.
    #[inline]
    pub fn palette(self) -> u8 { ((self.0 >> 10) & 7) as u8 }

    /// Priority bit.
    #[inline]
    pub fn priority(self) -> bool { (self.0 >> 13) & 1 != 0 }

    /// Horizontal flip.
    #[inline]
    pub fn flip_x(self) -> bool { (self.0 >> 14) & 1 != 0 }

    /// Vertical flip.
    #[inline]
    pub fn flip_y(self) -> bool { (self.0 >> 15) & 1 != 0 }

    /// Construct a new BgTile from components.
    pub fn new(tile_index: u16, palette: u8, priority: bool, flip_x: bool, flip_y: bool) -> Self {
        let mut v = tile_index & 0x3FF;
        v |= ((palette as u16) & 7) << 10;
        if priority { v |= 1 << 13; }
        if flip_x   { v |= 1 << 14; }
        if flip_y   { v |= 1 << 15; }
        Self(v)
    }
}

// ── OwTilemap ─────────────────────────────────────────────────────────────────

/// The visible tile grid for one submap (already sliced out of the shared buffer).
///
/// For the main map this is 40 × 27.
/// For submaps 1-6 this is 32 × 27 (one SNES screen).
#[derive(Clone, Debug)]
pub struct OwTilemap {
    pub submap_index: usize,
    /// Number of columns in this view (40 for main, 32 for submaps).
    pub cols: usize,
    /// Number of rows (always 27).
    pub rows: usize,
    /// Tile data, row-major, `rows * cols` entries.
    pub tiles: Vec<BgTile>,
}

impl Default for OwTilemap {
    fn default() -> Self {
        Self {
            submap_index: 0,
            cols: OW_MAIN_COLS,
            rows: OW_TILEMAP_ROWS,
            tiles: vec![BgTile::default(); OW_MAIN_COLS * OW_TILEMAP_ROWS],
        }
    }
}

impl OwTilemap {
    /// Get the tile at display position `(col, row)`.
    /// Returns a blank tile if out of bounds.
    pub fn get(&self, col: usize, row: usize) -> BgTile {
        self.tiles.get(row * self.cols + col).copied().unwrap_or_default()
    }

    /// Set the tile at display position `(col, row)`.
    pub fn set(&mut self, col: usize, row: usize, tile: BgTile) {
        let idx = row * self.cols + col;
        if let Some(slot) = self.tiles.get_mut(idx) {
            *slot = tile;
        }
    }
}

// ── OverworldMaps ─────────────────────────────────────────────────────────────

/// Parsed overworld data: one `OwTilemap` per submap plus the raw shared buffer.
#[derive(Clone, Debug)]
pub struct OverworldMaps {
    /// Per-submap views (7 total, indices 0-6).
    pub layer2: Vec<OwTilemap>,
    /// The full 40 × 27 raw buffer (used for saving / debugging).
    pub raw_buffer: Vec<BgTile>,
}

impl OverworldMaps {
    pub fn empty() -> Self {
        let mut layer2 = Vec::with_capacity(OW_SUBMAP_COUNT);
        for sm in 0..OW_SUBMAP_COUNT {
            let cols = if sm == 0 { OW_MAIN_COLS } else { OW_SUBMAP_COLS };
            layer2.push(OwTilemap {
                submap_index: sm,
                cols,
                rows: OW_TILEMAP_ROWS,
                tiles: vec![BgTile::default(); cols * OW_TILEMAP_ROWS],
            });
        }
        Self {
            layer2,
            raw_buffer: vec![BgTile::default(); OW_TILEMAP_SIZE],
        }
    }

    pub fn parse(disasm: &mut RomDisassembly) -> Result<Self, OverworldError> {
        let rom = disasm.rom.0.as_ref();

        // ── 1. Decompress the shared 40×27 buffer ────────────────────────────
        let tile_pc = lorom_to_pc(0x04A533)?;
        let attr_pc = lorom_to_pc(0x04C02B)?;

        let words = decompress_rle2(
            &rom[tile_pc..],
            &rom[attr_pc..],
            OW_TILEMAP_SIZE * 2,
        );

        if words.len() < OW_TILEMAP_SIZE {
            log::error!(
                "Overworld decompression produced only {} tiles (expected {})",
                words.len(), OW_TILEMAP_SIZE
            );
            return Err(OverworldError::Decompress);
        }

        let raw_buffer: Vec<BgTile> = words.iter().map(|&w| BgTile(w)).collect();

        // Debug dump – first 6 rows, first 16 cols
        log::debug!("Overworld raw buffer (first 6 rows × 16 cols):");
        for r in 0..6 {
            let row: String = (0..16)
                .map(|c| format!("{:04X} ", raw_buffer[r * OW_BUFFER_WIDTH + c].0))
                .collect();
            log::debug!("  row {:02}: {}", r, row);
        }

        // ── 2. Slice each submap's view out of the shared buffer ──────────────
        let mut layer2 = Vec::with_capacity(OW_SUBMAP_COUNT);

        for sm in 0..OW_SUBMAP_COUNT {
            // Main map: full 40-wide, origin (0, 0)
            // Submaps:  32-wide,      origin (2, 1)
            let (origin_col, origin_row, cols) = if sm == 0 {
                (0usize, 0usize, OW_MAIN_COLS)
            } else {
                (SUBMAP_ORIGIN_COL, SUBMAP_ORIGIN_ROW, OW_SUBMAP_COLS)
            };

            let mut tiles = Vec::with_capacity(cols * OW_TILEMAP_ROWS);
            for row in 0..OW_TILEMAP_ROWS {
                for col in 0..cols {
                    let br = origin_row + row;
                    let bc = origin_col + col;
                    let tile = if br < OW_BUFFER_HEIGHT && bc < OW_BUFFER_WIDTH {
                        raw_buffer[br * OW_BUFFER_WIDTH + bc]
                    } else {
                        BgTile::default()
                    };
                    tiles.push(tile);
                }
            }

            log::debug!(
                "Overworld submap {} origin=({},{}) size={}×{}  first non-sky: {:?}",
                sm, origin_col, origin_row, cols, OW_TILEMAP_ROWS,
                tiles.iter().position(|t| t.0 != 0x1C75 && t.0 != 0)
            );

            layer2.push(OwTilemap {
                submap_index: sm,
                cols,
                rows: OW_TILEMAP_ROWS,
                tiles,
            });
        }

        Ok(Self { layer2, raw_buffer })
    }

    /// Stub – LC_RLE2 recompression not yet implemented.
    pub fn write_to_rom_bytes(&self, _rom: &mut Vec<u8>) {}
}
