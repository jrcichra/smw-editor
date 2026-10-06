//! Overworld map data parsed from the ROM.
//!
//! 7 submaps: 0=Main, 1=Yoshi's Island, 2=Vanilla Dome, 3=Forest of Illusion,
//!            4=Valley of Bowser, 5=Special World, 6=Star World.
//!
//! Layer 1 (interactive tiles) lives uncompressed at ROM $0CF7DF → WRAM $7EC800.
//! Layer 2 (background) is RLE-compressed at $04A533/$04C02B → WRAM $7F4000.

pub mod event_ownership;
pub mod level_names;
pub mod reveal_list;
pub mod secret_exits;
pub mod sprites;
pub mod start_positions;
pub mod submap_music;

use crate::snes_utils::{
    addr::{AddrPc, AddrSnes},
    rom::Rom,
};

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
    if translevel < 0x25 {
        translevel
    } else {
        translevel - 0x24
    }
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
/// the Layer 2 event table at [`OW_L2_EVENT_TABLE_SNES`] below, or the
/// "silent" events it also describes).
#[derive(Debug)]
pub struct OverworldEvents {
    /// `layer1_tiles` byte offset touched by each event, len `OW_EVENT_COUNT`.
    pub tile_offsets:  Vec<u16>,
    /// "Before" tile IDs, len `OW_EVENT_REVEAL_COUNT`, parallel to `reveal_after`.
    pub reveal_before: Vec<u8>,
    /// "After" tile IDs, len `OW_EVENT_REVEAL_COUNT`, parallel to `reveal_before`.
    pub reveal_after:  Vec<u8>,
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

        let tile_offsets =
            rom.0[offsets_pc..offsets_end].chunks_exact(2).map(|w| u16::from_le_bytes([w[0], w[1]])).collect();
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

// ── Layer 2 event tiles ──────────────────────────────────────────────────────
// The same "destruction" events also drive Layer 2 changes (the animated
// event sequences: e.g. a path appearing, a castle crumbling on Layer 2).
// Two ROM data sources, both confirmed in SMWDisX `bank_04.asm`:
//
// 1. The Layer 2 event entry table at SNES $04DD8D: fixed-size 4-byte entries
//    `(data_word, dest_word)`. Which entries an event touches comes from the
//    cumulative boundary table at SNES $04E359: event `e` uses entries
//    `boundaries[e]..boundaries[e+1]`. Driven by `CODE_04E453` (event-number
//    path) and `CODE_04E6D3`/`CODE_04E6F9` (animation path).
// 2. The "silent event" tables at $04E8E4/$04E910/$04E93C/$04E994: 44 explicit
//    event numbers, each with a direct `(data_word, dest_word)` pair, driven
//    by `CODE_04E9EC` (only the entries whose `$04E910` flag has bit 0 set are
//    Layer 2 events; the rest are direct Map16 edits).

/// SNES address of the Layer 2 event entry table, 4 bytes per entry:
/// `data_word` (u16 LE) then `dest_word` (u16 LE).
pub const OW_L2_EVENT_TABLE_SNES: AddrSnes = AddrSnes(0x04DD8D);
pub const OW_L2_EVENT_ENTRY_SIZE: usize = 4;
/// The table spans SNES $04DD8D..$04E359 (exclusive): 1484 bytes = 371 entries.
/// The last per-event boundary is exactly 371, confirming the extent.
pub const OW_L2_EVENT_ENTRY_COUNT: usize = 371;

/// SNES address of the per-event entry-index boundary table (u16 LE words).
pub const OW_L2_EVENT_BOUNDARIES_SNES: AddrSnes = AddrSnes(0x04E359);
/// 121 boundary words: event `e` (0..120) uses entries
/// `boundaries[e]..boundaries[e+1]`. Slots past [`OW_EVENT_COUNT`] are present
/// but empty (all boundaries equal the table length).
pub const OW_L2_EVENT_BOUNDARY_COUNT: usize = 121;
/// Number of event slots the boundary table actually indexes.
pub const OW_L2_EVENT_SLOT_COUNT: usize = OW_L2_EVENT_BOUNDARY_COUNT - 1;

/// SNES address of the "silent event" event-number list (44 u8).
pub const OW_SILENT_EVENT_LIST_SNES: AddrSnes = AddrSnes(0x04E8E4);
/// SNES address of the "silent event" flags (44 u8); bit 0 set means the
/// entry is a Layer 2 event, clear means a direct Map16 tile edit.
pub const OW_SILENT_EVENT_FLAGS_SNES: AddrSnes = AddrSnes(0x04E910);
/// SNES address of the "silent event" dest words (44 u16 LE).
pub const OW_SILENT_EVENT_DEST_SNES: AddrSnes = AddrSnes(0x04E93C);
/// SNES address of the "silent event" data words (44 u16 LE).
pub const OW_SILENT_EVENT_DATA_SNES: AddrSnes = AddrSnes(0x04E994);
pub const OW_SILENT_EVENT_COUNT: usize = 44;

/// `data_word` threshold from the real game (`CODE_04E4A9` / `CODE_04EE30`):
/// `CPY.W #$0900; BCC <tile-stream path>`.
pub const OW_L2_EVENT_TILE_STREAM_THRESHOLD: u16 = 0x0900;

/// What a Layer 2 event entry does, decoded from its `data_word`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum L2EventKind {
    /// `data_word` < $0900: that many 8×8 tiles are streamed to VRAM through
    /// the dynamic stripe image (`CODE_04E824`).
    TileStream(u16),
    /// `data_word` >= $0900: an offset into the WRAM tilemap-data buffer
    /// (`$7F8000 + data_word`); its contents are copied onto
    /// `OWLayer2Tilemap` (`CODE_04E4D0` / `CODE_04E76C`).
    TilemapCopy(u16),
}

/// One 4-byte entry of the Layer 2 event table at [`OW_L2_EVENT_TABLE_SNES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct L2EventEntry {
    /// First word of the entry. Selects the kind (see [`L2EventKind`]).
    pub data_word: u16,
    /// Second word of the entry. Base offset into `OWLayer2Tilemap` and the
    /// source of the on-screen target position (see [`L2EventEntry::target_tile`]).
    pub dest_word: u16,
}

impl L2EventEntry {
    pub fn kind(&self) -> L2EventKind {
        if self.data_word < OW_L2_EVENT_TILE_STREAM_THRESHOLD {
            L2EventKind::TileStream(self.data_word)
        } else {
            L2EventKind::TilemapCopy(self.data_word)
        }
    }

    /// The overworld tile (8×8-tile coordinates, 64×32 map) this entry targets,
    /// decoded exactly like the real game does in `CODE_04E6F9`:
    /// `x_px = ((dest & $3E) << 2)`, `y_px = (((dest >> 3) as u8) & $F8)`.
    pub fn target_tile(&self) -> (u8, u8) {
        let x_px = ((self.dest_word & 0x3E) << 2) as u8;
        let y_px = ((self.dest_word >> 3) as u8) & 0xF8;
        (x_px / 8, y_px / 8)
    }
}

/// One row of the "silent event" tables: an explicit event number with a
/// direct `(data_word, dest_word)` pair (`CODE_04E9EC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SilentEvent {
    /// The destruction-event number (from `$04E8E4`).
    pub event_no:  u8,
    /// Bit 0 of the `$04E910` flag: true = Layer 2 event, false = direct
    /// Map16 tile edit.
    pub is_l2:     bool,
    /// Data word (from `$04E994`); same meaning as [`L2EventEntry::data_word`].
    pub data_word: u16,
    /// Dest word (from `$04E93C`); same meaning as [`L2EventEntry::dest_word`].
    pub dest_word: u16,
}

impl SilentEvent {
    /// View this silent event's pair as a Layer 2 entry (only meaningful when
    /// [`SilentEvent::is_l2`] is true).
    pub fn as_entry(&self) -> L2EventEntry {
        L2EventEntry { data_word: self.data_word, dest_word: self.dest_word }
    }
}

/// The parsed Layer 2 event data: the entry table, the per-event boundary
/// table, and the "silent event" rows.
#[derive(Debug)]
pub struct OverworldL2Events {
    /// 371 entries from [`OW_L2_EVENT_TABLE_SNES`].
    pub entries:       Vec<L2EventEntry>,
    /// 121 cumulative boundary words from [`OW_L2_EVENT_BOUNDARIES_SNES`].
    pub boundaries:    Vec<u16>,
    /// 44 rows from the `$04E8E4`/`$04E910`/`$04E93C`/`$04E994` tables.
    pub silent_events: Vec<SilentEvent>,
}

impl OverworldL2Events {
    fn read_u16_words(rom: &Rom, addr: AddrSnes, count: usize, what: &str) -> anyhow::Result<Vec<u16>> {
        let pc = AddrPc::try_from_lorom(addr).map_err(|e| anyhow::anyhow!("{what} addr conversion: {e}"))?.0 as usize;
        let end = pc + count * 2;
        if end > rom.0.len() {
            anyhow::bail!("{what} extends past end of ROM");
        }
        Ok(rom.0[pc..end].chunks_exact(2).map(|w| u16::from_le_bytes([w[0], w[1]])).collect())
    }

    pub fn parse(rom: &Rom) -> anyhow::Result<Self> {
        let pc = AddrPc::try_from_lorom(OW_L2_EVENT_TABLE_SNES)
            .map_err(|e| anyhow::anyhow!("OW L2 event table addr conversion: {e}"))?
            .0 as usize;
        let end = pc + OW_L2_EVENT_ENTRY_COUNT * OW_L2_EVENT_ENTRY_SIZE;
        if end > rom.0.len() {
            anyhow::bail!("OW L2 event table extends past end of ROM");
        }
        let entries = rom.0[pc..end]
            .chunks_exact(OW_L2_EVENT_ENTRY_SIZE)
            .map(|e| L2EventEntry {
                data_word: u16::from_le_bytes([e[0], e[1]]),
                dest_word: u16::from_le_bytes([e[2], e[3]]),
            })
            .collect();

        let boundaries = Self::read_u16_words(
            rom,
            OW_L2_EVENT_BOUNDARIES_SNES,
            OW_L2_EVENT_BOUNDARY_COUNT,
            "OW L2 event boundaries",
        )?;
        // The boundary table must be cumulative: event e uses entries
        // boundaries[e]..boundaries[e+1], and the last boundary is the entry
        // count. Validate rather than silently accepting a shifted ROM.
        for w in windows2(&boundaries) {
            if w[0] > w[1] {
                anyhow::bail!("OW L2 event boundaries are not cumulative");
            }
        }
        if boundaries.last().copied().unwrap_or(0) as usize > OW_L2_EVENT_ENTRY_COUNT {
            anyhow::bail!("OW L2 event boundaries exceed the entry table");
        }

        let list_pc = AddrPc::try_from_lorom(OW_SILENT_EVENT_LIST_SNES)
            .map_err(|e| anyhow::anyhow!("OW silent event list addr conversion: {e}"))?
            .0 as usize;
        let flags_pc = AddrPc::try_from_lorom(OW_SILENT_EVENT_FLAGS_SNES)
            .map_err(|e| anyhow::anyhow!("OW silent event flags addr conversion: {e}"))?
            .0 as usize;
        if list_pc + OW_SILENT_EVENT_COUNT > rom.0.len() || flags_pc + OW_SILENT_EVENT_COUNT > rom.0.len() {
            anyhow::bail!("OW silent event list/flags extend past end of ROM");
        }
        let dest_words =
            Self::read_u16_words(rom, OW_SILENT_EVENT_DEST_SNES, OW_SILENT_EVENT_COUNT, "OW silent event dest")?;
        let data_words =
            Self::read_u16_words(rom, OW_SILENT_EVENT_DATA_SNES, OW_SILENT_EVENT_COUNT, "OW silent event data")?;
        let silent_events = (0..OW_SILENT_EVENT_COUNT)
            .map(|i| SilentEvent {
                event_no:  rom.0[list_pc + i],
                is_l2:     rom.0[flags_pc + i] & 0x01 != 0,
                data_word: data_words[i],
                dest_word: dest_words[i],
            })
            .collect();

        Ok(Self { entries, boundaries, silent_events })
    }

    /// The entry-index range event `event` (0..[`OW_L2_EVENT_SLOT_COUNT`])
    /// touches. Returns `None` for out-of-range slots; the range may be empty
    /// (the event has no Layer 2 entries).
    pub fn entries_for_event(&self, event: usize) -> Option<std::ops::Range<usize>> {
        if event >= OW_L2_EVENT_SLOT_COUNT {
            return None;
        }
        let start = *self.boundaries.get(event)? as usize;
        let end = *self.boundaries.get(event + 1)? as usize;
        Some(start..end.min(self.entries.len()))
    }

    /// All Layer 2 silent-event rows for a given destruction-event number.
    pub fn silent_l2_events_for(&self, event_no: u8) -> Vec<&SilentEvent> {
        self.silent_events.iter().filter(|s| s.is_l2 && s.event_no == event_no).collect()
    }

    /// Number of entries in the table (== [`OW_L2_EVENT_ENTRY_COUNT`]).
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

fn windows2(v: &[u16]) -> impl Iterator<Item = &[u16]> {
    v.windows(2)
}

/// SNES address of the per-event RLE tilemap blob (`OWEventTileProp` in
/// SMWDisX `bank_0C.asm`, `ORG $0C8000`). The blob is a sequence of packets —
/// control byte `b`: `(b & 0x7F) + 1` bytes follow literally, or (bit 7 set) a
/// single byte repeated `(b & 0x7F) + 1` times — terminated by a `$FFFF` word
/// (see `CODE_04DD57`). Decoding the whole blob yields exactly
/// [`OW_EVENT_TILEMAP_DECODED_LEN`] bytes into WRAM `OWEventTilemap`.
pub const OW_EVENT_TILEMAP_PROP_SNES: AddrSnes = AddrSnes(0x0C8D00);

/// Decoded size of the [`OW_EVENT_TILEMAP_PROP_SNES`] blob: WRAM
/// `OWEventTilemap` is `skip 3328` in SMWDisX `rammap.asm`.
pub const OW_EVENT_TILEMAP_DECODED_LEN: usize = 0x0D00;

/// Write one RLE2-compressed overworld Layer 2 stream (`OWTileNumbers` /
/// `OWTilemap`) into `rom_bytes`, repointing the game's stream pointer when
/// the new payload no longer fits the old location. `start_pc_no_header` is
/// the stream's vanilla PC file offset (without SMC header); `output_len` is
/// the per-stream decompressed byte count.
///
/// When the payload fits, it is written in place over the old stream (any
/// leftover tail zeroed); otherwise fresh free space is allocated, the old
/// location is erased to `$FF`, and the single ROM reference to the old SNES
/// address is patched to the new one.
pub fn write_overworld_l2_stream(
    rom_bytes: &mut [u8], has_smc_header: bool, start_pc_no_header: usize, output_len: usize, compressed: &[u8],
    label: &str,
) -> anyhow::Result<()> {
    use crate::compression::lc_rle2;

    let header_offset = usize::from(has_smc_header) * 0x200;
    let start = start_pc_no_header + header_offset;
    let old_size = lc_rle2::compressed_size_for_output(
        rom_bytes.get(start..).ok_or_else(|| anyhow::anyhow!("{label} ROM source start out of bounds"))?,
        output_len,
    );
    if compressed.len() <= old_size {
        let dst = rom_bytes
            .get_mut(start..start + old_size)
            .ok_or_else(|| anyhow::anyhow!("{label} ROM write range out of bounds"))?;
        dst[..compressed.len()].copy_from_slice(compressed);
        dst[compressed.len()..].fill(0);
    } else {
        let new_pc = crate::freespace::find_free_space(rom_bytes, compressed.len(), 0x008000, header_offset)
            .ok_or_else(|| anyhow::anyhow!("{label} no free space found for {} bytes", compressed.len()))?;

        if let Some(dst) = rom_bytes.get_mut(start..start + old_size) {
            dst.fill(0xFF);
        }

        let new_file = new_pc + header_offset;
        rom_bytes
            .get_mut(new_file..new_file + compressed.len())
            .ok_or_else(|| anyhow::anyhow!("{label} new location write out of bounds"))?
            .copy_from_slice(compressed);

        let old_snes = AddrSnes::try_from_lorom(AddrPc(start_pc_no_header as u32))?.0;
        let new_snes = AddrSnes::try_from_lorom(AddrPc(new_pc as u32))?.0;
        patch_snes_pointer(rom_bytes, old_snes, new_snes, label)?;
    }
    Ok(())
}

/// Find the ROM file offset holding the 3-byte little-endian SNES pointer
/// `old_snes`, and rewrite it to `new_snes`. Refuses unless exactly one
/// reference exists, so a repoint never silently half-applies.
fn patch_snes_pointer(rom_bytes: &mut [u8], old_snes: u32, new_snes: u32, label: &str) -> anyhow::Result<()> {
    let old_bytes = old_snes.to_le_bytes();
    let new_bytes = new_snes.to_le_bytes();
    let matches: Vec<usize> = rom_bytes
        .windows(3)
        .enumerate()
        .filter_map(|(offset, window)| (window == &old_bytes[..3]).then_some(offset))
        .collect();
    let [offset] = matches.as_slice() else {
        anyhow::bail!(
            "{label} expected exactly one pointer to SNES ${old_snes:06X}, found {}; refusing to repoint",
            matches.len()
        )
    };
    rom_bytes[*offset..*offset + 3].copy_from_slice(&new_bytes[..3]);
    log::info!("{label} repointed from SNES ${old_snes:06X} to ${new_snes:06X}");
    Ok(())
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
        assert_eq!(
            data.translevel_at(70 % OW_WIDTH_TILES as usize as u32, 70 / OW_WIDTH_TILES as usize as u32),
            Some(1)
        );
        assert_eq!(
            data.translevel_at(130 % OW_WIDTH_TILES as usize as u32, 130 / OW_WIDTH_TILES as usize as u32),
            Some(2)
        );
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
            tile_offsets:  vec![0x0000, 0x0000, 0x0000, 0x0469, 0x044B, 0x0429, 0x0409, 0x00D3, 0x00E5],
            reveal_before: vec![
                0x6E, 0x6F, 0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x59, 0x53, 0x52, 0x83, 0x4D, 0x57, 0x5A, 0x76, 0x78,
                0x7A, 0x7B, 0x7D, 0x7F, 0x54,
            ],
            reveal_after:  vec![
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
        assert_eq!(
            operand,
            &[0x00, 0xD0, 0x7E],
            "expected the operand to currently point at OWLayer1Translevel ($7ED000)"
        );
    }

    // ── Layer 2 event table tests ────────────────────────────────────────────

    fn l2_entry(data_word: u16, dest_word: u16) -> L2EventEntry {
        L2EventEntry { data_word, dest_word }
    }

    #[test]
    fn l2_event_kind_classifies_on_0900_threshold() {
        assert_eq!(l2_entry(0x0000, 0).kind(), L2EventKind::TileStream(0x0000));
        assert_eq!(l2_entry(0x08FF, 0).kind(), L2EventKind::TileStream(0x08FF));
        assert_eq!(l2_entry(0x0900, 0).kind(), L2EventKind::TilemapCopy(0x0900));
        assert_eq!(l2_entry(0x1234, 0).kind(), L2EventKind::TilemapCopy(0x1234));
    }

    #[test]
    fn l2_event_target_tile_matches_game_decode() {
        // Mirrors the SNES decode in CODE_04E6F9:
        // x_px = ((dest & $3E) << 2); y_px = (((dest >> 3) as u8) & $F8).
        // Brute-force over all 16-bit dest words.
        for dest in (0..=0xFFFFu32).step_by(257) {
            let dest = dest as u16;
            let (col, row) = l2_entry(0, dest).target_tile();
            let x_px = (((dest & 0x3E) << 2) & 0xFF) as u8;
            let y_px = ((dest >> 3) as u8) & 0xF8;
            assert_eq!((col, row), (x_px / 8, y_px / 8), "dest_word={dest:#06X}");
        }
        // First vanilla entry: dest 0x23CC -> x_px = 0x30, y_px = 0x78.
        assert_eq!(l2_entry(0x0900, 0x23CC).target_tile(), (6, 15));
    }

    fn l2_events_with(boundaries: Vec<u16>, entries: Vec<L2EventEntry>) -> OverworldL2Events {
        OverworldL2Events { entries, boundaries, silent_events: Vec::new() }
    }

    #[test]
    fn l2_entries_for_event_uses_cumulative_boundaries() {
        let ev = l2_events_with(vec![0, 3, 3, 5], (0..5).map(|i| l2_entry(i, 0)).collect());
        assert_eq!(ev.entries_for_event(0), Some(0..3));
        assert_eq!(ev.entries_for_event(1), Some(3..3)); // empty range: no entries
        assert_eq!(ev.entries_for_event(2), Some(3..5));
        assert_eq!(ev.entries_for_event(3), None); // past OW_L2_EVENT_SLOT_COUNT? no: slot 3 of 3 slots is out of range
    }

    #[test]
    fn l2_entries_for_event_rejects_out_of_range_slots() {
        let ev = l2_events_with(vec![0, 1], vec![l2_entry(0, 0)]);
        assert_eq!(ev.entries_for_event(0), Some(0..1));
        assert_eq!(ev.entries_for_event(1), None);
        assert_eq!(ev.entries_for_event(usize::MAX), None);
    }

    #[test]
    fn l2_silent_event_filtering() {
        let ev = OverworldL2Events {
            entries:       Vec::new(),
            boundaries:    Vec::new(),
            silent_events: vec![
                SilentEvent { event_no: 0x06, is_l2: true, data_word: 0x24, dest_word: 0x215 },
                SilentEvent { event_no: 0x06, is_l2: false, data_word: 0x68, dest_word: 0x235 },
                SilentEvent { event_no: 0x14, is_l2: true, data_word: 0x10, dest_word: 0x300 },
            ],
        };
        let for_6 = ev.silent_l2_events_for(0x06);
        assert_eq!(for_6.len(), 1);
        assert_eq!(for_6[0].as_entry(), l2_entry(0x24, 0x215));
        assert_eq!(ev.silent_l2_events_for(0x14).len(), 1);
        assert!(ev.silent_l2_events_for(0xFF).is_empty());
    }

    /// Parse the real ROM and check the Layer 2 event tables against the
    /// disassembly: 371 entries, cumulative boundaries ending at 371, first
    /// entry `(0x0900, 0x23CC)`, 44 silent rows. Run with
    /// `ROM_PATH=/path/to/smw.smc cargo test -p smwe-rom --lib -- --ignored
    /// l2_event_tables_match_disassembly`.
    #[test]
    #[ignore]
    fn l2_event_tables_match_disassembly() {
        let rom_path = std::env::var("ROM_PATH").expect("set ROM_PATH");
        let raw = std::fs::read(rom_path).expect("read ROM");
        let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
        let rom = Rom::new(rom_bytes).expect("rom parse");

        let l2 = OverworldL2Events::parse(&rom).expect("L2 events parse");
        assert_eq!(l2.entry_count(), OW_L2_EVENT_ENTRY_COUNT);
        assert_eq!(l2.entries[0], l2_entry(0x0900, 0x23CC));
        assert_eq!(l2.entries[1], l2_entry(0x0904, 0x238C));
        assert_eq!(l2.entries[0].kind(), L2EventKind::TilemapCopy(0x0900));
        assert_eq!(l2.entries[0].target_tile(), (6, 15));

        assert_eq!(l2.boundaries.len(), OW_L2_EVENT_BOUNDARY_COUNT);
        assert_eq!(&l2.boundaries[..6], &[0, 0, 0x0D, 0x0D, 0x10, 0x15]);
        assert_eq!(*l2.boundaries.last().unwrap(), OW_L2_EVENT_ENTRY_COUNT as u16);
        // Event 1 (the first non-empty one) owns entries 0..13.
        assert_eq!(l2.entries_for_event(1), Some(0..0x0D));

        assert_eq!(l2.silent_events.len(), OW_SILENT_EVENT_COUNT);
        let first = &l2.silent_events[0];
        assert_eq!(first.event_no, 0x06);
        assert!(!first.is_l2); // flags[0] == 0 -> Map16 edit, not L2
        assert_eq!(first.as_entry(), l2_entry(0x68, 0x215));
        // Row 6 is the first L2 silent event (flags[6] & 1).
        let row6 = &l2.silent_events[6];
        assert!(row6.is_l2);
        assert_eq!(row6.as_entry().target_tile(), l2_entry(0, row6.dest_word).target_tile());
    }
}
