mod data;

pub use data::*;
use itertools::Itertools;
use nom::{combinator::map, multi::many0, number::complete::le_u16};
use thiserror::Error;

use crate::{
    disassembler::binary_block::{DataBlock, DataKind},
    objects::{
        animated_tile_data::AnimatedTileDataParseError,
        map16::{Block, Tile8x8},
    },
    snes_utils::{addr::AddrSnes, rom_slice::SnesSlice},
    RomDisassembly,
};

// -------------------------------------------------------------------------------------------------

#[derive(Debug, Error)]
#[error("Could not parse Map16 tiles at:\n- {0}")]
pub enum TilesetParseError {
    Slice(SnesSlice),
    AnimatedTileData(AnimatedTileDataParseError),
}

// -------------------------------------------------------------------------------------------------

pub const TILESETS_COUNT: usize = 5;
pub const OBJECT_TILESETS_COUNT: usize = 15;
pub const OBJECT_TO_MAP16_TILESET: [usize; OBJECT_TILESETS_COUNT] = [
    0, // 0: Normal 1
    1, // 1: Castle 1
    2, // 2: Rope 1
    3, // 3: Underground 1
    4, // 4: Switch Palace 1
    4, // 5: Ghost House 1
    2, // 6: Rope 2
    0, // 7: Normal 2
    2, // 8: Rope 3
    3, // 9: Underground 2
    3, // 10: Switch Palace 2
    3, // 11: Castle 2
    0, // 12: Cloud/Forest
    4, // 13: Ghost House 2
    3, // 14: Underground 3
];

// -------------------------------------------------------------------------------------------------

#[derive(Debug)]
pub struct Tilesets {
    pub tiles: Vec<Tile>,
    lm_map16: Option<LmMap16>,
}

#[derive(Debug)]
pub enum Tile {
    Shared(Block),
    TilesetSpecific([Block; TILESETS_COUNT]),
}

// -------------------------------------------------------------------------------------------------

impl Tilesets {
    pub fn parse(disasm: &mut RomDisassembly) -> Result<Self, TilesetParseError> {
        let mut parse_16x16 = |slice| parse_blocks(disasm, slice);

        let mut tiles: Vec<Tile> = Vec::with_capacity(0x200);

        let tiles_000_072 = parse_16x16(TILES_000_072)?.into_iter().map(Tile::Shared);
        let tiles_107_110 = parse_16x16(TILES_107_110)?.into_iter().map(Tile::Shared);
        let tiles_111_152 = parse_16x16(TILES_111_152)?.into_iter().map(Tile::Shared);
        let tiles_16e_1c3 = parse_16x16(TILES_16E_1C3)?.into_iter().map(Tile::Shared);
        let tiles_1c4_1c7 = parse_16x16(TILES_1C4_1C7)?.into_iter().map(Tile::Shared);
        let tiles_1c8_1eb = parse_16x16(TILES_1C8_1EB)?.into_iter().map(Tile::Shared);
        let tiles_1ec_1ef = parse_16x16(TILES_1EC_1EF)?.into_iter().map(Tile::Shared);
        let tiles_1f0_1ff = parse_16x16(TILES_1F0_1FF)?.into_iter().map(Tile::Shared);

        let mut parse_tileset_specific = |slices: [SnesSlice; 5]| {
            let it = itertools::izip!(
                parse_16x16(slices[0])?.into_iter(),
                parse_16x16(slices[1])?.into_iter(),
                parse_16x16(slices[2])?.into_iter(),
                parse_16x16(slices[3])?.into_iter(),
                parse_16x16(slices[4])?.into_iter(),
            )
            .map(|(e0, e1, e2, e3, e4)| Tile::TilesetSpecific([e0, e1, e2, e3, e4]));
            Ok(it)
        };

        let tiles_073_0ff = parse_tileset_specific(TILES_073_0FF)?;
        let tiles_100_106 = parse_tileset_specific(TILES_100_106)?;
        let tiles_153_16d = parse_tileset_specific(TILES_153_16D)?;

        tiles.extend(
            tiles_000_072
                .chain(tiles_073_0ff)
                .chain(tiles_100_106)
                .chain(tiles_107_110)
                .chain(tiles_111_152)
                .chain(tiles_153_16d)
                .chain(tiles_16e_1c3)
                .chain(tiles_1c4_1c7)
                .chain(tiles_1c8_1eb)
                .chain(tiles_1ec_1ef)
                .chain(tiles_1f0_1ff),
        );

        let lm_map16 = parse_lm_map16(disasm).ok();
        Ok(Tilesets { tiles, lm_map16 })
    }

    pub fn get_map16_tile(&self, tile_num: usize, tileset: usize) -> Option<Block> {
        if tile_num < self.tiles.len() && tileset < 5 {
            match self.tiles[tile_num] {
                Tile::Shared(tile) => Some(tile),
                Tile::TilesetSpecific(tiles) => Some(tiles[tileset]),
            }
        } else if let Some(lm) = &self.lm_map16 {
            lm.get(tile_num, tileset)
        } else {
            log::error!("Invalid tile_num ({:#X}) or tileset ({})", tile_num, tileset);
            None
        }
    }

    pub fn get_map16_tile_for_object_tileset(&self, tile_num: usize, object_tileset: usize) -> Option<Block> {
        let map16_tileset = object_tileset_to_map16_tileset(object_tileset);
        self.get_map16_tile(tile_num, map16_tileset)
    }
}

pub fn object_tileset_to_map16_tileset(object_tileset: usize) -> usize {
    if object_tileset < OBJECT_TILESETS_COUNT {
        OBJECT_TO_MAP16_TILESET[object_tileset]
    } else {
        OBJECT_TO_MAP16_TILESET[0]
    }
}

// -------------------------------------------------------------------------------------------------

#[derive(Debug)]
struct LmMap16 {
    blocks: Vec<Block>,
    present: Vec<bool>,
    page2_tileset_specific: Option<Vec<[Block; TILESETS_COUNT]>>,
}

impl LmMap16 {
    fn get(&self, tile_num: usize, tileset: usize) -> Option<Block> {
        if tile_num >= 0x200 && tile_num < 0x300 {
            if let Some(ts) = &self.page2_tileset_specific {
                if tileset < TILESETS_COUNT {
                    return Some(ts[tile_num - 0x200][tileset]);
                }
            }
        }
        if tile_num < self.blocks.len() && self.present[tile_num] {
            Some(self.blocks[tile_num])
        } else {
            None
        }
    }
}

fn parse_blocks(disasm: &mut RomDisassembly, slice: SnesSlice) -> Result<Vec<Block>, TilesetParseError> {
    let it = disasm
        .rom_slice_at_block(DataBlock { slice, kind: DataKind::Tileset }, |_| TilesetParseError::Slice(slice))?
        .parse(many0(map(le_u16, Tile8x8)))?
        .into_iter()
        .tuples::<(Tile8x8, Tile8x8, Tile8x8, Tile8x8)>()
        .map(Block::from_tuple);
    Ok(it.collect())
}

fn parse_block_from_bytes(bytes: &[u8]) -> Block {
    let ul = u16::from_le_bytes([bytes[0], bytes[1]]);
    let ll = u16::from_le_bytes([bytes[2], bytes[3]]);
    let ur = u16::from_le_bytes([bytes[4], bytes[5]]);
    let lr = u16::from_le_bytes([bytes[6], bytes[7]]);
    Block::from_tuple((Tile8x8(ul), Tile8x8(ll), Tile8x8(ur), Tile8x8(lr)))
}

fn blank_block() -> Block {
    Block::from_tuple((Tile8x8(0), Tile8x8(0), Tile8x8(0), Tile8x8(0)))
}

fn read_u8(disasm: &mut RomDisassembly, addr: u32) -> Result<u8, TilesetParseError> {
    let slice = SnesSlice::new(AddrSnes(addr), 1);
    let bytes = disasm
        .rom_slice_at_block(DataBlock { slice, kind: DataKind::Tileset }, |_| TilesetParseError::Slice(slice))?
        .as_bytes()?;
    bytes.get(0).copied().ok_or(TilesetParseError::Slice(slice))
}

fn read_u16(disasm: &mut RomDisassembly, addr: u32) -> Result<u16, TilesetParseError> {
    let slice = SnesSlice::new(AddrSnes(addr), 2);
    let bytes = disasm
        .rom_slice_at_block(DataBlock { slice, kind: DataKind::Tileset }, |_| TilesetParseError::Slice(slice))?
        .as_bytes()?;
    if bytes.len() < 2 {
        return Err(TilesetParseError::Slice(slice));
    }
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn parse_lm_map16(disasm: &mut RomDisassembly) -> Result<LmMap16, TilesetParseError> {
    // Sources:
    // - https://smwspeedruns.com/Level_Data_Format  (Map16 Data section)
    // - https://www.smwcentral.net/ (SMW Memory Map: Map16 page pointers)
    #[derive(Clone, Copy)]
    struct Range {
        start: u8,
        end: u8,
        lo: u32,
        bank: u32,
        add: u32,
        alt_add: Option<u32>,
    }

    let ranges = [
        Range { start: 0x02, end: 0x0F, lo: 0x06F553, bank: 0x06F557, add: 0, alt_add: Some(0x1000) },
        Range { start: 0x10, end: 0x1F, lo: 0x06F55C, bank: 0x06F560, add: 0, alt_add: Some(0x8000) },
        Range { start: 0x20, end: 0x2F, lo: 0x06F567, bank: 0x06F56B, add: 1, alt_add: None },
        Range { start: 0x30, end: 0x3F, lo: 0x06F570, bank: 0x06F574, add: 1, alt_add: Some(0x8000 + 1) },
        Range { start: 0x40, end: 0x4F, lo: 0x06F594, bank: 0x06F598, add: 0, alt_add: None },
        Range { start: 0x50, end: 0x5F, lo: 0x06F59D, bank: 0x06F5A1, add: 0, alt_add: Some(0x8000) },
        Range { start: 0x60, end: 0x6F, lo: 0x06F5A8, bank: 0x06F5AC, add: 1, alt_add: None },
        Range { start: 0x70, end: 0x7F, lo: 0x06F5B1, bank: 0x06F5B5, add: 1, alt_add: Some(0x8000 + 1) },
    ];

    let mut blocks = vec![blank_block(); 0x8000];
    let mut present = vec![false; 0x8000];

    let is_valid_lorom = |addr: u32| -> bool { (addr & 0xFFFF) >= 0x8000 };

    for r in ranges {
        let bank = read_u8(disasm, r.bank)? as u32;
        let lo = read_u16(disasm, r.lo)? as u32;
        let base = (bank << 16) | lo;
        let mut base_addr = AddrSnes(base.wrapping_add(r.add));

        let mut ok = true;
        if !is_valid_lorom(base_addr.0) {
            ok = false;
        }
        for page in r.start..=r.end {
            if !ok {
                break;
            }
            let offset = (page as u32 - r.start as u32) * 0x800;
            let slice = SnesSlice::new(AddrSnes(base_addr.0 + offset), 0x800);
            match parse_blocks(disasm, slice) {
                Ok(page_blocks) => {
                    for (i, block) in page_blocks.into_iter().take(0x100).enumerate() {
                        let tile_num = ((page as usize) << 8) | i;
                        if tile_num < blocks.len() {
                            blocks[tile_num] = block;
                            present[tile_num] = true;
                        }
                    }
                }
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }

        if !ok {
            if let Some(alt_add) = r.alt_add {
                let alt_lo = lo.wrapping_add(alt_add) & 0xFFFF;
                base_addr = AddrSnes((bank << 16) | alt_lo);
                if !is_valid_lorom(base_addr.0) {
                    log::warn!("Map16 pages {:02X}-{:02X} base address invalid", r.start, r.end);
                    continue;
                }
                for page in r.start..=r.end {
                    let offset = (page as u32 - r.start as u32) * 0x800;
                    let slice = SnesSlice::new(AddrSnes(base_addr.0 + offset), 0x800);
                    let page_blocks = parse_blocks(disasm, slice)?;
                    for (i, block) in page_blocks.into_iter().take(0x100).enumerate() {
                        let tile_num = ((page as usize) << 8) | i;
                        if tile_num < blocks.len() {
                            blocks[tile_num] = block;
                            present[tile_num] = true;
                        }
                    }
                }
                log::warn!(
                    "Map16 pages {:02X}-{:02X} used alternate base +{:#X}",
                    r.start,
                    r.end,
                    alt_add
                );
            } else {
                log::warn!("Map16 pages {:02X}-{:02X} could not be parsed", r.start, r.end);
            }
        }
    }

    let present_count = present.iter().filter(|p| **p).count();
    if present_count == 0 {
        // Fallback for Lunar Magic expanded ROMs: try a flat block starting at $0F8000.
        // This is a heuristic for ROMs that store Map16 pages contiguously.
        let base = AddrSnes(0x0F8000);
        let mut ok = true;
        for page in 0x02_u8..=0x7F_u8 {
            let offset = (page as u32 - 0x02) * 0x800;
            let slice = SnesSlice::new(AddrSnes(base.0 + offset), 0x800);
            match parse_blocks(disasm, slice) {
                Ok(page_blocks) => {
                    for (i, block) in page_blocks.into_iter().take(0x100).enumerate() {
                        let tile_num = ((page as usize) << 8) | i;
                        if tile_num < blocks.len() {
                            blocks[tile_num] = block;
                            present[tile_num] = true;
                        }
                    }
                }
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            log::warn!("Map16 pages 02-7F loaded using fallback base $0F8000");
        }
    }

    let present_count = present.iter().filter(|p| **p).count();
    if present_count == 0 {
        // Last-resort: scan for RATS-tagged Map16 data blocks and pick the best match.
        if let Some((base_off, pages, score)) = scan_rats_map16(&disasm.rom.0) {
            for page_idx in 0..pages {
                let page = 0x02_u8 + page_idx as u8;
                if page > 0x7F {
                    break;
                }
                let offset = base_off + page_idx * 0x800;
                let page_bytes = &disasm.rom.0[offset..offset + 0x800];
                for tile in 0..0x100_usize {
                    let t_off = tile * 8;
                    let block = parse_block_from_bytes(&page_bytes[t_off..t_off + 8]);
                    let tile_num = ((page as usize) << 8) | tile;
                    if tile_num < blocks.len() {
                        blocks[tile_num] = block;
                        present[tile_num] = true;
                    }
                }
            }
            log::warn!(
                "Map16 pages 02-{:02X} loaded from RATS block @0x{:X} (score {:.3})",
                0x02 + pages as u8 - 1,
                base_off,
                score
            );
        }
    }

    // Tileset-specific Map16 on page 2 (Lunar Magic).
    let page2_enabled = read_u8(disasm, 0x06F547)? != 0;
    let page2_tileset_specific = if page2_enabled {
        let bank = read_u8(disasm, 0x06F58A)? as u32;
        let lo = read_u16(disasm, 0x06F586)? as u32;
        let base = (bank << 16) | lo;
        let base = AddrSnes(base.wrapping_add(0x1000));
        if (base.0 & 0xFFFF) < 0x8000 {
            log::warn!("Tileset-specific page 2 base address invalid; skipping");
            None
        } else {
        let size = TILESETS_COUNT * 0x100 * 8;
        let slice = SnesSlice::new(base, size);
        let bytes = disasm
            .rom_slice_at_block(DataBlock { slice, kind: DataKind::Tileset }, |_| TilesetParseError::Slice(slice))?
            .as_bytes()?;
        let mut out: Vec<[Block; TILESETS_COUNT]> = Vec::with_capacity(0x100);
        for tile in 0..0x100_usize {
            let mut per_ts = [blank_block(); TILESETS_COUNT];
            for ts in 0..TILESETS_COUNT {
                let offset = (ts << 11) | (tile << 3);
                if offset + 8 <= bytes.len() {
                    per_ts[ts] = parse_block_from_bytes(&bytes[offset..offset + 8]);
                }
            }
            out.push(per_ts);
        }
        Some(out)
        }
    } else {
        None
    };

    Ok(LmMap16 { blocks, present, page2_tileset_specific })
}

fn scan_rats_map16(bytes: &[u8]) -> Option<(usize, usize, f32)> {
    let mut best: Option<(usize, usize, f32)> = None;
    let mut i = 0usize;
    while i + 8 < bytes.len() {
        if &bytes[i..i + 4] == b"STAR" {
            let size = u16::from_le_bytes([bytes[i + 4], bytes[i + 5]]) as usize;
            let inv = u16::from_le_bytes([bytes[i + 6], bytes[i + 7]]);
            if (size as u16) ^ inv == 0xFFFF {
                let len = size + 1;
                let data_start = i + 8;
                let data_end = data_start + len;
                if data_end <= bytes.len() && len >= 0x800 && len % 0x800 == 0 {
                    let pages = len / 0x800;
                    let score = map16_score(&bytes[data_start..data_end]);
                    let accept = score >= 0.85;
                    if accept {
                        let replace = match best {
                            None => true,
                            Some((_, best_pages, best_score)) => {
                                pages > best_pages || (pages == best_pages && score > best_score)
                            }
                        };
                        if replace {
                            best = Some((data_start, pages, score));
                        }
                    }
                }
            }
        }
        i += 1;
    }
    best
}

fn map16_score(data: &[u8]) -> f32 {
    let mut total = 0u32;
    let mut ok = 0u32;
    let stride = 8 * 8;
    let mut i = 0usize;
    while i + 8 <= data.len() && total < 2048 {
        let block = parse_block_from_bytes(&data[i..i + 8]);
        let tiles = [block.upper_left, block.lower_left, block.upper_right, block.lower_right];
        let mut good = true;
        for t in tiles {
            let tile_num = t.0 & 0x3FF;
            let pal = (t.0 >> 10) & 0x7;
            if tile_num > 0x3FF || pal > 7 {
                good = false;
                break;
            }
        }
        if good {
            ok += 1;
        }
        total += 1;
        i += stride;
    }
    if total == 0 {
        0.0
    } else {
        ok as f32 / total as f32
    }
}
