//! Lunar Magic v3.00 per-level entrance extras ("Edit menu → Change Other
//! Properties" data that has no vanilla storage).
//!
//! Covered: **face-left** on the main entrance ("added a new option for
//! entrances to have Mario face the left direction"), the **new FG/BG init
//! system** ("can set the FG relative to the player, and calculates the BG
//! position relative to the FG position, scroll settings, level height, and BG
//! height"), the **BG-relative-to-FG-only** sub-option ("meant mainly for
//! layer 2 levels"), and the **BG height** setting ("a new setting in the
//! 'Change Other Properties' dialog"). Source: official LM 3.63 `readme.txt`,
//! v3.00 (Dec 25, 2018) section.
//!
//! None of this has vanilla ROM storage — the vanilla secondary header's four
//! bytes are fully accounted for (`headers.rs`) — so the editor persists it in
//! its own RATS-tagged free-space block. A ROM nobody has authored this data
//! for simply has no block. In-game playback of every option here requires
//! Lunar Magic's ASM hacks; this editor authors and round-trips the data but
//! does not install ASM.

use std::collections::BTreeMap;

use thiserror::Error;

/// Number of levels that can carry entrance extras (matches `LEVEL_COUNT`).
pub const ENTRANCE_EXTRAS_LEVEL_COUNT: usize = 0x200;

/// Magic at the start of the RATS payload identifying the per-level entrance
/// extras block (distinct from `SMWESEX2` etc.).
pub const ENTRANCE_EXTRAS_MAGIC: &[u8; 8] = b"SMWENTR1";
const ENTRANCE_EXTRAS_VERSION: u8 = 1;

/// Bit flags for the per-level options record.
const FLAG_FACE_LEFT: u8 = 0b0000_0001;
const FLAG_NEW_FG_BG_INIT: u8 = 0b0000_0010;
const FLAG_BG_RELATIVE_TO_FG_ONLY: u8 = 0b0000_0100;

#[derive(Debug, Error)]
pub enum EntranceExtrasError {
    #[error("no entrance-extras block in this ROM")]
    NotFound,
    #[error("entrance-extras block is corrupt: {0}")]
    Corrupt(String),
    #[error("entrance-extras data too large for a RATS block ({0} bytes, max 65536)")]
    TooLarge(usize),
    #[error("no free space for {0} bytes of entrance-extras data")]
    NoFreeSpace(usize),
}

/// LM v3.00 per-level entrance extras for the **main** entrance. (Per-
/// *secondary*-entrance face-left / new-init-system options live on
/// [`crate::level::secondary_entrance::SecondaryExitOptions`] instead.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LevelEntranceExtras {
    /// LM v3.00 "Face left": Mario faces the left direction on the main
    /// entrance (also flips the "Shoot From Slanted Pipe Right" action).
    pub face_left:              bool,
    /// LM v3.00 new FG/BG init system: FG initial position is relative to
    /// the player, and the BG position is calculated from the FG position,
    /// scroll settings, level height, and [`Self::bg_height`].
    pub new_fg_bg_init:         bool,
    /// LM v3.00 sub-option: set the BG relative to the FG only (meant mainly
    /// for Layer 2 levels). Only meaningful with `new_fg_bg_init`.
    pub bg_relative_to_fg_only: bool,
    /// LM v3.00 "BG height" from the "Change Other Properties" dialog.
    /// `0` = unset (behaves like the vanilla game).
    pub bg_height:              u8,
}

/// Editor-owned per-level entrance extras. Sparse: only levels whose extras
/// differ from the default are stored.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LevelEntranceExtrasData {
    /// Sparse per-level extras, keyed by level number `0..0x200`.
    pub entries: BTreeMap<u16, LevelEntranceExtras>,
}

impl LevelEntranceExtrasData {
    /// True when nothing is stored. Such data is never written to ROM.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Extras for `level`, defaulting when the level has none stored.
    pub fn extras_for(&self, level: u16) -> LevelEntranceExtras {
        self.entries.get(&level).copied().unwrap_or_default()
    }

    /// Store `extras` for `level`, dropping the entry when it is default so
    /// untouched levels leave no trace.
    pub fn set(&mut self, level: u16, extras: LevelEntranceExtras) {
        if extras == LevelEntranceExtras::default() {
            self.entries.remove(&level);
        } else {
            self.entries.insert(level, extras);
        }
    }
}

// ── Codec ───────────────────────────────────────────────────────────────────

fn encode_extras(e: &LevelEntranceExtras, out: &mut Vec<u8>) {
    let mut flags = 0u8;
    if e.face_left {
        flags |= FLAG_FACE_LEFT;
    }
    if e.new_fg_bg_init {
        flags |= FLAG_NEW_FG_BG_INIT;
    }
    if e.bg_relative_to_fg_only {
        flags |= FLAG_BG_RELATIVE_TO_FG_ONLY;
    }
    out.push(flags);
    out.push(e.bg_height);
}

fn decode_extras(input: &[u8]) -> Result<(LevelEntranceExtras, usize), EntranceExtrasError> {
    if input.len() < 2 {
        return Err(EntranceExtrasError::Corrupt("extras record truncated".into()));
    }
    let flags = input[0];
    Ok((
        LevelEntranceExtras {
            face_left:              flags & FLAG_FACE_LEFT != 0,
            new_fg_bg_init:         flags & FLAG_NEW_FG_BG_INIT != 0,
            bg_relative_to_fg_only: flags & FLAG_BG_RELATIVE_TO_FG_ONLY != 0,
            bg_height:              input[1],
        },
        2,
    ))
}

fn encode_payload(data: &LevelEntranceExtrasData) -> Result<Vec<u8>, EntranceExtrasError> {
    let mut out = Vec::new();
    out.extend_from_slice(ENTRANCE_EXTRAS_MAGIC);
    out.push(ENTRANCE_EXTRAS_VERSION);

    let entries: Vec<(&u16, &LevelEntranceExtras)> =
        data.entries.iter().filter(|(_, e)| **e != LevelEntranceExtras::default()).collect();
    if entries.len() > u16::MAX as usize {
        return Err(EntranceExtrasError::TooLarge(entries.len()));
    }
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (level, e) in entries {
        if *level as usize >= ENTRANCE_EXTRAS_LEVEL_COUNT {
            return Err(EntranceExtrasError::Corrupt(format!("level {level:#X} out of range")));
        }
        out.extend_from_slice(&level.to_le_bytes());
        encode_extras(e, &mut out);
    }
    if out.len() > 0x10000 {
        return Err(EntranceExtrasError::TooLarge(out.len()));
    }
    Ok(out)
}

fn decode_payload(input: &[u8]) -> Result<LevelEntranceExtrasData, EntranceExtrasError> {
    let corrupt = |msg: &str| EntranceExtrasError::Corrupt(msg.to_string());
    if input.len() < 11 || &input[..8] != ENTRANCE_EXTRAS_MAGIC {
        return Err(corrupt("bad magic"));
    }
    if input[8] != ENTRANCE_EXTRAS_VERSION {
        return Err(corrupt(&format!("unsupported version {}", input[8])));
    }
    let mut pos = 9;
    let mut data = LevelEntranceExtrasData::default();

    let n = u16::from_le_bytes(input[pos..pos + 2].try_into().unwrap()) as usize;
    pos += 2;
    for _ in 0..n {
        if pos + 2 > input.len() {
            return Err(corrupt("extras level truncated"));
        }
        let level = u16::from_le_bytes([input[pos], input[pos + 1]]);
        pos += 2;
        if level as usize >= ENTRANCE_EXTRAS_LEVEL_COUNT {
            return Err(corrupt(&format!("level {level:#X} out of range")));
        }
        let (e, used) = decode_extras(&input[pos..])?;
        pos += used;
        if e != LevelEntranceExtras::default() {
            data.entries.insert(level, e);
        }
    }
    Ok(data)
}

/// Scan `rom_bytes` (raw file bytes, SMC header included if present) for the
/// entrance-extras RATS block. Returns the file offset of the `STAR` tag.
fn find_block(rom_bytes: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while i + 16 < rom_bytes.len() {
        if &rom_bytes[i..i + 4] == b"STAR" {
            let size = u16::from_le_bytes([rom_bytes[i + 4], rom_bytes[i + 5]]) as usize;
            let inv = u16::from_le_bytes([rom_bytes[i + 6], rom_bytes[i + 7]]);
            if size as u16 ^ inv == 0xFFFF {
                let data_start = i + 8;
                let payload_end = data_start.saturating_add(size).saturating_add(1);
                if payload_end <= rom_bytes.len()
                    && data_start + 11 <= rom_bytes.len()
                    && &rom_bytes[data_start..data_start + 8] == ENTRANCE_EXTRAS_MAGIC
                    && decode_payload(&rom_bytes[data_start..payload_end]).is_ok()
                {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

impl LevelEntranceExtrasData {
    /// Parse the entrance-extras block from raw ROM bytes. Returns
    /// [`EntranceExtrasError::NotFound`] when no block exists yet (a fresh ROM).
    pub fn parse(rom_bytes: &[u8]) -> Result<Self, EntranceExtrasError> {
        let tag = find_block(rom_bytes).ok_or(EntranceExtrasError::NotFound)?;
        let size = u16::from_le_bytes([rom_bytes[tag + 4], rom_bytes[tag + 5]]) as usize;
        let end = tag.saturating_add(8).saturating_add(size).saturating_add(1);
        let payload = rom_bytes
            .get(tag + 8..end)
            .ok_or_else(|| EntranceExtrasError::Corrupt("entrance-extras block overruns ROM".into()))?;
        decode_payload(payload)
    }

    /// Write the data to ROM: erase any existing block (fill with `0xFF` so
    /// it reads as free space again), allocate fresh free space, and write a
    /// new RATS-tagged block. Empty data erases the block without writing a
    /// new one, so untouched ROMs stay byte-identical.
    pub fn write_to_rom(&self, rom_bytes: &mut [u8], header_offset: usize) -> Result<(), EntranceExtrasError> {
        // Erase any existing block first.
        if let Some(tag) = find_block(rom_bytes) {
            let size = u16::from_le_bytes([rom_bytes[tag + 4], rom_bytes[tag + 5]]) as usize;
            let end = (tag + 8 + size + 1).min(rom_bytes.len());
            rom_bytes[tag..end].fill(0xFF);
        }

        // Nothing to store: leave the ROM without a block.
        if self.is_empty() {
            return Ok(());
        }
        let payload = encode_payload(self)?;

        let total = 8 + payload.len(); // RATS tag + payload
        let pc = crate::freespace::find_free_space(rom_bytes, total, 0x008000, header_offset)
            .ok_or(EntranceExtrasError::NoFreeSpace(total))?;
        let file_off = pc + header_offset;
        if payload.len() > 0x10000 {
            return Err(EntranceExtrasError::TooLarge(payload.len()));
        }
        let size_field = (payload.len() - 1) as u16;
        rom_bytes[file_off..file_off + 4].copy_from_slice(b"STAR");
        rom_bytes[file_off + 4..file_off + 6].copy_from_slice(&size_field.to_le_bytes());
        rom_bytes[file_off + 6..file_off + 8].copy_from_slice(&(!size_field).to_le_bytes());
        rom_bytes[file_off + 8..file_off + 8 + payload.len()].copy_from_slice(&payload);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snes_utils::rom::Rom;

    fn sample_data() -> LevelEntranceExtrasData {
        let mut data = LevelEntranceExtrasData::default();
        data.set(0x105, LevelEntranceExtras {
            face_left:              true,
            new_fg_bg_init:         true,
            bg_relative_to_fg_only: false,
            bg_height:              0x40,
        });
        data.set(0x001, LevelEntranceExtras {
            face_left:              false,
            new_fg_bg_init:         false,
            bg_relative_to_fg_only: true,
            bg_height:              0xFF,
        });
        data
    }

    #[test]
    fn payload_round_trip() {
        let data = sample_data();
        let payload = encode_payload(&data).expect("encode");
        let back = decode_payload(&payload).expect("decode");
        assert_eq!(back, data);
    }

    #[test]
    fn empty_data_encodes_minimally() {
        let data = LevelEntranceExtrasData::default();
        assert!(data.is_empty());
        let payload = encode_payload(&data).expect("encode");
        let back = decode_payload(&payload).expect("decode");
        assert_eq!(back, data);
    }

    #[test]
    fn default_extras_are_not_stored() {
        let mut data = LevelEntranceExtrasData::default();
        data.set(0x10, LevelEntranceExtras::default());
        assert!(data.is_empty(), "default extras must be dropped by set()");
        let payload = encode_payload(&data).expect("encode");
        let back = decode_payload(&payload).expect("decode");
        assert_eq!(back, data);
    }

    #[test]
    fn out_of_range_level_rejected() {
        let mut data = LevelEntranceExtrasData::default();
        data.entries.insert(0x200, LevelEntranceExtras { face_left: true, ..Default::default() });
        assert!(matches!(encode_payload(&data), Err(EntranceExtrasError::Corrupt(_))));
    }

    #[test]
    fn corrupt_payloads_rejected() {
        let payload = encode_payload(&sample_data()).expect("encode");
        // Truncated.
        assert!(decode_payload(&payload[..payload.len() / 2]).is_err());
        // Bad magic.
        let mut bad = payload.clone();
        bad[0] = b'X';
        assert!(matches!(decode_payload(&bad), Err(EntranceExtrasError::Corrupt(_))));
        // Bad version.
        let mut bad = payload.clone();
        bad[8] = 0x7F;
        assert!(matches!(decode_payload(&bad), Err(EntranceExtrasError::Corrupt(_))));
    }

    #[test]
    fn write_parse_round_trip_in_scratch_rom() {
        // A scratch "ROM": free space everywhere, like erased flash.
        let mut rom = vec![0xFFu8; 0x40000];
        let data = sample_data();
        data.write_to_rom(&mut rom, 0).expect("write");
        let back = LevelEntranceExtrasData::parse(&rom).expect("parse");
        assert_eq!(back, data);
    }

    #[test]
    fn empty_write_leaves_no_block() {
        let mut rom = vec![0xFFu8; 0x40000];
        LevelEntranceExtrasData::default().write_to_rom(&mut rom, 0).expect("write");
        assert!(matches!(LevelEntranceExtrasData::parse(&rom), Err(EntranceExtrasError::NotFound)));
        assert!(rom.iter().all(|&b| b == 0xFF), "empty write must not touch the ROM");
    }

    #[test]
    fn rewrite_erases_old_block() {
        let mut rom = vec![0xFFu8; 0x40000];
        let data = sample_data();
        data.write_to_rom(&mut rom, 0).expect("write 1");
        let tag1 = find_block(&rom).expect("block after write 1");
        LevelEntranceExtrasData::default().write_to_rom(&mut rom, 0).expect("erase");
        assert!(matches!(LevelEntranceExtrasData::parse(&rom), Err(EntranceExtrasError::NotFound)));
        // Old block region reads as free space again.
        assert!(rom[tag1..tag1 + 8].iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn ignores_unrelated_rats_blocks() {
        let mut rom = vec![0xFFu8; 0x40000];
        // Some other tool's RATS block.
        rom[0x1000..0x1004].copy_from_slice(b"STAR");
        rom[0x1004..0x1006].copy_from_slice(&7u16.to_le_bytes());
        rom[0x1006..0x1008].copy_from_slice(&(!7u16).to_le_bytes());
        rom[0x1008..0x1010].copy_from_slice(b"NOTOURS!");
        assert!(matches!(LevelEntranceExtrasData::parse(&rom), Err(EntranceExtrasError::NotFound)));
        // Our block coexists after it.
        sample_data().write_to_rom(&mut rom, 0).expect("write");
        assert!(LevelEntranceExtrasData::parse(&rom).is_ok());
    }

    /// Real-ROM test: a vanilla SMW ROM has no entrance-extras block, and its
    /// secondary header still parses (entrance action + FG/BG init intact).
    #[test]
    #[ignore]
    fn real_rom_has_no_ext_block_but_parses_secondary_header() {
        let path = std::env::var("ROM_PATH").expect("ROM_PATH must point at a real SMW ROM for ignored tests");
        let raw = std::fs::read(path).expect("cannot read ROM");
        assert!(matches!(LevelEntranceExtrasData::parse(&raw), Err(EntranceExtrasError::NotFound)));

        let rom = Rom::new(raw).expect("Rom::new");
        let h = crate::level::headers::SecondaryHeader::read_from_rom(&rom, 0x105).expect("read header");
        // Level 0x105 (Yoshi's Island 2) vanilla values — sanity that the
        // header parse the UI shows next to the new extras is intact.
        let _ = (h.main_entrance_mario_action(), h.fg_initial_pos(), h.bg_initial_pos());
    }

    /// Real-ROM test: write extras into an in-memory copy of the real ROM
    /// (never the file itself), re-parse, and confirm the secondary header
    /// tables are untouched.
    #[test]
    #[ignore]
    fn real_rom_ext_block_round_trip_on_copy() {
        let path = std::env::var("ROM_PATH").expect("ROM_PATH must point at a real SMW ROM for ignored tests");
        let raw = std::fs::read(path).expect("cannot read ROM");
        let header_offset = if raw.len() % 0x400 == 0x200 { 0x200 } else { 0 };
        let mut rom = raw.clone();

        let rom_r = Rom::new(raw).expect("Rom::new");
        let before = crate::level::headers::SecondaryHeader::read_from_rom(&rom_r, 0x105).expect("read").bytes;

        let data = sample_data();
        data.write_to_rom(&mut rom, header_offset).expect("write ext block");
        let back = LevelEntranceExtrasData::parse(&rom).expect("parse ext block");
        assert_eq!(back, data);

        // Secondary header table bytes untouched by the block write.
        let rom2 = Rom::new(rom).expect("Rom::new");
        let after = crate::level::headers::SecondaryHeader::read_from_rom(&rom2, 0x105).expect("read").bytes;
        assert_eq!(before, after, "secondary header changed by ext block write");
    }
}
