#![allow(clippy::identity_op)]

use std::{collections::HashSet, sync::Arc};

use wdc65816::{Cpu, Mem};

use crate::rom::Rom;

#[derive(Debug, Clone)]
pub struct CheckedMem {
    pub cart:       Arc<Rom>,
    pub wram:       Vec<u8>,
    pub regs:       Vec<u8>,
    pub vram:       Vec<u8>,
    pub cgram:      Vec<u8>,
    pub extram:     Vec<u8>,
    pub uninit:     HashSet<usize>,
    pub error:      Option<u32>,
    pub err_value:  Option<u8>,
    pub last_store: Option<u32>,
}

impl CheckedMem {
    pub fn new(rom: Arc<Rom>) -> Self {
        Self {
            cart:       rom,
            wram:       Vec::from([0; 0x20000]),
            regs:       Vec::from([0; 0x6000]),
            vram:       Vec::from([0; 0x10000]),
            cgram:      Vec::from([0; 0x200]),
            extram:     Vec::from([0; 0x10000]),
            uninit:     HashSet::new(),
            error:      None,
            err_value:  None,
            last_store: None,
        }
    }

    pub fn load_u8(&mut self, addr: u32) -> u8 { self.load(addr) }
    pub fn store_u8(&mut self, addr: u32, value: u8) { self.store(addr, value) }

    pub fn load_u16(&mut self, addr: u32) -> u16 {
        let l = self.load(addr);
        let h = self.load(addr + 1);
        u16::from_le_bytes([l, h])
    }

    pub fn load_u24(&mut self, addr: u32) -> u32 {
        let l = self.load(addr);
        let h = self.load(addr + 1);
        let b = self.load(addr + 2);
        u32::from_le_bytes([l, h, b, 0])
    }

    pub fn store_u16(&mut self, addr: u32, val: u16) {
        let val = val.to_le_bytes();
        self.store(addr, val[0]);
        self.store(addr + 1, val[1]);
    }

    pub fn store_u24(&mut self, addr: u32, val: u32) {
        let val = val.to_le_bytes();
        self.store(addr, val[0]);
        self.store(addr + 1, val[1]);
        self.store(addr + 2, val[2]);
    }

    pub fn process_dma_ch(&mut self, ch: u32) {
        let a = self.load_u24(0x4302 + ch);
        let size = self.load_u16(0x4305 + ch) as u32;
        let b = self.load(0x4301 + ch);
        let params = self.load(0x4300 + ch);
        if b == 0x18 {
            let dest = self.load_u16(0x2116) as u32;
            if params & 0x8 == 0 {
                for i in 0..size {
                    self.vram[(dest * 2 + i) as usize] = self.load(a + i);
                }
            }
            self.store_u16(0x2116, (dest + size) as u16);
        } else if b == 0x22 {
            let dest = self.load(0x2121) as u32;
            for i in 0..size {
                self.cgram[(dest * 2 + i) as usize] = self.load(a + i);
            }
            self.store_u16(0x2121, (dest + size) as u16);
        } else {
            println!("DMA size {size:04X}: ${b:02X} ${a:06X}");
        }
    }

    pub fn process_dma(&mut self) {
        let dma = self.load(0x420B);
        if dma != 0 {
            for i in 0..8 {
                if dma & (1 << i) != 0 {
                    self.process_dma_ch(i * 0x10);
                }
            }
            self.store(0x420B, 0);
        }
    }

    pub fn map(&mut self, addr: u32, write: Option<u8>) -> u8 {
        let bank = addr >> 16;
        let mutable = if bank & 0xFE == 0x7E {
            &mut self.wram[(addr & 0x1FFFF) as usize]
        } else if bank == 0x60 {
            &mut self.extram[(addr & 0xFFFF) as usize]
        } else if addr & 0xFFFF < 0x2000 {
            &mut self.wram[(addr & 0x1FFF) as usize]
        } else if addr & 0xFFFF < 0x8000 {
            let ptr = (addr & 0x7FFF) as usize;
            if let Some(value) = write {
                if ptr == 0x2118 {
                    let a = self.load_u16(0x2116);
                    self.vram[(a as usize) * 2] = value;
                } else if ptr == 0x2119 {
                    let a = self.load_u16(0x2116);
                    self.vram[(a as usize) * 2 + 1] = value;
                    self.store_u16(0x2116, a + 1);
                }
            }
            &mut self.regs[ptr - 0x2000]
        } else if addr & 0xFFFF >= 0x8000 {
            if let Some(c) = self.cart.read(addr) {
                return c;
            } else {
                self.error = Some(addr);
                self.err_value.get_or_insert(0)
            }
        } else {
            self.error = Some(addr);
            self.err_value.get_or_insert(0)
        };
        if let Some(c) = write {
            *mutable = c;
        }
        *mutable
    }
}

impl Mem for CheckedMem {
    fn load(&mut self, addr: u32) -> u8 { self.map(addr, None) }
    fn store(&mut self, addr: u32, value: u8) {
        self.map(addr, Some(value));
        self.last_store = Some(addr);
    }
}

fn run_routines(cpu: &mut Cpu<CheckedMem>, routines: &[&str], cycle_limit: u64) -> u64 {
    cpu.emulation = false;
    cpu.ill = false;
    cpu.s = 0x1FF;
    cpu.pc = 0x2000;
    cpu.pbr = 0x00;
    cpu.dbr = 0x00;
    cpu.trace = false;

    let mut addr = 0x2000u32;
    for symbol in routines {
        cpu.mem.store(addr, 0x22); // JSL
        cpu.mem.store_u24(
            addr + 1,
            cpu.mem.cart.resolve(symbol).unwrap_or_else(|| panic!("no symbol: {symbol}")),
        );
        addr += 4;
    }
    let end = addr as u16;

    let mut cy = 0u64;
    loop {
        cy += cpu.dispatch() as u64;
        if cpu.ill { log::warn!("illegal instruction at {:02X}:{:04X}", cpu.pbr, cpu.pc); break; }
        if cpu.pbr == 0 && cpu.pc == end { break; }
        if cy > cycle_limit { log::warn!("exceeded cycle limit"); break; }
        cpu.mem.process_dma();
    }
    cy
}

pub fn fetch_anim_frame(cpu: &mut Cpu<CheckedMem>) -> u64 {
    run_routines(cpu, &["CODE_05BB39", "CODE_00A390"], 20_000_000)
}

pub fn exec_sprite_id(cpu: &mut Cpu<CheckedMem>, id: u8) -> u64 {
    cpu.mem.store(0x9E, id);
    cpu.mem.store(0x1A, 0x00);
    cpu.mem.store(0x1C, 0x00);
    cpu.mem.store(0xD8, 0x80);
    cpu.mem.store(0xE4, 0x80);
    for i in 0..12 { cpu.mem.store(0x14C8 + i, 0); }
    cpu.mem.store(0x14C8, 1);
    cpu.y = 0;
    cpu.x = 0;
    run_routines(cpu, &["InitSpriteTables", "CODE_01808C", "CODE_01808C"], 10_000_000)
}

pub fn exec_sprites(cpu: &mut Cpu<CheckedMem>) -> u64 {
    run_routines(cpu, &["CODE_01808C"], 20_000_000)
}

pub fn decompress_sublevel(cpu: &mut Cpu<CheckedMem>, id: u16) -> u64 {
    let now = std::time::Instant::now();
    cpu.emulation = false;
    cpu.mem.store(0x1F11, (id >> 8) as _);
    cpu.mem.store(0x141A, 1);
    cpu.ill = false;
    cpu.s = 0x1FF;
    cpu.pc = 0x2000;
    cpu.pbr = 0x00;
    cpu.dbr = 0x00;
    cpu.trace = false;

    let routines = [
        "CODE_00A993", "CODE_00B888", "CODE_05D796", "CODE_05801E",
        "UploadSpriteGFX", "LoadPalette", "CODE_00922F",
    ];
    let mut addr = 0x2000u32;
    for i in routines {
        cpu.mem.store(addr, 0x22);
        cpu.mem.store_u24(addr + 1, cpu.mem.cart.resolve(i).unwrap_or_else(|| panic!("no symbol: {}", i)));
        addr += 4;
    }
    let end = addr as u16;
    let mut cy = 0u64;
    loop {
        cy += cpu.dispatch() as u64;
        if cpu.ill { println!("ILLEGAL INSTR"); break; }
        if cpu.pc == 0xD8B7 && cpu.pbr == 0x05 { cpu.mem.store_u16(0xE, id); }
        if cpu.pbr == 0 && cpu.pc == end { break; }
        cpu.mem.process_dma();
    }
    println!("decompress_sublevel took {}µs", now.elapsed().as_micros());
    cy
}

pub fn decompress_extram(cpu: &mut Cpu<CheckedMem>, id: u16) -> u64 {
    let now = std::time::Instant::now();
    cpu.emulation = false;
    cpu.mem.store(0x1F11, (id >> 8) as _);
    cpu.mem.store(0x141A, 1);
    cpu.ill = false;
    cpu.s = 0x1FF;
    cpu.pc = 0x2000;
    cpu.pbr = 0x00;
    cpu.dbr = 0x00;
    cpu.trace = false;

    let routines = [
        "CODE_00A993", "CODE_00B888", "CODE_05D796", "CODE_05801E",
        "UploadSpriteGFX", "LoadPalette", "CODE_00922F",
    ];
    let mut addr = 0x2000u32;
    for i in routines {
        cpu.mem.store(addr, 0x22);
        cpu.mem.store_u24(addr + 1, cpu.mem.cart.resolve(i).unwrap_or_else(|| panic!("no symbol: {}", i)));
        addr += 4;
    }
    let end = addr as u16;
    let layer1_data_ptr = cpu.mem.cart.resolve("Layer1DataPtr").unwrap();
    let mut cy = 0u64;
    loop {
        cy += cpu.dispatch() as u64;
        if cpu.ill { println!("ILLEGAL INSTR"); break; }
        if cpu.pc == 0xD8B7 && cpu.pbr == 0x05 { cpu.mem.store_u16(0xE, id); }
        if cpu.pbr == 0 && cpu.pc == end { break; }
        if cpu.pc == 0x200C { cpu.mem.store_u24(layer1_data_ptr, 0x600000); }
        cpu.mem.process_dma();
    }
    println!("decompress_extram took {}µs", now.elapsed().as_micros());
    cy
}

/// Load the overworld for the given submap (0=main, 1-6=submaps).
/// After return: vram/cgram filled, WRAM $7EC800=L1 tilemap, $7F4000=L2 tilemap.
pub fn load_overworld(cpu: &mut Cpu<CheckedMem>, submap: u8) -> u64 {
    let now = std::time::Instant::now();
    cpu.emulation = false;

    // CODE_04DC09 reads $0DD6 (PlayerTurnOW) and right-shifts by 2 to get the
    // submap index, then looks up OWPlayerSubmap[$0DD6>>2].  So store submap*4.
    cpu.mem.store(0x0DD6, submap.wrapping_mul(4));
    // OWPlayerSubmap table at $1F11: each byte is the submap palette/gfx bank.
    // Pre-fill with identity mapping so index N -> value N.
    for i in 0u8..7 { cpu.mem.store(0x1F11 + i as u32, i); }
    // CODE_05DBF2 reads $0DB3 (PlayerTurnLvl): 0 = main map, nonzero = submap.
    cpu.mem.store(0x0DB3, if submap == 0 { 0x00 } else { 0x01 });
    // Also store submap index where other routines expect it.
    cpu.mem.store(0x0DB4, submap); // OWPlayerSubmap table offset
    cpu.mem.store(0x141A, 1);

    let routines: &[&str] = &[
        "CODE_04DC09",           // Map16 pointers / OWL1TileData -> Map16TilesLow
        "DecompressOverworldL2", // L2 tiles -> $7F4000
        "CODE_05DBF2",           // OW L1 tilemap -> $7EC800
        "UploadSpriteGFX",       // GFX -> VRAM
        "CODE_00AD25",           // OW palette setup
        "CODE_00922F",           // palette -> CGRAM
    ];

    let cy = run_routines(cpu, routines, 50_000_000);
    log::info!("load_overworld(submap={submap}) took {}µs", now.elapsed().as_micros());
    cy
}
