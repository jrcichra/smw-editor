#![allow(clippy::identity_op)]

//! Storage for ROM files, mapper support, etc.

use std::collections::HashMap;

#[derive(Debug, Copy, Clone)]
pub enum Mapper {
    NoRom,
    LoRom,
    HiRom,
}

impl Mapper {
    pub fn map_to_file(&self, addr: usize) -> Option<usize> {
        match self {
            Mapper::NoRom => Some(addr),
            Mapper::LoRom => {
                if (addr&0xFE0000)==0x7E0000        //wram
                || (addr&0x408000)==0x000000        //hardware regs, ram mirrors, other strange junk
                || (addr&0x708000)==0x700000
                {
                    //sram (low parts of banks 70-7D)
                    None
                } else {
                    Some((addr & 0x7F0000) >> 1 | (addr & 0x7FFF))
                }
            }
            Mapper::HiRom => {
                if (addr&0xFE0000)==0x7E0000       //wram
                || (addr&0x408000)==0x000000
                {
                    //hardware regs, ram mirrors, other strange junk
                    None
                } else {
                    Some(addr & 0x3FFFFF)
                }
            }
        }
    }

    pub fn map_to_addr(&self, offset: usize) -> usize {
        match self {
            Mapper::NoRom => offset,
            Mapper::LoRom => {
                let in_bank = offset & 0x7FFF;
                let bank = offset >> 15;
                (bank << 16) + in_bank + 0x8000
            }
            Mapper::HiRom => offset | 0xC00000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Rom {
    buf:     Vec<u8>,
    mapper:  Mapper,
    symbols: HashMap<String, u32>,
}

/// Detect the cartridge mapper from an *unheadered* ROM buffer by validating the
/// SNES internal header's checksum/complement pair at the LoROM and HiROM
/// locations. This mirrors the heuristic used by real emulators and by
/// `smwe-rom`'s header parser, so an expanded LoROM hack (e.g. TOP2020) and a
/// HiROM hack both map correctly instead of everything being forced to LoROM.
pub fn detect_mapper(buf: &[u8]) -> Mapper {
    // Complement at header+0x1C, checksum at header+0x1E (little-endian u16s).
    let valid_at = |base: usize| -> bool {
        match (buf.get(base + 0x1C..base + 0x1E), buf.get(base + 0x1E..base + 0x20)) {
            (Some(cpl), Some(csm)) => {
                let cpl = u16::from_le_bytes([cpl[0], cpl[1]]);
                let csm = u16::from_le_bytes([csm[0], csm[1]]);
                (cpl ^ csm) == 0xFFFF
            }
            _ => false,
        }
    };
    // Map-mode byte sits at header+0x15; bit 0 distinguishes HiROM from LoROM.
    let lo_ok = valid_at(0x7FC0);
    let hi_ok = valid_at(0xFFC0);
    let mapper = match (lo_ok, hi_ok) {
        (true, false) => Mapper::LoRom,
        (false, true) => Mapper::HiRom,
        (true, true) => {
            // Both checksums validate (rare); fall back to the declared map mode.
            if buf.get(0x7FD5).map_or(false, |m| m & 0x01 != 0) {
                Mapper::HiRom
            } else {
                Mapper::LoRom
            }
        }
        (false, false) => {
            log::warn!("Could not validate ROM checksum at LoROM or HiROM header; assuming LoROM");
            Mapper::LoRom
        }
    };
    // SA-1 ($33-$36) and SuperFX ($13-$16) carts use mappings this emulator does
    // not model; warn so garbled output is at least explained.
    if let Some(&rom_type) = buf.get(0x7FD6).filter(|_| lo_ok).or_else(|| buf.get(0xFFD6).filter(|_| hi_ok)) {
        match rom_type & 0xF0 {
            0x30 => log::warn!("ROM declares SA-1 ($33-$36); SA-1 mapping is not yet supported and rendering may be wrong"),
            0x10 => log::warn!("ROM declares SuperFX; this mapper is not supported"),
            _ => {}
        }
    }
    log::info!("Detected cartridge mapper: {mapper:?}");
    mapper
}

impl Rom {
    /// Construct a ROM, auto-detecting the mapper from the (unheadered) buffer.
    pub fn new(buf: Vec<u8>) -> Self {
        let mapper = detect_mapper(&buf);
        Self { buf, mapper, symbols: HashMap::new() }
    }

    /// Construct a ROM with an explicit mapper, bypassing auto-detection.
    pub fn new_with_mapper(buf: Vec<u8>, mapper: Mapper) -> Self {
        Self { buf, mapper, symbols: HashMap::new() }
    }

    pub fn set_mapper(&mut self, mapper: Mapper) {
        self.mapper = mapper;
    }

    pub fn load_symbols(&mut self, data: &str) {
        for i in data.lines() {
            let i = if let Some(comment) = i.find(';') { &i[..comment] } else { i }.trim();
            if i.is_empty() {
                continue;
            }
            if let Some(v) = i.find(' ') {
                match u32::from_str_radix(&i[..v], 16) {
                    Ok(addr) => {
                        self.symbols.insert(i[v + 1..].to_string(), addr);
                    }
                    Err(_e) => {}
                }
            }
        }
    }

    pub fn resolve(&self, symbol: &str) -> Option<u32> {
        self.symbols.get(symbol).copied()
    }

    pub fn read(&self, addr: u32) -> Option<u8> {
        self.mapper.map_to_file(addr as _).and_then(|c| self.buf.get(c).copied())
    }

    pub fn read_u16(&self, addr: u32) -> Option<u16> {
        Some(u16::from_le_bytes([self.read(addr + 0)?, self.read(addr + 1)?]))
    }

    pub fn read_u32(&self, addr: u32) -> Option<u32> {
        Some(u32::from_le_bytes([self.read(addr + 0)?, self.read(addr + 1)?, self.read(addr + 2)?, 0]))
    }

    pub fn resize(&mut self, new_size: usize) {
        self.buf.resize(new_size, 0);
    }

    pub fn mapper(&self) -> Mapper {
        self.mapper
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }

    pub fn checksum(&self) -> u16 {
        let size = self.buf.len();
        if size == 0 {
            return 0;
        }
        let base: u16 = self.buf.iter().map(|&b| b as u16).sum();
        if size.is_power_of_two() {
            base
        } else {
            // Mirror the trailing non-power-of-2 portion to fill the gap, matching
            // what a real SNES cartridge exposes on the bus.
            let po2 = size.next_power_of_two() / 2;
            let remainder = size - po2;
            let mirror_sum: u16 = self.buf[po2..po2 + remainder].iter().map(|&b| b as u16).sum();
            base.wrapping_add(mirror_sum)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal ROM buffer with a valid checksum/complement pair written
    /// at the given header location, plus an optional map-mode/rom-type byte.
    fn rom_with_header(header_base: usize, map_mode: u8, rom_type: u8) -> Vec<u8> {
        let mut buf = vec![0u8; 0x10000];
        // Arbitrary checksum; complement is its bitwise inverse so cpl ^ csm == 0xFFFF.
        let checksum: u16 = 0x1234;
        let complement = !checksum;
        buf[header_base + 0x15] = map_mode;
        buf[header_base + 0x16] = rom_type;
        buf[header_base + 0x1C..header_base + 0x1E].copy_from_slice(&complement.to_le_bytes());
        buf[header_base + 0x1E..header_base + 0x20].copy_from_slice(&checksum.to_le_bytes());
        buf
    }

    #[test]
    fn detects_lorom() {
        let buf = rom_with_header(0x7FC0, 0x20, 0x00);
        assert!(matches!(detect_mapper(&buf), Mapper::LoRom));
    }

    #[test]
    fn detects_hirom() {
        let buf = rom_with_header(0xFFC0, 0x21, 0x00);
        assert!(matches!(detect_mapper(&buf), Mapper::HiRom));
    }

    #[test]
    fn defaults_to_lorom_without_valid_checksum() {
        let buf = vec![0u8; 0x10000];
        assert!(matches!(detect_mapper(&buf), Mapper::LoRom));
    }
}
