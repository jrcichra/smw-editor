#![allow(clippy::identity_op)]

use std::sync::Arc;

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
            error: None,
            err_value: None,
            last_store: None,
        }
    }

    pub fn load_u8(&mut self, addr: u32) -> u8 {
        self.load(addr)
    }
    pub fn store_u8(&mut self, addr: u32, value: u8) {
        self.store(addr, value)
    }

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
            log::debug!("DMA size {size:04X}: ${b:02X} ${a:06X}");
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
    fn load(&mut self, addr: u32) -> u8 {
        self.map(addr, None)
    }
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
        if cpu.ill {
            log::warn!("illegal instruction at {:02X}:{:04X}", cpu.pbr, cpu.pc);
            break;
        }
        if cpu.pbr == 0 && cpu.pc == end {
            break;
        }
        if cy > cycle_limit {
            log::warn!("exceeded cycle limit");
            break;
        }
        cpu.mem.process_dma();
    }
    cy
}

pub fn fetch_anim_frame(cpu: &mut Cpu<CheckedMem>) -> u64 {
    // CODE_00A5F9 is SMW's full animated-tile init loop: it cycles through all
    // 8 sub-frames so every animated VRAM slot (coins, ? blocks, turn blocks,
    // etc.) gets its correct graphics regardless of update interval.
    run_routines(cpu, &["CODE_00A5F9"], 20_000_000)
}

/// Advance the global SMW frame counter ($0014) by one tick and re-run the
/// animation routine so VRAM reflects the next animation frame.  Call this
/// once per desired animation tick, then re-upload VRAM to the renderer.
pub fn advance_anim_frame(cpu: &mut Cpu<CheckedMem>) {
    let counter = cpu.mem.load_u8(0x0014).wrapping_add(1);
    cpu.mem.store(0x0014, counter);
    fetch_anim_frame(cpu);
}

pub fn upload_sprite_tileset(cpu: &mut Cpu<CheckedMem>, sprite_tileset: u8) -> u64 {
    cpu.mem.store(0x192B, sprite_tileset);
    run_routines(cpu, &["UploadSpriteGFX"], 20_000_000)
}

fn clear_sprite_preview_state(cpu: &mut Cpu<CheckedMem>) {
    const SLOT_COUNT: u32 = 12;
    const SPRITE_TABLE_BASES: &[u32] = &[
        0x009E, 0x00AA, 0x00B6, 0x00C2, 0x00D8, 0x00E4, 0x14C8, 0x14D4, 0x14E0, 0x14EC, 0x14F8, 0x1504, 0x1510,
        0x151C, 0x1528, 0x1534, 0x1540, 0x154C, 0x1558, 0x1564, 0x1570, 0x157C, 0x1588, 0x1594, 0x15A0, 0x15AC,
        0x15B8, 0x15C4, 0x15D0, 0x15DC, 0x15EA, 0x15F6, 0x1602, 0x160E, 0x161A, 0x1626, 0x1632, 0x163E, 0x164A,
        0x1656, 0x1662, 0x166E, 0x167A, 0x1686, 0x187B,
    ];

    for &base in SPRITE_TABLE_BASES {
        for slot in 0..SLOT_COUNT {
            cpu.mem.store(base + slot, 0);
        }
    }
}

pub fn exec_sprite_id(cpu: &mut Cpu<CheckedMem>, id: u8) -> u64 {
    clear_sprite_preview_state(cpu);
    cpu.mem.store(0x9E, id);
    cpu.mem.store(0x1A, 0x00);
    cpu.mem.store(0x1C, 0x00);
    cpu.mem.store(0xD8, 0x80);
    cpu.mem.store(0xE4, 0x80);
    cpu.mem.store(0x14D4, 0x00);
    cpu.mem.store(0x14E0, 0x00);
    // Start in "initialization" status and explicitly run the sprite's init
    // routine before stepping the normal main routine. This matches the real
    // game flow more closely than forcing status 0x08 up front.
    cpu.mem.store(0x14C8, 0x01);
    cpu.y = 0;
    cpu.x = 0;
    clear_sprite_oam(cpu);
    run_routines(cpu, &["InitSpriteTables", "CallSpriteInit", "CODE_01808C"], 10_000_000)
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
/// expressed as signed offsets from the spawn anchor (x=0x80, y=0x80).
/// Returns an empty Vec if the sprite writes no OAM.
pub fn sprite_oam_tiles(cpu: &mut Cpu<CheckedMem>, id: u8) -> Vec<SpriteOamTile> {
    const ANCHOR_X: i32 = 0x80;
    const ANCHOR_Y: i32 = 0x80;

    exec_sprite_id(cpu, id);
    let mut best_tiles = collect_sprite_oam_tiles(cpu, ANCHOR_X, ANCHOR_Y);
    // Sample a few additional clean frames and keep the strongest frame.
    for _ in 0..4 {
        clear_sprite_oam(cpu);
        exec_sprites(cpu);
        let frame_tiles = collect_sprite_oam_tiles(cpu, ANCHOR_X, ANCHOR_Y);
        if score_sprite_frame(&frame_tiles) > score_sprite_frame(&best_tiles) {
            best_tiles = frame_tiles;
        }
    }
    best_tiles
}

fn collect_sprite_oam_tiles(cpu: &mut Cpu<CheckedMem>, anchor_x: i32, anchor_y: i32) -> Vec<SpriteOamTile> {
    let mut tiles = Vec::new();
    for slot in 0..64u32 {
        let raw_x = cpu.mem.load_u8(0x300 + slot * 4) as i32;
        let raw_y = cpu.mem.load_u8(0x301 + slot * 4) as i32;
        let tile = cpu.mem.load_u16(0x302 + slot * 4);
        let size = cpu.mem.load_u8(0x460 + slot);
        if raw_y >= 0xE0 || tile == 0 {
            continue;
        }

        let has_gfx = if (size & 0x02) != 0 {
            let base = ((tile & 0x01FF) as usize) + 0x600;
            [base, base + 1, base + 16, base + 17].into_iter().any(|tile_idx| tile_has_nonzero_gfx(&cpu.mem.vram, tile_idx))
        } else {
            let tile_idx = ((tile & 0x01FF) as usize) + 0x600;
            tile_has_nonzero_gfx(&cpu.mem.vram, tile_idx)
        };
        if !has_gfx {
            continue;
        }

        tiles.push(SpriteOamTile {
            dx: raw_x - anchor_x,
            dy: raw_y - anchor_y,
            tile_word: tile,
            is_16x16: (size & 0x02) != 0,
        });
    }
    tiles
}

fn score_sprite_frame(tiles: &[SpriteOamTile]) -> i32 {
    let mut score = 0;
    for tile in tiles {
        score += if tile.is_16x16 { 4 } else { 1 };
    }
    score
}

fn tile_has_nonzero_gfx(vram: &[u8], tile_idx: usize) -> bool {
    let off = tile_idx * 32;
    off + 32 <= vram.len() && vram[off..off + 32].iter().any(|&b| b != 0)
}

fn clear_sprite_oam(cpu: &mut Cpu<CheckedMem>) {
    for slot in 0..64u32 {
        cpu.mem.store(0x300 + slot * 4, 0);
        cpu.mem.store(0x301 + slot * 4, 0xF0);
        cpu.mem.store(0x302 + slot * 4, 0);
        cpu.mem.store(0x303 + slot * 4, 0);
        cpu.mem.store(0x460 + slot, 0);
    }
}

/// A single OAM entry as written by the SNES sprite engine.
/// X and Y are raw screen-space pixel positions as the game set them
/// (scroll-relative: sprite at screen pos x=0xD0 when camera is at x=0).
#[derive(Debug, Clone)]
pub struct RawOamEntry {
    pub x: u8,
    pub y: u8,
    pub tile_word: u16, // [attr_byte][tile_byte] little-endian u16
    pub is_16x16: bool,
}

/// Read all non-offscreen OAM entries after sprite execution.
/// The game writes sprite OAM to $0300-$03FF (x, y, tile, attr × 64 slots)
/// and size flags to $0460-$049F (one byte per slot, bit 1 = 16×16).
pub fn read_oam_snapshot(cpu: &mut Cpu<CheckedMem>) -> Vec<RawOamEntry> {
    let mut entries = Vec::new();
    for slot in 0..64u32 {
        let x = cpu.mem.load_u8(0x300 + slot * 4);
        let y = cpu.mem.load_u8(0x301 + slot * 4);
        let tile = cpu.mem.load_u16(0x302 + slot * 4);
        let size = cpu.mem.load_u8(0x460 + slot);
        if y >= 0xE0 || tile == 0 {
            continue;
        }
        entries.push(RawOamEntry { x, y, tile_word: tile, is_16x16: (size & 0x02) != 0 });
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
        "CODE_00A993",
        "CODE_00B888",
        "CODE_05D796",
        "CODE_05801E",
        "UploadSpriteGFX",
        "LoadPalette",
        "CODE_00922F",
        "InitSpriteTables",
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
        if cpu.ill {
            println!("ILLEGAL INSTR");
            break;
        }
        if cpu.pc == 0xD8B7 && cpu.pbr == 0x05 {
            cpu.mem.store_u16(0xE, id);
        }
        if cpu.pbr == 0 && cpu.pc == end {
            break;
        }
        cpu.mem.process_dma();
    }
    // Fix dragon coin palette: copy gold colors from row 10 colors 0-7 to 8-15
    // Dragon coin tiles use color indices 9, 12, 13 which are in the upper half
    let row_10_base = 0xA0 * 2;
    for c in 0..8usize {
        let src = row_10_base + c * 2;
        let dst = row_10_base + (c + 8) * 2;
        cpu.mem.cgram[dst] = cpu.mem.cgram[src];
        cpu.mem.cgram[dst + 1] = cpu.mem.cgram[src + 1];
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

    let routines =
        ["CODE_00A993", "CODE_00B888", "CODE_05D796", "CODE_05801E", "UploadSpriteGFX", "LoadPalette", "CODE_00922F"];
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
        if cpu.ill {
            println!("ILLEGAL INSTR");
            break;
        }
        if cpu.pc == 0xD8B7 && cpu.pbr == 0x05 {
            cpu.mem.store_u16(0xE, id);
        }
        if cpu.pbr == 0 && cpu.pc == end {
            break;
        }
        if cpu.pc == 0x200C {
            cpu.mem.store_u24(layer1_data_ptr, 0x600000);
        }
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
        if cpu.ill {
            log::warn!("illegal instruction at {:02X}:{:04X}", cpu.pbr, cpu.pc);
            break;
        }
        if cpu.pbr == 0 && cpu.pc == end {
            break;
        }
        if cy > 50_000_000 {
            log::warn!("exceeded cycle limit");
            break;
        }
        cpu.mem.process_dma();
    }
    log::info!("load_overworld(submap={submap}) took {}µs", now.elapsed().as_micros());
    cy
}
