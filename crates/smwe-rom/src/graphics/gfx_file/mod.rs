mod data;

use std::fmt::{self, Display, Formatter};

pub(crate) use data::GFX_FILES_META;
use epaint::Rgba;
use nom::{bytes::complete::take, IResult};
use smwe_render::color::Abgr1555;
use thiserror::Error;

use crate::{
    compression::{lc_lz2, DecompressionError},
    disassembler::binary_block::DataKind,
    snes_utils::{addr::AddrSnes, rom_slice::SnesSlice},
    RomDisassembly, RomError,
};

// -------------------------------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum GfxFileParseError {
    #[error("Isolating GFX data:\n- {0}")]
    IsolatingData(RomError),
    #[error("Decompressing GFX data:\n- {0}")]
    DecompressingData(DecompressionError),
    #[error("Parsing GFX tile")]
    ParsingTile,
}

// -------------------------------------------------------------------------------------------------

pub const N_PIXELS_IN_TILE: usize = 8 * 8;
const GFX_POINTER_TABLE_LEN: usize = 0x32;
const GFX_POINTER_TABLE_LOW: AddrSnes = AddrSnes(0x00B992);
const GFX_POINTER_TABLE_HIGH: AddrSnes = AddrSnes(0x00B9C4);
const GFX_POINTER_TABLE_BANK: AddrSnes = AddrSnes(0x00B9F6);

// -------------------------------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TileFormat {
    Tile2bpp,
    Tile3bpp,
    Tile4bpp,
    Tile8bpp,
    Tile3bppMode7,
}

#[derive(Debug, Clone)]
pub struct Tile {
    pub color_indices: Box<[u8]>,
}

#[derive(Debug, Clone)]
pub struct GfxFile {
    pub tile_format: TileFormat,
    pub tiles: Vec<Tile>,
}

// -------------------------------------------------------------------------------------------------

impl Display for TileFormat {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        use TileFormat::*;
        f.write_str(match self {
            Tile2bpp => "2BPP",
            Tile3bpp => "3BPP",
            Tile4bpp => "4BPP",
            Tile8bpp => "8BPP",
            Tile3bppMode7 => "3BPP Mode 7",
        })
    }
}

impl TileFormat {
    pub fn tile_size(self) -> usize {
        use TileFormat::*;
        match self {
            Tile2bpp => 2 * 8,
            Tile3bpp => 3 * 8,
            Tile4bpp => 4 * 8,
            Tile8bpp => 8 * 8,
            Tile3bppMode7 => 3 * 8,
        }
    }
}

impl Tile {
    pub fn from_2bpp(input: &[u8]) -> IResult<&[u8], Self> {
        Self::from_xbpp(input, 2)
    }

    pub fn from_3bpp(input: &[u8]) -> IResult<&[u8], Self> {
        let (input, bytes) = take(24usize)(input)?;
        let mut tile = Tile { color_indices: [0; N_PIXELS_IN_TILE].into() };

        for i in 0..N_PIXELS_IN_TILE {
            let (row, col) = (i / 8, 7 - (i % 8));
            let bit1 = (bytes[2 * row] >> col) & 1;
            let bit2 = (bytes[2 * row + 1] >> col) & 1;
            let bit3 = (bytes[16 + row] >> col) & 1;
            tile.color_indices[i] = (bit3 << 2) | (bit2 << 1) | bit1;
        }

        Ok((input, tile))
    }

    pub fn from_4bpp(input: &[u8]) -> IResult<&[u8], Self> {
        Self::from_xbpp(input, 4)
    }

    pub fn from_8bpp(input: &[u8]) -> IResult<&[u8], Self> {
        Self::from_xbpp(input, 8)
    }

    fn from_xbpp(input: &[u8], x: usize) -> IResult<&[u8], Self> {
        debug_assert!([2, 4, 8].contains(&x));
        let (input, bytes) = take(x * 8)(input)?;
        let mut tile = Tile { color_indices: [0; N_PIXELS_IN_TILE].into() };

        for i in 0..N_PIXELS_IN_TILE {
            let (row, col) = (i / 8, 7 - (i % 8));
            let mut color_idx = 0;
            for bit_idx in 0..x {
                let byte_idx = (2 * row) + (16 * (bit_idx / 2)) + (bit_idx % 2);
                let color_idx_bit = (bytes[byte_idx] >> col) & 1;
                color_idx |= color_idx_bit << bit_idx;
            }
            tile.color_indices[i] = color_idx;
        }

        Ok((input, tile))
    }

    pub fn from_3bpp_mode7(input: &[u8]) -> IResult<&[u8], Self> {
        let (input, bytes) = take(24usize)(input)?;
        let mut color_indices = [0u8; 64];
        for row in 0..8 {
            let raw_row = ((bytes[(3 * row) + 0] as u32) << 16)
                | ((bytes[(3 * row) + 1] as u32) << 8)
                | ((bytes[(3 * row) + 2] as u32) << 0);
            for row_pixel in 0..8 {
                let tile_pixel = (8 * row) + row_pixel;
                let index = (raw_row >> (3 * (7 - row_pixel))) & 0b111;
                color_indices[tile_pixel] = index as u8;
            }
        }
        let tile = Tile { color_indices: Box::new(color_indices) };
        Ok((input, tile))
    }

    pub fn to_bgr555(&self, palette: &[Abgr1555]) -> Box<[Abgr1555]> {
        self.color_indices
            .iter()
            .copied()
            .map(|color_index| {
                palette.get(color_index as usize).copied().unwrap_or_else(|| {
                    eprintln!("Tile::to_bgr555: i={color_index}, pl={}", palette.len());
                    Abgr1555::MAGENTA
                })
            })
            .collect()
    }

    pub fn to_rgba(&self, palette: &[Abgr1555]) -> Box<[Rgba]> {
        self.to_bgr555(palette).iter().copied().map(Rgba::from).collect()
    }

    pub fn to_bgr555_with_substitute_at(
        &self, palette: &[Abgr1555], sub_color: Abgr1555, sub_idx: u8,
    ) -> Box<[Abgr1555]> {
        self.color_indices
            .iter()
            .copied()
            .map(|color_index| {
                if color_index == sub_idx {
                    sub_color
                } else {
                    palette.get(color_index as usize).copied().unwrap_or_else(|| {
                        eprintln!("Tile::to_bgr555: i={color_index}, pl={}", palette.len());
                        Abgr1555::MAGENTA
                    })
                }
            })
            .collect()
    }

    pub fn to_rgba_with_substitute_at(&self, palette: &[Abgr1555], sub_color: Abgr1555, sub_idx: u8) -> Box<[Rgba]> {
        self.to_bgr555_with_substitute_at(palette, sub_color, sub_idx).iter().copied().map(Rgba::from).collect()
    }
}

impl GfxFile {
    fn read_pointer_byte(disasm: &mut RomDisassembly, addr: AddrSnes) -> Result<u8, GfxFileParseError> {
        let slice = SnesSlice::new(addr, 1);
        let bytes = disasm
            .rom_slice_at_block(
                crate::disassembler::binary_block::DataBlock { slice, kind: DataKind::GfxFile },
                GfxFileParseError::IsolatingData,
            )?
            .as_bytes()?;
        bytes.first().copied().ok_or(GfxFileParseError::ParsingTile)
    }

    fn resolve_slice(disasm: &mut RomDisassembly, file_num: usize) -> Result<SnesSlice, GfxFileParseError> {
        let (_, slice) = GFX_FILES_META[file_num];
        if file_num >= GFX_POINTER_TABLE_LEN {
            return Ok(slice);
        }

        let low = Self::read_pointer_byte(disasm, GFX_POINTER_TABLE_LOW + file_num)?;
        let high = Self::read_pointer_byte(disasm, GFX_POINTER_TABLE_HIGH + file_num)?;
        let bank = Self::read_pointer_byte(disasm, GFX_POINTER_TABLE_BANK + file_num)?;
        let start = AddrSnes(((bank as u32) << 16) | ((high as u32) << 8) | (low as u32));
        Ok(slice.move_to(start))
    }

    pub fn new(disasm: &mut RomDisassembly, file_num: usize, revised_gfx: bool) -> Result<Self, GfxFileParseError> {
        use TileFormat::*;
        type ParserFn = fn(&[u8]) -> IResult<&[u8], Tile>;

        debug_assert!(file_num < GFX_FILES_META.len());
        let (tile_format, _) = GFX_FILES_META[file_num];
        let slice = Self::resolve_slice(disasm, file_num)?;
        let (tile_parser, tile_size_bytes): (ParserFn, usize) = match tile_format {
            Tile2bpp => (Tile::from_2bpp, 2 * 8),
            Tile3bpp => (Tile::from_3bpp, 3 * 8),
            Tile4bpp => (Tile::from_4bpp, 4 * 8),
            Tile8bpp => (Tile::from_8bpp, 8 * 8),
            Tile3bppMode7 => (Tile::from_3bpp_mode7, 3 * 8),
        };

        let decompressed = disasm
            .rom
            .with_error_mapper(|e| match e {
                RomError::SliceSnes(_) | RomError::SlicePc(_) => GfxFileParseError::IsolatingData(e),
                RomError::Decompress(DecompressionError::LcLz2(l)) => GfxFileParseError::DecompressingData(l.into()),
                RomError::Parse => GfxFileParseError::ParsingTile,
                _ => unreachable!(),
            })
            .slice_lorom(slice.infinite())?
            .decompress(move |slice| lc_lz2::decompress(slice, revised_gfx))?;
        let bytes = decompressed.view().as_bytes()?;

        let mut tiles = Vec::with_capacity(bytes.len() / tile_size_bytes);
        let mut input = bytes;
        while input.len() >= tile_size_bytes {
            let (rest, tile) = tile_parser(input).map_err(|_| GfxFileParseError::ParsingTile)?;
            input = rest;
            tiles.push(tile);
        }
        if !input.is_empty() {
            log::warn!(
                "GFX file {file_num:02X} had {} trailing bytes after tile decode; ignoring remainder",
                input.len()
            );
        }

        Ok(Self { tile_format, tiles })
    }

    pub fn n_pixels(&self) -> usize {
        self.tiles.len() * N_PIXELS_IN_TILE
    }
}
