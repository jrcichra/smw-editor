use crate::compression::{lc_rle1, DecompressionError};

// -------------------------------------------------------------------------------------------------

pub type BackgroundTileID = u8;

#[derive(Debug, Clone)]
pub struct BackgroundData {
    tile_ids:        Vec<BackgroundTileID>,
    compressed_size: usize,
}

// -------------------------------------------------------------------------------------------------

impl BackgroundData {
    /// Returns self and the number of bytes consumed by parsing.
    pub fn read_from(input: &[u8]) -> Result<(Self, usize), DecompressionError> {
        let (tile_ids, bytes_consumed) = lc_rle1::decompress(input)?;
        Ok((Self { tile_ids, compressed_size: bytes_consumed }, bytes_consumed))
    }

    /// Build from already-decompressed tile IDs (e.g. imported from a .mwl);
    /// `compressed_size` reflects what an LC-RLE1 re-compression would take.
    pub fn from_tile_ids(tile_ids: Vec<BackgroundTileID>) -> Self {
        let compressed_size = lc_rle1::compress(&tile_ids).len();
        Self { tile_ids, compressed_size }
    }

    pub fn tile_ids(&self) -> &[BackgroundTileID] {
        &self.tile_ids
    }

    pub fn compressed_size(&self) -> usize {
        self.compressed_size
    }
}
