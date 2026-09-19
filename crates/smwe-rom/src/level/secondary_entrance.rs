use std::collections::BTreeMap;

use thiserror::Error;

use crate::{
    snes_utils::{addr::AddrSnes, rom::Rom, rom_slice::SnesSlice},
    RomError,
};

pub const SECONDARY_ENTRANCE_TABLE: SnesSlice = SnesSlice::new(AddrSnes(0x05F800), 512);

/// Vanilla secondary-exit table capacity: 0x200 entries × 4 bytes, stored as
/// four 512-byte byte-lanes at SNES `$05:F800`/`$05:FA00`/`$05:FC00`/`$05:FE00`
/// (confirmed against SMWDisX `bank_05.asm` `CODE_05D796` + `DATA_05F800`;
/// the `UseSecondaryExit` branch reads exactly these four lanes).
pub const SECONDARY_ENTRANCE_COUNT_VANILLA: usize = 0x200;
/// Lunar Magic v2.50 expansion capacity (ASM hack; "expanded the number of
/// secondary exits in the game to 0x2000, up from 0x200"). The editor stores
/// entries at indices `0x200..0x2000` in its own RATS block; in-game playback
/// of those entries requires LM's expansion ASM, which this editor does not
/// install.
pub const SECONDARY_ENTRANCE_COUNT_MAX: usize = 0x2000;
/// Lunar Magic v3.00 Star/Pipe teleport table capacity ("Star/Pipe table to
/// 0x100 entries"). No vanilla table exists for this (nothing matching in
/// SMWDisX); it is LM's table of overworld destinations for secondary exits
/// that exit to the overworld.
pub const OW_TELEPORT_TABLE_LEN: usize = 0x100;

#[derive(Debug)]
pub struct SecondaryEntrance([u8; 4]);

impl SecondaryEntrance {
    pub fn read_from_rom(rom: &Rom, entrance_id: usize) -> Result<Self, RomError> {
        let mut bytes = [0; 4];
        for (i, byte) in bytes.iter_mut().enumerate() {
            let slice = SECONDARY_ENTRANCE_TABLE.skip_forward(i);
            *byte = rom.slice_lorom(slice)?[entrance_id];
        }

        Ok(Self(bytes))
    }

    pub fn destination_level(&self) -> u16 {
        // dddddddd -------- -------- ----D---
        // destination_level = Ddddddddd
        let hi = (self.0[3] as u16 & 0b1000) << 5;
        let lo = self.0[0] as u16;
        hi | lo
    }

    pub fn bg_initial_pos(&self) -> u8 {
        // -------- bb------ -------- --------
        // by_initial_pos = bb
        self.0[1] >> 6
    }

    pub fn fg_initial_pos(&self) -> u8 {
        // -------- --ff---- -------- --------
        // fg_initial_pos = ff
        self.0[1] >> 4
    }

    pub fn entrance_xy_pos(&self) -> (u8, u8) {
        // -------- ----yyyy xxx----- --------
        // entrance_xy_pos = (xxx, yyyy)
        let x = self.0[2] >> 5;
        let y = self.0[1] & 0b1111;
        (x, y)
    }

    pub fn screen_number(&self) -> u8 {
        // -------- -------- ---SSSSS --------
        // screen_number = SSSSS
        self.0[2] & 0b11111
    }

    pub fn bytes(&self) -> [u8; 4] {
        self.0
    }
}

// ── Lunar Magic v3.00 "Modify Secondary Entrances" extended options ──────────
// The vanilla 4-byte entry has no storage for these (every bit is accounted
// for above, confirmed against SMWDisX `CODE_05D796`), so the editor persists
// them in its own RATS-tagged free-space block (`SecondaryExitExtData`).
// In-game playback of every option below requires LM's ASM hacks; this editor
// authors and round-trips the data but does not install ASM.

/// Normal vs. secret exit when a secondary exit sends the player to the
/// overworld (LM v3.00 "exit to the overworld" option).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OwExitKind {
    #[default]
    Normal,
    Secret,
}

/// Which player the game switches to on an exit-to-overworld (LM v3.00
/// "player switch" choice).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OwPlayerSwitch {
    /// Leave the current player alone.
    #[default]
    Keep,
    Mario,
    Luigi,
}

/// LM v3.00 exit-to-overworld settings for one secondary entrance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct OverworldExit {
    pub exit_kind:  OwExitKind,
    pub player:     OwPlayerSwitch,
    /// Base overworld event used when exiting (LM v3.00 "base event").
    pub base_event: u8,
    /// Index into the Star/Pipe teleport table (`OwTeleportEntry`).
    pub teleport:   u8,
}

/// LM v3.00 per-secondary-entrance options from the "Modify Secondary
/// Entrances" dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SecondaryExitOptions {
    /// LM v3.00 "make this a water level": the destination level plays as a
    /// water level when entered through this exit.
    pub water_level:       bool,
    /// LM v3.00 "exit to the overworld": the exit leaves the level instead of
    /// entering another one.
    pub exit_to_overworld: Option<OverworldExit>,
    /// LM v3.00 "midway entrance redirect": use another level's midway
    /// entrance instead of this exit's own destination.
    pub midway_redirect:   Option<u16>,
    /// LM v3.00 "Face left": Mario faces the left direction when entering
    /// through this exit (also flips the "Shoot From Slanted Pipe Right"
    /// entrance action).
    pub face_left:         bool,
    /// LM v3.00 new FG/BG init system for this entrance: FG initial position
    /// is relative to the player, and the BG position is calculated from the
    /// FG position, scroll settings, level height, and the destination
    /// level's BG height (see
    /// [`crate::level::entrance_extras::LevelEntranceExtras`]).
    pub new_fg_bg_init:    bool,
}

/// One LM v3.00 Star/Pipe teleport-table entry: where on the overworld the
/// player appears when a secondary exit sends them there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct OwTeleportEntry {
    /// Overworld submap (0 = Main Map .. 6 = Star World).
    pub submap:   u8,
    /// Overworld tile X (0..64).
    pub x:        u8,
    /// Overworld tile Y (0..32).
    pub y:        u8,
    /// Reserved for alignment; must be zero.
    pub reserved: u8,
}

// ── RATS-backed storage ─────────────────────────────────────────────────────

/// Magic at the start of the RATS payload identifying the secondary-exit
/// extended-data block (distinct from the `EXANIM_MAGIC` block etc.).
pub const SECEXIT_MAGIC: &[u8; 8] = b"SMWESEX2";
/// v2 adds `SecondaryExitOptions::{face_left, new_fg_bg_init}` (LM v3.00
/// entrance extras); v1 payloads still decode with those fields defaulted.
const SECEXIT_VERSION: u8 = 2;

/// Bit flags for the per-entry options record.
const FLAG_WATER: u8 = 0b0000_0001;
const FLAG_EXIT_OW: u8 = 0b0000_0010;
const FLAG_MIDWAY_REDIRECT: u8 = 0b0000_0100;
const FLAG_FACE_LEFT: u8 = 0b0000_1000;
const FLAG_NEW_FG_BG_INIT: u8 = 0b0001_0000;

#[derive(Debug, Error)]
pub enum SecExitExtError {
    #[error("no secondary-exit extended-data block in this ROM")]
    NotFound,
    #[error("secondary-exit extended-data block is corrupt: {0}")]
    Corrupt(String),
    #[error("secondary-exit extended data too large for a RATS block ({0} bytes, max 65536)")]
    TooLarge(usize),
    #[error("no free space for {0} bytes of secondary-exit extended data")]
    NoFreeSpace(usize),
}

/// Editor-owned secondary-exit data with no vanilla storage: per-entrance LM
/// v3.00 options, vanilla-format entries at indices `0x200..0x2000` (LM v2.50
/// table expansion), and the 0x100-entry Star/Pipe overworld teleport table
/// (LM v3.00). Stored as one RATS-tagged free-space block; a ROM nobody has
/// authored this data for simply has no block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecondaryExitExtData {
    /// Sparse per-entrance options, keyed by entrance index `0..0x2000`.
    pub options:          BTreeMap<u16, SecondaryExitOptions>,
    /// Vanilla-format 4-byte entries at indices `0x200..0x2000`.
    pub extended_entries: BTreeMap<u16, [u8; 4]>,
    /// Star/Pipe overworld teleport table (0x100 entries).
    pub teleport_table:   [OwTeleportEntry; OW_TELEPORT_TABLE_LEN],
}

impl Default for SecondaryExitExtData {
    fn default() -> Self {
        Self {
            options:          BTreeMap::new(),
            extended_entries: BTreeMap::new(),
            teleport_table:   [OwTeleportEntry::default(); OW_TELEPORT_TABLE_LEN],
        }
    }
}

impl SecondaryExitExtData {
    /// True when nothing is stored: no options, no extended entries, and a
    /// default (all-zero) teleport table. Such data is never written to ROM.
    pub fn is_empty(&self) -> bool {
        self.options.is_empty()
            && self.extended_entries.is_empty()
            && self.teleport_table.iter().all(|e| *e == OwTeleportEntry::default())
    }

    /// Options for `index`, defaulting when the entrance has none stored.
    pub fn options_for(&self, index: u16) -> SecondaryExitOptions {
        self.options.get(&index).copied().unwrap_or_default()
    }

    /// Vanilla-format bytes for `index`: `0x200..0x2000` come from the stored
    /// extended entries; anything else is `None` (indices below 0x200 live in
    /// the vanilla table, handled by `SecondaryEntrance::read_from_rom`).
    pub fn extended_entry(&self, index: u16) -> Option<[u8; 4]> {
        self.extended_entries.get(&index).copied()
    }
}

// ── Codec ───────────────────────────────────────────────────────────────────

fn encode_options(opts: &SecondaryExitOptions, out: &mut Vec<u8>) {
    let mut flags = 0u8;
    if opts.water_level {
        flags |= FLAG_WATER;
    }
    let ow = opts.exit_to_overworld.unwrap_or_default();
    if opts.exit_to_overworld.is_some() {
        flags |= FLAG_EXIT_OW;
    }
    if opts.midway_redirect.is_some() {
        flags |= FLAG_MIDWAY_REDIRECT;
    }
    if opts.face_left {
        flags |= FLAG_FACE_LEFT;
    }
    if opts.new_fg_bg_init {
        flags |= FLAG_NEW_FG_BG_INIT;
    }
    out.push(flags);
    out.push(match ow.exit_kind {
        OwExitKind::Normal => 0,
        OwExitKind::Secret => 1,
    });
    out.push(match ow.player {
        OwPlayerSwitch::Keep => 0,
        OwPlayerSwitch::Mario => 1,
        OwPlayerSwitch::Luigi => 2,
    });
    out.push(ow.base_event);
    out.push(ow.teleport);
    out.extend_from_slice(&opts.midway_redirect.unwrap_or(0).to_le_bytes());
}

fn decode_options(input: &[u8]) -> Result<(SecondaryExitOptions, usize), SecExitExtError> {
    if input.len() < 7 {
        return Err(SecExitExtError::Corrupt("options record truncated".into()));
    }
    let flags = input[0];
    let exit_kind = match input[1] {
        0 => OwExitKind::Normal,
        1 => OwExitKind::Secret,
        v => return Err(SecExitExtError::Corrupt(format!("bad exit kind {v}"))),
    };
    let player = match input[2] {
        0 => OwPlayerSwitch::Keep,
        1 => OwPlayerSwitch::Mario,
        2 => OwPlayerSwitch::Luigi,
        v => return Err(SecExitExtError::Corrupt(format!("bad player switch {v}"))),
    };
    Ok((
        SecondaryExitOptions {
            water_level:       flags & FLAG_WATER != 0,
            exit_to_overworld: (flags & FLAG_EXIT_OW != 0).then_some(OverworldExit {
                exit_kind,
                player,
                base_event: input[3],
                teleport: input[4],
            }),
            midway_redirect:   (flags & FLAG_MIDWAY_REDIRECT != 0).then_some(u16::from_le_bytes([input[5], input[6]])),
            face_left:         flags & FLAG_FACE_LEFT != 0,
            new_fg_bg_init:    flags & FLAG_NEW_FG_BG_INIT != 0,
        },
        7,
    ))
}

fn encode_payload(data: &SecondaryExitExtData) -> Result<Vec<u8>, SecExitExtError> {
    let mut out = Vec::new();
    out.extend_from_slice(SECEXIT_MAGIC);
    out.push(SECEXIT_VERSION);

    // Section 1: sparse per-entrance options.
    let opts: Vec<(&u16, &SecondaryExitOptions)> =
        data.options.iter().filter(|(_, o)| **o != SecondaryExitOptions::default()).collect();
    if opts.len() > u16::MAX as usize {
        return Err(SecExitExtError::TooLarge(opts.len()));
    }
    out.extend_from_slice(&(opts.len() as u16).to_le_bytes());
    for (index, o) in opts {
        if *index as usize >= SECONDARY_ENTRANCE_COUNT_MAX {
            return Err(SecExitExtError::Corrupt(format!("entrance index {index:#X} out of range")));
        }
        out.extend_from_slice(&index.to_le_bytes());
        encode_options(o, &mut out);
    }

    // Section 2: sparse extended vanilla entries (0x200..0x2000).
    let ext: Vec<(&u16, &[u8; 4])> =
        data.extended_entries.iter().filter(|(i, _)| **i as usize >= SECONDARY_ENTRANCE_COUNT_VANILLA).collect();
    out.extend_from_slice(&(ext.len() as u16).to_le_bytes());
    for (index, bytes) in ext {
        if *index as usize >= SECONDARY_ENTRANCE_COUNT_MAX {
            return Err(SecExitExtError::Corrupt(format!("entrance index {index:#X} out of range")));
        }
        out.extend_from_slice(&index.to_le_bytes());
        out.extend_from_slice(bytes);
    }

    // Section 3: teleport table (only when non-default).
    let table_default = data.teleport_table.iter().all(|e| *e == OwTeleportEntry::default());
    out.push(u8::from(!table_default));
    if !table_default {
        for e in &data.teleport_table {
            out.extend_from_slice(&[e.submap, e.x, e.y, e.reserved]);
        }
    }
    if out.len() > 0x10000 {
        return Err(SecExitExtError::TooLarge(out.len()));
    }
    Ok(out)
}

fn decode_payload(input: &[u8]) -> Result<SecondaryExitExtData, SecExitExtError> {
    let corrupt = |msg: &str| SecExitExtError::Corrupt(msg.to_string());
    if input.len() < 11 || &input[..8] != SECEXIT_MAGIC {
        return Err(corrupt("bad magic"));
    }
    // v1 payloads decode with the v2 fields defaulted (see
    // `SECEXIT_VERSION`); reject anything else.
    if input[8] != 1 && input[8] != SECEXIT_VERSION {
        return Err(corrupt(&format!("unsupported version {}", input[8])));
    }
    let mut pos = 9;
    let mut data = SecondaryExitExtData::default();

    // Section 1: options.
    let n_opts = u16::from_le_bytes(input[pos..pos + 2].try_into().unwrap()) as usize;
    pos += 2;
    for _ in 0..n_opts {
        if pos + 2 > input.len() {
            return Err(corrupt("options index truncated"));
        }
        let index = u16::from_le_bytes([input[pos], input[pos + 1]]);
        pos += 2;
        if index as usize >= SECONDARY_ENTRANCE_COUNT_MAX {
            return Err(corrupt(&format!("entrance index {index:#X} out of range")));
        }
        let (opts, used) = decode_options(&input[pos..])?;
        pos += used;
        if opts != SecondaryExitOptions::default() {
            data.options.insert(index, opts);
        }
    }

    // Section 2: extended entries.
    if pos + 2 > input.len() {
        return Err(corrupt("extended-entry count truncated"));
    }
    let n_ext = u16::from_le_bytes([input[pos], input[pos + 1]]) as usize;
    pos += 2;
    for _ in 0..n_ext {
        if pos + 6 > input.len() {
            return Err(corrupt("extended entry truncated"));
        }
        let index = u16::from_le_bytes([input[pos], input[pos + 1]]);
        if index as usize >= SECONDARY_ENTRANCE_COUNT_MAX || (index as usize) < SECONDARY_ENTRANCE_COUNT_VANILLA {
            return Err(corrupt(&format!("extended entry index {index:#X} out of range")));
        }
        let bytes: [u8; 4] = input[pos + 2..pos + 6].try_into().unwrap();
        pos += 6;
        data.extended_entries.insert(index, bytes);
    }

    // Section 3: teleport table.
    if pos + 1 > input.len() {
        return Err(corrupt("teleport-table flag truncated"));
    }
    let present = input[pos] != 0;
    pos += 1;
    if present {
        if pos + OW_TELEPORT_TABLE_LEN * 4 > input.len() {
            return Err(corrupt("teleport table truncated"));
        }
        for (i, e) in data.teleport_table.iter_mut().enumerate() {
            let b = &input[pos + i * 4..pos + i * 4 + 4];
            *e = OwTeleportEntry { submap: b[0], x: b[1], y: b[2], reserved: b[3] };
        }
    }
    Ok(data)
}

/// Scan `rom_bytes` (raw file bytes, SMC header included if present) for the
/// secondary-exit extended-data RATS block. Returns the file offset of the
/// `STAR` tag.
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
                    && &rom_bytes[data_start..data_start + 8] == SECEXIT_MAGIC
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

impl SecondaryExitExtData {
    /// Parse the extended-data block from raw ROM bytes. Returns
    /// [`SecExitExtError::NotFound`] when no block exists yet (a fresh ROM).
    pub fn parse(rom_bytes: &[u8]) -> Result<Self, SecExitExtError> {
        let tag = find_block(rom_bytes).ok_or(SecExitExtError::NotFound)?;
        let size = u16::from_le_bytes([rom_bytes[tag + 4], rom_bytes[tag + 5]]) as usize;
        let end = tag.saturating_add(8).saturating_add(size).saturating_add(1);
        let payload = rom_bytes
            .get(tag + 8..end)
            .ok_or_else(|| SecExitExtError::Corrupt("extended-data block overruns ROM".into()))?;
        decode_payload(payload)
    }

    /// Write the data to ROM: erase any existing block (fill with `0xFF` so
    /// it reads as free space again), allocate fresh free space, and write a
    /// new RATS-tagged block. Empty data erases the block without writing a
    /// new one, so untouched ROMs stay byte-identical.
    pub fn write_to_rom(&self, rom_bytes: &mut [u8], header_offset: usize) -> Result<(), SecExitExtError> {
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
            .ok_or(SecExitExtError::NoFreeSpace(total))?;
        let file_off = pc + header_offset;
        if payload.len() > 0x10000 {
            return Err(SecExitExtError::TooLarge(payload.len()));
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

    fn sample_data() -> SecondaryExitExtData {
        let mut data = SecondaryExitExtData::default();
        data.options.insert(0x001, SecondaryExitOptions {
            water_level: true,
            exit_to_overworld: Some(OverworldExit {
                exit_kind:  OwExitKind::Secret,
                player:     OwPlayerSwitch::Luigi,
                base_event: 0x2A,
                teleport:   0x07,
            }),
            midway_redirect: Some(0x105),
            ..Default::default()
        });
        data.options.insert(0x1FF, SecondaryExitOptions { water_level: true, ..Default::default() });
        data.options.insert(0x002, SecondaryExitOptions {
            face_left: true,
            new_fg_bg_init: true,
            ..Default::default()
        });
        // Index 0x2000 (out of range) must be rejected by the encoder.
        data.extended_entries.insert(0x200, [0x10, 0x20, 0x30, 0x08]);
        data.extended_entries.insert(0x1FFF, [0x00, 0x00, 0x00, 0x00]);
        data.teleport_table[0x07] = OwTeleportEntry { submap: 3, x: 12, y: 20, reserved: 0 };
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
        let data = SecondaryExitExtData::default();
        assert!(data.is_empty());
        let payload = encode_payload(&data).expect("encode");
        let back = decode_payload(&payload).expect("decode");
        assert_eq!(back, data);
    }

    #[test]
    fn default_options_are_not_stored() {
        let mut data = SecondaryExitExtData::default();
        data.options.insert(0x10, SecondaryExitOptions::default());
        let payload = encode_payload(&data).expect("encode");
        let back = decode_payload(&payload).expect("decode");
        assert!(back.options.is_empty(), "default options must be dropped by the codec");
    }

    #[test]
    fn out_of_range_index_rejected() {
        let mut data = SecondaryExitExtData::default();
        data.options.insert(0x2000, SecondaryExitOptions { water_level: true, ..Default::default() });
        assert!(matches!(encode_payload(&data), Err(SecExitExtError::Corrupt(_))));
    }

    #[test]
    fn v1_payload_decodes_with_new_fields_defaulted() {
        // Hand-built v1 payload: magic + version 1 + one options record with
        // only the v1 flags (water) set, no extended entries, no teleport
        // table.
        let mut payload = Vec::new();
        payload.extend_from_slice(SECEXIT_MAGIC);
        payload.push(1);
        payload.extend_from_slice(&1u16.to_le_bytes()); // one options record
        payload.extend_from_slice(&0x10u16.to_le_bytes());
        payload.push(0b0000_0001); // FLAG_WATER only
        payload.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // kind/player/event/teleport/redirect
        payload.extend_from_slice(&0u16.to_le_bytes()); // no extended entries
        payload.push(0); // no teleport table
        let back = decode_payload(&payload).expect("v1 decode");
        let opts = back.options_for(0x10);
        assert!(opts.water_level);
        assert!(!opts.face_left, "v1 payload must default face_left off");
        assert!(!opts.new_fg_bg_init, "v1 payload must default new_fg_bg_init off");
    }

    #[test]
    fn corrupt_payloads_rejected() {
        let payload = encode_payload(&sample_data()).expect("encode");
        // Truncated.
        assert!(decode_payload(&payload[..payload.len() / 2]).is_err());
        // Bad magic.
        let mut bad = payload.clone();
        bad[0] = b'X';
        assert!(matches!(decode_payload(&bad), Err(SecExitExtError::Corrupt(_))));
        // Bad version.
        let mut bad = payload.clone();
        bad[8] = 0x7F;
        assert!(matches!(decode_payload(&bad), Err(SecExitExtError::Corrupt(_))));
    }

    #[test]
    fn write_parse_round_trip_in_scratch_rom() {
        // A scratch "ROM": free space everywhere, like erased flash.
        let mut rom = vec![0xFFu8; 0x40000];
        let data = sample_data();
        data.write_to_rom(&mut rom, 0).expect("write");
        let back = SecondaryExitExtData::parse(&rom).expect("parse");
        assert_eq!(back, data);
    }

    #[test]
    fn empty_write_leaves_no_block() {
        let mut rom = vec![0xFFu8; 0x40000];
        SecondaryExitExtData::default().write_to_rom(&mut rom, 0).expect("write");
        assert!(matches!(SecondaryExitExtData::parse(&rom), Err(SecExitExtError::NotFound)));
        assert!(rom.iter().all(|&b| b == 0xFF), "empty write must not touch the ROM");
    }

    #[test]
    fn rewrite_erases_old_block() {
        let mut rom = vec![0xFFu8; 0x40000];
        let data = sample_data();
        data.write_to_rom(&mut rom, 0).expect("write 1");
        let tag1 = find_block(&rom).expect("block after write 1");
        SecondaryExitExtData::default().write_to_rom(&mut rom, 0).expect("erase");
        assert!(matches!(SecondaryExitExtData::parse(&rom), Err(SecExitExtError::NotFound)));
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
        assert!(matches!(SecondaryExitExtData::parse(&rom), Err(SecExitExtError::NotFound)));
        // Our block coexists after it.
        sample_data().write_to_rom(&mut rom, 0).expect("write");
        assert!(SecondaryExitExtData::parse(&rom).is_ok());
    }

    /// Real-ROM test: a vanilla SMW ROM has no extended-data block, and the
    /// vanilla secondary-entrance table still parses to 0x200 entries.
    #[test]
    #[ignore]
    fn real_rom_has_no_ext_block_but_parses_vanilla_table() {
        let path = std::env::var("ROM_PATH").expect("ROM_PATH must point at a real SMW ROM for ignored tests");
        let raw = std::fs::read(path).expect("cannot read ROM");
        assert!(matches!(SecondaryExitExtData::parse(&raw), Err(SecExitExtError::NotFound)));

        let rom = Rom::new(raw).expect("Rom::new");
        let mut count = 0;
        for i in 0..SECONDARY_ENTRANCE_COUNT_VANILLA {
            let se = SecondaryEntrance::read_from_rom(&rom, i).expect("read entrance");
            let _ = se.bytes();
            count += 1;
        }
        assert_eq!(count, SECONDARY_ENTRANCE_COUNT_VANILLA);
    }

    /// Real-ROM test: write extended data into an in-memory copy of the real
    /// ROM (never the file itself), re-parse, and confirm the vanilla table
    /// is untouched.
    #[test]
    #[ignore]
    fn real_rom_ext_block_round_trip_on_copy() {
        let path = std::env::var("ROM_PATH").expect("ROM_PATH must point at a real SMW ROM for ignored tests");
        let raw = std::fs::read(path).expect("cannot read ROM");
        let header_offset = if raw.len() % 0x400 == 0x200 { 0x200 } else { 0 };
        let mut rom = raw.clone();

        // Snapshot the vanilla table bytes.
        let rom_r = Rom::new(raw).expect("Rom::new");
        let vanilla: Vec<[u8; 4]> = (0..SECONDARY_ENTRANCE_COUNT_VANILLA)
            .map(|i| SecondaryEntrance::read_from_rom(&rom_r, i).expect("read").bytes())
            .collect();

        let data = sample_data();
        data.write_to_rom(&mut rom, header_offset).expect("write ext block");
        let back = SecondaryExitExtData::parse(&rom).expect("parse ext block");
        assert_eq!(back, data);

        // Vanilla table untouched by the block write.
        let rom2 = Rom::new(rom).expect("Rom::new");
        for (i, want) in vanilla.iter().enumerate() {
            let se = SecondaryEntrance::read_from_rom(&rom2, i).expect("read");
            assert_eq!(&se.bytes(), want, "vanilla entry {i:#X} changed");
        }
    }
}
