//! Overworld level names ("YOSHI'S ISLAND 1" in the OW status bar).
//!
//! The vanilla game composes each name from up to three "pieces":
//! `LevelNames` ($04A0FC) holds 2 bytes per translevel; byte 1 (`& 0x7F`)
//! indexes the piece-1 offset table (`DATA_049C91`), byte 0's high nibble the
//! piece-2 table (`DATA_049CCF`), and byte 0's low nibble the piece-3 table
//! (`DATA_049CED`). Each table entry is a 16-bit offset into
//! `LevelNameStrings` ($049AC5); a string ends at the first byte with bit 7
//! set. Display quirks mirrored from `CODE_049D07` (bank_04.asm): piece 1 is
//! skipped when its first byte has bit 7 set, piece 2 when its first byte is
//! exactly `0x9F` (the shared blank), and the whole name is padded/truncated
//! to 19 characters.
//!
//! All addresses/sizes verified against the U ROM by decoding all 0x5D
//! vanilla names to the expected text (see tests).

use crate::snes_utils::{
    addr::{AddrPc, AddrSnes},
    rom::Rom,
};

/// `LevelNameStrings` (bank_04.asm): concatenated name-piece strings.
pub const LEVEL_NAME_STRINGS_SNES: AddrSnes = AddrSnes(0x049AC5);
/// Fixed size of the strings region (`DATA_049C91 - LevelNameStrings`).
pub const LEVEL_NAME_STRINGS_SIZE: usize = 0x1CC;

/// `DATA_049C91`: piece-1 ("YOSHI'S", "DONUT", ...) offset table.
pub const LEVEL_NAME_PIECE1_OFFSETS_SNES: AddrSnes = AddrSnes(0x049C91);
pub const LEVEL_NAME_PIECE1_COUNT: usize = 31;
/// `DATA_049CCF`: piece-2 ("ISLAND", "GHOST HOUSE", ...) offset table.
pub const LEVEL_NAME_PIECE2_OFFSETS_SNES: AddrSnes = AddrSnes(0x049CCF);
pub const LEVEL_NAME_PIECE2_COUNT: usize = 15;
/// `DATA_049CED`: piece-3 ("1".."5", "PALACE", ...) offset table.
pub const LEVEL_NAME_PIECE3_OFFSETS_SNES: AddrSnes = AddrSnes(0x049CED);
pub const LEVEL_NAME_PIECE3_COUNT: usize = 13;

/// `LevelNames` (bank_04.asm): 2 bytes per translevel.
pub const LEVEL_NAMES_SNES: AddrSnes = AddrSnes(0x04A0FC);
/// Same translevel count as the event table.
pub const LEVEL_NAMES_COUNT: usize = super::TRANSLEVEL_EVENTS_COUNT;

/// The status-bar name field is 19 tiles wide (`CODE_049D07` reserves
/// `$26 / 2` stripe slots).
pub const LEVEL_NAME_DISPLAY_WIDTH: usize = 19;

/// The shared one-byte blank string (space with the end bit set).
pub const BLANK_PIECE_BYTE: u8 = 0x9F;

/// Squished two-letters-per-tile glyph runs used by two vanilla names. Decoded
/// to/encoded from their readable text so users can type the real words.
const SPECIAL_RUNS: [(&[u8], &str); 2] = [
    (&[0x32, 0x33, 0x34, 0x35, 0x36, 0x37], " ILLUSI"), // FOREST OF| ILLUSI|ON
    (&[0x38, 0x39, 0x3A, 0x3B, 0x3C], "YELLOW"),        // YELLOW| SWITCH PALACE
];

/// Map an OW-font tile byte (end bit stripped) to the character it shows.
///
/// Verified by decoding all vanilla level names: A-Z at 0x00-0x19 and the
/// 0x1A-0x1F punctuation block match the message-box font; digits '1'-'7'
/// live at 0x64-0x6A (vanilla never shows '0', '8' or '9', so those glyphs
/// are unverified and unmapped); '#' 0x5A, '\'' 0x5D.
pub fn ow_name_byte_to_char(byte: u8) -> Option<char> {
    let byte = byte & 0x7F;
    Some(match byte {
        0x00..=0x19 => (b'A' + byte) as char,
        0x1A => '!',
        0x1B => '.',
        0x1C => '-',
        0x1D => ',',
        0x1E => '?',
        0x1F => ' ',
        0x5A => '#',
        0x5D => '\'',
        0x64..=0x6A => (b'1' + byte - 0x64) as char,
        _ => return None,
    })
}

/// Inverse of [`ow_name_byte_to_char`].
pub fn ow_name_char_to_byte(c: char) -> Option<u8> {
    Some(match c {
        'A'..='Z' => c as u8 - b'A',
        'a'..='z' => c.to_ascii_uppercase() as u8 - b'A',
        '!' => 0x1A,
        '.' => 0x1B,
        '-' => 0x1C,
        ',' => 0x1D,
        '?' => 0x1E,
        ' ' => 0x1F,
        '#' => 0x5A,
        '\'' => 0x5D,
        '1'..='7' => 0x64 + (c as u8 - b'1'),
        _ => return None,
    })
}

/// Decode one raw piece (terminator bit included on its last byte) into
/// `(text, tile_cost)` chunks: one chunk per glyph, except the known squished
/// runs which decode as one multi-character chunk costing the run's tile
/// count. Unmappable tiles outside those runs become `¤`.
fn decode_chunks(bytes: &[u8]) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    'outer: while i < bytes.len() {
        for (run, text) in SPECIAL_RUNS {
            let end = i + run.len();
            if end <= bytes.len() && bytes[i..end].iter().map(|b| b & 0x7F).eq(run.iter().copied()) {
                out.push((text.to_string(), run.len()));
                i = end;
                continue 'outer;
            }
        }
        out.push((ow_name_byte_to_char(bytes[i]).unwrap_or('¤').to_string(), 1));
        i += 1;
    }
    out
}

/// Decode one raw piece (terminator bit included on its last byte) to text.
pub fn decode_piece(bytes: &[u8]) -> String {
    decode_chunks(bytes).into_iter().map(|(text, _)| text).collect()
}

/// Encode piece text back to raw bytes, setting the end bit on the last byte.
/// An empty string becomes the shared blank. Returns `Err` with the first
/// character the OW font can't show.
pub fn encode_piece(text: &str) -> Result<Vec<u8>, char> {
    let mut out = Vec::new();
    let mut rest = text;
    'outer: while !rest.is_empty() {
        for (run, special) in SPECIAL_RUNS {
            if let Some(stripped) = rest.strip_prefix(special) {
                out.extend_from_slice(run);
                rest = stripped;
                continue 'outer;
            }
        }
        let c = rest.chars().next().unwrap();
        out.push(ow_name_char_to_byte(c).ok_or(c)?);
        rest = &rest[c.len_utf8()..];
    }
    match out.last_mut() {
        Some(last) => *last |= 0x80,
        None => out.push(BLANK_PIECE_BYTE),
    }
    Ok(out)
}

/// Which pieces one translevel's name is built from (table indices).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelNameEntry {
    pub piece1: u8,
    pub piece2: u8,
    pub piece3: u8,
}

/// Editable copy of the OW level-name data. Rewritten fully in place on save
/// (all four tables have fixed locations and sizes), so it needs no
/// repointing; the only budget is `LEVEL_NAME_STRINGS_SIZE` bytes for the
/// combined piece strings.
#[derive(Debug, Clone)]
pub struct OwLevelNames {
    /// Raw piece strings (terminator bit on the last byte of each).
    pub piece1:  Vec<Vec<u8>>,
    pub piece2:  Vec<Vec<u8>>,
    pub piece3:  Vec<Vec<u8>>,
    /// Per-translevel piece selection, `LEVEL_NAMES_COUNT` entries.
    pub entries: Vec<LevelNameEntry>,
}

fn table_pc(addr: AddrSnes, what: &str) -> anyhow::Result<usize> {
    Ok(AddrPc::try_from_lorom(addr).map_err(|e| anyhow::anyhow!("{what} addr conversion: {e}"))?.0 as usize)
}

fn read_piece(strings: &[u8], offset: u16, what: &str) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = offset as usize;
    loop {
        let &b = strings.get(i).ok_or_else(|| anyhow::anyhow!("{what} string at {offset:#06X} runs off the region"))?;
        out.push(b);
        i += 1;
        if b & 0x80 != 0 {
            return Ok(out);
        }
    }
}

impl OwLevelNames {
    pub fn parse(rom: &Rom) -> anyhow::Result<Self> {
        let strings_pc = table_pc(LEVEL_NAME_STRINGS_SNES, "LevelNameStrings")?;
        let names_pc = table_pc(LEVEL_NAMES_SNES, "LevelNames")?;
        if strings_pc + LEVEL_NAME_STRINGS_SIZE > rom.0.len() || names_pc + LEVEL_NAMES_COUNT * 2 > rom.0.len() {
            anyhow::bail!("OW level name tables extend past end of ROM");
        }
        let strings = &rom.0[strings_pc..strings_pc + LEVEL_NAME_STRINGS_SIZE];

        let read_table = |addr: AddrSnes, count: usize, what: &str| -> anyhow::Result<Vec<Vec<u8>>> {
            let pc = table_pc(addr, what)?;
            if pc + count * 2 > rom.0.len() {
                anyhow::bail!("{what} extends past end of ROM");
            }
            rom.0[pc..pc + count * 2]
                .chunks_exact(2)
                .map(|w| read_piece(strings, u16::from_le_bytes([w[0], w[1]]), what))
                .collect()
        };
        let piece1 = read_table(LEVEL_NAME_PIECE1_OFFSETS_SNES, LEVEL_NAME_PIECE1_COUNT, "piece-1 offsets")?;
        let piece2 = read_table(LEVEL_NAME_PIECE2_OFFSETS_SNES, LEVEL_NAME_PIECE2_COUNT, "piece-2 offsets")?;
        let piece3 = read_table(LEVEL_NAME_PIECE3_OFFSETS_SNES, LEVEL_NAME_PIECE3_COUNT, "piece-3 offsets")?;

        let entries = rom.0[names_pc..names_pc + LEVEL_NAMES_COUNT * 2]
            .chunks_exact(2)
            .map(|e| LevelNameEntry {
                piece1: (e[1] & 0x7F).min(LEVEL_NAME_PIECE1_COUNT as u8 - 1),
                piece2: (e[0] >> 4).min(LEVEL_NAME_PIECE2_COUNT as u8 - 1),
                piece3: (e[0] & 0x0F).min(LEVEL_NAME_PIECE3_COUNT as u8 - 1),
            })
            .collect();

        Ok(Self { piece1, piece2, piece3, entries })
    }

    /// The raw pieces this translevel's name displays, applying
    /// `CODE_049D07`'s skip rules (piece 1 hidden when its first byte has bit
    /// 7 set, piece 2 when it is the shared blank).
    fn visible_pieces(&self, translevel: u8) -> Vec<&[u8]> {
        let Some(entry) = self.entries.get(translevel as usize) else { return Vec::new() };
        let mut out = Vec::new();
        if let Some(p) = self.piece1.get(entry.piece1 as usize) {
            if p.first().is_some_and(|&b| b & 0x80 == 0) {
                out.push(p.as_slice());
            }
        }
        if let Some(p) = self.piece2.get(entry.piece2 as usize) {
            if p.first().is_some_and(|&b| b != BLANK_PIECE_BYTE) {
                out.push(p.as_slice());
            }
        }
        if let Some(p) = self.piece3.get(entry.piece3 as usize) {
            out.push(p.as_slice());
        }
        out
    }

    /// The name the status bar would show for this translevel, mirroring
    /// `CODE_049D07`'s skip rules and its [`LEVEL_NAME_DISPLAY_WIDTH`]-tile
    /// field (names too long are cut off, exactly like in-game).
    pub fn display_name(&self, translevel: u8) -> String {
        let mut out = String::new();
        let mut tiles = 0;
        'pieces: for piece in self.visible_pieces(translevel) {
            for (text, cost) in decode_chunks(piece) {
                if tiles + cost > LEVEL_NAME_DISPLAY_WIDTH {
                    break 'pieces;
                }
                out.push_str(&text);
                tiles += cost;
            }
        }
        out.trim_end().to_string()
    }

    /// How many status-bar tiles this translevel's name wants (the squished
    /// glyph runs pack more characters than tiles). The field holds
    /// [`LEVEL_NAME_DISPLAY_WIDTH`] tiles; anything longer is cut off in-game.
    pub fn display_tiles(&self, translevel: u8) -> usize {
        self.visible_pieces(translevel).iter().map(|p| p.len()).sum()
    }

    /// Total bytes the piece strings need after dedup/substring sharing, to
    /// show against the [`LEVEL_NAME_STRINGS_SIZE`] budget.
    pub fn strings_size(&self) -> usize {
        self.build_strings_blob().len()
    }

    fn build_strings_blob(&self) -> Vec<u8> {
        // Longest-first so shorter pieces can reuse a substring of a longer
        // one (safe because each piece's bytes include its terminator).
        let mut all: Vec<&Vec<u8>> = self.piece1.iter().chain(&self.piece2).chain(&self.piece3).collect();
        all.sort_by_key(|p| std::cmp::Reverse(p.len()));
        let mut blob: Vec<u8> = Vec::new();
        for piece in all {
            if !blob.windows(piece.len().max(1)).any(|w| w == piece.as_slice()) {
                blob.extend_from_slice(piece);
            }
        }
        blob
    }

    /// Serialize all four tables for an in-place write. Returns
    /// `(strings_region, piece1_offsets, piece2_offsets, piece3_offsets,
    /// entries)`; errors if the strings don't fit the fixed region.
    #[allow(clippy::type_complexity)]
    pub fn to_rom_tables(&self) -> anyhow::Result<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)> {
        let blob = self.build_strings_blob();
        if blob.len() > LEVEL_NAME_STRINGS_SIZE {
            anyhow::bail!(
                "level name strings need {} bytes; only {} fit in the vanilla region",
                blob.len(),
                LEVEL_NAME_STRINGS_SIZE
            );
        }
        let offsets_of = |pieces: &[Vec<u8>]| -> Vec<u8> {
            pieces
                .iter()
                .flat_map(|p| {
                    let at = blob.windows(p.len()).position(|w| w == p.as_slice()).expect("piece is in blob") as u16;
                    at.to_le_bytes()
                })
                .collect()
        };
        let t1 = offsets_of(&self.piece1);
        let t2 = offsets_of(&self.piece2);
        let t3 = offsets_of(&self.piece3);
        let entries = self
            .entries
            .iter()
            .flat_map(|e| [(e.piece2 << 4) | (e.piece3 & 0x0F), e.piece1 & 0x7F])
            .collect();
        let mut strings = blob;
        strings.resize(LEVEL_NAME_STRINGS_SIZE, BLANK_PIECE_BYTE);
        Ok((strings, t1, t2, t3, entries))
    }

    /// Write all four tables back into `rom_bytes` at their fixed locations.
    /// `header_offset` is 0x200 for `.smc` images with a copier header, 0
    /// otherwise.
    pub fn write_to_rom(&self, rom_bytes: &mut [u8], header_offset: usize) -> anyhow::Result<()> {
        let (strings, t1, t2, t3, entries) = self.to_rom_tables()?;
        for (addr, bytes, what) in [
            (LEVEL_NAME_STRINGS_SNES, &strings, "LevelNameStrings"),
            (LEVEL_NAME_PIECE1_OFFSETS_SNES, &t1, "piece-1 offsets"),
            (LEVEL_NAME_PIECE2_OFFSETS_SNES, &t2, "piece-2 offsets"),
            (LEVEL_NAME_PIECE3_OFFSETS_SNES, &t3, "piece-3 offsets"),
            (LEVEL_NAMES_SNES, &entries, "LevelNames"),
        ] {
            let pc = table_pc(addr, what)? + header_offset;
            if pc + bytes.len() > rom_bytes.len() {
                anyhow::bail!("{what} extends past end of ROM");
            }
            rom_bytes[pc..pc + bytes.len()].copy_from_slice(bytes);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charset_round_trips() {
        for byte in 0..=0x7Fu8 {
            if let Some(c) = ow_name_byte_to_char(byte) {
                assert_eq!(ow_name_char_to_byte(c), Some(byte), "byte {byte:02X} -> {c:?} did not round-trip");
            }
        }
    }

    #[test]
    fn piece_text_round_trips_including_special_runs() {
        for text in ["YOSHI'S ", "#1 IGGY'S ", "YELLOW ", "OF ILLUSION ", "", "CHOCO-GHOST HOUSE "] {
            let encoded = encode_piece(text).unwrap();
            assert!(encoded.last().unwrap() & 0x80 != 0, "{text:?} missing end bit");
            assert_eq!(decode_piece(&encoded), if text.is_empty() { " ".to_string() } else { text.to_string() });
        }
    }

    #[test]
    fn encode_rejects_unsupported_chars() {
        assert_eq!(encode_piece("MODE 8"), Err('8'));
        assert_eq!(encode_piece("A+B"), Err('+'));
    }

    fn sample_names() -> OwLevelNames {
        OwLevelNames {
            piece1:  vec![vec![BLANK_PIECE_BYTE], encode_piece("YOSHI'S ").unwrap()],
            piece2:  vec![vec![BLANK_PIECE_BYTE], encode_piece("ISLAND ").unwrap()],
            piece3:  vec![vec![BLANK_PIECE_BYTE], encode_piece("1").unwrap(), encode_piece("PALACE").unwrap()],
            entries: vec![
                LevelNameEntry { piece1: 1, piece2: 1, piece3: 1 },
                LevelNameEntry { piece1: 0, piece2: 0, piece3: 2 },
            ],
        }
    }

    #[test]
    fn display_name_applies_skip_rules_and_width() {
        let names = sample_names();
        assert_eq!(names.display_name(0), "YOSHI'S ISLAND 1");
        // Blank piece1 (bit-7 first byte) and blank piece2 are skipped.
        assert_eq!(names.display_name(1), "PALACE");
        assert_eq!(names.display_name(0x50), "");
    }

    #[test]
    fn rom_tables_round_trip_through_parse() {
        let names = sample_names();
        let mut names = names;
        names.entries.resize(LEVEL_NAMES_COUNT, LevelNameEntry { piece1: 0, piece2: 0, piece3: 0 });
        names.piece1.resize(LEVEL_NAME_PIECE1_COUNT, vec![BLANK_PIECE_BYTE]);
        names.piece2.resize(LEVEL_NAME_PIECE2_COUNT, vec![BLANK_PIECE_BYTE]);
        names.piece3.resize(LEVEL_NAME_PIECE3_COUNT, vec![BLANK_PIECE_BYTE]);

        // Big enough fake LoROM image.
        let mut rom_bytes = vec![0u8; 0x40000];
        names.write_to_rom(&mut rom_bytes, 0).unwrap();
        let reparsed = OwLevelNames::parse(&Rom(rom_bytes.into())).unwrap();
        assert_eq!(reparsed.entries, names.entries);
        assert_eq!(reparsed.piece1, names.piece1);
        assert_eq!(reparsed.piece2, names.piece2);
        assert_eq!(reparsed.piece3, names.piece3);
    }

    /// Decodes every vanilla name and checks a sample against the known list.
    /// Run with `ROM_PATH=/path/to/smw.smc cargo test -p smwe-rom --lib --
    /// --ignored vanilla_level_names_decode_correctly`.
    #[test]
    #[ignore]
    fn vanilla_level_names_decode_correctly() {
        let rom_path = std::env::var("ROM_PATH").expect("set ROM_PATH");
        let raw = std::fs::read(rom_path).expect("read ROM");
        let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
        let names = OwLevelNames::parse(&Rom(rom_bytes.into())).expect("parse");
        for (translevel, expected) in [
            (0x03u8, "TOP SECRET AREA"),
            (0x09, "DONUT PLAINS 2"),
            (0x14, "YELLOW SWITCH PALACE"),
            (0x18, "SUNKEN GHOST SHIP"),
            (0x25, "#1 IGGY'S CASTLE"),
            (0x28, "YOSHI'S HOUSE"),
            (0x29, "YOSHI'S ISLAND 1"),
            (0x42, "FOREST OF ILLUSION 1"),
            (0x4C, "GROOVY"),
            (0x58, "STAR WORLD 1"),
        ] {
            assert_eq!(names.display_name(translevel), expected, "translevel {translevel:#04X}");
        }
    }

    /// The vanilla data must round-trip: serialize and reparse must preserve
    /// every displayed name, and the rebuilt strings must fit the region.
    /// Run with `ROM_PATH=... cargo test ... --ignored vanilla_level_names_round_trip`.
    #[test]
    #[ignore]
    fn vanilla_level_names_round_trip() {
        let rom_path = std::env::var("ROM_PATH").expect("set ROM_PATH");
        let raw = std::fs::read(rom_path).expect("read ROM");
        let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
        let names = OwLevelNames::parse(&Rom(rom_bytes.clone().into())).expect("parse");
        assert!(names.strings_size() <= LEVEL_NAME_STRINGS_SIZE, "vanilla rebuild must fit");

        let mut rewritten = rom_bytes.clone();
        names.write_to_rom(&mut rewritten, 0).unwrap();
        let reparsed = OwLevelNames::parse(&Rom(rewritten.into())).unwrap();
        for tl in 0..LEVEL_NAMES_COUNT as u8 {
            assert_eq!(names.display_name(tl), reparsed.display_name(tl), "translevel {tl:#04X}");
        }
    }
}
