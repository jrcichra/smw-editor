//! Headless screenshot of "Copy Overworld to Another ROM" (Lunar Magic v3.40
//! parity).
//!
//! egui can't render headless, so this composes an honest mock: the dialog
//! text is quoted from the real `copy_overworld_window` strings, and the
//! before/after overworld images are REAL emulator renders — the destination
//! scratch ROM is rendered before the transfer and again after running the
//! real `smwe_rom::overworld_transfer::transfer_overworld` on it, with the
//! source carrying a real L1-tile edit made through the same byte path the
//! transfer copies. The red outline is the real pixel diff bounding box.
//!
//! ```sh
//! cargo run --bin render_copy_overworld -- --rom=smw.smc --out=docs/screenshots/copy-overworld.png
//! ```

use std::{env, path::Path, sync::Arc};

use ab_glyph::{Font, FontRef, Point, PxScale, ScaleFont};
use image::{Rgb, RgbImage};
use smw_editor::render_util::render_tile;
use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};
use smwe_rom::{
    overworld::{OverworldData, OWL1_TILE_DATA_SNES},
    overworld_transfer::transfer_overworld,
    snes_utils::{addr::AddrPc, rom::Rom},
};

const VRAM_L1_TILEMAP_BASE: usize = 0x2000 * 2;
const VRAM_L2_TILEMAP_BASE: usize = 0x3000 * 2;
const OW_COLS: u32 = 64;
const OW_ROWS: u32 = 64;

const SANS_CANDIDATES: &[&str] = &["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"];
const SANS_BOLD_CANDIDATES: &[&str] = &["/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"];

fn load_font(candidates: &[&str]) -> anyhow::Result<FontRef<'static>> {
    for p in candidates {
        if let Ok(data) = std::fs::read(p) {
            let leaked: &'static [u8] = Box::leak(data.into_boxed_slice());
            return FontRef::try_from_slice(leaked).map_err(|e| anyhow::anyhow!("{p}: font error: {e}"));
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
        prev = Some(id);
        let glyph = id.with_scale_and_position(px, Point { x: caret_x, y: baseline });
        if let Some(out) = scaled.outline_glyph(glyph) {
            let bb = out.px_bounds();
            out.draw(|gx, gy, v| {
                let px_x = bb.min.x as i32 + gx as i32;
                let px_y = bb.min.y as i32 + gy as i32;
                if px_x >= 0 && px_y >= 0 && (px_x as u32) < img.width() && (px_y as u32) < img.height() {
                    let dst = img.get_pixel(px_x as u32, px_y as u32);
                    let a = v;
                    let r = (color[0] as f32 * a + dst[0] as f32 * (1.0 - a)) as u8;
                    let g = (color[1] as f32 * a + dst[1] as f32 * (1.0 - a)) as u8;
                    let b = (color[2] as f32 * a + dst[2] as f32 * (1.0 - a)) as u8;
                    img.put_pixel(px_x as u32, px_y as u32, Rgb([r, g, b]));
                }
            });
        }
        caret_x += scaled.h_advance(id);
    }
}

fn tilemap_vram_addr(base: usize, col: u32, row: u32) -> usize {
    let quadrant = ((row / 32) * 2) + (col / 32);
    let sub_row = row % 32;
    let sub_col = col % 32;
    let quadrant_offset = quadrant * 32 * 32 * 2;
    let idx = quadrant_offset + ((sub_row * 32 + sub_col) * 2);
    base + idx as usize
}

fn render_bg(vram: &[u8], tilemap_base: usize, scroll_x: i32, scroll_y: i32, cgram: &[u8], pixels: &mut [u8]) {
    for row in 0..OW_ROWS {
        for col in 0..OW_COLS {
            let addr = tilemap_vram_addr(tilemap_base, col, row);
            let t0 = vram[addr] as u16;
            let t1 = vram[addr + 1] as u16;
            let x = (col * 8) as i32 - scroll_x;
            let y = (row * 8) as i32 - scroll_y;
            if x <= -8 || y <= -8 || x >= 512 || y >= 512 {
                continue;
            }
            render_tile(
                vram,
                cgram,
                (t0 | ((t1 & 3) << 8)) as usize,
                ((t1 >> 2) & 7) as usize,
                (t1 & 0x40) != 0,
                (t1 & 0x80) != 0,
                x.max(0) as u32,
                y.max(0) as u32,
                512,
                pixels,
            );
        }
    }
}

/// Render overworld submap 0 of `rom_bytes` (headerless) through the real
/// game init, like `render_ow_submap --submap=0`.
fn render_submap0(rom_bytes: &[u8]) -> RgbImage {
    let mut emu_rom = EmuRom::new(rom_bytes.to_vec());
    emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    let mut cpu = Cpu::new(CheckedMem::new(Arc::new(emu_rom)));
    for addr in 0x1F02u32..=0x1F60 {
        cpu.mem.store_u8(addr, 0xFF);
    }
    smwe_emu::emu::load_overworld(&mut cpu, 0);
    let l2_scroll_x = i16::from_le_bytes(cpu.mem.load_u16(0x001E).to_le_bytes()) as i32;
    let l2_scroll_y = i16::from_le_bytes(cpu.mem.load_u16(0x0020).to_le_bytes()) as i32;
    let mut pixels = vec![0u8; 512 * 512 * 3];
    render_bg(&cpu.mem.vram, VRAM_L2_TILEMAP_BASE, l2_scroll_x, l2_scroll_y, &cpu.mem.cgram, &mut pixels);
    render_bg(&cpu.mem.vram, VRAM_L1_TILEMAP_BASE, l2_scroll_x, l2_scroll_y, &cpu.mem.cgram, &mut pixels);
    RgbImage::from_raw(512, 512, pixels).expect("image buffer")
}

/// Bounding box of pixels that differ between the two renders (with a small
/// tolerance for emulator nondeterminism), padded for visibility.
fn diff_bbox(a: &RgbImage, b: &RgbImage) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..a.height() {
        for x in 0..a.width() {
            let pa = a.get_pixel(x, y);
            let pb = b.get_pixel(x, y);
            let d = pa[0].abs_diff(pb[0]) as u32 + pa[1].abs_diff(pb[1]) as u32 + pa[2].abs_diff(pb[2]) as u32;
            if d > 24 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x0 == u32::MAX {
        return None;
    }
    let pad = 10;
    Some((
        x0.saturating_sub(pad),
        y0.saturating_sub(pad),
        (x1 + pad).min(a.width() - 1),
        (y1 + pad).min(a.height() - 1),
    ))
}

fn draw_rect(img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, color: Rgb<u8>, w: u32) {
    for t in 0..w {
        for x in x0..=x1 {
            for (y, ok) in [(y0 + t, true), (y1.saturating_sub(t), true)] {
                if ok && y < img.height() {
                    img.put_pixel(x, y, color);
                }
            }
        }
        for y in y0..=y1 {
            for (x, ok) in [(x0 + t, true), (x1.saturating_sub(t), true)] {
                if ok && x < img.width() {
                    img.put_pixel(x, y, color);
                }
            }
        }
    }
}

fn blit(dst: &mut RgbImage, src: &RgbImage, ox: u32, oy: u32) {
    for y in 0..src.height() {
        for x in 0..src.width() {
            dst.put_pixel(ox + x, oy + y, *src.get_pixel(x, y));
        }
    }
}

fn fill_rect(img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, color: Rgb<u8>) {
    for y in y0..=y1.min(img.height() - 1) {
        for x in x0..=x1.min(img.width() - 1) {
            img.put_pixel(x, y, color);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    let rom_path =
        args.iter().find_map(|a| a.strip_prefix("--rom=")).map(Path::new).unwrap_or_else(|| Path::new("smw.smc"));
    let output = args.iter().find_map(|a| a.strip_prefix("--out=")).unwrap_or("docs/screenshots/copy-overworld.png");

    let raw = std::fs::read(rom_path).expect("cannot read ROM");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };

    // ── Source scratch: real ROM + a visible L1 edit through the real path ──
    let mut src_image = rom_bytes.clone();
    {
        let ow = OverworldData::parse(&Rom::new(src_image.clone()).unwrap()).unwrap();
        assert_eq!(ow.layer1_tiles.len(), 0x800);
        let l1_pc = AddrPc::try_from_lorom(OWL1_TILE_DATA_SNES).unwrap().as_index();
        // Stamp an 8x4 block of level tiles onto the main map (cols 6-13,
        // rows 4-7): unmistakable in the render, copied by the transfer's
        // L1 section exactly like the world editor's save writes it.
        for row in 4..8u32 {
            for col in 6..14u32 {
                src_image[l1_pc + (row * 64 + col) as usize] = 0x56;
            }
        }
    }
    let dest_image = rom_bytes.clone();

    // ── The real transfer ──
    let outcome = transfer_overworld(&src_image, &dest_image).expect("transfer");
    let report_lines: Vec<String> =
        outcome.report.summary().lines().map(|l| l.trim_start_matches("• ").to_string()).collect();

    // ── Real renders ──
    let before = render_submap0(&dest_image);
    let after = render_submap0(&outcome.dest_bytes);
    let bbox = diff_bbox(&before, &after);
    let mut after_marked = after.clone();
    if let Some((x0, y0, x1, y1)) = bbox {
        draw_rect(&mut after_marked, x0, y0, x1, y1, Rgb([255, 40, 40]), 3);
    }

    // ── Compose ──
    let font = load_font(SANS_CANDIDATES)?;
    let bold = load_font(SANS_BOLD_CANDIDATES)?;
    let (w, label_h, dlg_h, footer_h) = (1088u32, 34u32, 300u32, 40u32 + report_lines.len() as u32 * 22);
    let h = dlg_h + label_h * 2 + 512 + 16 + footer_h + 16;
    let mut img = RgbImage::new(w, h);
    let bg = Rgb([30, 30, 34]);
    let panel = Rgb([42, 42, 48]);
    let fg = Rgb([225, 225, 228]);
    let dim = Rgb([150, 150, 158]);
    let accent = Rgb([90, 140, 220]);
    fill_rect(&mut img, 0, 0, w - 1, h - 1, bg);

    // Dialog mock (real strings from copy_overworld_window).
    fill_rect(&mut img, 16, 16, w - 17, 16 + dlg_h - 1, panel);
    draw_text(&mut img, &bold, "Copy Overworld to Another ROM", 32, 28, 20.0, fg);
    draw_text(&mut img, &font, "From (current ROM, unsaved edits included):", 32, 62, 14.0, dim);
    draw_text(&mut img, &font, "source.smc  (edited: L1 tile block stamped on the main map)", 32, 82, 14.0, fg);
    draw_text(&mut img, &font, "To:", 32, 106, 14.0, dim);
    draw_text(&mut img, &font, "dest.smc  (pristine copy — its overworld will be replaced)", 32, 126, 14.0, fg);
    draw_text(
        &mut img,
        &font,
        "Tilemaps, event data, sprites, palettes, submap music, level names, message/boss",
        32,
        156,
        14.0,
        dim,
    );
    draw_text(
        &mut img,
        &font,
        "text, start positions, reveal lists, warp links, secret exits, OW ExAnimation.",
        32,
        176,
        14.0,
        dim,
    );
    draw_text(
        &mut img,
        &font,
        "Levels, graphics and other data are left untouched. 512 KiB destinations grow to 1 MiB.",
        32,
        196,
        14.0,
        dim,
    );
    // Buttons.
    fill_rect(&mut img, 32, 228, 192, 262, accent);
    draw_text(&mut img, &bold, "Copy Overworld", 48, 236, 15.0, Rgb([255, 255, 255]));
    fill_rect(&mut img, 208, 228, 288, 262, Rgb([70, 70, 78]));
    draw_text(&mut img, &font, "Close", 232, 236, 15.0, fg);

    // Before / after renders.
    let ry = 16 + dlg_h + 8;
    draw_text(&mut img, &bold, "Destination: before", 32, ry as i32, 16.0, fg);
    draw_text(&mut img, &bold, "Destination: after transfer", 32 + 544, ry as i32, 16.0, fg);
    blit(&mut img, &before, 32, ry + label_h);
    blit(&mut img, &after_marked, 32 + 544, ry + label_h);
    if bbox.is_some() {
        draw_text(
            &mut img,
            &font,
            "red outline = real pixel diff",
            32 + 544,
            (ry + label_h + 512 + 4) as i32,
            13.0,
            dim,
        );
    }

    // Footer: the real transfer report.
    let fy = ry + label_h + 512 + 16;
    draw_text(&mut img, &bold, "Transfer report:", 32, fy as i32, 15.0, fg);
    for (i, line) in report_lines.iter().enumerate() {
        draw_text(&mut img, &font, &format!("• {line}"), 32, fy as i32 + 24 + i as i32 * 22, 13.5, dim);
    }

    img.save(output).expect("save png");
    println!("wrote {output}");
    Ok(())
}
