//! Boss sequence text ("Edit Boss Sequence Text" in Lunar Magic's overworld
//! editor, v1.50; clear-text / clear-all buttons v3.20).
//!
//! After defeating each of the 7 Koopalings, SMW plays a "boss sequence"
//! cutscene (bank $0C, `CODE_0CC97E`). The text shown during these cutscenes
//! ("Mario  has  defeated the / demented  Iggy Koopa / ...") is stored as
//! **pre-composed Layer 3 stripe image data** — not as text bytes like the
//! message boxes. Each message is a `$FF`-terminated blob of stripe commands:
//!
//! ```text
//! [VRAM-hi][VRAM-lo][flags-hi][flags-lo][tile-lo][tile-hi]... [$FF]
//! ```
//!
//! (VRAM dest and flags/len are big-endian words; tile words are
//! little-endian. Flags/len bit 15 = direction, bit 14 = RLE, low 14 bits =
//! payload byte count − 1 — the same format `smwe_emu::parse_stripe_commands`
//! understands.)
//!
//! Tile words are `$39TT`: palette 6, priority 1, no flip — the same VRAM
//! window the message boxes use. The character for a tile is
//! `FontMap::real()` looked up at `TT = tile & 0x7F` (the message-box font,
//! GFX2A "Message Box Letters"). Three additions for boss text, verified
//! against the real ROM:
//! - `0x5A` → `'#'` (the "castle #N" number sign; GFX2A tile $5A renders `#`,
//!   not `z`, in this context)
//! - `0x63-0x6C` → `'0'-'9'` (castle numbers: Iggy's "castle #1" uses `$64`,
//!   Ludwig's "castle #4" uses `$67`)
//! - `0x5B`/`0x5C` → `'"'` (opening/closing double-quotes in Roy's "the
//!   dangerous "but tasty" Chocolate Island!")
//!
//! There are 7 bosses × 7–8 messages = 53 messages total, at fixed ROM
//! locations (labels `C1Message1Stripe` … `C7Message8Stripe` in
//! `symbols/SMW_U.sym`). Each message's stripe blob must keep its EXACT byte
//! length — the data is addressed directly, not repointable. Editing therefore
//! replaces tile bytes in place; text shorter than the slot is space-padded,
//! longer text is rejected.
//!
//! Not supported for the Japanese version (per LM v1.50 changelog): the J ROM
//! stores completely different stripe data at these labels.

use crate::{
    font_map::FontMap,
    snes_utils::{
        addr::{AddrPc, AddrSnes},
        rom::Rom,
    },
};

/// Boss names in cutscene order (C1–C7).
pub const BOSS_NAMES: [&str; 7] = ["Iggy", "Morton", "Lemmy", "Ludwig", "Roy", "Wendy", "Larry"];

/// SNES addresses of each boss's message stripes, in order.
/// From `symbols/SMW_U.sym` (`C1Message1Stripe` … `C7Message8Stripe`).
const BOSS_STRIPE_ADDRS: [&[u32]; 7] = [
    &[0x0CBE85, 0x0CBEBA, 0x0CBEEF, 0x0CBF24, 0x0CBF59, 0x0CBF8E, 0x0CBFC3],
    &[0x0CBFF2, 0x0CC027, 0x0CC05C, 0x0CC091, 0x0CC0C6, 0x0CC0FB, 0x0CC130, 0x0CC165],
    &[0x0CC190, 0x0CC1C5, 0x0CC1FA, 0x0CC22F, 0x0CC264, 0x0CC299, 0x0CC2CE],
    &[0x0CC2F9, 0x0CC32E, 0x0CC363, 0x0CC398, 0x0CC3CD, 0x0CC402, 0x0CC437, 0x0CC46C],
    &[0x0CC49F, 0x0CC4D4, 0x0CC509, 0x0CC53E, 0x0CC573, 0x0CC5A8, 0x0CC5DD],
    &[0x0CC612, 0x0CC647, 0x0CC67C, 0x0CC6B1, 0x0CC6E6, 0x0CC71B, 0x0CC750, 0x0CC785],
    &[0x0CC7BA, 0x0CC7EF, 0x0CC824, 0x0CC859, 0x0CC88E, 0x0CC8C3, 0x0CC8F8, 0x0CC92D],
];

/// Exclusive end of the boss text data (start of `CODE_0CC94E`, real code).
const BOSS_TEXT_END_SNES: u32 = 0x0CC94E;

/// Total number of boss messages (7+8+7+8+7+8+8).
pub const BOSS_MESSAGE_COUNT: usize = 53;

/// Message counts per boss.
pub const BOSS_MESSAGE_COUNTS: [usize; 7] = [7, 8, 7, 8, 7, 8, 8];

/// One stripe command within a message: `[VRAM][flags/len][tile words...]`.
#[derive(Debug, Clone)]
pub struct BossStripeCommand {
    /// VRAM destination (big-endian word in ROM).
    pub vram:      u16,
    /// Flags/length word (big-endian in ROM): bit 15 = direction, bit 14 =
    /// RLE, low 14 bits = payload byte count − 1.
    pub flags_len: u16,
    /// Tile words (little-endian in ROM), typically `$39TT`.
    pub tiles:     Vec<u16>,
}

/// One boss message: a `$FF`-terminated stripe blob at a fixed ROM address.
#[derive(Debug, Clone)]
pub struct BossMessage {
    /// SNES address of the stripe blob.
    pub snes:     AddrSnes,
    /// Parsed stripe commands.
    pub commands: Vec<BossStripeCommand>,
    /// Total blob byte length including the `$FF` terminator. Re-encoded
    /// blobs must match this exactly.
    pub raw_len:  usize,
}

impl BossMessage {
    /// Flatten all tiles across commands into character bytes (`tile & 0x7F`).
    pub fn char_bytes(&self) -> Vec<u8> {
        self.commands.iter().flat_map(|c| c.tiles.iter().map(|&t| (t & 0x7F) as u8)).collect()
    }

    /// Decode to readable text via the boss font map. Unmapped tiles decode
    /// as `'�'`.
    pub fn text(&self) -> String {
        self.text_with_table(None)
    }

    /// Decode to readable text, optionally through a Lunar Magic v3.40
    /// custom table file ([`crate::table_file::Table`]) instead of the
    /// built-in boss font map. With a table, unmapped tiles decode as the
    /// table's `<XX>` hex escapes.
    pub fn text_with_table(&self, table: Option<&crate::table_file::Table>) -> String {
        let bytes = self.char_bytes();
        match table {
            Some(t) => t.decode(&bytes),
            None => {
                let map = boss_font_map();
                bytes.iter().map(|&b| map.char_for(b).unwrap_or('�')).collect()
            }
        }
    }

    /// Number of character positions (tiles) in this message.
    pub fn len(&self) -> usize {
        self.commands.iter().map(|c| c.tiles.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Replace the message text, preserving the stripe structure (VRAM
    /// addresses, command breaks, tile attribute bytes). `text` is a sequence
    /// of characters; each must map in the boss font (use `' '` for blanks).
    /// Shorter text is space-padded to the slot length; longer text is an
    /// error. Unmapped characters are an error.
    pub fn set_text(&mut self, text: &str) -> anyhow::Result<()> {
        self.set_text_with_table(text, None)
    }

    /// Replace the message text, optionally through a Lunar Magic v3.40
    /// custom table file ([`crate::table_file::Table`]) instead of the
    /// built-in boss font map.
    ///
    /// With `None`, behaves exactly like [`set_text`]. With a table, the
    /// text is encoded with the table's greedy longest-match; the budget is
    /// counted in *bytes* (a MultiTile entry emits several tiles per match),
    /// so over-long input is refused exactly when the encoded bytes exceed
    /// the slot. Shorter text is padded to the slot length with the table's
    /// encoding of `" "` (falling back to the vanilla space tile `0x1F`
    /// when the table doesn't map space). Unmapped characters are skipped,
    /// per the "Custom Table File" help topic — never an error.
    pub fn set_text_with_table(&mut self, text: &str, table: Option<&crate::table_file::Table>) -> anyhow::Result<()> {
        let total = self.len();
        let mut bytes: Vec<u8> = match table {
            Some(t) => t.encode(text),
            None => {
                let map = boss_font_map();
                let mut b = Vec::with_capacity(text.chars().count());
                for c in text.chars() {
                    b.push(
                        map.byte_for(c)
                            .ok_or_else(|| anyhow::anyhow!("Character {c:?} has no tile in the boss font"))?,
                    );
                }
                b
            }
        };
        if bytes.len() > total {
            anyhow::bail!("Text encodes to {} tiles but the message slot holds only {total} — shorten it", bytes.len());
        }
        // Pad to the slot length.
        let pad: Vec<u8> = match table {
            Some(t) => {
                let p = t.encode(" ");
                if p.is_empty() {
                    vec![0x1F]
                } else {
                    p
                }
            }
            None => vec![0x1F],
        };
        let mut pi = 0;
        while bytes.len() < total {
            bytes.push(pad[pi % pad.len()]);
            pi += 1;
        }
        // Write back into the tile words, preserving attribute bytes ($39).
        let mut idx = 0;
        for cmd in &mut self.commands {
            for tile in &mut cmd.tiles {
                let attr = *tile & 0xFF00;
                *tile = attr | bytes[idx] as u16;
                idx += 1;
            }
        }
        Ok(())
    }

    /// Clear the message text (all tiles → space), keeping the structure.
    pub fn clear(&mut self) {
        for cmd in &mut self.commands {
            for tile in &mut cmd.tiles {
                *tile = (*tile & 0xFF00) | 0x1F;
            }
        }
    }

    /// Re-encode to the exact original byte layout (commands + `$FF`).
    /// The result is always `raw_len` bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.raw_len);
        for cmd in &self.commands {
            out.extend_from_slice(&cmd.vram.to_be_bytes());
            out.extend_from_slice(&cmd.flags_len.to_be_bytes());
            for &tile in &cmd.tiles {
                out.extend_from_slice(&tile.to_le_bytes());
            }
        }
        out.push(0xFF);
        debug_assert_eq!(out.len(), self.raw_len);
        out
    }
}

/// All 53 boss messages, indexed `[boss][message]`.
#[derive(Debug, Clone)]
pub struct BossText {
    pub messages: Vec<Vec<BossMessage>>,
}

impl BossText {
    pub fn parse(rom: &Rom) -> anyhow::Result<Self> {
        let end_pc = AddrPc::try_from_lorom(AddrSnes(BOSS_TEXT_END_SNES))
            .map_err(|e| anyhow::anyhow!("BossText end addr conversion: {e}"))?
            .0 as usize;
        // Flatten all stripe addresses with their end boundaries.
        let mut flat: Vec<(u32, u32)> = Vec::new();
        for boss_addrs in &BOSS_STRIPE_ADDRS {
            for &snes in *boss_addrs {
                flat.push((snes, 0));
            }
        }
        for i in 0..flat.len() {
            let end_snes = if i + 1 < flat.len() { flat[i + 1].0 } else { BOSS_TEXT_END_SNES };
            flat[i].1 = end_snes;
        }

        let mut messages: Vec<Vec<BossMessage>> = Vec::with_capacity(7);
        let mut flat_idx = 0;
        for boss_addrs in &BOSS_STRIPE_ADDRS {
            let mut boss_msgs = Vec::with_capacity(boss_addrs.len());
            for &snes_u32 in *boss_addrs {
                let (start_snes, end_snes) = flat[flat_idx];
                debug_assert_eq!(start_snes, snes_u32);
                flat_idx += 1;
                let start_pc = AddrPc::try_from_lorom(AddrSnes(start_snes))
                    .map_err(|e| anyhow::anyhow!("BossText addr conversion: {e}"))?
                    .0 as usize;
                let end_pc_msg = if end_snes == BOSS_TEXT_END_SNES {
                    end_pc
                } else {
                    AddrPc::try_from_lorom(AddrSnes(end_snes))
                        .map_err(|e| anyhow::anyhow!("BossText end addr conversion: {e}"))?
                        .0 as usize
                };
                if end_pc_msg > rom.0.len() || start_pc >= end_pc_msg {
                    anyhow::bail!("Boss message at ${start_snes:06X} out of range");
                }
                let blob = &rom.0[start_pc..end_pc_msg];
                let (commands, raw_len) =
                    parse_stripe_blob(blob).map_err(|e| anyhow::anyhow!("Boss message ${start_snes:06X}: {e}"))?;
                boss_msgs.push(BossMessage { snes: AddrSnes(start_snes), commands, raw_len });
            }
            messages.push(boss_msgs);
        }
        Ok(Self { messages })
    }

    /// Clear every message's text (LM v3.20 "clear all text").
    pub fn clear_all(&mut self) {
        for boss in &mut self.messages {
            for msg in boss {
                msg.clear();
            }
        }
    }
}

/// Parse a `$FF`-terminated stripe blob into commands. Returns the commands
/// and the total bytes consumed (including the terminator).
fn parse_stripe_blob(blob: &[u8]) -> Result<(Vec<BossStripeCommand>, usize), String> {
    let mut commands = Vec::new();
    let mut i = 0;
    loop {
        if i >= blob.len() {
            return Err("unterminated stripe blob (no $FF)".to_string());
        }
        if blob[i] == 0xFF {
            i += 1;
            break;
        }
        if blob.len() - i < 4 {
            return Err(format!("truncated stripe header at offset {i:#04X}"));
        }
        let vram = u16::from_be_bytes([blob[i], blob[i + 1]]);
        let flags_len = u16::from_be_bytes([blob[i + 2], blob[i + 3]]);
        if flags_len & 0x4000 != 0 {
            return Err(format!("RLE stripe command not supported at offset {i:#04X}"));
        }
        let nbytes = ((flags_len & 0x3FFF) + 1) as usize;
        if nbytes % 2 != 0 {
            return Err(format!("odd stripe payload byte count {nbytes} at offset {i:#04X}"));
        }
        if i + 4 + nbytes > blob.len() {
            return Err(format!("truncated stripe payload at offset {i:#04X}"));
        }
        let tiles: Vec<u16> =
            blob[i + 4..i + 4 + nbytes].chunks_exact(2).map(|w| u16::from_le_bytes([w[0], w[1]])).collect();
        commands.push(BossStripeCommand { vram, flags_len, tiles });
        i += 4 + nbytes;
    }
    Ok((commands, i))
}

/// The boss-text font map: the real SMW message-box font (`FontMap::real`)
/// plus `'#'` at `0x5A` and `'0'-'9'` at `0x63-0x6C` (castle numbers —
/// verified: Iggy's "castle #1" uses tiles `$5A $64`, Ludwig's "castle #4"
/// uses `$5A $67`).
#[derive(Debug, Clone)]
pub struct BossFontMap {
    map: [Option<char>; 128],
    rev: std::collections::HashMap<char, u8>,
}

pub fn boss_font_map() -> BossFontMap {
    let real = FontMap::real();
    let mut map: [Option<char>; 128] = [None; 128];
    let mut rev = std::collections::HashMap::new();
    for b in 0..128u8 {
        if let Some(c) = real.char_for(b) {
            // Boss text overrides 'z' at 0x5A with '#'.
            if b == 0x5A {
                continue;
            }
            map[b as usize] = Some(c);
            rev.entry(c).or_insert(b);
        }
    }
    map[0x5A] = Some('#');
    rev.insert('#', 0x5A);
    // 0x5B/0x5C are the opening/closing double-quotes ("the dangerous "but
    // tasty" Chocolate Island!"); both decode as '"' and '"' encodes back as
    // the opening quote tile 0x5B. Verified 2026-09-17 against real GFX2A.
    map[0x5B] = Some('"');
    map[0x5C] = Some('"');
    rev.insert('"', 0x5B);
    for (i, c) in ('0'..='9').enumerate() {
        let b = (0x63 + i) as u8;
        map[b as usize] = Some(c);
        rev.entry(c).or_insert(b);
    }
    BossFontMap { map, rev }
}

impl BossFontMap {
    pub fn char_for(&self, byte: u8) -> Option<char> {
        self.map[(byte & 0x7F) as usize]
    }

    pub fn byte_for(&self, c: char) -> Option<u8> {
        self.rev.get(&c).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boss_counts_add_up() {
        assert_eq!(BOSS_MESSAGE_COUNTS.iter().sum::<usize>(), BOSS_MESSAGE_COUNT);
        assert_eq!(BOSS_STRIPE_ADDRS.iter().map(|a| a.len()).sum::<usize>(), BOSS_MESSAGE_COUNT);
    }

    #[test]
    fn stripe_addrs_are_increasing() {
        let flat: Vec<u32> = BOSS_STRIPE_ADDRS.iter().flat_map(|a| a.iter().copied()).collect();
        for w in flat.windows(2) {
            assert!(w[0] < w[1], "{:06X} >= {:06X}", w[0], w[1]);
        }
        assert!(*flat.last().unwrap() < BOSS_TEXT_END_SNES);
    }

    #[test]
    fn parse_single_command_blob() {
        // C1Message1Stripe header: VRAM $5264, 24 tiles, then $FF.
        let mut blob = vec![0x52, 0x64, 0x00, 0x2F];
        for b in [0x0Cu8, 0x40, 0x51, 0x48] {
            blob.extend_from_slice(&[b, 0x39]);
        }
        // Pad to 24 tiles.
        for _ in 0..20 {
            blob.extend_from_slice(&[0x1F, 0x39]);
        }
        blob.push(0xFF);
        let (cmds, len) = parse_stripe_blob(&blob).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].vram, 0x5264);
        assert_eq!(cmds[0].tiles.len(), 24);
        assert_eq!(len, blob.len());
    }

    #[test]
    fn set_text_round_trips_and_pads() {
        let mut msg = BossMessage {
            snes:     AddrSnes(0x0CBE85),
            commands: vec![BossStripeCommand { vram: 0x5264, flags_len: 0x002F, tiles: vec![0x391F; 24] }],
            raw_len:  4 + 48 + 1,
        };
        msg.set_text("Hi").unwrap();
        let bytes = msg.char_bytes();
        assert_eq!(bytes[0], 0x07); // 'H'
        assert_eq!(bytes[1], 0x48); // 'i'
        assert_eq!(bytes[2], 0x1F); // padded space
        assert_eq!(msg.to_bytes().len(), msg.raw_len);
    }

    #[test]
    fn set_text_rejects_overflow() {
        let mut msg = BossMessage {
            snes:     AddrSnes(0x0CBE85),
            commands: vec![BossStripeCommand { vram: 0x5264, flags_len: 0x0001, tiles: vec![0x391F; 2] }],
            raw_len:  4 + 4 + 1,
        };
        assert!(msg.set_text("abc").is_err());
    }

    /// Table used by the table-file tests: 07=H, 48=i, 1F=space, plus a
    /// MultiTile entry 4849=HI.
    fn test_table() -> crate::table_file::Table {
        let (file, warnings) = crate::table_file::parse_lmtbl("@BossSequence\n07=H\n48=i\n1F= \n4849=HI\n").unwrap();
        assert!(warnings.is_empty());
        file.table_for(crate::table_file::TableDialog::BossSequence).unwrap().clone()
    }

    fn test_msg() -> BossMessage {
        BossMessage {
            snes:     AddrSnes(0x0CBE85),
            commands: vec![BossStripeCommand { vram: 0x5264, flags_len: 0x002F, tiles: vec![0x391F; 24] }],
            raw_len:  4 + 48 + 1,
        }
    }

    #[test]
    fn table_text_decodes_with_hex_escapes() {
        let t = test_table();
        let mut msg = test_msg();
        // Tiles: H, unmapped 0x60, then spaces.
        msg.commands[0].tiles[0] = 0x3907;
        msg.commands[0].tiles[1] = 0x3960;
        let text = msg.text_with_table(Some(&t));
        assert!(text.starts_with("H<60>"), "unexpected decode: {text:?}");
        // Without a table, the built-in map is used.
        assert!(msg.text().starts_with("H�"), "unexpected built-in decode: {:?}", msg.text());
    }

    #[test]
    fn table_set_text_counts_bytes_and_pads_with_table_space() {
        let t = test_table();
        let mut msg = test_msg();
        // "HI" via the MultiTile entry -> 2 tiles, not 1 char.
        msg.set_text_with_table("HI", Some(&t)).unwrap();
        let bytes = msg.char_bytes();
        assert_eq!(&bytes[0..2], &[0x48, 0x49]);
        // Padded with the table's space (0x1F) to the 24-tile slot.
        assert!(bytes[2..].iter().all(|&b| b == 0x1F));
        assert_eq!(msg.to_bytes().len(), msg.raw_len);
    }

    #[test]
    fn table_set_text_skips_unmapped_chars_and_rejects_byte_overflow() {
        let t = test_table();
        let mut msg = test_msg();
        // 'Z' is unmapped: skipped, not an error.
        msg.set_text_with_table("HZi", Some(&t)).unwrap();
        assert_eq!(&msg.char_bytes()[0..2], &[0x07, 0x48]);
        // 13 "HI" pairs = 26 tiles > 24-tile slot: refused (byte budget).
        let mut msg2 = test_msg();
        assert!(msg2.set_text_with_table(&"HI".repeat(13), Some(&t)).is_err());
        // 12 pairs = 24 tiles: exactly fits.
        let mut msg3 = test_msg();
        assert!(msg3.set_text_with_table(&"HI".repeat(12), Some(&t)).is_ok());
    }

    #[test]
    fn table_set_text_without_table_matches_set_text() {
        let mut a = test_msg();
        let mut b = test_msg();
        a.set_text("Hi castle #1").unwrap();
        b.set_text_with_table("Hi castle #1", None).unwrap();
        assert_eq!(a.char_bytes(), b.char_bytes());
    }

    #[test]
    fn boss_font_has_hash_and_digits() {
        let map = boss_font_map();
        assert_eq!(map.char_for(0x5A), Some('#'));
        assert_eq!(map.char_for(0x64), Some('1'));
        assert_eq!(map.char_for(0x67), Some('4'));
        assert_eq!(map.char_for(0x5B), Some('"'));
        assert_eq!(map.char_for(0x5C), Some('"'));
        assert_eq!(map.byte_for('#'), Some(0x5A));
        assert_eq!(map.byte_for('7'), Some(0x6A));
        assert_eq!(map.byte_for('"'), Some(0x5B));
    }
}

#[cfg(test)]
mod real_rom_tests {
    use super::*;
    use crate::SmwRom;

    /// Parses all 53 boss messages from the real ROM and prints their decoded
    /// text. Run with `ROM_PATH=/path/to/smw.smc cargo test -p smwe-rom
    /// --lib -- --ignored real_rom_boss_text -- --nocapture`.
    #[test]
    #[ignore]
    fn real_rom_boss_text() {
        let rom_path = std::env::var("ROM_PATH").expect("set ROM_PATH");
        let rom = SmwRom::from_file(rom_path).expect("parse ROM");
        let boss = &rom.boss_text;
        assert_eq!(boss.messages.len(), 7);
        let mut total = 0;
        for (bi, msgs) in boss.messages.iter().enumerate() {
            assert_eq!(msgs.len(), BOSS_MESSAGE_COUNTS[bi]);
            println!("=== {} ({} messages) ===", BOSS_NAMES[bi], msgs.len());
            for (mi, msg) in msgs.iter().enumerate() {
                println!("  M{} (${:06X}, {} tiles): {:?}", mi + 1, msg.snes.0, msg.len(), msg.text());
                // Re-encoding must preserve the exact byte length.
                assert_eq!(msg.to_bytes().len(), msg.raw_len);
                total += 1;
            }
        }
        assert_eq!(total, BOSS_MESSAGE_COUNT);
    }
}
