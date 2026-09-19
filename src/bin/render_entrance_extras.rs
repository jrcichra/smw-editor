//! Headless mock screenshots of the LM v3.00 entrance extras UI:
//! the Level Header "Entrance Extras" section and the secondary-exit
//! extended options with the new face-left / new-FG/BG-init checkboxes.
//!
//! egui can't render headless, so this composes honest mocks of the editor
//! windows: every value drawn is REAL — the secondary-header entrance bytes
//! come from the ROM via the same `SecondaryHeader` accessors the UI uses,
//! and the extras shown are sample blocks first round-tripped through the
//! real RATS codecs (`SMWENTR1` v1, `SMWESEX2` v2). Only the window chrome
//! (title bar, checkboxes, sliders) is drawn rather than real egui widgets.
//!
//! ```sh
//! cargo run --bin render_entrance_extras -- --rom=smw.smc
//! ```
//! Writes `docs/screenshots/entrance-extras.png` and
//! `docs/screenshots/secondary-exit-v300-extras.png`.

use ab_glyph::{Font, FontRef, Glyph, Point, PxScale, ScaleFont};
use image::{Rgb, RgbImage};
use smw_editor::render_util::{fill_rect, rect_border};
use smwe_rom::{
    level::{
        entrance_extras::{LevelEntranceExtras, LevelEntranceExtrasData},
        headers::SecondaryHeader,
        secondary_entrance::{SecondaryExitExtData, SecondaryExitOptions},
    },
    snes_utils::rom::Rom,
};

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

struct Fonts {
    mono:      FontRef<'static>,
    sans:      FontRef<'static>,
    sans_bold: FontRef<'static>,
}

/// Draw one line of text; returns the advance width in px.
fn draw_text(img: &mut RgbImage, font: &FontRef, text: &str, x: i32, y: i32, px: f32, color: Rgb<u8>) -> i32 {
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
                let px_x = bb.min.x as i32 + gx as i32;
                let px_y = bb.min.y as i32 + gy as i32;
                if px_x >= 0 && px_y >= 0 {
                    let (px_x, px_y) = (px_x as u32, px_y as u32);
                    if px_x < img.width() && px_y < img.height() {
                        let d = img.get_pixel(px_x, px_y).0;
                        let s = color.0;
                        let a = (v * 255.0) as u16;
                        let inv = 255 - a;
                        img.put_pixel(
                            px_x,
                            px_y,
                            Rgb([
                                ((s[0] as u16 * a + d[0] as u16 * inv) / 255) as u8,
                                ((s[1] as u16 * a + d[1] as u16 * inv) / 255) as u8,
                                ((s[2] as u16 * a + d[2] as u16 * inv) / 255) as u8,
                            ]),
                        );
                    }
                }
            });
        }
        caret_x += scaled.h_advance(id);
        prev = Some(id);
    }
    (caret_x - x as f32) as i32
}

fn title_bar(img: &mut RgbImage, fonts: &Fonts, title: &str, w: u32) {
    fill_rect(img, 0, 0, w, 52, Rgb([0x2B, 0x2B, 0x2B]));
    draw_text(img, &fonts.sans_bold, title, 24, 15, 19.0, Rgb([0xFF, 0xFF, 0xFF]));
}

/// Draw a checkbox row; returns the y of the next row.
fn checkbox_row(
    img: &mut RgbImage, fonts: &Fonts, label: &str, hover: &str, checked: bool, enabled: bool, x: u32, y: u32,
) -> u32 {
    let ink = if enabled { Rgb([0x1A, 0x1A, 0x1A]) } else { Rgb([0x99, 0x99, 0x99]) };
    if checked {
        fill_rect(img, x, y, 18, 18, Rgb([0x1A, 0x5A, 0x9A]));
        draw_text(img, &fonts.sans_bold, "✓", (x + 4) as i32, (y as i32) - 2, 15.0, Rgb([0xFF, 0xFF, 0xFF]));
    } else {
        rect_border(img, x, y, 18, 18, Rgb([0x99, 0x99, 0x99]));
    }
    draw_text(img, &fonts.sans, label, (x + 28) as i32, y as i32, 15.0, ink);
    draw_text(img, &fonts.sans, hover, (x + 28) as i32, (y + 22) as i32, 12.0, Rgb([0x66, 0x66, 0x66]));
    y + 52
}

/// Prove the sample extras survive the real SMWENTR1 RATS codec.
fn sample_extras() -> LevelEntranceExtrasData {
    let mut data = LevelEntranceExtrasData::default();
    data.set(0x105, LevelEntranceExtras {
        face_left:              true,
        new_fg_bg_init:         true,
        bg_relative_to_fg_only: true,
        bg_height:              0x40,
    });
    // A second sparse entry proves multi-level storage.
    data.set(0x001, LevelEntranceExtras { face_left: true, ..Default::default() });
    let mut rom = vec![0xFFu8; 0x40000];
    data.write_to_rom(&mut rom, 0).expect("encode sample");
    let back = LevelEntranceExtrasData::parse(&rom).expect("decode sample");
    assert_eq!(back, data, "sample entrance extras must survive the RATS codec");
    back
}

/// Prove the sample secondary-exit options survive the real SMWESEX2 v2 codec.
fn sample_se_options() -> SecondaryExitOptions {
    let mut data = SecondaryExitExtData::default();
    let opts = SecondaryExitOptions { water_level: true, face_left: true, new_fg_bg_init: true, ..Default::default() };
    data.options.insert(0x002, opts);
    let mut rom = vec![0xFFu8; 0x40000];
    data.write_to_rom(&mut rom, 0).expect("encode sample");
    let back = SecondaryExitExtData::parse(&rom).expect("decode sample");
    let back_opts = back.options_for(0x002);
    assert_eq!(back_opts, opts, "sample secondary-exit options must survive the v2 RATS codec");
    back_opts
}

fn render_entrance_extras(fonts: &Fonts, rom: &Rom, data: &LevelEntranceExtrasData, out: &str) -> anyhow::Result<()> {
    let (w, h) = (1100u32, 640u32);
    let mut img = RgbImage::new(w, h);
    let bg = Rgb([0xF2, 0xF2, 0xF2]);
    let ink = Rgb([0x1A, 0x1A, 0x1A]);
    let gray = Rgb([0x66, 0x66, 0x66]);
    for p in img.pixels_mut() {
        *p = bg;
    }
    title_bar(&mut img, fonts, "Level Header — Entrance Extras (LM v3.00) — headless mock", w);

    // Real secondary-header entrance values for level 0x105, via the same
    // accessors the UI uses.
    let sh = SecondaryHeader::read_from_rom(rom, 0x105).expect("read level 0x105 secondary header");
    let (ex, ey) = sh.main_entrance_xy_pos();
    let mut y = 76u32;
    draw_text(
        &mut img,
        &fonts.sans_bold,
        "Level 0x105 — vanilla entrance bytes (real ROM data)",
        24,
        y as i32,
        16.0,
        ink,
    );
    y += 32;
    draw_text(
        &mut img,
        &fonts.mono,
        &format!(
            "Entrance screen: 0x{:02X}   X: {}   Y: {}   Action: {}   FG init: {}   BG init: {}",
            sh.main_entrance_screen(),
            ex,
            ey,
            sh.main_entrance_mario_action(),
            sh.fg_initial_pos(),
            sh.bg_initial_pos()
        ),
        24,
        y as i32,
        14.0,
        ink,
    );
    y += 34;
    draw_text(
        &mut img,
        &fonts.sans,
        "Alt+Right-click the red M entrance marker in sprite mode to open this window (LM v2.20);",
        24,
        y as i32,
        13.0,
        gray,
    );
    y += 22;
    draw_text(
        &mut img,
        &fonts.sans,
        "Shift+drag the marker to move the entrance — the header bytes above are rewritten on save.",
        24,
        y as i32,
        13.0,
        gray,
    );
    y += 40;

    draw_text(
        &mut img,
        &fonts.sans_bold,
        "Entrance Extras (LM v3.00) — sample for level 0x105",
        24,
        y as i32,
        16.0,
        ink,
    );
    y += 32;
    draw_text(
        &mut img,
        &fonts.sans,
        "No vanilla ROM storage — persisted in the editor's RATS block (SMWENTR1).",
        24,
        y as i32,
        13.0,
        gray,
    );
    y += 30;

    let e = data.extras_for(0x105);
    y = checkbox_row(
        &mut img,
        fonts,
        "Face Left",
        "LM v3.00: Mario faces left on the main entrance (also flips \"Shoot From Slanted Pipe Right\").",
        e.face_left,
        true,
        24,
        y,
    );
    y = checkbox_row(
        &mut img,
        fonts,
        "New FG/BG Init System",
        "LM v3.00: FG initial position relative to the player; BG calculated from FG, scroll, level height, BG height.",
        e.new_fg_bg_init,
        true,
        24,
        y,
    );
    let e1 = data.extras_for(0x105);
    y = checkbox_row(
        &mut img,
        fonts,
        "BG Relative to FG Only",
        "LM v3.00 sub-option, mainly for Layer 2 levels. Enabled only with the new FG/BG init system.",
        e1.bg_relative_to_fg_only,
        e1.new_fg_bg_init,
        24,
        y,
    );

    // BG height slider row.
    draw_text(&mut img, &fonts.sans, "BG Height:", 24, y as i32, 15.0, ink);
    draw_text(
        &mut img,
        &fonts.sans,
        "LM v3.00 \"Change Other Properties\" setting. 0 = unset (vanilla behavior).",
        24,
        (y + 22) as i32,
        12.0,
        gray,
    );
    let track_x0 = 320u32;
    let track_w = 400u32;
    let frac = e.bg_height as f32 / 255.0;
    fill_rect(&mut img, track_x0, y + 6, track_w, 6, Rgb([0xCC, 0xCC, 0xCC]));
    fill_rect(&mut img, track_x0, y + 6, (track_w as f32 * frac) as u32, 6, Rgb([0x1A, 0x5A, 0x9A]));
    let knob_x = track_x0 + (track_w as f32 * frac) as u32;
    fill_rect(&mut img, knob_x.saturating_sub(6), y, 12, 18, Rgb([0x33, 0x33, 0x33]));
    draw_text(
        &mut img,
        &fonts.mono,
        &format!("0x{:02X}", e.bg_height),
        (track_x0 + track_w + 16) as i32,
        y as i32,
        15.0,
        ink,
    );

    let cy = h - 64;
    draw_text(
        &mut img,
        &fonts.sans,
        "Mock window chrome — the header bytes are real ROM data and the extras were round-tripped through",
        24,
        cy as i32,
        13.0,
        gray,
    );
    draw_text(
        &mut img,
        &fonts.sans,
        "the real SMWENTR1 RATS codec. In-game playback of every option needs Lunar Magic's ASM hacks.",
        24,
        (cy + 22) as i32,
        13.0,
        gray,
    );

    img.save(out)?;
    println!("wrote {out} ({w}x{h})");
    Ok(())
}

fn render_se_extras(fonts: &Fonts, opts: &SecondaryExitOptions, out: &str) -> anyhow::Result<()> {
    let (w, h) = (1100u32, 560u32);
    let mut img = RgbImage::new(w, h);
    let bg = Rgb([0xF2, 0xF2, 0xF2]);
    let gray = Rgb([0x66, 0x66, 0x66]);
    for p in img.pixels_mut() {
        *p = bg;
    }
    title_bar(&mut img, fonts, "Secondary Entrance 0x002 — extended options — headless mock", w);

    let mut y = 76u32;
    y = checkbox_row(
        &mut img,
        fonts,
        "Water level — the destination plays as a water level",
        "(pre-existing LM v3.00 option; badges: W)",
        opts.water_level,
        true,
        24,
        y,
    );
    y = checkbox_row(
        &mut img,
        fonts,
        "Face left — Mario faces the left direction on this entrance",
        "New in this PR (LM v3.00). Stored in the SMWESEX2 block, v2 flags; badges: FL.",
        opts.face_left,
        true,
        24,
        y,
    );
    y = checkbox_row(
        &mut img,
        fonts,
        "New FG/BG init system — FG relative to player, BG computed from FG/scroll/height",
        "New in this PR (LM v3.00). Stored in the SMWESEX2 block, v2 flags; badges: FG.",
        opts.new_fg_bg_init,
        true,
        24,
        y,
    );

    y += 10;
    draw_text(
        &mut img,
        &fonts.sans,
        "v1 SMWESEX2 payloads (from the earlier secondary-entrances PR) still decode with the new flags defaulted off.",
        24,
        y as i32,
        13.0,
        gray,
    );

    let cy = h - 64;
    draw_text(
        &mut img,
        &fonts.sans,
        "Mock window chrome — the three checked options are a sample that was round-tripped through the real",
        24,
        cy as i32,
        13.0,
        gray,
    );
    draw_text(
        &mut img,
        &fonts.sans,
        "SMWESEX2 v2 RATS codec. In-game playback needs Lunar Magic's ASM hacks, which this editor does not install.",
        24,
        (cy + 22) as i32,
        13.0,
        gray,
    );

    img.save(out)?;
    println!("wrote {out} ({w}x{h})");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = args
        .iter()
        .find_map(|a| a.strip_prefix("--rom="))
        .or_else(|| args.iter().skip(1).find(|a| !a.starts_with("--")).map(|a| a.as_str()))
        .unwrap_or("smw.smc");
    let out_dir = args.iter().find_map(|a| a.strip_prefix("--out-dir=")).unwrap_or("docs/screenshots");

    let fonts = Fonts {
        mono:      load_font(MONO_CANDIDATES)?,
        sans:      load_font(SANS_CANDIDATES)?,
        sans_bold: load_font(SANS_BOLD_CANDIDATES)?,
    };

    let raw = std::fs::read(rom_path)?;
    let rom = Rom::new(raw)?;
    let extras = sample_extras();
    let se_opts = sample_se_options();

    render_entrance_extras(&fonts, &rom, &extras, &format!("{out_dir}/entrance-extras.png"))?;
    render_se_extras(&fonts, &se_opts, &format!("{out_dir}/secondary-exit-v300-extras.png"))?;
    Ok(())
}
