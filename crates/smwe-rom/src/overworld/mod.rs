/// SMW Overworld tilemap parser + serialiser.
///
/// # Memory layout (LoROM, after stripping SMC header)
///
/// Layer-2 BG (background terrain) tilemaps — 6 submaps × 0x800 bytes:
///   submap 0  $0C8000
///   submap 1  $0C8800
///   submap 2  $0C9000
///   submap 3  $0C9800
///   submap 4  $0CA000
///   submap 5  $0CA800
///
/// Layer-1 FG (paths / events / overlay) tilemaps — 6 submaps × 0x800 bytes:
///   submap 0  $0CAC00
///   submap 1  $0CB400
///   submap 2  $0CBC00
///   submap 3  $0CC400
///   submap 4  $0CCC00
///   submap 5  $0CD400
///
/// Each page is a 32-column × 32-row = 1024-entry grid of 16-bit little-endian SNES BG entries.
/// The game displays only the top 27 rows (rows 0-26); rows 27-31 are not visible.
///
/// # Tile-entry bit layout
/// ```
///   15  14  13  12 11 10   9 … 0
///    Y   X   P  C2 C1 C0  T9 … T0
/// ```
/// Y/X = vertical/horizontal flip, P = BG priority, CCC = sub-palette (0-7),
/// T = 10-bit CHR tile index.
///
/// # GFX / VRAM mapping for layer-2 tiles
/// GFX file 00 → CHR 0x000–0x07F   (3bpp tiles decoded into 8×8 pixel blocks)
/// GFX file 01 → CHR 0x080–0x0FF
///
/// # Palette rows used by OW layer-2
/// Sub-palette 0 → colour-palette row 4  (CGRAM offsets 0x40–0x4F)
/// Sub-palette 1 → colour-palette row 5  (0x50–0x5F)
/// Sub-palette 2 → colour-palette row 6  (0x60–0x6F)
/// Sub-palette 3 → colour-palette row 7  (0x70–0x7F)
use thiserror::Error;

use crate::disassembler::RomDisassembly;

// ── Constants ─────────────────────────────────────────────────────────────────

pub const OW_SUBMAP_COUNT: usize = 6;
pub const OW_TILEMAP_COLS: usize = 32;
pub const OW_TILEMAP_ROWS: usize = 32;
pub const OW_VISIBLE_ROWS: usize = 27;
/// Total entries (tiles) in one tilemap page.
pub const OW_TILEMAP_SIZE: usize = OW_TILEMAP_COLS * OW_TILEMAP_ROWS; // 1024
/// Bytes for one tilemap page.
pub const OW_TILEMAP_BYTES: usize = OW_TILEMAP_SIZE * 2; // 2048

// SNES base addresses — each submap = +0x800 from the previous.
const OW_LAYER2_BASE: u32 = 0x0C8000;
const OW_LAYER1_BASE: u32 = 0x0CAC00;

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum OverworldError {
    #[error("Failed to read OW layer-2 tilemap for submap {0}")]
    Layer2(usize),
    #[error("Failed to read OW layer-1 tilemap for submap {0}")]
    Layer1(usize),
}

// ── BgTile ────────────────────────────────────────────────────────────────────

/// One 16-bit SNES BG tile-map entry.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct BgTile(pub u16);

impl BgTile {
    /// 10-bit CHR tile index (0–1023).
    #[inline]
    pub fn tile_index(self) -> u16 {
        self.0 & 0x3FF
    }
    /// Sub-palette selector (0–7).
    #[inline]
    pub fn palette(self) -> u8 {
        ((self.0 >> 10) & 7) as u8
    }
    /// BG priority bit.
    #[inline]
    pub fn priority(self) -> bool {
        (self.0 >> 13) & 1 != 0
    }
    /// Horizontal flip.
    #[inline]
    pub fn flip_x(self) -> bool {
        (self.0 >> 14) & 1 != 0
    }
    /// Vertical flip.
    #[inline]
    pub fn flip_y(self) -> bool {
        (self.0 >> 15) & 1 != 0
    }

    /// Build a new entry from components.
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

// ── OwTilemap ─────────────────────────────────────────────────────────────────

/// A 32×32 page of BG tile entries for one OW submap layer (row-major).
#[derive(Clone, Debug)]
pub struct OwTilemap {
    pub tiles: Vec<BgTile>, // length = OW_TILEMAP_SIZE
}

impl Default for OwTilemap {
    fn default() -> Self {
        Self { tiles: vec![BgTile::default(); OW_TILEMAP_SIZE] }
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
    /// Layer-2 (terrain background) tilemaps, one per submap.
    pub layer2: Vec<OwTilemap>,
    /// Layer-1 (paths / events) tilemaps, one per submap.
    pub layer1: Vec<OwTilemap>,
}

impl OverworldMaps {
    pub fn empty() -> Self {
        Self {
            layer2: (0..OW_SUBMAP_COUNT).map(|_| OwTilemap::default()).collect(),
            layer1: (0..OW_SUBMAP_COUNT).map(|_| OwTilemap::default()).collect(),
        }
    }

    pub fn parse(disasm: &mut RomDisassembly) -> Result<Self, OverworldError> {
        let mut layer2 = Vec::with_capacity(OW_SUBMAP_COUNT);
        let mut layer1 = Vec::with_capacity(OW_SUBMAP_COUNT);

        for sm in 0..OW_SUBMAP_COUNT {
            layer2.push(parse_one(disasm, OW_LAYER2_BASE, sm, OverworldError::Layer2)?);
            layer1.push(parse_one(disasm, OW_LAYER1_BASE, sm, OverworldError::Layer1)?);
        }

        Ok(Self { layer2, layer1 })
    }

    /// Serialise layer-1 tilemaps back into the raw ROM byte slice.
    pub fn write_layer1_to_rom(&self, rom: &mut Vec<u8>) {
        write_layer(rom, OW_LAYER1_BASE, &self.layer1);
    }

    /// Serialise all tilemaps (both layers) back into the raw ROM byte slice.
    pub fn write_to_rom_bytes(&self, rom: &mut Vec<u8>) {
        write_layer(rom, OW_LAYER2_BASE, &self.layer2);
        write_layer(rom, OW_LAYER1_BASE, &self.layer1);
    }
}

/// Write an edited layer-2 map (provided separately from ROM) into raw ROM bytes.
pub fn write_layer2_to_rom(rom: &mut Vec<u8>, maps: &[OwTilemap]) {
    write_layer(rom, OW_LAYER2_BASE, maps);
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_one(
    disasm: &mut RomDisassembly, base_snes: u32, submap: usize, mk_err: impl Fn(usize) -> OverworldError,
) -> Result<OwTilemap, OverworldError> {
    // Read directly from raw ROM bytes to avoid block-tracking conflicts with
    // level data that may have already claimed overlapping regions.
    let snes = base_snes + (submap * OW_TILEMAP_BYTES) as u32;
    let pc = lorom_pc(snes).ok_or_else(|| mk_err(submap))?;
    let rom = disasm.rom.0.as_ref();
    let bytes = rom.get(pc..pc + OW_TILEMAP_BYTES).ok_or_else(|| mk_err(submap))?;
    let tiles = bytes.chunks_exact(2).map(|c| BgTile(u16::from_le_bytes([c[0], c[1]]))).collect();
    Ok(OwTilemap { tiles })
}

fn lorom_pc(snes: u32) -> Option<usize> {
    if snes & 0x8000 == 0 {
        return None;
    }
    Some((((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)) as usize)
}

fn write_layer(rom: &mut Vec<u8>, base_snes: u32, maps: &[OwTilemap]) {
    for (sm, tilemap) in maps.iter().enumerate() {
        let snes = base_snes + (sm * OW_TILEMAP_BYTES) as u32;
        if let Some(pc) = lorom_pc(snes) {
            for (i, tile) in tilemap.tiles.iter().enumerate() {
                let off = pc + i * 2;
                if off + 1 < rom.len() {
                    let bytes = tile.0.to_le_bytes();
                    rom[off] = bytes[0];
                    rom[off + 1] = bytes[1];
                }
            }
        }
    }
}
