//! Byte→character font map for message-box (dialog) text, derived empirically
//! from the real SMW (U) ROM.
//!
//! The game's message bytes are NOT ASCII: each byte (0x00-0x7F) is a tile
//! index into the message font tileset (GFX2A, "Message Box Letters"),
//! drawn via SMW's "dynamic stripe image" (Layer 3) mechanism. The routine
//! `CODE_05B208` (bank_05.asm, U version) emits 8 rows × 18 cells:
//!
//! - At the start of each 18-cell row, the fill flag (`_3`) is cleared.
//! - For each cell: if the fill flag is set, emit tile `$1F` (blank) WITHOUT
//!   consuming a source byte.
//! - Otherwise read the next source byte into `_3`, emit `byte & 0x7F` as the
//!   tile index, and consume the byte. If the byte had bit 7 set, the fill
//!   flag remains set for the rest of the row.
//!
//! So bit 7 means: **emit this glyph, then fill the REMAINDER of the current
//! 18-cell row with `$1F` blanks**. Source consumption resumes at the start
//! of the next row. It is NOT hold/repeat, and it is NOT a single blank.
//! Every vanilla message contains exactly 8 bit-7 bytes — one row terminator
//! per row — which is why short messages (e.g. Ghost House, 90 source bytes)
//! show blank trailing rows: the routine always emits all 8 rows.
//!
//! There are no control codes: the real routine consumes every source byte
//! (while the fill flag is clear) and always emits exactly 144 cells.
//!
//! # Real font map (SMW U, verified 2026-09-10)
//!
//! Derived by running all 22 vanilla messages through the real `CODE_05B1BC`
//! via `smwe_emu::emu::render_message` and aligning the 8×18 tile output
//! against the known English text, then confirmed pixel-for-pixel against
//! GFX2A ("Message Box Letters", SNES $0BCB7B, 2bpp, 128 tiles):
//! - `0x00-0x19` → `A-Z` (uppercase)
//! - `0x40-0x59` → `a-z` (lowercase)
//! - `0x1A` → `!`, `0x1B` → `.`, `0x1D` → `,`, `0x1E` → `?`, `0x1F` → space
//! - `0x1C` → `"` (decorative quote around titles like "POINT OF ADVICE")
//! - `0x5D` → `'` (apostrophe)
//! - `0x60-0x63`, `0x64`, `0x6B`, … → non-text graphic tiles (Yoshi's
//!   signature, bonus-star icons, …), left unmapped.
//!
//! # Synthetic fixtures
//!
//! The unit tests below use INVENTED byte→character pairings. They are NOT the
//! real SMW font; they exist to prove the row-fill decoder and the derivation
//! algorithm handle alignment, repeated bytes, and the bit-7 row fill. Use
//! [`FontMap::real`] for the true SMW (U) mapping.

/// A byte (0x00-0x7F, bit 7 masked) → character mapping for message text.
#[derive(Debug, Clone)]
pub struct FontMap {
    map: [Option<char>; 128],
}

/// The 8×18 tile indices of a message, exactly as the real `CODE_05B208`
/// emits them: each cell holds `source byte & 0x7F`; a source byte with bit 7
/// set fills the remainder of its 18-cell row with `$1F` (blank); the fill
/// flag resets at the start of each row and source consumption resumes there.
///
/// Short input is padded with `$1F` blanks (the real routine would keep
/// reading past the message into whatever follows in ROM; padding is the sane
/// editor behavior for a truncated/edited message). Bytes beyond the 8 rows
/// are ignored.
pub fn message_cells(bytes: &[u8]) -> [[u8; 18]; 8] {
    let mut grid = [[0x1Fu8; 18]; 8];
    let mut y = 0usize;
    for row in grid.iter_mut() {
        let mut fill = false;
        for cell in row.iter_mut() {
            if fill {
                *cell = 0x1F;
            } else if let Some(&b) = bytes.get(y) {
                y += 1;
                if b & 0x80 != 0 {
                    fill = true;
                }
                *cell = b & 0x7F;
            } else {
                *cell = 0x1F;
            }
        }
    }
    grid
}

impl FontMap {
    /// The real SMW (U) message font map, derived empirically from the ROM
    /// (verified 2026-09-10 by running all 22 messages through the real
    /// `CODE_05B1BC`, and confirmed against the GFX2A "Message Box Letters"
    /// tile graphics). See module docs for the derivation method.
    pub fn real() -> Self {
        let mut map: [Option<char>; 128] = [None; 128];
        // 0x00-0x19: A-Z (uppercase)
        for (i, c) in ('A'..='Z').enumerate() {
            map[i] = Some(c);
        }
        // 0x40-0x59: a-z (lowercase)
        for (i, c) in ('a'..='z').enumerate() {
            map[0x40 + i] = Some(c);
        }
        // Punctuation and space
        map[0x1A] = Some('!');
        map[0x1B] = Some('.');
        map[0x1C] = Some('"'); // decorative quote around titles
        map[0x1D] = Some(',');
        map[0x1E] = Some('?');
        map[0x1F] = Some(' ');
        map[0x5D] = Some('\''); // apostrophe
        Self { map }
    }

    /// Look up the character for a raw message byte. Bit 7 is the game's
    /// row-fill flag and is masked off, matching `AND #$7F` in `CODE_05B208`.
    pub fn char_for(&self, byte: u8) -> Option<char> {
        self.map[(byte & 0x7F) as usize]
    }

    /// Decode raw message bytes to 8 rows × 18 characters, following the real
    /// `CODE_05B208` via [`message_cells`]. Bytes with no mapping (non-text
    /// graphic tiles) decode as `'?'`.
    pub fn to_rows(&self, bytes: &[u8]) -> [String; 8] {
        message_cells(bytes).map(|row| row.iter().map(|&b| self.map[b as usize].unwrap_or('?')).collect())
    }

    /// The 8 decoded rows joined by newlines.
    pub fn to_text(&self, bytes: &[u8]) -> String {
        self.to_rows(bytes).join("\n")
    }

    /// Inverse lookup: character → raw message byte (0x00-0x7F).
    ///
    /// Returns `None` for characters with no font glyph, including
    /// [`GRAPHIC_PLACEHOLDER`] (which is preserved positionally from the
    /// original bytes by [`encode_editable_text`], never mapped to a byte).
    pub fn byte_for(&self, c: char) -> Option<u8> {
        self.map.iter().position(|&m| m == Some(c)).map(|i| i as u8)
    }
}

/// Placeholder shown in editable message text for bytes with no font-map
/// entry (non-text graphic tiles such as Yoshi's signature or the bonus-star
/// icons).
///
/// It is U+FFFD REPLACEMENT CHARACTER, deliberately distinct from `'?'`
/// (which is a real mapped glyph, byte `0x1E`): a typed `'?'` always encodes
/// to `0x1E`, while `'�'` reuses the original byte at the same cell — but
/// only if that cell held an unmapped graphic byte in the first place.
/// Typing `'�'` where the original cell was text/blank is an encode error,
/// and deleting a `'�'` drops that graphic tile. Graphics can therefore be
/// preserved in place or removed, but not moved or inserted, via the text
/// field (the raw byte grid below remains for byte-level surgery).
pub const GRAPHIC_PLACEHOLDER: char = '�';

/// Decode message bytes to editable text: the 8×18 grid from
/// [`message_cells`] as 8 lines joined by `'\n'`, with unmapped graphic bytes
/// shown as [`GRAPHIC_PLACEHOLDER`].
///
/// The result round-trips through [`encode_editable_text`] byte-exactly when
/// left unedited (each vanilla message's 8 bit-7 row terminators are
/// regenerated from the row structure).
pub fn decode_editable_text(map: &FontMap, bytes: &[u8]) -> String {
    message_cells(bytes)
        .iter()
        .map(|row| row.iter().map(|&b| map.char_for(b).unwrap_or(GRAPHIC_PLACEHOLDER)).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Encode edited message text back to raw message bytes.
///
/// `text` is up to 8 lines (excess lines are an error); each line holds up to
/// 18 characters (longer lines are an error — the game only draws 18 cells
/// per row). `\n` in the text is the line-break representation; there are no
/// other control codes (the real `CODE_05B208` has none — see module docs).
///
/// Encoding inverts the row-fill: trailing spaces of each row are dropped and
/// the last content byte gets bit 7 set ("fill the remainder of this row
/// with `$1F` blanks"); a fully blank row encodes to a single `0x9F` byte,
/// matching the vanilla pattern of one bit-7 row terminator per row.
///
/// `original` is the message's current bytes, used only to preserve graphic
/// tiles: a [`GRAPHIC_PLACEHOLDER`] at cell (r, c) reuses the original
/// emitted cell's byte, which must itself be unmapped. Any other unmappable
/// character is an error.
///
/// This enforces the *encoding* only, not the size budget — use
/// [`encode_message_checked`] to also enforce a message's byte budget.
pub fn encode_editable_text(map: &FontMap, original: &[u8], text: &str) -> anyhow::Result<Vec<u8>> {
    let old_cells = message_cells(original);
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() > 8 {
        anyhow::bail!("message text has {} lines; the game draws exactly 8 rows", lines.len());
    }
    let mut out = Vec::new();
    for r in 0..8 {
        let line = lines.get(r).copied().unwrap_or("");
        let chars: Vec<char> = line.chars().collect();
        if chars.len() > 18 {
            anyhow::bail!("line {} has {} characters; the game draws 18 cells per row", r + 1, chars.len());
        }
        match chars.iter().rposition(|&c| c != ' ') {
            None => {
                // Fully blank row: one space byte with the row-fill flag,
                // exactly the vanilla pattern (verified: every vanilla
                // message has 8 bit-7 bytes, one per row).
                out.push(0x9F);
            }
            Some(last) => {
                for (c_idx, &c) in chars[..=last].iter().enumerate() {
                    let byte = match map.byte_for(c) {
                        Some(b) => b,
                        None if c == GRAPHIC_PLACEHOLDER => {
                            let ob = old_cells[r][c_idx];
                            if map.char_for(ob).is_none() {
                                ob
                            } else {
                                anyhow::bail!(
                                    "line {} col {}: '�' has no graphic tile to preserve here \
                                     (original cell was text/blank)",
                                    r + 1,
                                    c_idx + 1
                                );
                            }
                        }
                        None => {
                            anyhow::bail!("line {} col {}: character {c:?} has no message-font glyph", r + 1, c_idx + 1)
                        }
                    };
                    out.push(byte);
                }
                // Bit 7 on the last content byte: fill the rest of the row
                // with $1F blanks (the real CODE_05B208 semantics).
                let last_byte = out.last_mut().expect("non-empty row pushed no bytes");
                *last_byte |= 0x80;
            }
        }
    }
    Ok(out)
}

/// [`encode_editable_text`] plus the per-message byte-budget check: the
/// encoded bytes must fit within `budget` (the message's vanilla length —
/// the combined 22-message blob isn't repointable, so no single message may
/// grow past what it originally occupied).
pub fn encode_message_checked(map: &FontMap, original: &[u8], budget: usize, text: &str) -> anyhow::Result<Vec<u8>> {
    let bytes = encode_editable_text(map, original, text)?;
    if bytes.len() > budget {
        anyhow::bail!("encoded text is {} bytes, over this message's {}-byte budget", bytes.len(), budget);
    }
    Ok(bytes)
}

/// Decode message bytes to editable text through a Lunar Magic v3.40 custom
/// table file ([`crate::table_file::Table`]) instead of the built-in font
/// map.
///
/// The 8×18 row structure is unchanged (it comes from the real `CODE_05B208`
/// via [`message_cells`]); only the tile→character mapping is replaced.
/// Each row's cell bytes (bit 7 already masked by `message_cells`) are
/// decoded with the table's greedy longest-match; unmapped bytes show as
/// the table's `<XX>` hex escapes.
pub fn decode_message_with_table(table: &crate::table_file::Table, bytes: &[u8]) -> String {
    message_cells(bytes).iter().map(|row| table.decode(row)).collect::<Vec<_>>().join("\n")
}

/// Encode edited message text back to raw message bytes through a Lunar
/// Magic v3.40 custom table file ([`crate::table_file::Table`]).
///
/// Mirrors [`encode_editable_text`] but the tile↔character mapping comes
/// from the table: up to 8 lines, each line's table-encoded bytes must fit
/// the game's 18 cells per row. Like the built-in encoder, trailing spaces
/// of each row are dropped and the last content byte gets bit 7 (row-fill);
/// a fully blank row encodes to the table's space byte(s) with bit 7, or a
/// single `0x9F` when the table doesn't map space (the vanilla pattern).
///
/// Differences from the built-in encoder, all following from the table
/// replacing the mapping entirely (per the "Custom Table File" help topic):
/// - Unmapped characters are skipped, never an error.
/// - There is no graphic-placeholder concept: `<XX>` escapes are
///   display-only (typing them encodes the four characters through the
///   table like any other text).
/// - `original` is not needed (no placeholder preservation).
///
/// Like [`encode_message_checked`], refuses text whose encoded bytes exceed
/// `budget` (the message's vanilla byte span) instead of truncating.
pub fn encode_message_with_table(
    table: &crate::table_file::Table, budget: usize, text: &str,
) -> anyhow::Result<Vec<u8>> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() > 8 {
        anyhow::bail!("message text has {} lines; the game draws exactly 8 rows", lines.len());
    }
    let mut out = Vec::new();
    for r in 0..8 {
        let line = lines.get(r).copied().unwrap_or("");
        // Drop trailing spaces: the game fills the rest of the row from
        // the bit-7 flag, exactly like the built-in encoder.
        let content = line.trim_end_matches(' ');
        let bytes = table.encode(content);
        if bytes.len() > 18 {
            anyhow::bail!("line {} encodes to {} bytes; the game draws 18 cells per row", r + 1, bytes.len());
        }
        if bytes.is_empty() {
            // Fully blank row: the table's space encoding with the
            // row-fill flag; vanilla 0x9F when space is unmapped.
            let mut blank = table.encode(" ");
            if blank.is_empty() {
                blank.push(0x1F);
            }
            let last = blank.len() - 1;
            blank[last] |= 0x80;
            out.extend_from_slice(&blank);
        } else {
            out.extend_from_slice(&bytes);
            // Bit 7 on the last content byte: fill the rest of the row
            // with $1F blanks (the real CODE_05B208 semantics).
            let last_byte = out.last_mut().expect("non-empty row pushed no bytes");
            *last_byte |= 0x80;
        }
    }
    if out.len() > budget {
        anyhow::bail!("encoded text is {} bytes, over this message's {}-byte budget", out.len(), budget);
    }
    Ok(out)
}

/// Derive a [`FontMap`] from `(byte sequence, expected 8×18 text rows)` pairs.
///
/// Each pair's text is the 8 rows of 18 characters the message decodes to.
/// The walk mirrors `CODE_05B208` exactly: for each row, the fill flag starts
/// clear; a bit-7 source byte maps `byte & 0x7F` to its text cell and then
/// every remaining cell of that row must be a space (the `$1F` fill); the
/// next row resumes consuming source bytes.
///
/// Errors if a byte maps to two different characters, if the source bytes run
/// out before the 8 rows do, if bytes are left unconsumed, if a row isn't 18
/// characters, or if a fill cell isn't a space. Any of those means a wrong
/// pairing, never a silent wrong map.
pub fn derive_font_map(pairs: &[(&[u8], [&str; 8])]) -> anyhow::Result<FontMap> {
    let mut map: [Option<char>; 128] = [None; 128];
    for (msg_i, (bytes, rows)) in pairs.iter().enumerate() {
        let rows: Vec<Vec<char>> = rows.iter().map(|r| r.chars().collect()).collect();
        for (ri, row) in rows.iter().enumerate() {
            if row.len() != 18 {
                anyhow::bail!("message {msg_i} row {ri}: expected 18 characters, found {}", row.len());
            }
        }
        let mut y = 0usize;
        for (ri, row) in rows.iter().enumerate() {
            let mut fill = false;
            for (ci, &c) in row.iter().enumerate() {
                if fill {
                    if c != ' ' {
                        anyhow::bail!("message {msg_i} row {ri} cell {ci}: bit-7 fill expects a space, found {c:?}");
                    }
                    continue;
                }
                let &b = bytes
                    .get(y)
                    .ok_or_else(|| anyhow::anyhow!("message {msg_i}: ran out of source bytes at row {ri} cell {ci}"))?;
                y += 1;
                let b7 = b & 0x7F;
                match map[b7 as usize] {
                    None => map[b7 as usize] = Some(c),
                    Some(prev) if prev == c => {}
                    Some(prev) => anyhow::bail!("message {msg_i}: byte {b7:#04X} maps to both {prev:?} and {c:?}"),
                }
                if b & 0x80 != 0 {
                    fill = true;
                }
            }
        }
        if y != bytes.len() {
            anyhow::bail!("message {msg_i}: {} source byte(s) left unconsumed", bytes.len() - y);
        }
    }
    Ok(FontMap { map })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic "font" used ONLY by these tests — invented pairings, not the
    // real SMW font: 0x00='A', 0x01='B', 0x02='C', 0x1F=' ' (space).
    const PAD_ROW: &str = "                  "; // 18 spaces

    /// One byte that decodes to a full blank row: 0x9F = space + bit-7 fill.
    const BLANK: u8 = 0x9F;

    #[test]
    fn derivation_aligns_and_maps_consistently_across_messages() {
        // Row 0: 'A','B'+fill -> "AB" + 16 spaces (2 source bytes).
        // Row 1: 'C','A'+fill -> "CA" + 16 spaces (2 source bytes, exercises
        // cross-row consistency: 0x00 must map to 'A' in both rows).
        let row0 = "AB                ";
        let row1 = "CA                ";
        let bytes: Vec<u8> = vec![0x00, 0x81, 0x02, 0x80, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK];
        let rows = [row0, row1, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW];
        let map = derive_font_map(&[(&bytes, rows)]).unwrap();
        assert_eq!(map.char_for(0x00), Some('A'));
        assert_eq!(map.char_for(0x81), Some('B')); // bit 7 masked
        assert_eq!(map.char_for(0x02), Some('C'));
        // 0x1F is mapped to space by the blank-row bytes (0x9F & 0x7F).
        assert_eq!(map.char_for(0x1F), Some(' '));
        let decoded = map.to_rows(&bytes);
        assert_eq!(decoded[0], row0);
        assert_eq!(decoded[1], row1);
        for r in &decoded[2..] {
            assert_eq!(r, PAD_ROW);
        }
    }

    #[test]
    fn bit7_fills_rest_of_row_not_just_one_blank() {
        // 0x81 = 'B' with bit 7: emits 'B', then fills the remaining 17
        // cells of row 0 with blanks. This is the corrected semantics: the
        // old (wrong) model inserted a single blank.
        let bytes: Vec<u8> = vec![0x81, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK];
        let rows = ["B                 ", PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW];
        let map = derive_font_map(&[(&bytes, rows)]).unwrap();
        let decoded = map.to_rows(&bytes);
        assert_eq!(decoded[0], "B                 ");
        assert_eq!(decoded[0].len(), 18);
        for r in &decoded[1..] {
            assert_eq!(r, PAD_ROW);
        }
        // Flat text keeps the row structure.
        assert_eq!(map.to_text(&bytes).lines().next().unwrap(), "B                 ");
    }

    #[test]
    fn bit7_fill_resets_each_row_and_source_resumes() {
        // 0x81 fills the rest of ROW 0; the next source byte (0x80='A') is
        // consumed at the start of ROW 1, not in row 0. This is the key
        // behavioral difference from "insert one blank".
        let bytes: Vec<u8> = vec![0x81, 0x80, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK];
        let rows = ["B                 ", "A                 ", PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW];
        let map = derive_font_map(&[(&bytes, rows)]).unwrap();
        let decoded = map.to_rows(&bytes);
        assert_eq!(decoded[0], "B                 ");
        assert_eq!(decoded[1], "A                 ");
        // message_cells agrees: byte 1 is NOT consumed in row 0.
        let cells = message_cells(&[0x81, 0x80]);
        assert_eq!(cells[0][0], 0x01);
        assert_eq!(cells[0][1], 0x1F); // fill, not the 0x80 byte
        assert_eq!(cells[1][0], 0x00); // consumed here
    }

    #[test]
    fn bit7_as_last_cell_fills_nothing() {
        // 18 source bytes, the last with bit 7: no cells remain to fill.
        let mut bytes: Vec<u8> = vec![0x00; 17];
        bytes.push(0x80); // 'A' + fill flag, but row is already full
        bytes.extend([BLANK; 7]);
        let rows = ["AAAAAAAAAAAAAAAAAA", PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW];
        let map = derive_font_map(&[(&bytes, rows)]).unwrap();
        assert_eq!(map.to_rows(&bytes)[0], "AAAAAAAAAAAAAAAAAA");
    }

    #[test]
    fn unmapped_bytes_decode_as_question_mark() {
        let bytes: Vec<u8> = vec![0x80, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK];
        let rows = ["A                 ", PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW];
        let map = derive_font_map(&[(&bytes, rows)]).unwrap();
        assert_eq!(map.char_for(0x02), None);
        assert_eq!(map.to_rows(&[0x02])[0].chars().next().unwrap(), '?');
    }

    #[test]
    fn short_input_is_padded_with_blanks() {
        // Fewer bytes than 8 rows: the real routine would read past the
        // message; we pad with blanks instead.
        let map = FontMap::real();
        let rows = map.to_rows(&[0x07]); // 'H'
        assert_eq!(rows[0], "H                 ");
        for r in &rows[1..] {
            assert_eq!(r, PAD_ROW);
        }
    }

    #[test]
    fn conflicting_byte_mapping_is_an_error() {
        let bytes: Vec<u8> = vec![0x80, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK];
        let rows_a = ["A                 ", PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW];
        let rows_x = ["X                 ", PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW];
        let err = derive_font_map(&[(&bytes, rows_a), (&bytes, rows_x)]).unwrap_err();
        assert!(err.to_string().contains("maps to both"), "unexpected error: {err}");
    }

    #[test]
    fn fill_cell_must_be_a_space() {
        // 0x81 sets fill; the expected text wrongly has 'X' in a fill cell.
        let mut row0 = String::from("B");
        row0.push('X');
        row0.push_str(&" ".repeat(16));
        let bytes: Vec<u8> = vec![0x81, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK];
        let rows = [row0.as_str(), PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW];
        let err = derive_font_map(&[(&bytes, rows)]).unwrap_err();
        assert!(err.to_string().contains("fill expects a space"), "unexpected error: {err}");
    }

    #[test]
    fn bytes_longer_than_rows_is_an_error() {
        // 9 bytes but the 8 rows only consume 8: one left unconsumed.
        let bytes: Vec<u8> = vec![0x81, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK];
        let rows = ["B                 ", PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW];
        let err = derive_font_map(&[(&bytes, rows)]).unwrap_err();
        assert!(err.to_string().contains("unconsumed"), "unexpected error: {err}");
    }

    #[test]
    fn bytes_shorter_than_rows_is_an_error() {
        // Row 0 needs 2 bytes ("AB") but only 1 is provided.
        let bytes: Vec<u8> = vec![0x00];
        let rows = ["AB                ", PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW];
        let err = derive_font_map(&[(&bytes, rows)]).unwrap_err();
        assert!(err.to_string().contains("ran out of source bytes"), "unexpected error: {err}");
    }

    #[test]
    fn row_must_be_18_characters() {
        let bytes: Vec<u8> = vec![0x80, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK, BLANK];
        let rows = ["too short", PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW, PAD_ROW];
        let err = derive_font_map(&[(&bytes, rows)]).unwrap_err();
        assert!(err.to_string().contains("expected 18 characters"), "unexpected error: {err}");
    }

    #[test]
    fn empty_input_yields_an_empty_map() {
        let map = derive_font_map(&[]).unwrap();
        assert_eq!(map.char_for(0x00), None);
        assert_eq!(map.to_rows(&[0x00])[0].chars().next().unwrap(), '?');
    }

    #[test]
    fn byte_for_inverts_char_for_for_every_mapped_byte() {
        // The real map must be 1:1 so typed text encodes deterministically.
        let map = FontMap::real();
        for b in 0u8..128 {
            if let Some(c) = map.char_for(b) {
                assert_eq!(map.byte_for(c), Some(b), "char {c:?} (byte {b:#04X}) is not 1:1");
            }
        }
        // '?' is a real glyph (0x1E), distinct from the graphic placeholder.
        assert_eq!(map.byte_for('?'), Some(0x1E));
        assert_eq!(map.byte_for(GRAPHIC_PLACEHOLDER), None);
    }

    const EDIT_PAD: &str = "                  "; // 18 spaces

    fn edit_text_row0(row0: &str) -> String {
        let mut s = String::from(row0);
        for _ in 1..8 {
            s.push('\n');
            s.push_str(EDIT_PAD);
        }
        s
    }

    #[test]
    fn editable_round_trip_is_byte_exact() {
        let map = FontMap::real();
        // Row 0: 'A','B'+fill; rows 1-7: blank (0x9F each) — vanilla shape:
        // one bit-7 row terminator per row.
        let bytes: Vec<u8> = vec![0x00, 0x81, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F];
        let text = decode_editable_text(&map, &bytes);
        assert_eq!(text, edit_text_row0("AB                "));
        let back = encode_editable_text(&map, &bytes, &text).unwrap();
        assert_eq!(back, bytes);
    }

    #[test]
    fn editable_encode_drops_trailing_spaces_and_sets_bit7() {
        let map = FontMap::real();
        let original = vec![0x9F; 8];
        let back = encode_editable_text(&map, &original, &edit_text_row0("Hi")).unwrap();
        // "Hi" -> [0x07, 0x48|0x80]; 7 blank rows -> 7 × 0x9F.
        assert_eq!(&back[..2], &[0x07, 0xC8]);
        assert_eq!(&back[2..], &[0x9F; 7]);
        // ... and it decodes back to the same text.
        assert_eq!(decode_editable_text(&map, &back), edit_text_row0("Hi                "));
    }

    #[test]
    fn editable_encode_rejects_long_lines() {
        let map = FontMap::real();
        let original = vec![0x9F; 8];
        let text = edit_text_row0(&"A".repeat(19));
        let err = encode_editable_text(&map, &original, &text).unwrap_err();
        assert!(err.to_string().contains("18 cells"), "unexpected error: {err}");
    }

    #[test]
    fn editable_encode_rejects_too_many_lines() {
        let map = FontMap::real();
        let original = vec![0x9F; 8];
        let text = (0..9).map(|_| "x").collect::<Vec<_>>().join("\n");
        let err = encode_editable_text(&map, &original, &text).unwrap_err();
        assert!(err.to_string().contains("8 rows"), "unexpected error: {err}");
    }

    #[test]
    fn editable_encode_rejects_unknown_characters() {
        let map = FontMap::real();
        let original = vec![0x9F; 8];
        let err = encode_editable_text(&map, &original, &edit_text_row0("#nope")).unwrap_err();
        assert!(err.to_string().contains("no message-font glyph"), "unexpected error: {err}");
    }

    #[test]
    fn editable_placeholder_preserves_graphic_byte_in_place() {
        let map = FontMap::real();
        // 0x60 is an unmapped graphic tile; 0xE0 = graphic + row-fill flag.
        let bytes: Vec<u8> = vec![0x00, 0xE0, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F];
        let text = decode_editable_text(&map, &bytes);
        assert!(text.starts_with("A�"), "graphic byte must decode as placeholder: {text:?}");
        let back = encode_editable_text(&map, &bytes, &text).unwrap();
        assert_eq!(back, bytes, "placeholder must reuse the original graphic byte");
    }

    /// Table used by the table-file tests: 00=A, 01=B, 1F=space.
    fn test_table() -> crate::table_file::Table {
        let (file, warnings) = crate::table_file::parse_lmtbl("@MessageBox\n00=A\n01=B\n1F= \n").unwrap();
        assert!(warnings.is_empty());
        file.table_for(crate::table_file::TableDialog::MessageBox).unwrap().clone()
    }

    #[test]
    fn table_decode_keeps_row_structure_with_hex_escapes() {
        let t = test_table();
        // Row 0: A, unmapped 0x60, B+fill; rows 1-7: blank.
        let bytes: Vec<u8> = vec![0x00, 0x60, 0x81, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F];
        let text = decode_message_with_table(&t, &bytes);
        let rows: Vec<&str> = text.lines().collect();
        assert_eq!(rows.len(), 8);
        // Row 0 cells: 00 60 01 1F*15 -> "A<60>B" + 15 spaces.
        assert_eq!(rows[0], "A<60>B               ");
        for r in &rows[1..] {
            assert_eq!(*r, "                  ");
        }
    }

    #[test]
    fn table_encode_sets_row_fill_and_drops_trailing_spaces() {
        let t = test_table();
        // "AB" on row 0, rest blank: trailing spaces are dropped like the
        // built-in encoder, so this is 2 + 7 bytes, not 8*18.
        let text = "AB\n".to_string() + &"\n".repeat(7);
        let bytes = encode_message_with_table(&t, 256, &text).unwrap();
        assert_eq!(bytes, vec![0x00, 0x81, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F]);
    }

    #[test]
    fn table_decode_encode_round_trips() {
        let t = test_table();
        let bytes: Vec<u8> = vec![0x00, 0x81, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F];
        let text = decode_message_with_table(&t, &bytes);
        let back = encode_message_with_table(&t, 256, &text).unwrap();
        assert_eq!(back, bytes);
    }

    #[test]
    fn table_encode_skips_unmapped_chars_instead_of_erroring() {
        let t = test_table();
        // 'Z' is unmapped: skipped (LM behavior), unlike the built-in
        // encoder which errors on unknown characters.
        let bytes = encode_message_with_table(&t, 256, "AZB").unwrap();
        assert_eq!(&bytes[0..2], &[0x00, 0x81]);
    }

    #[test]
    fn table_encode_rejects_overlong_rows_and_over_budget() {
        let t = test_table();
        // 19 bytes on one row: refused (18 cells per row).
        let err = encode_message_with_table(&t, 256, &"A".repeat(19)).unwrap_err();
        assert!(err.to_string().contains("18 cells"), "unexpected error: {err}");
        // 9 lines: refused (8 rows).
        let err = encode_message_with_table(&t, 256, &"A\n".repeat(9)).unwrap_err();
        assert!(err.to_string().contains("8 rows"), "unexpected error: {err}");
        // Over the message's byte budget: refused, not truncated.
        // ("AB" encodes to 9 bytes: 2 + 7 blank rows.)
        let err = encode_message_with_table(&t, 8, "AB").unwrap_err();
        assert!(err.to_string().contains("over this message's"), "unexpected error: {err}");
    }

    #[test]
    fn editable_placeholder_without_original_graphic_is_an_error() {
        let map = FontMap::real();
        // Original row 0 is "AB"+fill — no graphic at col 2.
        let bytes: Vec<u8> = vec![0x00, 0x81, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F, 0x9F];
        let err = encode_editable_text(&map, &bytes, &edit_text_row0("A�                ")).unwrap_err();
        assert!(err.to_string().contains("no graphic tile to preserve"), "unexpected error: {err}");
    }

    #[test]
    fn editable_typed_question_mark_is_the_real_glyph() {
        let map = FontMap::real();
        let original = vec![0x9F; 8];
        // Typed '?' must encode to 0x1E (the real glyph), never to a graphic.
        let back = encode_editable_text(&map, &original, &edit_text_row0("?")).unwrap();
        assert_eq!(back[0], 0x1E | 0x80);
    }

    #[test]
    fn encode_message_checked_enforces_the_byte_budget() {
        let map = FontMap::real();
        let original = vec![0x9F; 8];
        let text = edit_text_row0("Hello");
        // "Hello" encodes to 5 bytes + 7 blank rows = 12 bytes.
        let ok = encode_message_checked(&map, &original, 12, &text).unwrap();
        assert_eq!(ok.len(), 12);
        let err = encode_message_checked(&map, &original, 11, &text).unwrap_err();
        assert!(err.to_string().contains("over this message's 11-byte budget"), "unexpected error: {err}");
    }
}
