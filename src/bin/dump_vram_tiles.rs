//! Dump the emulator's VRAM after level load as tile-sheet PNGs (2bpp and
//! 4bpp interpretations) to locate graphics empirically.
use std::{env, path::Path, sync::Arc};

use image::{ImageBuffer, Rgb};
use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};

fn main() {
    let args: Vec<String> = env::args().collect();
    let level = args
        .iter()
        .find_map(|a| a.strip_prefix("--level="))
        .and_then(|s| u16::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0x105);
    let rom_path = args.iter().find_map(|a| a.strip_prefix("--rom=")).map(Path::new).unwrap_or(Path::new("smw.smc"));
    let out_prefix = args.iter().find_map(|a| a.strip_prefix("--out=")).unwrap_or("/tmp/vram");

    let raw = std::fs::read(rom_path).expect("cannot read ROM");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
    let mut emu_rom = EmuRom::new(rom_bytes);
    emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    let mut cpu = Cpu::new(CheckedMem::new(Arc::new(emu_rom)));
    smwe_emu::emu::decompress_sublevel(&mut cpu, level);

    let vram = &cpu.mem.vram;
    // 2bpp sheet: 4096 tiles, 64 per row -> 512x512.
    let tiles_per_row = 64u32;
    let rows = (vram.len() / 16) as u32 / tiles_per_row;
    let (w, h) = (tiles_per_row * 8, rows * 8);
    let mut px = vec![0u8; (w * h * 3) as usize];
    for t in 0..(vram.len() / 16) as u32 {
        let (tx, ty) = (t % tiles_per_row, t / tiles_per_row);
        for y in 0..8u32 {
            let off = (t * 16 + y * 2) as usize;
            let (b0, b1) = (vram[off], vram[off + 1]);
            for x in 0..8u32 {
                let bit = 7 - x;
                let c = ((b0 >> bit) & 1) | (((b1 >> bit) & 1) << 1);
                let v = c * 85;
                let p = (((ty * 8 + y) * w + tx * 8 + x) * 3) as usize;
                px[p] = v;
                px[p + 1] = v;
                px[p + 2] = v;
            }
        }
    }
    ImageBuffer::<Rgb<u8>, _>::from_raw(w, h, px).unwrap().save(format!("{out_prefix}_2bpp.png")).unwrap();
    println!("wrote {out_prefix}_2bpp.png ({} tiles, {} per row)", vram.len() / 16, tiles_per_row);
}
