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

/// Highest level number directly representable through the vanilla translevel
/// remap (see `translevel_to_level_number`): `0xFF - 0x24`.
pub const MAX_ASSIGNABLE_LEVEL_NUMBER: u8 = 0xDB;

/// SNES address of the 3-byte operand of `LDA.L $7ED000,X` in `bank_05.asm`
/// (right before `CODE_05D8A2`), confirmed byte-for-byte against a real ROM
/// (`BF 00 D0 7E` at PC `$02D89B`). Repointing this operand from
/// `OWLayer1Translevel` (WRAM, vanilla) to a custom ROM table (same `0x800`-
/// byte layout as `layer1_tiles`) lets each OW tile's level number be freely
/// assigned, without inserting any new code — see
/// `encode_custom_level_number` for why this doesn't need a JSL hijack.
pub const LEVEL_NUMBER_PATCH_OPERAND_SNES: AddrSnes = AddrSnes(0x05D89C);

/// Encode a desired level number so that, after the vanilla remap this ROM
/// patch leaves untouched (`translevel_to_level_number`), the tile resolves
/// to exactly `level_number`. This is why free level-number assignment here
/// doesn't need new ASM: we only ever change *what data* the existing
/// instruction reads, not the instruction itself or the remap that follows it.
///
/// Returns `None` if `level_number > MAX_ASSIGNABLE_LEVEL_NUMBER` (the remap's
/// u8 range can't represent it without a deeper ASM change).
pub fn encode_custom_level_number(level_number: u8) -> Option<u8> {
    if level_number < 0x25 {
        Some(level_number)
    } else if level_number <= MAX_ASSIGNABLE_LEVEL_NUMBER {
        Some(level_number + 0x24)
    } else {
        None
    }
}

/// Number of "destruction" events (castles/fortresses/switch palaces changing
/// tile after being beaten). Confirmed in SMWDisX `bank_04.asm` (the caller of
/// `CODE_04DA49` loops until `_F == 0x6F`).
pub const OW_EVENT_COUNT: usize = 0x6F;

/// Per-event byte offset into the layer-1 tilemap (same index space as
/// `OverworldData::layer1_tiles`), SNES $04D85D, 2 bytes/entry little-endian.
pub const OW_EVENT_TILE_OFFSET_SNES: AddrSnes = AddrSnes(0x04D85D);

/// "Before" tile IDs for the reveal-tile swap, SNES $04DA1D, 1 byte/entry.
pub const OW_EVENT_REVEAL_BEFORE_SNES: AddrSnes = AddrSnes(0x04DA1D);
/// "After" tile IDs for the reveal-tile swap, SNES $04DA33, 1 byte/entry,
/// parallel to `OW_EVENT_REVEAL_BEFORE_SNES`.
pub const OW_EVENT_REVEAL_AFTER_SNES: AddrSnes = AddrSnes(0x04DA33);
pub const OW_EVENT_REVEAL_COUNT: usize = 22;

/// Overworld "destruction" event data: which tile changes to which other tile
/// once a given event (numbered 0..[`OW_EVENT_COUNT`]) has been triggered
/// (typically by beating the level on that tile). Ported from SMWDisX
/// `bank_04.asm` `CODE_04DA49`; covers the reveal-tile-swap events only (not
/// the separate Layer 2 event table at `$04DD8D` or "silent" events at `$04E910`).
#[derive(Debug)]
pub struct OverworldEvents {
    /// `layer1_tiles` byte offset touched by each event, len `OW_EVENT_COUNT`.
    pub tile_offsets: Vec<u16>,
    /// "Before" tile IDs, len `OW_EVENT_REVEAL_COUNT`, parallel to `reveal_after`.
    pub reveal_before: Vec<u8>,
    /// "After" tile IDs, len `OW_EVENT_REVEAL_COUNT`, parallel to `reveal_before`.
    pub reveal_after: Vec<u8>,
}

impl OverworldEvents {
    pub fn parse(rom: &Rom) -> anyhow::Result<Self> {
        let offsets_pc = AddrPc::try_from_lorom(OW_EVENT_TILE_OFFSET_SNES)
            .map_err(|e| anyhow::anyhow!("OW event tile offset addr conversion: {e}"))?
            .0 as usize;
        let before_pc = AddrPc::try_from_lorom(OW_EVENT_REVEAL_BEFORE_SNES)
            .map_err(|e| anyhow::anyhow!("OW event reveal-before addr conversion: {e}"))?
            .0 as usize;
        let after_pc = AddrPc::try_from_lorom(OW_EVENT_REVEAL_AFTER_SNES)
            .map_err(|e| anyhow::anyhow!("OW event reveal-after addr conversion: {e}"))?
            .0 as usize;

        let offsets_end = offsets_pc + OW_EVENT_COUNT * 2;
        let before_end = before_pc + OW_EVENT_REVEAL_COUNT;
        let after_end = after_pc + OW_EVENT_REVEAL_COUNT;
        if offsets_end > rom.0.len() || before_end > rom.0.len() || after_end > rom.0.len() {
            anyhow::bail!("OW event data extends past end of ROM");
        }

        let tile_offsets = rom.0[offsets_pc..offsets_end]
            .chunks_exact(2)
            .map(|w| u16::from_le_bytes([w[0], w[1]]))
            .collect();
        let reveal_before = rom.0[before_pc..before_end].to_vec();
        let reveal_after = rom.0[after_pc..after_end].to_vec();

        Ok(Self { tile_offsets, reveal_before, reveal_after })
    }

    /// Apply the tile-reveal effect of every event in `active_events` (indices
    /// into `0..OW_EVENT_COUNT`) onto `layer1_tiles`, matching the real game's
    /// `CODE_04DA49`: for each active event, if the tile currently at its
    /// offset matches a "before" ID, replace it with the parallel "after" ID.
    /// One reveal entry (the last one, matching vanilla's switch-palace-reveal
    /// special case) also writes the following tile position.
    pub fn apply(&self, layer1_tiles: &mut [u8], active_events: &[bool]) {
        for (event_idx, &offset) in self.tile_offsets.iter().enumerate() {
            if !active_events.get(event_idx).copied().unwrap_or(false) {
                continue;
            }
            let pos = offset as usize;
            let Some(&current) = layer1_tiles.get(pos) else { continue };
            let Some(reveal_idx) = self.reveal_before.iter().position(|&b| b == current) else { continue };
            layer1_tiles[pos] = self.reveal_after[reveal_idx];
            if reveal_idx == self.reveal_before.len() - 1 {
                if let Some(slot) = layer1_tiles.get_mut(pos + 1) {
                    *slot = self.reveal_after[reveal_idx];
                }
            }
        }
    }
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
    fn encode_custom_level_number_inverts_the_vanilla_remap_exhaustively() {
        for level_number in 0..=MAX_ASSIGNABLE_LEVEL_NUMBER {
            let encoded = encode_custom_level_number(level_number)
                .unwrap_or_else(|| panic!("{level_number:#04X} should be representable"));
            assert_eq!(
                translevel_to_level_number(encoded),
                level_number,
                "round-trip failed for level_number={level_number:#04X} (encoded={encoded:#04X})"
            );
        }
    }

    #[test]
    fn encode_custom_level_number_rejects_out_of_range() {
        assert_eq!(encode_custom_level_number(0xDC), None);
        assert_eq!(encode_custom_level_number(0xFF), None);
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

    fn vanilla_events() -> OverworldEvents {
        // Values transcribed from SMWDisX bank_04.asm (DATA_04D85D/DATA_04DA1D/DATA_04DA33).
        OverworldEvents {
            tile_offsets: vec![0x0000, 0x0000, 0x0000, 0x0469, 0x044B, 0x0429, 0x0409, 0x00D3, 0x00E5],
            reveal_before: vec![
                0x6E, 0x6F, 0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x59, 0x53, 0x52, 0x83, 0x4D, 0x57, 0x5A, 0x76, 0x78,
                0x7A, 0x7B, 0x7D, 0x7F, 0x54,
            ],
            reveal_after: vec![
                0x66, 0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x58, 0x43, 0x44, 0x45, 0x25, 0x5E, 0x5F, 0x77, 0x79,
                0x63, 0x7C, 0x7E, 0x80, 0x23,
            ],
        }
    }

    #[test]
    fn event_apply_swaps_matching_before_tile() {
        let events = vanilla_events();
        let mut tiles = vec![0u8; OWL1_TILE_DATA_SIZE];
        tiles[0x0469] = 0x6E; // matches reveal_before[0]
        let mut active = vec![false; 9];
        active[3] = true; // event 3 -> offset 0x0469
        events.apply(&mut tiles, &active);
        assert_eq!(tiles[0x0469], 0x66); // reveal_after[0]
    }

    #[test]
    fn event_apply_noop_when_tile_does_not_match() {
        let events = vanilla_events();
        let mut tiles = vec![0u8; OWL1_TILE_DATA_SIZE];
        tiles[0x0469] = 0x00; // not in reveal_before
        let mut active = vec![false; 9];
        active[3] = true;
        events.apply(&mut tiles, &active);
        assert_eq!(tiles[0x0469], 0x00);
    }

    #[test]
    fn event_apply_last_reveal_entry_also_writes_next_tile() {
        let events = vanilla_events();
        let mut tiles = vec![0u8; OWL1_TILE_DATA_SIZE];
        tiles[0x0469] = 0x54; // matches reveal_before[21], the last/special entry
        let mut active = vec![false; 9];
        active[3] = true;
        events.apply(&mut tiles, &active);
        assert_eq!(tiles[0x0469], 0x23);
        assert_eq!(tiles[0x046A], 0x23);
    }

    #[test]
    fn event_apply_inactive_event_does_nothing() {
        let events = vanilla_events();
        let mut tiles = vec![0u8; OWL1_TILE_DATA_SIZE];
        tiles[0x0469] = 0x6E;
        let active = vec![false; 9];
        events.apply(&mut tiles, &active);
        assert_eq!(tiles[0x0469], 0x6E);
    }

    /// Confirms `LEVEL_NUMBER_PATCH_OPERAND_SNES` resolves to the exact bytes
    /// of the `LDA.L $7ED000,X` operand, byte-for-byte against a real ROM.
    /// Run with `ROM_PATH=/path/to/smw.smc cargo test -p smwe-rom --lib --
    /// --ignored level_number_patch_operand_is_correct`.
    #[test]
    #[ignore]
    fn level_number_patch_operand_is_correct() {
        let rom_path = std::env::var("ROM_PATH").expect("set ROM_PATH");
        let raw = std::fs::read(rom_path).expect("read ROM");
        let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };

        let opcode_pc = AddrPc::try_from_lorom(LEVEL_NUMBER_PATCH_OPERAND_SNES).unwrap().0 as usize - 1;
        assert_eq!(rom_bytes[opcode_pc], 0xBF, "expected LDA.L opcode right before the patch operand");
        let operand = &rom_bytes[opcode_pc + 1..opcode_pc + 4];
        assert_eq!(operand, &[0x00, 0xD0, 0x7E], "expected the operand to currently point at OWLayer1Translevel ($7ED000)");
    }
}
