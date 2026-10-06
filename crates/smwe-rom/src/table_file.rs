//! Lunar Magic v3.40+ "Custom Table File" (`.lmtbl`) support.
//!
//! Official sources: the LM v3.40 changelog ("added basic table file support
//! for the overworld editor's \"Edit Level Names\", \"Edit Message Box Text\",
//! and \"Edit Boss Sequence Text\" dialogs that can be used for editing and
//! displaying the text in different languages", 2023-09-24) and the help
//! topic "Custom Table File" (LM 3.71 help file), which defines the format:
//!
//! - A UTF-8 text file named like the ROM with a lowercase `.lmtbl`
//!   extension, in the same folder as the ROM.
//! - One mapping per line: `Tile Number (in hex) = character to use for
//!   tile`. The tile number has an even number of hex digits and there are
//!   no spaces before the `=` sign. Example: `00=A`.
//! - Both MultiTile (several bytes → one string, e.g. `38393A3B3C=YELLOW`)
//!   and MultiChar (one byte → several characters) entries are allowed.
//! - Separate named tables per dialog via `@LevelNames`, `@MessageBox`, or
//!   `@BossSequence` on their own line before that table's entries. If
//!   named tables are used, all tables in the file must be named.
//! - While a table is active, none of the program's built-in tile/char
//!   mapping is used: unmapped tiles are displayed with hex escape
//!   sequences, and unmapped characters are simply skipped on encode.
//!
//! Honest limits (the public docs don't specify these):
//! - The exact hex-escape format LM shows for unmapped tiles is not
//!   documented; this module uses `<XX>` (e.g. `<3A>`).
//! - Decode/encode conflict resolution is not documented; this module uses
//!   greedy longest-match (longest key first on decode, longest value first
//!   on encode), the standard `.tbl` behavior.
//! - Whether spaces are allowed after the `=` sign is not documented; the
//!   value is taken verbatim (everything after the first `=`).
//! - Comment lines are not mentioned in the help; lines that don't parse as
//!   entries (including `;` comment lines) are skipped with a warning rather
//!   than treated as comments or hard errors.
//! - Mixed named + unnamed tables are rejected (the help says all tables
//!   must be named in that case). Unknown `@Name` sections are skipped with
//!   a warning.
//! - `<XX>` escapes are display-only: typing them does not re-insert the
//!   byte (LM's help describes the escapes as a display behavior; encoding
//!   `<`, `X`, `X`, `>` goes through the table like any other characters).
//! - An entry with an empty value (`00=`) decodes byte `0x00` to the empty
//!   string and is excluded from encoding (it would otherwise match
//!   everywhere with zero advance).

use std::collections::{HashMap, HashSet};

/// Which of the three overworld text dialogs a named table belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TableDialog {
    LevelNames,
    MessageBox,
    BossSequence,
}

impl TableDialog {
    /// The `@Name` section header used in `.lmtbl` files.
    pub fn section_name(self) -> &'static str {
        match self {
            TableDialog::LevelNames => "LevelNames",
            TableDialog::MessageBox => "MessageBox",
            TableDialog::BossSequence => "BossSequence",
        }
    }

    /// Parse a `@Name` section header (without the `@`). Exact match only;
    /// the help topic lists exactly these three names.
    pub fn from_section_name(name: &str) -> Option<Self> {
        match name {
            "LevelNames" => Some(TableDialog::LevelNames),
            "MessageBox" => Some(TableDialog::MessageBox),
            "BossSequence" => Some(TableDialog::BossSequence),
            _ => None,
        }
    }
}

/// One dialog's byte-sequence ↔ text mappings, parsed from a `.lmtbl` file.
///
/// Decoding is greedy longest-key-first over the byte stream; bytes with no
/// mapping decode to the `<XX>` hex escape. Encoding is greedy
/// longest-value-first over the text; characters with no mapping are
/// skipped, matching Lunar Magic's documented behavior.
#[derive(Debug, Clone, Default)]
pub struct Table {
    /// `(byte key, text)` pairs in file order (duplicate keys resolved:
    /// last wins).
    entries:      Vec<(Vec<u8>, String)>,
    /// Indices into `entries`, longest key first (stable for ties).
    decode_order: Vec<usize>,
    /// `(text, byte key)` for entries with non-empty text, longest text
    /// first in characters (stable for ties). Built once so encoding is a
    /// linear scan without re-sorting.
    encode_list:  Vec<(String, Vec<u8>)>,
}

impl Table {
    fn from_entries(mut entries: Vec<(Vec<u8>, String)>) -> Self {
        // Duplicate keys: last wins, keeping file order otherwise.
        let mut seen = HashSet::new();
        let mut deduped = Vec::new();
        for (key, value) in entries.drain(..).rev() {
            if seen.insert(key.clone()) {
                deduped.push((key, value));
            }
        }
        deduped.reverse();

        let mut decode_order: Vec<usize> = (0..deduped.len()).collect();
        decode_order.sort_by(|&a, &b| deduped[b].0.len().cmp(&deduped[a].0.len()));

        let mut encode_list: Vec<(String, Vec<u8>)> =
            deduped.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (v.clone(), k.clone())).collect();
        encode_list.sort_by(|a, b| b.0.chars().count().cmp(&a.0.chars().count()));

        Self { entries: deduped, decode_order, encode_list }
    }

    /// Number of mappings in the table.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table has no mappings.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Decode a byte stream to text. Greedy longest-key match at each
    /// position; a byte with no mapping becomes the `<XX>` hex escape.
    pub fn decode(&self, bytes: &[u8]) -> String {
        let mut out = String::new();
        let mut i = 0;
        while i < bytes.len() {
            let mut matched = false;
            for &idx in &self.decode_order {
                let (key, value) = &self.entries[idx];
                if bytes[i..].starts_with(key) {
                    out.push_str(value);
                    i += key.len();
                    matched = true;
                    break;
                }
            }
            if !matched {
                out.push_str(&format!("<{:02X}>", bytes[i]));
                i += 1;
            }
        }
        out
    }

    /// Encode text to bytes. Greedy longest-value match at each character
    /// position; characters with no mapping are skipped (Lunar Magic
    /// behavior: "unmapped characters will simply be skipped").
    pub fn encode(&self, text: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < text.len() {
            let rest = &text[i..];
            let mut matched = false;
            for (value, key) in &self.encode_list {
                if rest.starts_with(value.as_str()) {
                    out.extend_from_slice(key);
                    i += value.len();
                    matched = true;
                    break;
                }
            }
            if !matched {
                // Skip one character. `i` is always at a char boundary:
                // it starts at 0 and advances by whole values/chars.
                i += rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            }
        }
        out
    }
}

/// A parsed `.lmtbl` file: an optional global (unnamed) table plus optional
/// named per-dialog tables.
#[derive(Debug, Clone, Default)]
pub struct TableFile {
    /// Table from entries before any `@Name` line (applies to every dialog).
    pub global: Option<Table>,
    /// Tables from `@LevelNames` / `@MessageBox` / `@BossSequence` sections.
    pub named:  HashMap<TableDialog, Table>,
}

impl TableFile {
    /// Resolve the table for one dialog: the named table when the file uses
    /// named tables, otherwise the global table. `None` means the dialog
    /// keeps its built-in mapping (e.g. the file has named tables but none
    /// for this dialog).
    pub fn table_for(&self, dialog: TableDialog) -> Option<&Table> {
        if self.named.is_empty() {
            self.global.as_ref()
        } else {
            self.named.get(&dialog)
        }
    }

    /// Whether the file uses `@Name` sections.
    pub fn uses_named_tables(&self) -> bool {
        !self.named.is_empty()
    }
}

/// Parse `.lmtbl` text per the "Custom Table File" help topic.
///
/// Returns the tables plus non-fatal warnings (one per skipped line).
/// Errors only when the file mixes named and unnamed tables, which the help
/// topic forbids ("all the tables in the file must be named").
pub fn parse_lmtbl(text: &str) -> anyhow::Result<(TableFile, Vec<String>)> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut global_entries: Vec<(Vec<u8>, String)> = Vec::new();
    let mut named_entries: HashMap<TableDialog, Vec<(Vec<u8>, String)>> = HashMap::new();
    let mut current: Option<TableDialog> = None;
    let mut seen_named = false;
    let mut warnings = Vec::new();

    for (lineno, raw_line) in text.lines().enumerate() {
        let line_no = lineno + 1;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(name) = trimmed.strip_prefix('@') {
            match TableDialog::from_section_name(name.trim()) {
                Some(dialog) => {
                    current = Some(dialog);
                    seen_named = true;
                }
                None => warnings.push(format!("line {line_no}: unknown table name '@{name}' — skipped")),
            }
            continue;
        }
        let Some(eq) = line.find('=') else {
            warnings.push(format!("line {line_no}: no '=' — skipped"));
            continue;
        };
        let (key_raw, value) = line.split_at(eq);
        let value = &value[1..]; // everything after the first '='
                                 // The help requires an even number of hex digits and no spaces
                                 // before the '=' sign.
        if key_raw.is_empty() || key_raw.len() % 2 != 0 || !key_raw.bytes().all(|b| b.is_ascii_hexdigit()) {
            warnings.push(format!(
                "line {line_no}: bad tile number '{key_raw}' (need an even number of hex digits, no spaces) — skipped"
            ));
            continue;
        }
        if seen_named && current.is_none() {
            anyhow::bail!(
                "line {line_no}: file mixes named (@...) and unnamed tables — \
                 Lunar Magic requires all tables in the file to be named when any is"
            );
        }
        let key: Vec<u8> = (0..key_raw.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&key_raw[i..i + 2], 16))
            .collect::<Result<_, _>>()
            .expect("key validated as even hex digits above");
        match current {
            Some(dialog) => named_entries.entry(dialog).or_default().push((key, value.to_string())),
            None => global_entries.push((key, value.to_string())),
        }
    }

    // Entries before the first @Name line in a file that also uses named
    // tables are equally mixed (the in-loop check only catches entries
    // after a named section).
    if seen_named && !global_entries.is_empty() {
        anyhow::bail!(
            "file mixes named (@...) and unnamed tables — \
             Lunar Magic requires all tables in the file to be named when any is"
        );
    }

    let global = if global_entries.is_empty() { None } else { Some(Table::from_entries(global_entries)) };
    let named = named_entries.into_iter().map(|(d, e)| (d, Table::from_entries(e))).collect();
    Ok((TableFile { global, named }, warnings))
}

/// Load a `.lmtbl` file from disk and resolve the table for one dialog.
///
/// Returns the table, the file name (for display), and non-fatal parse
/// warnings. Errors when the file can't be read or parsed, or when it uses
/// named tables but has none for this dialog (the dialog then keeps its
/// built-in mapping — the caller reports this instead of applying anything).
pub fn load_table_file_for_dialog(
    path: &std::path::Path, dialog: TableDialog,
) -> anyhow::Result<(Table, String, Vec<String>)> {
    let text = std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
    let (file, warnings) = parse_lmtbl(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let table = file.table_for(dialog).cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "{} uses named tables but has no @{} table — this dialog keeps its built-in mapping",
            path.display(),
            dialog.section_name()
        )
    })?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    Ok((table, name, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_table() -> Table {
        let (file, warnings) = parse_lmtbl("00=A\n01=B\n1F= \n38393A3B3C=YELLOW\n").unwrap();
        assert!(warnings.is_empty());
        file.global.unwrap()
    }

    #[test]
    fn single_byte_round_trip() {
        let t = sample_table();
        assert_eq!(t.decode(&[0x00, 0x01, 0x1F]), "AB ");
        assert_eq!(t.encode("AB "), vec![0x00, 0x01, 0x1F]);
    }

    #[test]
    fn multibyte_key_decodes_to_string() {
        let t = sample_table();
        // LM's own example: 38 39 3A 3B 3C -> "YELLOW".
        assert_eq!(t.decode(&[0x38, 0x39, 0x3A, 0x3B, 0x3C]), "YELLOW");
    }

    #[test]
    fn multibyte_key_encodes_from_string() {
        let t = sample_table();
        assert_eq!(t.encode("YELLOW"), vec![0x38, 0x39, 0x3A, 0x3B, 0x3C]);
    }

    #[test]
    fn longest_key_wins_on_decode() {
        let (file, _) = parse_lmtbl("38=A\n3839=BC\n").unwrap();
        let t = file.global.unwrap();
        assert_eq!(t.decode(&[0x38, 0x39]), "BC");
        assert_eq!(t.decode(&[0x38]), "A");
    }

    #[test]
    fn longest_value_wins_on_encode() {
        let (file, _) = parse_lmtbl("01=A\n02=AB\n").unwrap();
        let t = file.global.unwrap();
        assert_eq!(t.encode("AB"), vec![0x02]);
        assert_eq!(t.encode("A"), vec![0x01]);
    }

    #[test]
    fn multichar_entry_single_byte_to_many_chars() {
        let (file, _) = parse_lmtbl("00=YOU\n").unwrap();
        let t = file.global.unwrap();
        assert_eq!(t.decode(&[0x00]), "YOU");
        assert_eq!(t.encode("YOU"), vec![0x00]);
    }

    #[test]
    fn unmapped_bytes_become_hex_escapes() {
        let t = sample_table();
        assert_eq!(t.decode(&[0x00, 0x7E, 0x01]), "A<7E>B");
    }

    #[test]
    fn unmapped_chars_are_skipped_on_encode() {
        let t = sample_table();
        // 'Z' is not mapped: skipped, not an error.
        assert_eq!(t.encode("AZB"), vec![0x00, 0x01]);
        assert_eq!(t.encode("ZZZ"), Vec::<u8>::new());
    }

    #[test]
    fn duplicate_keys_last_wins() {
        let (file, _) = parse_lmtbl("00=A\n00=B\n").unwrap();
        let t = file.global.unwrap();
        assert_eq!(t.decode(&[0x00]), "B");
        assert_eq!(t.encode("B"), vec![0x00]);
    }

    #[test]
    fn named_tables_resolve_per_dialog() {
        let (file, warnings) = parse_lmtbl("@LevelNames\n00=A\n\n@MessageBox\n71=A\n").unwrap();
        assert!(warnings.is_empty());
        assert!(file.uses_named_tables());
        assert_eq!(file.table_for(TableDialog::LevelNames).unwrap().decode(&[0x00]), "A");
        assert_eq!(file.table_for(TableDialog::MessageBox).unwrap().decode(&[0x71]), "A");
        // No @BossSequence table: falls back to built-in mapping (None).
        assert!(file.table_for(TableDialog::BossSequence).is_none());
        // The @MessageBox table does not apply to level names.
        assert_eq!(file.table_for(TableDialog::LevelNames).unwrap().decode(&[0x71]), "<71>");
    }

    #[test]
    fn global_table_applies_to_all_dialogs() {
        let (file, _) = parse_lmtbl("00=A\n").unwrap();
        assert!(!file.uses_named_tables());
        for d in [TableDialog::LevelNames, TableDialog::MessageBox, TableDialog::BossSequence] {
            assert_eq!(file.table_for(d).unwrap().decode(&[0x00]), "A");
        }
    }

    #[test]
    fn mixed_named_and_unnamed_is_an_error() {
        assert!(parse_lmtbl("00=A\n@LevelNames\n01=B\n").is_err());
    }

    #[test]
    fn malformed_lines_warn_and_skip() {
        let (file, warnings) =
            parse_lmtbl("00=A\nnot an entry\n0=odd\n0G=badhex\n00 =space-before-eq\n@Bogus\n").unwrap();
        assert_eq!(warnings.len(), 5);
        let t = file.global.unwrap();
        // Only the valid entry survived.
        assert_eq!(t.len(), 1);
        assert_eq!(t.decode(&[0x00]), "A");
    }

    #[test]
    fn value_is_verbatim_after_equals() {
        let (file, _) = parse_lmtbl("00= A\n01=B=C\n").unwrap();
        let t = file.global.unwrap();
        assert_eq!(t.decode(&[0x00]), " A");
        assert_eq!(t.decode(&[0x01]), "B=C");
    }

    #[test]
    fn bom_and_crlf_are_tolerated() {
        let (file, warnings) = parse_lmtbl("\u{FEFF}00=A\r\n01=B\r\n").unwrap();
        assert!(warnings.is_empty());
        assert_eq!(file.global.unwrap().decode(&[0x00, 0x01]), "AB");
    }

    #[test]
    fn empty_value_decodes_but_never_encodes() {
        let (file, _) = parse_lmtbl("00=\n01=A\n").unwrap();
        let t = file.global.unwrap();
        assert_eq!(t.decode(&[0x00]), "");
        // Empty values are excluded from encoding (zero-advance guard).
        assert_eq!(t.encode("A"), vec![0x01]);
        assert!(t.encode("").is_empty());
    }

    #[test]
    fn non_ascii_values_round_trip() {
        let (file, _) = parse_lmtbl("00=é\n").unwrap();
        let t = file.global.unwrap();
        assert_eq!(t.decode(&[0x00]), "é");
        assert_eq!(t.encode("é"), vec![0x00]);
        // Skipping works on char boundaries, not bytes.
        assert_eq!(t.encode("aé"), vec![0x00]);
    }
}
