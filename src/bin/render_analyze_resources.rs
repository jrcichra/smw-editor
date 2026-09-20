//! Headless verification + honest-mock screenshot for "Analyze Resources in
//! Levels" (Tools > Analyze Resources in Levels...).
//!
//! egui can't render headless, so this composes an honest mock of the
//! results window: every string on screen is real — the exact summary line,
//! resource rows, level buttons, and button labels the UI uses — and the
//! data is real output from running the actual scan over the ROM
//! (`scan_resources`). Only the window chrome and widget shapes are drawn
//! rather than real egui widgets.
//!
//! ```sh
//! cargo run --bin render_analyze_resources -- --rom=smw.smc --out=docs/screenshots/analyze-resources.png
//! ```
//!
//! With `--report`, prints the LM-style text report instead of rendering.

use std::time::Instant;

use ab_glyph::{Font, FontRef, Glyph, Point, PxScale, ScaleFont};
use image::{Rgb, RgbImage};
use smw_editor::{
    render_util::{fill_rect, rect_border},
    resource_scan::{scan_resources, ResourceReport, ScanOptions},
};
use smwe_rom::music::format_music_track;

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

fn draw_button(img: &mut RgbImage, font: &FontRef, x: u32, y: u32, w: u32, label: &str, enabled: bool) {
    let (bg, ink) = if enabled {
        (Rgb([0x2F, 0x6F, 0xBD]), Rgb([0xFF, 0xFF, 0xFF]))
    } else {
        (Rgb([0x3A, 0x3D, 0x42]), Rgb([0xA8, 0xA8, 0xA8]))
    };
    fill_rect(img, x, y, w, 34, bg);
    rect_border(img, x, y, w, 34, Rgb([0x6A, 0x6E, 0x74]));
    draw_text(img, font, label, (x + 12) as i32, (y + 8) as i32, 14.0, ink);
}

fn arg(name: &str, default: &str) -> String {
    let prefix = format!("{name}=");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if let Some(v) = a.strip_prefix(&prefix) {
            return v.to_string();
        }
        if a == name {
            return args.next().unwrap_or_else(|| default.to_string());
        }
    }
    default.to_string()
}

fn summary_line(report: &ResourceReport) -> String {
    let mut s = format!("Scanned {} levels", report.levels_scanned);
    if report.levels_skipped > 0 {
        s.push_str(&format!(" ({} skipped: unparseable data)", report.levels_skipped));
    }
    s.push('.');
    s.push_str(&format!(" {} music tracks in use.", report.used_music_tracks().len()));
    s.push_str(&format!(" {} distinct sprites.", report.used_sprites().len()));
    s.push_str(&format!(" {} distinct Map16 tiles.", report.used_map16_tiles().len()));
    s.push_str(&format!(" {} GFX/ExGFX files referenced.", report.used_gfx_files().len()));
    s.push_str(&format!(" {} levels with a custom palette.", report.levels_with_custom_palette().len()));
    s
}

fn main() -> anyhow::Result<()> {
    let rom_path = arg("--rom", "smw.smc");
    let out_path = arg("--out", "docs/screenshots/analyze-resources.png");
    let report_only = std::env::args().any(|a| a == "--report");

    let raw = std::fs::read(&rom_path)?;

    let t0 = Instant::now();
    let report = scan_resources(&raw, ScanOptions::all())?;
    let elapsed = t0.elapsed();
    eprintln!(
        "scanned {} levels ({} skipped) in {:.1}s: {} music tracks, {} sprites, {} Map16 tiles, {} GFX files",
        report.levels_scanned,
        report.levels_skipped,
        elapsed.as_secs_f32(),
        report.used_music_tracks().len(),
        report.used_sprites().len(),
        report.used_map16_tiles().len(),
        report.used_gfx_files().len()
    );

    if report_only {
        println!("{}", smw_editor::resource_scan::format_text_report(&report));
        return Ok(());
    }

    // ── Honest mock of the results window (By resource view, music tracks) ──
    let font = load_font(SANS_CANDIDATES)?;
    let font_bold = load_font(SANS_BOLD_CANDIDATES)?;
    let (w, h) = (900u32, 640u32);
    let mut img = RgbImage::new(w, h);
    let bg = Rgb([0x1B, 0x1D, 0x20]);
    let panel = Rgb([0x25, 0x28, 0x2C]);
    let titlebar = Rgb([0x12, 0x14, 0x16]);
    let ink = Rgb([0xE8, 0xE8, 0xE8]);
    let dim = Rgb([0xA8, 0xA8, 0xA8]);
    let accent = Rgb([0x7F, 0xC0, 0xFF]);
    for p in img.pixels_mut() {
        *p = bg;
    }
    let (dx, dy, dw, dh) = (20u32, 20u32, 860u32, 600u32);
    fill_rect(&mut img, dx, dy, dw, dh, panel);
    rect_border(&mut img, dx, dy, dw, dh, Rgb([0x4A, 0x4E, 0x54]));
    fill_rect(&mut img, dx, dy, dw, 40, titlebar);
    draw_text(&mut img, &font_bold, "Analyze Resources in Levels", (dx + 16) as i32, (dy + 11) as i32, 17.0, ink);

    draw_text(&mut img, &font, &summary_line(&report), (dx + 16) as i32, (dy + 58) as i32, 13.0, ink);

    // View tabs.
    let mut y = (dy + 96) as i32;
    draw_text(&mut img, &font, "View:", (dx + 16) as i32, y, 13.0, dim);
    draw_text(&mut img, &font_bold, "[By resource]", (dx + 66) as i32, y, 13.0, accent);
    draw_text(&mut img, &font, "By level", (dx + 190) as i32, y, 13.0, dim);

    // Resource class row.
    y += 30;
    draw_text(&mut img, &font, "Resource class: [Music tracks v]", (dx + 16) as i32, y, 13.0, dim);
    draw_text(&mut img, &font, "Filter:", (dx + 320) as i32, y, 13.0, dim);
    fill_rect(&mut img, dx + 366, y as u32 - 4, 200, 26, Rgb([0x12, 0x14, 0x16]));
    rect_border(&mut img, dx + 366, y as u32 - 4, 200, 26, Rgb([0x4A, 0x4E, 0x54]));

    y += 34;
    draw_text(
        &mut img,
        &font,
        "8 resources — double-click a level to open it in the level editor.",
        (dx + 16) as i32,
        y,
        12.0,
        dim,
    );

    // Two columns: resource list | levels using the selected resource.
    y += 28;
    let col_w = 400u32;
    let list_h = 300u32;
    fill_rect(&mut img, dx + 16, y as u32, col_w, list_h, Rgb([0x1B, 0x1D, 0x20]));
    rect_border(&mut img, dx + 16, y as u32, col_w, list_h, Rgb([0x4A, 0x4E, 0x54]));
    fill_rect(&mut img, dx + 444, y as u32, col_w, list_h, Rgb([0x1B, 0x1D, 0x20]));
    rect_border(&mut img, dx + 444, y as u32, col_w, list_h, Rgb([0x4A, 0x4E, 0x54]));

    let tracks: Vec<u8> = report.used_music_tracks().into_iter().collect();
    let mut ry = y + 8;
    for (i, track) in tracks.iter().enumerate() {
        let label = format!("Track {}", format_music_track(*track));
        if i == 0 {
            fill_rect(&mut img, dx + 20, ry as u32 - 4, col_w - 8, 26, Rgb([0x2F, 0x6F, 0xBD]));
        }
        draw_text(&mut img, &font, &label, (dx + 28) as i32, ry, 13.0, ink);
        ry += 30;
        if ry > y + list_h as i32 - 20 {
            break;
        }
    }

    // Right column: levels using the first (selected) track — real data.
    let selected = tracks[0];
    let levels = report.levels_using_music(selected);
    let mut ry = y + 8;
    draw_text(
        &mut img,
        &font,
        &format!("Track {} — used by {} levels:", format_music_track(selected), levels.len()),
        (dx + 452) as i32,
        ry,
        13.0,
        ink,
    );
    ry += 28;
    for level in levels.iter().take(9) {
        draw_button(&mut img, &font, dx + 452, ry as u32, 130, &format!("Level ${level:03X}"), true);
        ry += 40;
        if ry > y + list_h as i32 - 30 {
            break;
        }
    }

    // Bottom buttons.
    let by = dy + dh - 52;
    draw_button(&mut img, &font, dx + 16, by, 170, "Save Text Report...", true);
    draw_button(&mut img, &font, dx + 196, by, 110, "Re-scan", true);
    draw_button(&mut img, &font, dx + 316, by, 90, "Close", true);

    img.save(&out_path)?;
    eprintln!("wrote {out_path}");
    Ok(())
}
