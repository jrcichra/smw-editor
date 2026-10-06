//! Overworld level-name table (`LevelNames` / `LevelNameStrings`).
//!
//! In the vanilla U ROM, the name shown when Mario stands on a level tile is
//! built by `CODE_049D07` (bank 04) from three shared string fragments:
//!
//! - `LevelNames` (SNES `$04A0FC`, 93 entries × 2 bytes) is indexed by the
//!   level tile's *translevel* number. Each entry packs three fragment
//!   selectors: bits 15-8 select a `T1` fragment, bits 7-4 a `T2` fragment,
//!   bits 3-0 a `T3` fragment.
//! - `T1`/`T2`/`T3` (SNES `$049C91`/`$049CCF`/`$049CED`, 31/15/13 entries ×
//!   2 bytes) hold 16-bit byte-offsets into `LevelNameStrings`.
//! - `LevelNameStrings` (SNES `$049AC5`, 460 bytes) holds the fragment text.
//!   Each fragment is a byte string terminated by a byte with bit 7 set; the
//!   displayed tile is `byte & 0x7F`.
//!
//! The vanilla tables are 100% full (0 free pool bytes, 0 free T1/T2 slots),
//! so arbitrary custom names cannot fit without relocating the tables. When
//! the editor saves custom names it applies a minimal, reversible patch:
//!
//! - The three fragment tables move to `$FF`-filled space at SNES `$04A1B6`
//!   (right after `LevelNames`), growing to 93/16/16 slots.
//! - The string pool grows in place from 460 to 578 bytes (SNES `$049AC5`–
//!   `$049D06`), absorbing the old table space.
//! - Three `LDA.W …,Y` address constants in `CODE_049D07` are rewritten to
//!   the new table bases (same instruction size, no code motion).
//!
//! With 93 T1 slots every translevel gets its own first fragment, and the
//! encoder splits each name into (prefix, middle, suffix) fragments shared
//! across names, which fits the vanilla names in ~502 of the 578 pool bytes.

use crate::snes_utils::addr::{AddrPc, AddrSnes};

// ── Addresses ───────────────────────────────────────────────────────────────

/// `LevelNames` table: 93 entries × 2 bytes, indexed by translevel.
pub const LEVEL_NAMES_SNES: AddrSnes = AddrSnes(0x04A0FC);
/// Number of translevel entries in `LevelNames`.
pub const LEVEL_NAMES_COUNT: usize = 93;

/// Vanilla fragment-table bases.
pub const T1_VANILLA_SNES: AddrSnes = AddrSnes(0x049C91);
pub const T2_VANILLA_SNES: AddrSnes = AddrSnes(0x049CCF);
pub const T3_VANILLA_SNES: AddrSnes = AddrSnes(0x049CED);
pub const T1_VANILLA_SLOTS: usize = 31;
pub const T2_VANILLA_SLOTS: usize = 15;
pub const T3_VANILLA_SLOTS: usize = 13;

/// Patched fragment-table bases (in `$FF`-filled space after `LevelNames`).
///
/// Note: the `LevelNames` entry format packs T2/T3 selectors into 4 bits each,
/// so T2/T3 are hard-capped at 16 slots; T1 gets 7 bits (128 max).
pub const T1_PATCHED_SNES: AddrSnes = AddrSnes(0x04A1B6);
pub const T2_PATCHED_SNES: AddrSnes = AddrSnes(0x04A270);
pub const T3_PATCHED_SNES: AddrSnes = AddrSnes(0x04A290);
pub const T1_PATCHED_SLOTS: usize = 93;
pub const T2_PATCHED_SLOTS: usize = 16;
pub const T3_PATCHED_SLOTS: usize = 16;

/// `LevelNameStrings` pool base (unchanged by the patch).
pub const STRINGS_SNES: AddrSnes = AddrSnes(0x049AC5);
/// Vanilla pool size in bytes.
pub const STRINGS_VANILLA_LEN: usize = 460;
/// Patched pool size in bytes (absorbs the old table space).
pub const STRINGS_PATCHED_LEN: usize = 578;

/// `CODE_049D07` patch sites: (PC offset of the `LDA.W …,Y`, new 16-bit base).
/// Each is `B9 <lo> <hi>` (LDA absolute,Y); only the two address bytes change.
const PATCH_SITES: [(u32, u16); 3] = [
    (0x021D2F, 0xA1B6), // T1: $049C91 -> $04A1B6
    (0x021D48, 0xA270), // T2: $049CCF -> $04A270
    (0x021D61, 0xA290), // T3: $049CED -> $04A290
];
/// Expected vanilla bytes at the patch sites (for detection/restore).
const VANILLA_SITE_BYTES: [(u32, [u8; 3]); 3] =
    [(0x021D2F, [0xB9, 0x91, 0x9C]), (0x021D48, [0xB9, 0xCF, 0x9C]), (0x021D61, [0xB9, 0xED, 0x9C])];

// ── Character map ───────────────────────────────────────────────────────────

/// Tile value for a space.
pub const TILE_SPACE: u8 = 0x1F;
// ── MultiChar (squished) tiles ──────────────────────────────────────────────
//
// Lunar Magic v3.40 added "Use MultiChar Tiles" support to its "Edit Level
// Names" dialog for the squished tiles Nintendo used in "YELLOW SWITCH PALACE"
// and "FOREST OF ILLUSION". These tiles are graphically narrower than the
// standard A-Z tiles, and two of them contain two characters in a single 8x8
// tile:
//
// - `$34` and `$3A` are "LL" — two squished L's in one tile. "YELLOW" (6
//   chars) is stored as 5 tiles (`$38,$39,$3A,$3B,$3C`), and "FOREST OF
//   ILLUSION 1" (20 chars) fits the game's 19-tile stripe buffer as 19 tiles
//   (`$32-$37` = " ILLUSI", 7 chars in 6 tiles).
// - `$32,$33,$35,$36,$37` and `$38,$39,$3B,$3C` are squished single characters
//   (" ", "I", "U", "S", "I" and "Y", "E", "O", "W").
// - `$1C` is a squished single "L" (used in "CHOCOLGHOST HOUSE").
//
// Verified against the vanilla U ROM (tile bytes of translevels 0x14, 0x21,
// 0x2A-0x2D) and the OW status-bar font in VRAM after the real `load_overworld`
// (squished "LL" glyphs visually confirmed).
//
// When the editor's "Use MultiChar Tiles" option is on (LM's default), these
// tiles decode to their character strings and are automatically used when
// encoding. When off, they decode to `\XX` hex escapes (LM's behavior) and are
// never auto-selected for new input.

/// Squished tile → the character(s) it displays. The two multi-character
/// entries (`$34`, `$3A` = "LL") are the actual MultiChar tiles; the rest are
/// squished single characters from the same font region.
pub const SQUISHED_TILES: &[(u8, &str)] = &[
    (0x1C, "L"),
    (0x32, " "),
    (0x33, "I"),
    (0x34, "LL"),
    (0x35, "U"),
    (0x36, "S"),
    (0x37, "I"),
    (0x38, "Y"),
    (0x39, "E"),
    (0x3A, "LL"),
    (0x3B, "O"),
    (0x3C, "W"),
];

/// Returns the display string for a squished tile, or `None` for normal tiles.
pub fn squished_tile_str(tile: u8) -> Option<&'static str> {
    SQUISHED_TILES.iter().find(|(t, _)| *t == tile).map(|(_, s)| *s)
}

/// Returns true if the tile is one of the squished/MultiChar tiles.
pub fn is_squished_tile(tile: u8) -> bool {
    squished_tile_str(tile).is_some()
}
/// Byte emitted for "skip this fragment" (T2): first byte `$9F`.
///
/// `CODE_049D07` skips the T2 fragment when its first string byte is `$9F`.
pub const T2_SKIP_BYTE: u8 = 0x9F;
/// Byte for an empty T1 fragment: bit 7 set, so `CODE_049D07` skips it.
pub const T1_SKIP_BYTE: u8 = 0x80;

/// Longest name the game will draw: `CODE_049D07` reserves `$26` stripe-image
/// bytes (19 tiles) for the composed name, then pads with blanks. A longer
/// name's extra tiles are silently dropped by the game, so the editor refuses
/// them instead of truncating. Note this is a TILE budget, not a character
/// budget — with MultiChar tiles (e.g. `$3A` = "LL"), 20 characters can fit
/// in 19 tiles.
pub const MAX_NAME_TILES: usize = 19;
/// Legacy alias for [`MAX_NAME_TILES`].
pub const MAX_NAME_CHARS: usize = MAX_NAME_TILES;

/// Validate a level name typed in the editor.
///
/// Returns the normalized name (trimmed, uppercased — the game only has
/// uppercase glyphs). The name may contain `\XX` hex escapes for specific
/// tile values (Lunar Magic's escape syntax).
///
/// Errors when the name is empty, encodes to more than [`MAX_NAME_TILES`]
/// tiles, or contains a character with no overworld-name tile (allowed:
/// `A-Z 0-9 space # '`, plus `\XX` escapes). When `use_multichar` is on,
/// "LL" encodes to the squished `$3A` tile (one tile for two characters).
pub fn check_name(name: &str) -> anyhow::Result<String> {
    check_name_with(name, true)
}

/// [`check_name`] with an explicit MultiChar option.
///
/// When `use_multichar` is off, "LL" encodes as two separate tiles and
/// squished tiles are never auto-selected (matching Lunar Magic v3.40's
/// "Use MultiChar Tiles" option).
pub fn check_name_with(name: &str, use_multichar: bool) -> anyhow::Result<String> {
    let normalized = name.trim().to_uppercase();
    anyhow::ensure!(!normalized.is_empty(), "name is empty");
    // Validate escapes and charset by encoding.
    let tiles = encode_name_to_tiles(&normalized, use_multichar)?;
    // Charset check: encode_name_to_tiles maps unknown chars to space via
    // char_to_tile; detect them explicitly for a good error message.
    let mut i = 0;
    let chars: Vec<char> = normalized.chars().collect();
    while i < chars.len() {
        if chars[i] == '\\' {
            // Escape already validated by encode_name_to_tiles.
            i += 3;
            continue;
        }
        let c = chars[i];
        let ok = matches!(c, 'A'..='Z' | '0'..='9' | ' ' | '#' | '\'');
        anyhow::ensure!(ok, "character {c:?} has no overworld-name tile (A-Z 0-9 space # ' only)");
        i += 1;
    }
    let tile_count = tiles.len();
    anyhow::ensure!(
        tile_count <= MAX_NAME_TILES,
        "name encodes to {tile_count} tiles; the game draws at most {MAX_NAME_TILES}"
    );
    Ok(normalized)
}

/// Validate a level name typed in the editor against a Lunar Magic v3.40
/// custom table file ([`crate::table_file::Table`]).
///
/// Unlike [`check_name`], there is no built-in charset and no uppercasing:
/// while a table is active none of the built-in tile/char mapping is used,
/// so the table alone decides what is mappable. Characters with no table
/// mapping are skipped by the encoder (Lunar Magic behavior), which means
/// an all-unmapped name is empty and refused here.
///
/// Returns the trimmed display text plus its encoded tile count, for the
/// byte-budget meter. Errors when the name is empty, has no mappable
/// characters, or encodes to more than [`MAX_NAME_CHARS`] tiles.
pub fn check_name_with_table(name: &str, table: &crate::table_file::Table) -> anyhow::Result<(String, usize)> {
    let trimmed = name.trim().to_string();
    anyhow::ensure!(!trimmed.is_empty(), "name is empty");
    let bytes = table.encode(&trimmed);
    anyhow::ensure!(
        !bytes.is_empty(),
        "name has no characters mapped by the table file (unmapped characters are skipped)"
    );
    anyhow::ensure!(
        bytes.len() <= MAX_NAME_CHARS,
        "name encodes to {} tiles; the game draws at most {}",
        bytes.len(),
        MAX_NAME_CHARS
    );
    Ok((trimmed, bytes.len()))
}

/// Encode a character to an overworld-name tile value.
///
/// Uppercase ASCII letters map to tiles `$00-$19`, space to `$1F`, `#` to
/// `$5A`, `'` to `$5D`, and digits to `$64-$6D`. Anything else becomes a
/// space so the encoder never emits an undefined tile.
pub fn char_to_tile(c: char) -> u8 {
    match c {
        'A'..='Z' => (c as u8) - b'A',
        'a'..='z' => (c as u8) - b'a',
        ' ' => TILE_SPACE,
        '#' => 0x5A,
        '\'' => 0x5D,
        '1'..='9' => 0x64 + (c as u8 - b'1'),
        '0' => 0x6D,
        _ => TILE_SPACE,
    }
}

/// Decode an overworld-name tile value (bit 7 already masked) to a character.
///
/// Covers the standard tiles plus the alternate encodings the vanilla ROM
/// actually uses (`$32-$37` for `" ILLUS"`, `$38-$3C` for `"YELLO"`, `$1C`
/// for `L` in `"CHOCOLATE"`). Unknown tiles decode as `'?'`.
///
/// Note: this is the single-character view. The squished tiles `$34`/`$3A`
/// actually contain "LL" (two characters); use [`decode_tile_str`] for the
/// MultiChar-aware decoding.
pub fn tile_to_char(tile: u8) -> char {
    match tile {
        0x00..=0x19 => (b'A' + tile) as char,
        0x1F | 0x32 => ' ',
        0x5A => '#',
        0x5D => '\'',
        0x64..=0x6C => (b'1' + (tile - 0x64)) as char,
        0x6D => '0',
        // Vanilla alternate encodings (see module docs).
        0x33 => 'I',
        0x34 | 0x35 => 'L',
        0x36 => 'U',
        0x37 => 'S',
        0x38 => 'Y',
        0x39 => 'E',
        0x3A | 0x3B => 'L',
        0x3C => 'O',
        0x1C => 'L',
        _ => '?',
    }
}

/// Decode a single name tile (bit 7 already masked) to its display string.
///
/// When `use_multichar` is on (Lunar Magic v3.40's "Use MultiChar Tiles",
/// on by default), squished tiles decode to their character strings (`$3A`
/// → `"LL"`). When off, squished tiles decode to `\XX` hex escapes, matching
/// LM's behavior ("existing entries that use them will just be displayed in
/// the editor using hex escape sequences").
pub fn decode_tile_str(tile: u8, use_multichar: bool) -> String {
    if let Some(s) = squished_tile_str(tile) {
        if use_multichar {
            return s.to_string();
        } else {
            return format!("\\{tile:02X}");
        }
    }
    tile_to_char(tile).to_string()
}

// ── Patch detection ─────────────────────────────────────────────────────────

/// Parse a `\XX` hex escape at the start of `s` (after the backslash).
/// Returns the tile byte and the number of chars consumed from `s` (2).
fn parse_hex_escape(s: &str) -> Option<(u8, usize)> {
    let hex: String = s.chars().take(2).collect();
    if hex.len() == 2 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        u8::from_str_radix(&hex, 16).ok().map(|b| (b, 2))
    } else {
        None
    }
}

/// Encode a name string to tile bytes.
///
/// The string may contain `\XX` hex escapes (backslash + 2 hex digits) for
/// specific tile values — Lunar Magic's "Edit Level Names" dialog uses the
/// same escape syntax for tiles with no keystroke representation.
///
/// When `use_multichar` is on, the encoder greedily matches the longest
/// squished-tile strings first (so `"LL"` → `$3A`, `"YELLOW"` → `$38-$3C`
/// via the individual tile mappings); otherwise every character maps through
/// [`char_to_tile`] and squished tiles are never emitted.
pub fn encode_name_to_tiles(name: &str, use_multichar: bool) -> anyhow::Result<Vec<u8>> {
    let mut tiles = Vec::new();
    let chars: Vec<char> = name.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // Hex escape: \XX
        if chars[i] == '\\' {
            let rest: String = chars[i + 1..].iter().collect();
            match parse_hex_escape(&rest) {
                Some((b, _)) => {
                    tiles.push(b);
                    i += 3; // backslash + 2 hex digits
                    continue;
                }
                None => anyhow::bail!("invalid hex escape at position {i} (expected \\XX)"),
            }
        }
        // MultiChar: longest squished-tile string match first.
        if use_multichar {
            // Try 2-char matches ("LL"), then single-char squished tiles.
            let mut matched = false;
            // Check 2-char sequences first.
            if i + 1 < chars.len() {
                let pair: String = [chars[i], chars[i + 1]].iter().collect();
                let pair_upper = pair.to_uppercase();
                // Prefer $3A for "LL" (vanilla "YELLOW" usage); $34 is the
                // "ILLUSION" variant. Both decode to "LL".
                if pair_upper == "LL" {
                    tiles.push(0x3A);
                    i += 2;
                    matched = true;
                }
            }
            if !matched {
                // Single squished char? (e.g., typed "Y" could be $38 or $18;
                // prefer the standard tile for single chars to avoid
                // surprising substitutions — squished singles are only used
                // when explicitly escaped or via the full-name patterns below.)
                //
                // Actually, for LM parity ("automatically be used when making
                // new entries"), we DO want auto-substitution. But to avoid
                // over-substitution, we only auto-use the multi-char "LL"
                // tile. Single squished tiles ($32-$33 etc.) are available via
                // \XX escapes. This matches the vanilla ROM, where only "LL"
                // is a true 2-char tile; the others are stylistic variants.
                tiles.push(char_to_tile(chars[i]));
                i += 1;
            }
        } else {
            tiles.push(char_to_tile(chars[i]));
            i += 1;
        }
    }
    Ok(tiles)
}

/// Count the tiles a name string encodes to (for the 19-tile budget).
/// Hex escapes count as one tile; "LL" counts as one tile when `use_multichar`.
pub fn count_name_tiles(name: &str, use_multichar: bool) -> usize {
    match encode_name_to_tiles(name, use_multichar) {
        Ok(tiles) => tiles.len(),
        Err(_) => name.chars().count(), // fallback for invalid escapes
    }
}

/// Decode an overworld-name tile value (bit 7 already masked) to a character.
///
/// Covers the standard tiles plus the alternate encodings the vanilla ROM
/// Returns true if the level-name table-relocation patch is applied to the ROM.
pub fn is_patch_applied(rom: &[u8], header_offset: usize) -> bool {
    PATCH_SITES.iter().all(|(pc, base)| {
        let i = *pc as usize + header_offset;
        rom.get(i..i + 3).map(|b| b == [0xB9, (base & 0xFF) as u8, (base >> 8) as u8]).unwrap_or(false)
    })
}

/// Returns true if the ROM has vanilla (unpatched) level-name tables.
pub fn is_vanilla(rom: &[u8], header_offset: usize) -> bool {
    VANILLA_SITE_BYTES.iter().all(|(pc, bytes)| {
        let i = *pc as usize + header_offset;
        rom.get(i..i + 3).map(|b| b == bytes).unwrap_or(false)
    })
}

// ── Decoding ────────────────────────────────────────────────────────────────

/// Decode one fragment string at a pool-relative byte offset.
fn decode_fragment(rom: &[u8], strings_pc: usize, offset: usize, use_multichar: bool) -> String {
    let mut s = String::new();
    let mut i = strings_pc + offset;
    loop {
        let b = match rom.get(i) {
            Some(&b) => b,
            None => break,
        };
        // A first byte with bit 7 set and value $80 means "empty fragment".
        // (T1 skip.) Don't emit it.
        if s.is_empty() && b == T1_SKIP_BYTE {
            break;
        }
        s.push_str(&decode_tile_str(b & 0x7F, use_multichar));
        i += 1;
        if b & 0x80 != 0 {
            break;
        }
        // Safety cap: fragments are short; bail on corrupt data.
        if s.len() > 64 {
            break;
        }
    }
    s
}

/// Decode the level name for a translevel (0..93).
///
/// `patched` selects the relocated (post-patch) or vanilla table addresses.
/// When `use_multichar` is on, squished tiles decode to their character
/// strings (`$3A` → `"LL"`); when off, they decode to `\XX` hex escapes
/// (Lunar Magic v3.40's "Use MultiChar Tiles" option).
/// Returns `None` if the translevel is out of range or the ROM is truncated.
pub fn decode_name(
    rom: &[u8], header_offset: usize, translevel: usize, patched: bool, use_multichar: bool,
) -> Option<String> {
    if translevel >= LEVEL_NAMES_COUNT {
        return None;
    }
    let (t1_base, t2_base, t3_base) = if patched {
        (T1_PATCHED_SNES, T2_PATCHED_SNES, T3_PATCHED_SNES)
    } else {
        (T1_VANILLA_SNES, T2_VANILLA_SNES, T3_VANILLA_SNES)
    };
    let t1_pc = AddrPc::try_from_lorom(t1_base).ok()?.as_index() as usize + header_offset;
    let t2_pc = AddrPc::try_from_lorom(t2_base).ok()?.as_index() as usize + header_offset;
    let t3_pc = AddrPc::try_from_lorom(t3_base).ok()?.as_index() as usize + header_offset;
    let names_pc = AddrPc::try_from_lorom(LEVEL_NAMES_SNES).ok()?.as_index() as usize + header_offset;
    let strings_pc = AddrPc::try_from_lorom(STRINGS_SNES).ok()?.as_index() as usize + header_offset;

    let e = names_pc + translevel * 2;
    let entry = u16::from_le_bytes([*rom.get(e)?, *rom.get(e + 1)?]);
    let lo = (entry & 0xFF) as usize;
    let hi = (entry >> 8) as usize;

    let mut name = String::new();

    // Piece 1 (T1): skipped if the fragment starts with a bit-7 byte.
    let t1_off =
        u16::from_le_bytes([*rom.get(t1_pc + (hi & 0x7F) * 2)?, *rom.get(t1_pc + (hi & 0x7F) * 2 + 1)?]) as usize;
    if rom.get(strings_pc + t1_off).copied().unwrap_or(0x80) & 0x80 == 0 {
        name.push_str(&decode_fragment(rom, strings_pc, t1_off, use_multichar));
    }

    // Piece 2 (T2): skipped if the fragment is exactly $9F.
    let t2_off =
        u16::from_le_bytes([*rom.get(t2_pc + ((lo >> 4) & 0xF) * 2)?, *rom.get(t2_pc + ((lo >> 4) & 0xF) * 2 + 1)?])
            as usize;
    if rom.get(strings_pc + t2_off).copied().unwrap_or(T2_SKIP_BYTE) != T2_SKIP_BYTE {
        name.push_str(&decode_fragment(rom, strings_pc, t2_off, use_multichar));
    }

    // Piece 3 (T3): always emitted.
    let t3_off =
        u16::from_le_bytes([*rom.get(t3_pc + (lo & 0xF) * 2)?, *rom.get(t3_pc + (lo & 0xF) * 2 + 1)?]) as usize;
    name.push_str(&decode_fragment(rom, strings_pc, t3_off, use_multichar));

    Some(name)
}

/// Decode all 93 level names.
///
/// When `use_multichar` is on, squished tiles decode to their character
/// strings; when off, to `\XX` hex escapes.
pub fn decode_all(rom: &[u8], header_offset: usize, patched: bool, use_multichar: bool) -> Option<Vec<String>> {
    (0..LEVEL_NAMES_COUNT).map(|t| decode_name(rom, header_offset, t, patched, use_multichar)).collect()
}

/// Raw tile bytes of one fragment: like [`decode_fragment`] but returns the
/// bytes (bit 7 stripped from the terminator) instead of mapped characters.
fn fragment_tiles(rom: &[u8], strings_pc: usize, offset: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = strings_pc + offset;
    loop {
        let b = match rom.get(i) {
            Some(&b) => b,
            None => break,
        };
        // A first byte with bit 7 set and value $80 means "empty fragment".
        // (T1 skip.) Don't emit it.
        if out.is_empty() && b == T1_SKIP_BYTE {
            break;
        }
        out.push(b & 0x7F);
        i += 1;
        if b & 0x80 != 0 {
            break;
        }
        // Safety cap: fragments are short; bail on corrupt data.
        if out.len() > 64 {
            break;
        }
    }
    out
}

/// Decode one level name to its raw tile bytes (bit 7 stripped, skip
/// fragments resolved exactly like [`decode_name`]: a T1 skip emits nothing,
/// a T2 skip emits one space tile).
///
/// This is the table-file entry point: the caller maps the bytes through a
/// [`crate::table_file::Table`] (or through [`tile_to_char`] to reproduce
/// [`decode_name`] exactly — asserted by the real-ROM test below).
pub fn decode_name_tiles(rom: &[u8], header_offset: usize, translevel: usize, patched: bool) -> Option<Vec<u8>> {
    if translevel >= LEVEL_NAMES_COUNT {
        return None;
    }
    let (t1_base, t2_base, t3_base) = if patched {
        (T1_PATCHED_SNES, T2_PATCHED_SNES, T3_PATCHED_SNES)
    } else {
        (T1_VANILLA_SNES, T2_VANILLA_SNES, T3_VANILLA_SNES)
    };
    let t1_pc = AddrPc::try_from_lorom(t1_base).ok()?.as_index() as usize + header_offset;
    let t2_pc = AddrPc::try_from_lorom(t2_base).ok()?.as_index() as usize + header_offset;
    let t3_pc = AddrPc::try_from_lorom(t3_base).ok()?.as_index() as usize + header_offset;
    let names_pc = AddrPc::try_from_lorom(LEVEL_NAMES_SNES).ok()?.as_index() as usize + header_offset;
    let strings_pc = AddrPc::try_from_lorom(STRINGS_SNES).ok()?.as_index() as usize + header_offset;

    let e = names_pc + translevel * 2;
    let entry = u16::from_le_bytes([*rom.get(e)?, *rom.get(e + 1)?]);
    let lo = (entry & 0xFF) as usize;
    let hi = (entry >> 8) as usize;

    let mut tiles = Vec::new();

    // Piece 1 (T1): skipped if the fragment starts with a bit-7 byte.
    let t1_off =
        u16::from_le_bytes([*rom.get(t1_pc + (hi & 0x7F) * 2)?, *rom.get(t1_pc + (hi & 0x7F) * 2 + 1)?]) as usize;
    if rom.get(strings_pc + t1_off).copied().unwrap_or(0x80) & 0x80 == 0 {
        tiles.extend(fragment_tiles(rom, strings_pc, t1_off));
    }

    // Piece 2 (T2): skipped if the fragment is exactly $9F.
    let t2_off =
        u16::from_le_bytes([*rom.get(t2_pc + ((lo >> 4) & 0xF) * 2)?, *rom.get(t2_pc + ((lo >> 4) & 0xF) * 2 + 1)?])
            as usize;
    if rom.get(strings_pc + t2_off).copied().unwrap_or(T2_SKIP_BYTE) != T2_SKIP_BYTE {
        tiles.extend(fragment_tiles(rom, strings_pc, t2_off));
    }

    // Piece 3 (T3): always emitted.
    let t3_off =
        u16::from_le_bytes([*rom.get(t3_pc + (lo & 0xF) * 2)?, *rom.get(t3_pc + (lo & 0xF) * 2 + 1)?]) as usize;
    tiles.extend(fragment_tiles(rom, strings_pc, t3_off));

    Some(tiles)
}

/// Decode all 93 level names through a Lunar Magic v3.40 custom table file
/// ([`crate::table_file::Table`]) instead of the built-in tile map.
pub fn decode_all_with_table(
    rom: &[u8], header_offset: usize, patched: bool, table: &crate::table_file::Table,
) -> Option<Vec<String>> {
    (0..LEVEL_NAMES_COUNT)
        .map(|t| decode_name_tiles(rom, header_offset, t, patched).map(|tiles| table.decode(&tiles)))
        .collect()
}

// ── Encoding ────────────────────────────────────────────────────────────────

/// A name split into up to three fragments.
#[derive(Debug, Clone)]
struct Split {
    p1: Option<String>,
    p2: Option<String>,
    p3: String,
}

/// Split a name into (prefix, middle, suffix) at word boundaries.
///
/// - 1 word: everything in `p3`.
/// - 2 words: `p1` = first word + space, `p3` = second word.
/// - 3+ words: `p1` = first word + space, `p2` = middle words + space,
///   `p3` = last word.
fn split_name(name: &str) -> Split {
    let words: Vec<&str> = name.split_whitespace().collect();
    match words.len() {
        0 => Split { p1: None, p2: None, p3: " ".to_string() },
        1 => Split { p1: None, p2: None, p3: words[0].to_string() },
        2 => Split { p1: Some(format!("{} ", words[0])), p2: None, p3: words[1].to_string() },
        _ => Split {
            p1: Some(format!("{} ", words[0])),
            p2: Some(format!("{} ", words[1..words.len() - 1].join(" "))),
            p3: words[words.len() - 1].to_string(),
        },
    }
}

/// Finish a fragment's tile bytes: bit 7 on the last byte (the fragment
/// terminator the game scans for). An empty byte vec encodes as the T2 skip
/// byte, matching the historical behavior for empty fragments.
fn finish_fragment(tiles: Vec<u8>) -> Vec<u8> {
    if tiles.is_empty() {
        return vec![T2_SKIP_BYTE];
    }
    let mut out = tiles;
    let last = out.len() - 1;
    out[last] |= 0x80;
    out
}

/// Encode a fragment to pool bytes (tiles, bit 7 on the last byte).
fn encode_fragment(text: &str, use_multichar: bool) -> Vec<u8> {
    finish_fragment(encode_name_to_tiles(text, use_multichar).unwrap_or_default())
}

/// The encoded pool + tables + `LevelNames` entries, ready to write.
pub struct EncodedNames {
    /// Pool bytes (fragments concatenated).
    pub pool:    Vec<u8>,
    /// T1 offsets (pool-relative).
    pub t1:      Vec<u16>,
    /// T2 offsets (pool-relative).
    pub t2:      Vec<u16>,
    /// T3 offsets (pool-relative).
    pub t3:      Vec<u16>,
    /// `LevelNames` entries (93 × u16).
    pub entries: Vec<u16>,
}

/// Count distinct strings in an iterator.
fn count_distinct<'a, I>(iter: I) -> usize
where
    I: Iterator<Item = &'a String>,
{
    let mut seen = std::collections::HashSet::new();
    for s in iter {
        seen.insert(s);
    }
    seen.len()
}

/// Merge the rarest T2 fragment into T1.
///
/// For each name using the rarest T2 piece, prepend it to T1 (or create T1)
/// and clear T2. Returns false if no T2 pieces exist.
fn merge_rarest_t2(splits: &mut [Split]) -> bool {
    // Find the least frequent T2 piece.
    let mut freq = std::collections::HashMap::new();
    for s in splits.iter() {
        if let Some(p2) = &s.p2 {
            *freq.entry(p2.clone()).or_insert(0) += 1;
        }
    }
    let rarest = match freq.iter().min_by_key(|(_, &c)| c) {
        Some((p, _)) => p.clone(),
        None => return false,
    };
    // Merge it into T1 for all names that use it.
    for s in splits.iter_mut() {
        if s.p2.as_ref() == Some(&rarest) {
            let merged = match &s.p1 {
                Some(p1) => format!("{p1}{rarest}"),
                None => rarest.clone(),
            };
            s.p1 = Some(merged);
            s.p2 = None;
        }
    }
    true
}

/// Merge the rarest T3 fragment into T1.
///
/// For each name using the rarest T3 piece, append the T1+T2 content into a
/// single T1 piece and move the T3 content... actually, we merge by extending
/// T1 to include everything except we need a T3. Instead, we prepend T1+T2
/// into a new longer T1 and keep T3. Wait — simpler: if a T3 piece is rare,
/// we can move the whole name into T1 and use a blank T3.
///
/// Actually, the cleanest: for names with rare T3, set T1 = full name,
/// T2 = None, T3 = " " (blank).
fn merge_rarest_t3(splits: &mut [Split]) -> bool {
    let mut freq = std::collections::HashMap::new();
    for s in splits.iter() {
        *freq.entry(s.p3.clone()).or_insert(0) += 1;
    }
    let rarest = match freq.iter().min_by_key(|(_, &c)| c) {
        Some((p, _)) => p.clone(),
        None => return false,
    };
    // Don't merge the blank " " piece; it's the default.
    if rarest == " " {
        // Find the next rarest non-blank.
        let mut sorted: Vec<_> = freq.iter().collect();
        sorted.sort_by_key(|(_, &c)| c);
        let rarest = match sorted.iter().find(|(p, _)| *p != " ") {
            Some((p, _)) => (*p).clone(),
            None => return false,
        };
        for s in splits.iter_mut() {
            if s.p3 == rarest {
                let full = format!("{}{}{}", s.p1.as_deref().unwrap_or(""), s.p2.as_deref().unwrap_or(""), s.p3);
                s.p1 = Some(full);
                s.p2 = None;
                s.p3 = " ".to_string();
            }
        }
    } else {
        for s in splits.iter_mut() {
            if s.p3 == rarest {
                let full = format!("{}{}{}", s.p1.as_deref().unwrap_or(""), s.p2.as_deref().unwrap_or(""), s.p3);
                s.p1 = Some(full);
                s.p2 = None;
                s.p3 = " ".to_string();
            }
        }
    }
    true
}

/// Encode 93 level names into the patched pool/table format.
///
/// Names are uppercased and split into shared (prefix, middle, suffix)
/// fragments. When `use_multichar` is on, "LL" encodes to the squished `$3A`
/// tile (Lunar Magic v3.40's "Use MultiChar Tiles"); otherwise all characters
/// map through [`char_to_tile`]. Names may contain `\XX` hex escapes for
/// specific tile values.
/// Returns an error if the fragments don't fit the patched pool
/// (578 bytes) or table slot counts (93/16/16).
pub fn encode_names(names: &[String], use_multichar: bool) -> anyhow::Result<EncodedNames> {
    encode_names_inner(names, true, use_multichar, None)
}

/// Encode all 93 level names through a Lunar Magic v3.40 custom table file
/// ([`crate::table_file::Table`]) instead of the built-in tile map.
///
/// Same fragment/patch machinery as [`encode_names`], but the display text
/// is encoded with the table (no uppercasing — the table defines the
/// charset; MultiChar squishing does not apply) and an all-unmapped fragment
/// falls back to the structural skip byte.
pub fn encode_names_with_table(names: &[String], table: &crate::table_file::Table) -> anyhow::Result<EncodedNames> {
    encode_names_inner(names, false, false, Some(table))
}

/// Shared encoder: `uppercase` selects built-in-map normalization,
/// `use_multichar` selects MultiChar squished-tile encoding for the
/// built-in map, and `table` switches the fragment codec to the custom
/// table file.
fn encode_names_inner(
    names: &[String], uppercase: bool, use_multichar: bool, table: Option<&crate::table_file::Table>,
) -> anyhow::Result<EncodedNames> {
    anyhow::ensure!(names.len() == LEVEL_NAMES_COUNT, "need exactly {} names, got {}", LEVEL_NAMES_COUNT, names.len());

    // Normalize: collapse whitespace (and uppercase for the built-in map —
    // the game only has uppercase glyphs; a custom table defines its own
    // charset so the text is left as typed).
    let normalized: Vec<String> = names
        .iter()
        .map(|n| {
            let collapsed = n.split_whitespace().collect::<Vec<_>>().join(" ");
            if uppercase {
                collapsed.to_uppercase()
            } else {
                collapsed
            }
        })
        .collect();
    let mut splits: Vec<Split> = normalized.iter().map(|n| split_name(n)).collect();

    // Merge rare T2/T3 pieces into T1 until we fit the slot caps.
    // T1 has 93 slots, so it absorbs the overflow.
    // Reserve 1 slot each for T1/T2 skips (p1/p2 can be None); T3 needs no skip.
    const T2_TARGET: usize = T2_PATCHED_SLOTS - 1;
    const T3_TARGET: usize = T3_PATCHED_SLOTS;
    for _ in 0..100 {
        let t2_count = count_distinct(splits.iter().filter_map(|s| s.p2.as_ref()));
        let t3_count = count_distinct(splits.iter().map(|s| &s.p3));
        if t2_count <= T2_TARGET && t3_count <= T3_TARGET {
            break;
        }
        // Find the least frequent piece in the overfull table.
        if t2_count > T2_TARGET {
            if !merge_rarest_t2(&mut splits) {
                break;
            }
        } else if t3_count > T3_TARGET {
            if !merge_rarest_t3(&mut splits) {
                break;
            }
        }
    }

    // Deduplicate fragments, preserving first-seen order.
    let mut t1_list: Vec<String> = Vec::new();
    let mut t2_list: Vec<String> = Vec::new();
    let mut t3_list: Vec<String> = Vec::new();
    for s in &splits {
        if let Some(p1) = &s.p1 {
            if !t1_list.contains(p1) {
                t1_list.push(p1.clone());
            }
        }
        if let Some(p2) = &s.p2 {
            if !t2_list.contains(p2) {
                t2_list.push(p2.clone());
            }
        }
        if !t3_list.contains(&s.p3) {
            t3_list.push(s.p3.clone());
        }
    }

    anyhow::ensure!(
        t1_list.len() <= T1_PATCHED_SLOTS,
        "too many distinct first fragments ({} > {})",
        t1_list.len(),
        T1_PATCHED_SLOTS
    );
    anyhow::ensure!(
        t2_list.len() <= T2_PATCHED_SLOTS,
        "too many distinct middle fragments ({} > {})",
        t2_list.len(),
        T2_PATCHED_SLOTS
    );
    anyhow::ensure!(
        t3_list.len() <= T3_PATCHED_SLOTS,
        "too many distinct last fragments ({} > {})",
        t3_list.len(),
        T3_PATCHED_SLOTS
    );

    // Lay out the pool: T1 fragments, T2 fragments, T3 fragments,
    // then the two skip fragments.
    let mut pool: Vec<u8> = Vec::new();
    let mut t1_off = Vec::new();
    let mut t2_off = Vec::new();
    let mut t3_off = Vec::new();
    // Fragment codec: the built-in tile map (with MultiChar squishing when
    // enabled) or the custom table file's encoder. Both return finished
    // pool bytes (bit 7 on the last byte).
    let fragment_tiles = |p: &str| -> Vec<u8> {
        match table {
            Some(t) => finish_fragment(t.encode(p)),
            None => encode_fragment(p, use_multichar),
        }
    };
    for p in &t1_list {
        t1_off.push(pool.len() as u16);
        pool.extend_from_slice(&fragment_tiles(p));
    }
    for p in &t2_list {
        t2_off.push(pool.len() as u16);
        pool.extend_from_slice(&fragment_tiles(p));
    }
    for p in &t3_list {
        t3_off.push(pool.len() as u16);
        pool.extend_from_slice(&fragment_tiles(p));
    }
    // Skip fragments: T1 skip ($80) and T2 skip ($9F).
    // Only add them if actually needed (some name has p1/p2 == None).
    let need_t1_skip = splits.iter().any(|s| s.p1.is_none());
    let need_t2_skip = splits.iter().any(|s| s.p2.is_none());
    let t1_skip_idx = if need_t1_skip {
        let idx = t1_off.len();
        t1_off.push(pool.len() as u16);
        pool.push(T1_SKIP_BYTE);
        idx
    } else {
        usize::MAX
    };
    let t2_skip_idx = if need_t2_skip {
        let idx = t2_off.len();
        t2_off.push(pool.len() as u16);
        pool.push(T2_SKIP_BYTE);
        idx
    } else {
        usize::MAX
    };

    anyhow::ensure!(t1_off.len() <= T1_PATCHED_SLOTS, "T1 table overflow ({} > {})", t1_off.len(), T1_PATCHED_SLOTS);
    anyhow::ensure!(t2_off.len() <= T2_PATCHED_SLOTS, "T2 table overflow ({} > {})", t2_off.len(), T2_PATCHED_SLOTS);

    let index_of = |list: &[String], p: &Option<String>, skip_idx: usize| -> usize {
        match p {
            Some(text) => list.iter().position(|x| x == text).expect("fragment in list"),
            None => skip_idx,
        }
    };

    let mut entries = Vec::with_capacity(LEVEL_NAMES_COUNT);
    for s in &splits {
        let a = index_of(&t1_list, &s.p1, t1_skip_idx);
        let b = index_of(&t2_list, &s.p2, t2_skip_idx);
        let c = t3_list.iter().position(|x| *x == s.p3).expect("p3 in list");
        anyhow::ensure!(a < 128, "T1 index out of range");
        anyhow::ensure!(b < 16, "T2 index out of range");
        anyhow::ensure!(c < 16, "T3 index out of range");
        entries.push(((a as u16) << 8) | ((b as u16) << 4) | (c as u16));
    }

    anyhow::ensure!(
        pool.len() <= STRINGS_PATCHED_LEN,
        "encoded names need {} pool bytes, only {} available",
        pool.len(),
        STRINGS_PATCHED_LEN
    );

    Ok(EncodedNames { pool, t1: t1_off, t2: t2_off, t3: t3_off, entries })
}

// ── Applying to the ROM ─────────────────────────────────────────────────────

/// Apply the table-relocation patch and write encoded names to the ROM.
///
/// `rom` is the full ROM image; `header_offset` is 0 or 512. This:
/// 1. Rewrites the three `LDA.W` bases in `CODE_049D07`.
/// 2. Writes `enc`'s pool, tables, and `LevelNames` entries.
///
/// If the patch is already applied, the code bytes are left alone (the write
/// is idempotent).
pub fn apply_to_rom(rom: &mut [u8], header_offset: usize, enc: &EncodedNames) -> anyhow::Result<()> {
    // 1. Patch the code.
    for (pc, base) in PATCH_SITES {
        let i = pc as usize + header_offset;
        let bytes = rom.get_mut(i..i + 3).ok_or_else(|| anyhow::anyhow!("ROM truncated at patch site {:#08X}", pc))?;
        bytes[0] = 0xB9;
        bytes[1] = (base & 0xFF) as u8;
        bytes[2] = (base >> 8) as u8;
    }

    // 2. Write the pool.
    let strings_pc =
        AddrPc::try_from_lorom(STRINGS_SNES).map_err(|e| anyhow::anyhow!("{e:?}"))?.as_index() as usize + header_offset;
    let pool_end = strings_pc + enc.pool.len();
    rom.get_mut(strings_pc..pool_end)
        .ok_or_else(|| anyhow::anyhow!("ROM truncated in string pool"))?
        .copy_from_slice(&enc.pool);
    // Zero the rest of the patched pool region.
    let pool_cap = strings_pc + STRINGS_PATCHED_LEN;
    for b in rom.get_mut(pool_end..pool_cap).ok_or_else(|| anyhow::anyhow!("ROM truncated in string pool"))? {
        *b = 0x00;
    }

    // 3. Write the tables.
    let write_table = |rom: &mut [u8], base: AddrSnes, offs: &[u16]| -> anyhow::Result<()> {
        let pc =
            AddrPc::try_from_lorom(base).map_err(|e| anyhow::anyhow!("{e:?}"))?.as_index() as usize + header_offset;
        for (i, off) in offs.iter().enumerate() {
            let j = pc + i * 2;
            let slot = rom.get_mut(j..j + 2).ok_or_else(|| anyhow::anyhow!("ROM truncated in fragment table"))?;
            slot.copy_from_slice(&off.to_le_bytes());
        }
        Ok(())
    };
    write_table(rom, T1_PATCHED_SNES, &enc.t1)?;
    write_table(rom, T2_PATCHED_SNES, &enc.t2)?;
    write_table(rom, T3_PATCHED_SNES, &enc.t3)?;

    // 4. Write LevelNames entries.
    let names_pc = AddrPc::try_from_lorom(LEVEL_NAMES_SNES).map_err(|e| anyhow::anyhow!("{e:?}"))?.as_index() as usize
        + header_offset;
    for (i, e) in enc.entries.iter().enumerate() {
        let j = names_pc + i * 2;
        rom.get_mut(j..j + 2)
            .ok_or_else(|| anyhow::anyhow!("ROM truncated in LevelNames"))?
            .copy_from_slice(&e.to_le_bytes());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic fixtures use invented tile values; they prove the split and
    /// encode logic, not the real SMW font.
    #[test]
    fn split_names() {
        let s = split_name("DONUT PLAINS 1");
        assert_eq!(s.p1.as_deref(), Some("DONUT "));
        assert_eq!(s.p2.as_deref(), Some("PLAINS "));
        assert_eq!(s.p3, "1");

        let s = split_name("FUNKY");
        assert_eq!(s.p1, None);
        assert_eq!(s.p2, None);
        assert_eq!(s.p3, "FUNKY");

        let s = split_name("YOSHI'S ISLAND 2");
        assert_eq!(s.p1.as_deref(), Some("YOSHI'S "));
        assert_eq!(s.p2.as_deref(), Some("ISLAND "));
        assert_eq!(s.p3, "2");
    }

    #[test]
    fn char_round_trip() {
        for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ #'1234567890".chars() {
            let tile = char_to_tile(c);
            let back = tile_to_char(tile);
            assert_eq!(back, c, "round trip failed for {c}");
        }
    }

    #[test]
    fn check_name_enforces_budget_and_charset() {
        // Normalization: trim + uppercase.
        assert_eq!(check_name("  yoshi's hideout ").unwrap(), "YOSHI'S HIDEOUT");
        // Exactly at the budget is fine.
        assert!(check_name(&"A".repeat(MAX_NAME_CHARS)).is_ok());
        // One over is refused, not truncated.
        let err = check_name(&"A".repeat(MAX_NAME_CHARS + 1)).unwrap_err();
        assert!(err.to_string().contains("at most 19"), "unexpected error: {err}");
        // Unknown characters are refused.
        let err = check_name("DONUT-PLAINS").unwrap_err();
        assert!(err.to_string().contains("has no overworld-name tile"), "unexpected error: {err}");
        // Empty is refused.
        assert!(check_name("   ").is_err());
        // The allowed set passes.
        assert_eq!(check_name("#7 LARRY'S CASTLE 9").unwrap(), "#7 LARRY'S CASTLE 9");
    }

    #[test]
    fn encode_small_set() {
        // 93 names required; pad with blanks.
        let mut names = vec![" ".to_string(); LEVEL_NAMES_COUNT];
        names[0] = "DONUT PLAINS 1".to_string();
        names[1] = "DONUT PLAINS 2".to_string();
        names[2] = "FUNKY".to_string();
        let enc = encode_names(&names, true).unwrap();
        // "DONUT " shared, "PLAINS " shared, "1"/"2"/"FUNKY" distinct.
        assert!(enc.pool.len() < STRINGS_PATCHED_LEN);
        assert_eq!(enc.entries.len(), LEVEL_NAMES_COUNT);
    }

    /// Build a small test table: A->0x41, B->0x42, space->0x1F, plus a
    /// MultiTile entry like LM's own YELLOW example.
    fn test_table() -> crate::table_file::Table {
        let (file, warnings) = crate::table_file::parse_lmtbl("@LevelNames\n41=A\n42=B\n1F= \n4342=CB\n").unwrap();
        assert!(warnings.is_empty());
        file.table_for(crate::table_file::TableDialog::LevelNames).unwrap().clone()
    }

    #[test]
    fn check_name_with_table_uses_table_bytes_for_budget() {
        let t = test_table();
        // Trimmed, NOT uppercased (the table defines the charset).
        let (normalized, tiles) = check_name_with_table("  AB ", &t).unwrap();
        assert_eq!(normalized, "AB");
        assert_eq!(tiles, 2);
        // Unmapped characters are skipped by the encoder (LM behavior), so
        // they don't count toward the budget.
        let (normalized, tiles) = check_name_with_table("AZB", &t).unwrap();
        assert_eq!(normalized, "AZB");
        assert_eq!(tiles, 2);
        // Budget is in encoded tiles: MultiTile "CB" is one entry but two tiles.
        let long = "CB".repeat(MAX_NAME_CHARS / 2 + 1);
        assert!(check_name_with_table(&long, &t).is_err());
        // All-unmapped encodes to nothing: refused.
        assert!(check_name_with_table("zzz", &t).is_err());
        // Empty is refused.
        assert!(check_name_with_table("   ", &t).is_err());
    }

    #[test]
    fn encode_names_with_table_emits_table_bytes() {
        let t = test_table();
        let mut names = vec![" ".to_string(); LEVEL_NAMES_COUNT];
        names[0] = "AB".to_string();
        names[1] = "CB".to_string();
        let enc = encode_names_with_table(&names, &t).unwrap();
        assert_eq!(enc.entries.len(), LEVEL_NAMES_COUNT);
        // The pool must contain the table's byte values (0x41/0x42 for
        // "AB", 0x43 0x42 for the MultiTile "CB"), not the built-in
        // 0x00/0x01 tiles.
        let pool_hex: Vec<u8> = enc.pool.clone();
        assert!(pool_hex.windows(2).any(|w| w == [0x41, 0xC2]), "pool lacks table-encoded AB: {pool_hex:02X?}");
        assert!(pool_hex.windows(2).any(|w| w == [0x43, 0xC2]), "pool lacks table-encoded CB: {pool_hex:02X?}");
        assert!(!pool_hex.contains(&0x00), "pool should not contain built-in tile 0x00");
    }

    #[test]
    fn encode_names_with_table_matches_builtin_without_table() {
        // A table that reproduces the built-in A-Z/space mapping must
        // produce the same pool bytes as encode_names.
        let mut tbl_src = String::new();
        for (i, c) in ('A'..='Z').enumerate() {
            tbl_src.push_str(&format!("{i:02X}={c}\n"));
        }
        tbl_src.push_str("1F= \n");
        let (file, _) = crate::table_file::parse_lmtbl(&tbl_src).unwrap();
        let t = file.global.unwrap();
        let mut names = vec![" ".to_string(); LEVEL_NAMES_COUNT];
        names[0] = "DONUT PLAINS 1".to_string();
        names[1] = "YOSHI'S ISLAND".to_string();
        // Note: apostrophe is unmapped in this table, so it is skipped;
        // compare against the built-in encode of the skipped form.
        let builtin = encode_names(&names, true).unwrap();
        let via_table = encode_names_with_table(&names, &t).unwrap();
        // "YOSHI'S ISLAND" -> "YOSHIS ISLAND" under the table (apostrophe
        // skipped): the table pool has no 0x5D apostrophe tile, the
        // built-in one does. Everything else encodes identically.
        assert!(builtin.pool.contains(&0x5D), "built-in pool should contain the apostrophe tile");
        assert!(!via_table.pool.contains(&0x5D), "table pool must not contain the skipped apostrophe tile");
        assert_eq!(builtin.entries.len(), via_table.entries.len());
        assert!(via_table.pool.len() <= STRINGS_PATCHED_LEN);
    }

    /// Real-ROM test: `decode_name_tiles` mapped through `tile_to_char` must
    /// reproduce `decode_name` exactly for all 93 names (guards the
    /// table-file decode path against the long-standing decoder).
    /// Run with: `ROM_PATH=/path/to/smw.smc cargo test -p smwe-rom --lib -- --ignored`
    #[test]
    #[ignore]
    fn real_rom_name_tiles_match_decode_name() {
        let path = std::env::var("ROM_PATH").expect("ROM_PATH not set");
        let rom = std::fs::read(path).expect("can't read ROM");
        let header_offset = if rom.len() % 0x400 == 0x200 { 512 } else { 0 };

        for t in 0..LEVEL_NAMES_COUNT {
            let tiles = decode_name_tiles(&rom, header_offset, t, false).expect("tiles");
            let via_tiles: String = tiles.iter().map(|b| decode_tile_str(*b, true)).collect();
            let direct = decode_name(&rom, header_offset, t, false, true).expect("name");
            assert_eq!(via_tiles, direct, "translevel {t}: {via_tiles:?} vs {direct:?}");
        }
        println!("decode_name_tiles matches decode_name for all {LEVEL_NAMES_COUNT} names");
    }

    /// Real-ROM test: table round-trip of the vanilla names — decode all 93
    /// names through a table that mirrors the built-in map, then re-encode
    /// and verify the pool bytes are unchanged.
    #[test]
    #[ignore]
    fn real_rom_table_round_trip() {
        let path = std::env::var("ROM_PATH").expect("ROM_PATH not set");
        let rom = std::fs::read(path).expect("can't read ROM");
        let header_offset = if rom.len() % 0x400 == 0x200 { 512 } else { 0 };

        // Table mirroring the built-in map for A-Z, 0-9, space, '#', '\''.
        let mut src = String::new();
        for (i, c) in ('A'..='Z').enumerate() {
            src.push_str(&format!("{i:02X}={c}\n"));
        }
        for (i, c) in ('1'..='9').enumerate() {
            src.push_str(&format!("{:02X}={c}\n", 0x64 + i));
        }
        src.push_str("6D=0\n1F= \n5A=#\n5D='\n");
        let (file, warnings) = crate::table_file::parse_lmtbl(&src).unwrap();
        assert!(warnings.is_empty());
        let t = file.global.unwrap();

        let names = decode_all_with_table(&rom, header_offset, false, &t).expect("decode");
        assert_eq!(names.len(), LEVEL_NAMES_COUNT);
        // Spot-check: the table decode must show the real vanilla names.
        let joined: Vec<String> = names.iter().map(|n| n.trim().to_string()).collect();
        assert!(joined.iter().any(|n| n == "VANILLA SECRET 2"), "VANILLA SECRET 2");

        let enc = encode_names_with_table(&names, &t).expect("encode");
        assert!(enc.pool.len() <= STRINGS_PATCHED_LEN);
        println!("table round-trip: {} pool bytes", enc.pool.len());
    }

    /// Real-ROM test: decode all 93 vanilla names and re-encode them.
    /// Run with: `ROM_PATH=/path/to/smw.smc cargo test -p smwe-rom --lib -- --ignored`
    #[test]
    #[ignore]
    fn real_rom_vanilla_names() {
        let path = std::env::var("ROM_PATH").expect("ROM_PATH not set");
        let rom = std::fs::read(path).expect("can't read ROM");
        let header_offset = if rom.len() % 0x400 == 0x200 { 512 } else { 0 };

        assert!(is_vanilla(&rom, header_offset));
        assert!(!is_patch_applied(&rom, header_offset));

        let names = decode_all(&rom, header_offset, false, true).expect("decode");
        assert_eq!(names.len(), LEVEL_NAMES_COUNT);

        // Spot-check known vanilla names.
        let joined: Vec<String> = names.iter().map(|n| n.trim().to_string()).collect();
        assert!(joined.iter().any(|n| n == "VANILLA SECRET 2"), "VANILLA SECRET 2");
        assert!(joined.iter().any(|n| n == "DONUT GHOST HOUSE"), "DONUT GHOST HOUSE");
        assert!(joined.iter().any(|n| n == "GREEN SWITCH PALACE"), "GREEN SWITCH PALACE");
        assert!(joined.iter().any(|n| n.starts_with("YOSHI'S ISLAND")), "YOSHI'S ISLAND");
        assert!(joined.iter().any(|n| n == "FOREST OF ILLUSION 1" || n == "FOREST OF ILLUSON 1"));

        // Re-encode must fit the patched pool.
        let enc = encode_names(&names, true).expect("encode vanilla names");
        assert!(enc.pool.len() <= STRINGS_PATCHED_LEN, "pool {} > {}", enc.pool.len(), STRINGS_PATCHED_LEN);
        println!(
            "vanilla re-encode: {} pool bytes, T1 {}/93, T2 {}/31, T3 {}/31",
            enc.pool.len(),
            enc.t1.len(),
            enc.t2.len(),
            enc.t3.len()
        );
    }

    /// Real-ROM test: apply the patch to a copy and verify round-trip.
    #[test]
    #[ignore]
    fn real_rom_patch_round_trip() {
        let path = std::env::var("ROM_PATH").expect("ROM_PATH not set");
        let mut rom = std::fs::read(path).expect("can't read ROM");
        let header_offset = if rom.len() % 0x400 == 0x200 { 512 } else { 0 };

        let names = decode_all(&rom, header_offset, false, true).expect("decode");
        let enc = encode_names(&names, true).expect("encode");
        apply_to_rom(&mut rom, header_offset, &enc).expect("apply");

        assert!(is_patch_applied(&rom, header_offset));

        let back = decode_all(&rom, header_offset, true, true).expect("re-decode");
        for (a, b) in names.iter().zip(back.iter()) {
            assert_eq!(a.trim(), b.trim(), "round-trip mismatch: {a:?} vs {b:?}");
        }
    }

    #[test]
    fn multichar_decode_encode() {
        // Squished tiles decode to their strings when use_multichar is on.
        assert_eq!(decode_tile_str(0x3A, true), "LL");
        assert_eq!(decode_tile_str(0x34, true), "LL");
        assert_eq!(decode_tile_str(0x38, true), "Y");
        assert_eq!(decode_tile_str(0x32, true), " ");
        // When off, they decode to hex escapes (LM v3.40 behavior).
        assert_eq!(decode_tile_str(0x3A, false), "\\3A");
        assert_eq!(decode_tile_str(0x34, false), "\\34");
        assert_eq!(decode_tile_str(0x38, false), "\\38");
        // Normal tiles are unaffected.
        assert_eq!(decode_tile_str(0x00, true), "A");
        assert_eq!(decode_tile_str(0x00, false), "A");

        // "LL" encodes to the squished $3A tile when use_multichar is on.
        assert_eq!(encode_name_to_tiles("LL", true).unwrap(), vec![0x3A]);
        assert_eq!(encode_name_to_tiles("HELLO", true).unwrap(), vec![0x07, 0x04, 0x3A, 0x0E]);
        // When off, "LL" is two separate tiles.
        assert_eq!(encode_name_to_tiles("LL", false).unwrap(), vec![0x0B, 0x0B]);
        // Hex escapes work regardless.
        assert_eq!(encode_name_to_tiles("\\3A", true).unwrap(), vec![0x3A]);
        assert_eq!(encode_name_to_tiles("\\3A", false).unwrap(), vec![0x3A]);
        assert_eq!(encode_name_to_tiles("A\\3ABC", true).unwrap(), vec![0x00, 0x3A, 0x01, 0x02]);
        // Invalid escapes are rejected.
        assert!(encode_name_to_tiles("A\\ZZ", true).is_err());
        assert!(encode_name_to_tiles("A\\3", true).is_err());

        // Tile budget: "LL" is 1 tile with multichar, 2 without.
        assert_eq!(count_name_tiles("LL", true), 1);
        assert_eq!(count_name_tiles("LL", false), 2);
        assert_eq!(count_name_tiles("\\3A", true), 1);
    }

    #[test]
    fn check_name_multichar_budget() {
        // 20 chars in 19 tiles via the "LL" multichar tile is allowed.
        assert!(check_name_with("YELLOW SWITCH PALACE", true).is_ok());
        // Without multichar, it's 20 tiles — over budget.
        assert!(check_name_with("YELLOW SWITCH PALACE", false).is_err());
        // Escapes are allowed and count as tiles.
        assert!(check_name_with("A\\3A", true).is_ok());
    }

    /// Real-ROM test: vanilla squished names decode correctly with multichar on.
    #[test]
    #[ignore]
    fn real_rom_multichar_names() {
        let path = std::env::var("ROM_PATH").expect("ROM_PATH not set");
        let rom = std::fs::read(path).expect("can't read ROM");
        let header_offset = if rom.len() % 0x400 == 0x200 { 512 } else { 0 };

        let names = decode_all(&rom, header_offset, false, true).expect("decode");
        let joined: Vec<String> = names.iter().map(|n| n.trim().to_string()).collect();
        // "YELLOW SWITCH PALACE" (20 chars, 19 tiles via $3A="LL"). The ROM
        // stores 21 bytes; the game draws the first 19 tiles.
        assert!(
            joined.iter().any(|n| n.starts_with("YELLOW SWITCH PALACE")),
            "YELLOW SWITCH PALACE not found; got: {:?}",
            joined.iter().filter(|n| n.contains("YELLOW") || n.contains("YELLO")).collect::<Vec<_>>()
        );
        // "FOREST OF ILLUSION 1" (20 chars, 19 tiles via $34="LL").
        assert!(
            joined.iter().any(|n| n == "FOREST OF ILLUSION 1"),
            "FOREST OF ILLUSION 1 not found; got: {:?}",
            joined.iter().filter(|n| n.contains("ILLUSION") || n.contains("ILLUS")).collect::<Vec<_>>()
        );

        // With multichar off, squished tiles show as hex escapes.
        let names_off = decode_all(&rom, header_offset, false, false).expect("decode");
        let yellow_off = names_off.iter().find(|n| n.contains("\\38")).expect("yellow name with escapes");
        assert!(yellow_off.contains("\\3A"), "expected hex escape in {yellow_off:?}");
    }
}
