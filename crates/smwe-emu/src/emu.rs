#![allow(clippy::identity_op)]

use std::{collections::HashSet, sync::Arc};

use wdc65816::{Cpu, Mem};

use crate::rom::Rom;

#[derive(Debug, Clone)]
pub struct CheckedMem {
    pub cart: Arc<Rom>,
    pub wram: Vec<u8>,
    pub regs: Vec<u8>,
    pub vram: Vec<u8>,
    pub cgram: Vec<u8>,
    pub extram: Vec<u8>,
    pub uninit: HashSet<usize>,
    pub error: Option<u32>,
    pub err_value: Option<u8>,
    pub last_store: Option<u32>,
}

impl CheckedMem {
    pub fn new(rom: Arc<Rom>) -> Self {
        Self {
            cart: rom,
            wram: Vec::from([0; 0x20000]),
            regs: Vec::from([0; 0x6000]),
            vram: Vec::from([0; 0x10000]),
            cgram: Vec::from([0; 0x200]),
            extram: Vec::from([0; 0x10000]),
            uninit: HashSet::new(),
            error: None,
            err_value: None,
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
                } else if ptr == 0x2122 {
                    // CGRAM data port: writes alternate low/high byte of a color entry.
                    // $2121 (regs[0x0121]) holds the current word address; bit 0 of
                    // an internal latch (regs[0x0120]) tracks which byte we're writing.
                    let latch = self.regs[0x0120];
                    let word_addr = self.regs[0x0121] as usize;
                    let byte_offset = word_addr * 2 + latch as usize;
                    if byte_offset < self.cgram.len() {
                        self.cgram[byte_offset] = value;
                    }
                    if latch == 0 {
                        self.regs[0x0120] = 1;
                    } else {
                        self.regs[0x0120] = 0;
                        // Advance word address after writing the high byte.
                        let next = (word_addr + 1) as u8;
                        self.regs[0x0121] = next;
                    }
                } else if ptr == 0x2121 {
                    // Writing to CGRAM address register resets the latch.
                    self.regs[0x0120] = 0;
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
        cpu.mem.store(addr, 0x22);
        cpu.mem.store_u24(addr + 1, cpu.mem.cart.resolve(symbol).unwrap_or_else(|| panic!("no symbol: {symbol}")));
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
    for i in 0..12 {
        cpu.mem.store(0x14C8 + i, 0);
    }
    cpu.mem.store(0x14C8, 1);
    cpu.y = 0;
    cpu.x = 0;
    run_routines(cpu, &["InitSpriteTables", "CODE_01808C", "CODE_01808C"], 10_000_000)
}

pub fn exec_sprites(cpu: &mut Cpu<CheckedMem>) -> u64 {
    run_routines(cpu, &["CODE_01808C"], 20_000_000)
}

/// One OAM tile emitted by a sprite, expressed as a pixel offset from the
/// spawn anchor (x = 0xD0, y = 0x80 as set by exec_sprite_id).
#[derive(Debug, Clone)]
pub struct SpriteOamTile {
    pub dx: i32,
    pub dy: i32,
    pub tile_word: u16,
    pub is_16x16: bool,
}

/// Run exec_sprite_id for the given ID, then tick extra frames so that sprites
/// whose draw routine fires on frame 2+ (e.g. Dragon Coin 0xA6) still produce
/// OAM. Collect ALL OAM tiles with non-zero tile words AND non-blank VRAM,
/// expressed as signed offsets from the spawn anchor (x=0xD0, y=0x80).
/// Returns an empty Vec if the sprite writes no OAM.
pub fn sprite_oam_tiles(cpu: &mut Cpu<CheckedMem>, id: u8) -> Vec<SpriteOamTile> {
    const ANCHOR_X: i32 = 0xD0;
    const ANCHOR_Y: i32 = 0x80;

    exec_sprite_id(cpu, id);
    // Extra ticks for sprites that draw on later frames
    for _ in 0..4 {
        exec_sprites(cpu);
    }

    let mut tiles = Vec::new();
    for slot in 0..64u32 {
        let raw_x = cpu.mem.load_u8(0x300 + slot * 4) as i32;
        let raw_y = cpu.mem.load_u8(0x301 + slot * 4) as i32;
        let tile  = cpu.mem.load_u16(0x302 + slot * 4);
        let size  = cpu.mem.load_u8(0x460 + slot);
        if raw_y >= 0xE0 || tile == 0 { continue; }

        tiles.push(SpriteOamTile {
            dx: raw_x - ANCHOR_X,
            dy: raw_y - ANCHOR_Y,
            tile_word: tile,
            is_16x16: (size & 0x02) != 0,
        });
    }
    tiles
}

/// A single OAM entry as written by the SNES sprite engine.
/// X and Y are raw screen-space pixel positions as the game set them
/// (scroll-relative: sprite at screen pos x=0xD0 when camera is at x=0).
#[derive(Debug, Clone)]
pub struct RawOamEntry {
    pub x: u8,
    pub y: u8,
    pub tile_word: u16,  // [attr_byte][tile_byte] little-endian u16
    pub is_16x16: bool,
}

/// Read all non-offscreen OAM entries after sprite execution.
/// The game writes sprite OAM to $0300-$03FF (x, y, tile, attr × 64 slots)
/// and size flags to $0460-$049F (one byte per slot, bit 1 = 16×16).
pub fn read_oam_snapshot(cpu: &mut Cpu<CheckedMem>) -> Vec<RawOamEntry> {
    let mut entries = Vec::new();
    for slot in 0..64u32 {
        let x    = cpu.mem.load_u8(0x300 + slot * 4);
        let y    = cpu.mem.load_u8(0x301 + slot * 4);
        let tile = cpu.mem.load_u16(0x302 + slot * 4);
        let size = cpu.mem.load_u8(0x460 + slot);
        if y >= 0xE0 || tile == 0 { continue; }
        entries.push(RawOamEntry {
            x,
            y,
            tile_word: tile,
            is_16x16: (size & 0x02) != 0,
        });
    }
    entries
}

pub fn decompress_sublevel(cpu: &mut Cpu<CheckedMem>, id: u16) -> u64 {
    let now = std::time::Instant::now();
    cpu.emulation = false;
    cpu.mem.store(0x1F11, (id >> 8) as _);
    cpu.mem.store(0x141A, 1);
    cpu.mem.store_u16(0x10B, id);
    cpu.ill = false;
    cpu.s = 0x1FF;
    cpu.pc = 0x2000;
    cpu.pbr = 0x00;
    cpu.dbr = 0x00;
    cpu.trace = false;

    let routines = [
        "CODE_00A993", "CODE_00B888", "CODE_05D796", "CODE_05801E",
        "UploadSpriteGFX", "LoadPalette", "CODE_00922F", "InitSpriteTables",
        // Upload the dynamic palette table to CGRAM - this commits any palette
        // entries (e.g. Dragon Coin gold oval) that LoadPalette queued into
        // DynPaletteTable but didn't write to CGRAM directly.
        "CODE_00A488",
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

pub fn load_overworld(cpu: &mut Cpu<CheckedMem>, submap: u8) -> u64 {
    let now = std::time::Instant::now();
    cpu.emulation = false;
    const OW_VIEW_X: [u16; 7] = [0x0000, 0xFFEF, 0xFFEF, 0xFFEF, 0x00F0, 0x00F0, 0x00F0];
    const OW_VIEW_Y: [u16; 7] = [0x0000, 0xFFD8, 0x0080, 0x0128, 0xFFD8, 0x0080, 0x0128];

    cpu.mem.store(0x0DB3, 0x00);
    cpu.mem.store(0x0DD6, 0x00);
    cpu.mem.store(0x1F11, submap);
    cpu.mem.store(0x1F12, submap);
    let idx = submap as usize;
    let view_x = *OW_VIEW_X.get(idx).unwrap_or(&OW_VIEW_X[0]);
    let view_y = *OW_VIEW_Y.get(idx).unwrap_or(&OW_VIEW_Y[0]);
    cpu.mem.store_u16(0x001A, view_x);
    cpu.mem.store_u16(0x001C, view_y);
    cpu.mem.store_u16(0x001E, view_x);
    cpu.mem.store_u16(0x0020, view_y);
    cpu.mem.store_u16(0x1462, view_x);
    cpu.mem.store_u16(0x1464, view_y);
    cpu.mem.store_u16(0x1466, view_x);
    cpu.mem.store_u16(0x1468, view_y);
    cpu.mem.store(0x141A, 1);

    cpu.ill = false;
    cpu.s = 0x1FF;
    cpu.pc = 0x2000;
    cpu.pbr = 0x00;
    cpu.dbr = 0x00;
    cpu.trace = false;

    let mut addr = 0x2000u32;
    for symbol in ["CODE_04DC09", "DecompressOverworldL2", "UploadSpriteGFX"] {
        cpu.mem.store(addr, 0x22);
        cpu.mem.store_u24(addr + 1, cpu.mem.cart.resolve(symbol).unwrap_or_else(|| panic!("no symbol: {symbol}")));
        addr += 4;
    }
    cpu.mem.store(addr, 0xA0);
    cpu.mem.store(addr + 1, 0x14);
    addr += 2;
    for symbol in ["PrepareGraphicsFile", "CODE_00AD25", "CODE_00922F", "CODE_04D6E9"] {
        cpu.mem.store(addr, 0x22);
        cpu.mem.store_u24(addr + 1, cpu.mem.cart.resolve(symbol).unwrap_or_else(|| panic!("no symbol: {symbol}")));
        addr += 4;
    }

    let end = addr as u16;
    let mut cy = 0u64;
    loop {
        cy += cpu.dispatch() as u64;
        if cpu.ill { log::warn!("illegal instruction at {:02X}:{:04X}", cpu.pbr, cpu.pc); break; }
        if cpu.pbr == 0 && cpu.pc == end { break; }
        if cy > 50_000_000 { log::warn!("exceeded cycle limit"); break; }
        cpu.mem.process_dma();
    }
    log::info!("load_overworld(submap={submap}) took {}µs", now.elapsed().as_micros());
    cy
}
