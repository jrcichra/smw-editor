//! Empirically resolve Lunar Magic extended Map16 pointers by running LM's
//! own GetMap16 routine at $06F540 in the emulator, and compare with the
//! editor's static `lm_map16_ptr` reconstruction.
use std::{collections::BTreeSet, env, path::Path, sync::Arc};

use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};

fn call_lm_get_map16(cpu: &mut Cpu, block_id: u16) -> (u16, u32, u16, [u16; 4]) {
    // Trampoline: JSL $06F540; STP-like end marker (we watch PC).
    cpu.emulation = false;
    cpu.ill = false;
    cpu.s = 0x1FF;
    cpu.pbr = 0;
    cpu.dbr = 0;
    cpu.pc = 0x1F00;
    // REP #$30 : LDA #id*2 : JSL $06F540 : NOP (end marker)
    cpu.mem.store_u8(0x1F00, 0xC2);
    cpu.mem.store_u8(0x1F01, 0x30);
    cpu.mem.store_u8(0x1F02, 0xA9);
    cpu.mem.store_u16(0x1F03, block_id.wrapping_mul(2));
    cpu.mem.store_u8(0x1F05, 0x22); // JSL
    cpu.mem.store_u24(0x1F06, 0x06F540);
    cpu.mem.store_u8(0x1F09, 0xEA); // NOP (end marker)
    let mut steps = 0;
    loop {
        cpu.dispatch();
        steps += 1;
        if cpu.ill || (cpu.pbr == 0 && cpu.pc == 0x1F09) || steps > 10000 {
            break;
        }
    }
    let a = cpu.a;
    let dp_b = cpu.mem.load_u16(0x0B);
    // Candidate pointer interpretations:
    // A = low 16 bits of data address, $0C = bank (per STY $0B storing bank in high byte)
    let bank = (dp_b >> 8) as u32;
    let ptr = (bank << 16) | a as u32;
    let mut words = [0u16; 4];
    for (i, w) in words.iter_mut().enumerate() {
        *w = cpu.mem.load_u16(ptr + i as u32 * 2);
    }
    (a, ptr, dp_b, words)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let level = args
        .iter()
        .find_map(|a| a.strip_prefix("--level="))
        .and_then(|s| u16::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0x105);
    let rom_path =
        args.iter().find_map(|a| a.strip_prefix("--rom=")).map(Path::new).unwrap_or(Path::new("TOP2020.smc"));

    let raw = std::fs::read(rom_path).expect("cannot read ROM");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
    let mut emu_rom = EmuRom::new(rom_bytes);
    emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    let mut cpu = Cpu::new(CheckedMem::new(Arc::new(emu_rom)));
    smwe_emu::emu::decompress_sublevel(&mut cpu, level);

    // Collect distinct extended block ids used by layer 1 of this level.
    let mut ids = BTreeSet::new();
    for idx in 0..(512 * 27u32) {
        let id = cpu.mem.load_u8(0x7EC800 + idx) as u16 | (((cpu.mem.load_u8(0x7FC800 + idx) as u16) & 0x3F) << 8);
        if id >= 0x200 {
            ids.insert(id);
        }
    }
    println!(
        "distinct extended FG ids in level {:X}: {:?}",
        level,
        ids.iter().map(|i| format!("{i:03X}")).collect::<Vec<_>>()
    );

    for &id in ids.iter().take(12) {
        let (a, ptr, dp_b, words) = call_lm_get_map16(&mut cpu, id);
        println!(
            "id {:03X}: LM routine -> A={:04X} $0B={:04X} ptr={:06X} words=[{:04X} {:04X} {:04X} {:04X}]",
            id, a, dp_b, ptr, words[0], words[1], words[2], words[3]
        );
    }
    // Also probe a vanilla id for calibration (its words are known via $0FBE).
    for id in [0x25u16, 0x100] {
        let (a, ptr, dp_b, words) = call_lm_get_map16(&mut cpu, id);
        let fbe = cpu.mem.load_u16(0x0FBE + id as u32 * 2) as u32 + 0x0D0000;
        let mut expect = [0u16; 4];
        for (i, w) in expect.iter_mut().enumerate() {
            *w = cpu.mem.load_u16(fbe + i as u32 * 2);
        }
        println!(
            "vanilla id {:03X}: LM -> A={:04X} $0B={:04X} ptr={:06X} words={:04X?} | $0FBE ptr={:06X} words={:04X?}",
            id, a, dp_b, ptr, words, fbe, expect
        );
    }
}
