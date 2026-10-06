//! Headless mock screenshot of the Lunar Magic v3.40 "Custom Table File"
//! (.lmtbl) support in the three overworld text dialogs.
//!
//! egui can't render headless, so this composes an honest mock: every string
//! on screen is real — the `.lmtbl` source is the exact text fed to the real
//! `smwe_rom::table_file::parse_lmtbl`, the parse summary comes from the
//! real parse result, the level-name bytes are real vanilla ROM bytes decoded
//! with the real `decode_name_tiles` + `Table::decode`, the message bytes
//! are the real Intro message decoded with the real
//! `decode_message_with_table`, and the encode demo uses the real
//! `Table::encode` with the real byte budget. Only the window chrome (title
//! bar, buttons, panels) is drawn rather than real egui widgets.
//!
//! ```sh
//! cargo run --bin render_table_file -- --rom=smw.smc --out=docs/screenshots/table-file-support.png
//! ```

use std::path::Path;

use ab_glyph::{Font, FontRef, Glyph, Point, PxScale, ScaleFont};
use image::{Rgb, RgbImage};
use smw_editor::render_util::{fill_rect, rect_border};

const MONO_CANDIDATES: &[&str] = &["/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"];
const SANS_CANDIDATES: &[&str] = &["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"];
const SANS_BOLD_CANDIDATES: &[&str] = &["/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"];

fn load_font(candidates: &[&str]) -> anyhow::Result<FontRef<'static>> {
    for p in candidates {
        if let Ok(data) = std::fs::read(p) {
            let leaked: &'static [u8] = Box::leak(data.into_boxed_slice());
            return FontRef::try_from_slice(leaked).map_err(|e| anyhow::anyhow!("{p}: {e}"));
        }
    }
    anyhow::bail!("no font file found; tried {candidates:?}")
}

fn draw_text(img: &mut RgbImage, font: &FontRef, text: &str, x: i32, y: i32, px: f32, color: Rgb<u8>) {
    let scaled = font.as_scaled(PxScale::from(px));
    let mut caret_x = x as f32;
    let baseline = y as f32 + scaled.ascent();
    let mut prev = None;
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        if let Some(p) = prev {
            caret_x += scaled.kern(p, id);
        }
        let glyph = Glyph { id, scale: PxScale::from(px), position: Point { x: caret_x, y: baseline } };
        if let Some(o) = scaled.outline_glyph(glyph) {
            let bb = o.px_bounds();
            o.draw(|gx, gy, v| {
                let (px_x, px_y) = (bb.min.x as i32 + gx as i32, bb.min.y as i32 + gy as i32);
                if px_x >= 0 && px_y >= 0 && (px_x as u32) < img.width() && (px_y as u32) < img.height() {
                    let d = img.get_pixel(px_x as u32, px_y as u32).0;
                    let s = color.0;
                    let a = (v * 255.0) as u16;
                    let inv = 255 - a;
                    img.put_pixel(
                        px_x as u32,
                        px_y as u32,
                        Rgb([
                            ((s[0] as u16 * a + d[0] as u16 * inv) / 255) as u8,
                            ((s[1] as u16 * a + d[1] as u16 * inv) / 255) as u8,
                            ((s[2] as u16 * a + d[2] as u16 * inv) / 255) as u8,
                        ]),
                    );
                }
            });
        }
        caret_x += scaled.h_advance(id);
        prev = Some(id);
    }
}

fn hex_dump(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

/// The demo `.lmtbl`: built programmatically so the message/boss tables
/// cover the real fonts (readable decodes), while keeping LM's MultiTile
/// example `38393A3B3C=YELLOW`. This exact text is fed to the real parser;
/// the left panel shows it abridged (labeled as such).
fn demo_lmtbl() -> String {
    let mut s = String::new();
    // Level names: A-Z, 0-9, space, and LM's own YELLOW MultiTile example.
    s.push_str("@LevelNames\n");
    for (i, c) in ('A'..='Z').enumerate() {
        s.push_str(&format!("{i:02X}={c}\n"));
    }
    for (i, c) in ('1'..='9').enumerate() {
        s.push_str(&format!("{:02X}={c}\n", 0x64 + i));
    }
    s.push_str("6D=0\n1F= \n38393A3B3C=YELLOW\n\n");
    // Message boxes: the real message font (A-Z, a-z, space, punctuation).
    s.push_str("@MessageBox\n");
    for (i, c) in ('A'..='Z').enumerate() {
        s.push_str(&format!("{i:02X}={c}\n"));
    }
    for (i, c) in ('a'..='z').enumerate() {
        s.push_str(&format!("{:02X}={c}\n", 0x40 + i));
    }
    s.push_str("1F= \n1A=!\n1B=.\n1C=\"\n1D=,\n1E=?\n5D='\n\n");
    // Boss sequences: message font + '#' and digits (the real boss font).
    s.push_str("@BossSequence\n");
    for (i, c) in ('A'..='Z').enumerate() {
        s.push_str(&format!("{i:02X}={c}\n"));
    }
    for (i, c) in ('a'..='z').enumerate() {
        s.push_str(&format!("{:02X}={c}\n", 0x40 + i));
    }
    s.push_str("1F= \n1A=!\n1E=?\n5A=#\n");
    for (i, c) in ('1'..='9').enumerate() {
        s.push_str(&format!("{:02X}={c}\n", 0x63 + i));
    }
    s.push_str("6C=0\n");
    s
}

fn draw_panel(img: &mut RgbImage, sans_bold: &FontRef, x: u32, y: u32, w: u32, h: u32, title: &str) -> u32 {
    fill_rect(img, x, y, w, h, Rgb([0x25, 0x28, 0x2C]));
    rect_border(img, x, y, w, h, Rgb([0x4A, 0x4E, 0x54]));
    fill_rect(img, x, y, w, 34, Rgb([0x12, 0x14, 0x16]));
    draw_text(img, sans_bold, title, (x + 14) as i32, (y + 9) as i32, 14.0, Rgb([0xE8, 0xE8, 0xE8]));
    y + 48
}

fn draw_button(img: &mut RgbImage, sans: &FontRef, x: u32, y: u32, label: &str) {
    let w = 150u32;
    fill_rect(img, x, y, w, 30, Rgb([0x2F, 0x6F, 0xBD]));
    rect_border(img, x, y, w, 30, Rgb([0x6A, 0x6E, 0x74]));
    draw_text(img, sans, label, (x + 12) as i32, (y + 7) as i32, 13.0, Rgb([0xFF, 0xFF, 0xFF]));
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let output =
        args.iter().find_map(|a| a.strip_prefix("--out=")).unwrap_or("docs/screenshots/table-file-support.png");
    let rom_path =
        args.iter().find_map(|a| a.strip_prefix("--rom=")).map(Path::new).unwrap_or_else(|| Path::new("smw.smc"));

    let raw = std::fs::read(rom_path)?;
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
    let header_offset = 0usize;

    // ---- Real parse of the demo .lmtbl ----
    let demo_src = demo_lmtbl();
    let (file, warnings) = smwe_rom::table_file::parse_lmtbl(&demo_src)?;
    let level_table = file.table_for(smwe_rom::table_file::TableDialog::LevelNames).expect("LevelNames table");
    let msg_table = file.table_for(smwe_rom::table_file::TableDialog::MessageBox).expect("MessageBox table");
    let boss_table = file.table_for(smwe_rom::table_file::TableDialog::BossSequence).expect("BossSequence table");

    // ---- Real level-name decode: the translevel whose vanilla name uses
    // the squished YELLO tiles, decoded through the table's MultiTile entry.
    let vanilla: Vec<String> =
        smwe_rom::overworld::level_names::decode_all(&rom_bytes, header_offset, false, true).unwrap_or_default();
    let yellow_tl = vanilla.iter().position(|n| n.contains("YELLO")).unwrap_or(0);
    let yellow_tiles = smwe_rom::overworld::level_names::decode_name_tiles(&rom_bytes, header_offset, yellow_tl, false)
        .unwrap_or_default();
    let yellow_decoded = level_table.decode(&yellow_tiles);

    // ---- Real encode demo: "YELLOW SWITCH PALACE" through the table ----
    let typed = "YELLOW SWITCH PALACE";
    let encoded = level_table.encode(typed);
    let budget_ok = encoded.len() <= smwe_rom::overworld::level_names::MAX_NAME_CHARS;

    // ---- Real message decode: the Intro message through the @MessageBox table ----
    let rom = smwe_rom::snes_utils::rom::Rom::new(rom_bytes.clone())?;
    let messages = smwe_rom::message_boxes::MessageBoxes::parse(&rom)?;
    let intro_bytes = &messages.messages[0];
    let intro_text = smwe_rom::font_map::decode_message_with_table(msg_table, intro_bytes);
    let intro_rows: Vec<&str> = intro_text.lines().collect();

    // ---- Real boss decode: Iggy's first message through the @BossSequence table ----
    let boss_text = smwe_rom::boss_text::BossText::parse(&rom)?;
    let iggy0 = &boss_text.messages[0][0];
    let iggy_tiles = iggy0.char_bytes();
    let iggy_decoded = iggy0.text_with_table(Some(boss_table));

    // ---- Compose ----
    let (w, h) = (1280u32, 960u32);
    let mut img = RgbImage::new(w, h);
    fill_rect(&mut img, 0, 0, w, h, Rgb([0x1A, 0x1D, 0x21]));

    let sans = load_font(SANS_CANDIDATES)?;
    let sans_bold = load_font(SANS_BOLD_CANDIDATES)?;
    let mono = load_font(MONO_CANDIDATES)?;
    let ink = Rgb([0xE8, 0xE8, 0xE8]);
    let dim = Rgb([0xA8, 0xA8, 0xA8]);
    let accent = Rgb([0x7F, 0xC8, 0xFF]);

    // Title bar.
    fill_rect(&mut img, 0, 0, w, 52, Rgb([0x12, 0x14, 0x16]));
    draw_text(&mut img, &sans_bold, "Custom Table File (.lmtbl) — Lunar Magic v3.40 parity", 20, 15, 17.0, ink);

    // Left: the .lmtbl source (abridged for display) + parse summary.
    // Every shown line is verbatim from the real parser input.
    let mut y = draw_panel(&mut img, &sans_bold, 24, 68, 560, 800, "demo.lmtbl — parsed by the real parser");
    let display_lines = [
        "@LevelNames",
        "00=A",
        "01=B",
        "02=C",
        "...",
        "38393A3B3C=YELLOW",
        " ",
        "@MessageBox",
        "00=A",
        "01=B",
        "1F= ",
        "...",
        " ",
        "@BossSequence",
        "00=A",
        "01=B",
        "1F= ",
        "...",
        "5A=#",
    ];
    for show in display_lines {
        let color = if show.starts_with('@') { accent } else { ink };
        draw_text(&mut img, &mono, show, 44, y as i32, 12.5, color);
        y += 19;
    }
    draw_text(
        &mut img,
        &mono,
        &format!("... ({} total lines; abridged for display)", demo_src.lines().count()),
        44,
        y as i32,
        12.5,
        dim,
    );
    y += 28;
    draw_text(
        &mut img,
        &sans,
        &format!(
            "parsed: {} named tables ({} + {} + {} entries), {} warning(s)",
            file.named.len(),
            level_table.len(),
            msg_table.len(),
            boss_table.len(),
            warnings.len()
        ),
        44,
        y as i32,
        12.5,
        dim,
    );
    y += 24;
    draw_text(&mut img, &sans, "MultiTile 38393A3B3C=YELLOW: 5 tiles -> \"YELLOW\"", 44, y as i32, 12.5, dim);
    y += 24;
    draw_text(
        &mut img,
        &sans,
        "While a table is active, the built-in mapping is fully replaced;",
        44,
        y as i32,
        12.5,
        dim,
    );
    y += 20;
    draw_text(
        &mut img,
        &sans,
        "unmapped bytes show as <XX>, unmapped typed chars are skipped.",
        44,
        y as i32,
        12.5,
        dim,
    );

    // Right top: level names dialog.
    let mut y = draw_panel(&mut img, &sans_bold, 608, 68, 648, 330, "Edit Level Names — table active");
    draw_text(
        &mut img,
        &sans,
        &format!("ROM bytes (translevel {yellow_tl}): {}", hex_dump(&yellow_tiles)),
        628,
        y as i32,
        12.5,
        dim,
    );
    y += 24;
    draw_text(&mut img, &sans, "Decoded with table:", 628, y as i32, 12.5, dim);
    y += 22;
    draw_text(&mut img, &mono, &yellow_decoded, 628, y as i32, 14.0, ink);
    y += 30;
    draw_text(
        &mut img,
        &sans,
        &format!("Typed: \"{typed}\"  ->  bytes: {}", hex_dump(&encoded)),
        628,
        y as i32,
        12.5,
        dim,
    );
    y += 24;
    draw_text(
        &mut img,
        &sans,
        &format!(
            "Encodes to {} / {} tiles — {}",
            encoded.len(),
            smwe_rom::overworld::level_names::MAX_NAME_CHARS,
            if budget_ok { "within budget" } else { "OVER BUDGET (refused)" }
        ),
        628,
        y as i32,
        12.5,
        if budget_ok { dim } else { Rgb([0xFF, 0x60, 0x60]) },
    );
    y += 30;
    draw_button(&mut img, &sans, 628, y, "Load Table File...");
    draw_text(
        &mut img,
        &sans,
        &format!("Table file: demo.lmtbl ({} entries)", level_table.len()),
        792,
        (y + 7) as i32,
        12.5,
        dim,
    );

    // Right middle: message box dialog.
    let mut y = draw_panel(&mut img, &sans_bold, 608, 414, 648, 250, "Edit Message Box Text — table active");
    draw_text(
        &mut img,
        &sans,
        &format!("ROM bytes (Intro, first 20): {}", hex_dump(&intro_bytes[..intro_bytes.len().min(20)])),
        628,
        y as i32,
        12.5,
        dim,
    );
    y += 24;
    draw_text(&mut img, &sans, "Decoded rows 1-2 (8x18 structure kept):", 628, y as i32, 12.5, dim);
    y += 22;
    for row in intro_rows.iter().take(2) {
        draw_text(&mut img, &mono, row.trim_end(), 628, y as i32, 13.0, ink);
        y += 22;
    }
    y += 6;
    draw_text(&mut img, &sans, "Unmapped bytes render as <XX> hex escapes (display-only).", 628, y as i32, 12.5, dim);

    // Right bottom: boss sequence dialog.
    let mut y = draw_panel(&mut img, &sans_bold, 608, 680, 648, 188, "Edit Boss Sequence Text — table active");
    draw_text(
        &mut img,
        &sans,
        &format!("ROM tiles (Iggy M1, first 12): {}", hex_dump(&iggy_tiles[..iggy_tiles.len().min(12)])),
        628,
        y as i32,
        12.5,
        dim,
    );
    y += 24;
    draw_text(&mut img, &sans, "Decoded with table:", 628, y as i32, 12.5, dim);
    y += 22;
    let short: String = iggy_decoded.chars().take(48).collect();
    draw_text(&mut img, &mono, short.trim_end(), 628, y as i32, 13.0, ink);
    y += 28;
    draw_text(&mut img, &sans, "Slot budget counts encoded bytes; shorter text re-pads.", 628, y as i32, 12.5, dim);

    // Caption strip.
    fill_rect(&mut img, 24, 884, 1232, 52, Rgb([0x12, 0x14, 0x16]));
    rect_border(&mut img, 24, 884, 1232, 52, Rgb([0x4A, 0x4E, 0x54]));
    draw_text(
        &mut img,
        &sans,
        "headless mock — all strings are real (table_file parser/codec over real ROM bytes; only window chrome is drawn)",
        40,
        900,
        12.5,
        ink,
    );

    img.save(output)?;
    println!("wrote {output}");
    Ok(())
}
