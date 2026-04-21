#![allow(clippy::identity_op)]

pub mod compression;
pub mod disassembler;
pub mod graphics;
pub mod internal_header;
pub mod level;
pub mod objects;
pub mod overworld;
pub mod snes_utils;

use std::{fs, path::Path};

use crate::{
    disassembler::{
        binary_block::{DataBlock, DataKind},
        RomDisassembly,
    },
    graphics::Gfx,
    internal_header::{InternalHeaderParseError, RegionCode, RomInternalHeader},
    level::{
        secondary_entrance::{SecondaryEntrance, SECONDARY_ENTRANCE_TABLE},
        Level, LEVEL_COUNT,
    },
    objects::tilesets::Tilesets,
    overworld::OverworldData,
    snes_utils::{
        addr::AddrSnes,
        rom::{Rom, RomError},
        rom_slice::SnesSlice,
    },
};

// -------------------------------------------------------------------------------------------------

#[derive(Debug)]
pub struct SmwRom {
    pub disassembly: RomDisassembly,
    pub internal_header: RomInternalHeader,
    pub levels: Vec<Level>,
    pub secondary_entrances: Vec<SecondaryEntrance>,
    pub gfx: Gfx,
    pub map16_tilesets: Tilesets,
    pub overworld: OverworldData,
}

// -------------------------------------------------------------------------------------------------

impl SmwRom {
    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        log::info!("Reading ROM from file: {}", path.as_ref().display());
        let bytes = fs::read(path)?;
        let rom = Rom::new(bytes)?;
        let smw_rom = Self::from_rom(rom);
        if smw_rom.is_ok() {
            log::info!("Success parsing ROM");
        }
        smw_rom
    }

    pub fn from_rom(rom: Rom) -> anyhow::Result<Self> {
        log::info!("Parsing internal ROM header");
        let internal_header = RomInternalHeader::parse(&rom)?;

        log::info!("Creating disassembly map");
        let mut disassembly = RomDisassembly::new(rom, &internal_header);

        disassembly.rom_slice_at_block(
            DataBlock {
                slice: SnesSlice::new(AddrSnes(0x00FFC0), internal_header::sizes::INTERNAL_HEADER),
                kind: DataKind::InternalRomHeader,
            },
            |_| InternalHeaderParseError::NotFound,
        )?;

        log::info!("Parsing level data");
        let levels = Self::parse_levels(&mut disassembly)?;

        log::info!("Parsing secondary entrances");
        let secondary_entrances = Self::parse_secondary_entrances(&mut disassembly)?;

        log::info!("Parsing GFX files");
        let gfx = Gfx::parse(&mut disassembly, &levels, &internal_header)?;

        log::info!("Parsing Map16 tilesets");
        let map16_tilesets = Tilesets::parse(&mut disassembly)?;

        log::info!("Parsing overworld data");
        let overworld = OverworldData::parse(&disassembly.rom)
            .unwrap_or_else(|e| {
                log::warn!("Could not parse overworld data: {e}");
                OverworldData { layer1_tiles: vec![0u8; overworld::OWL1_TILE_DATA_SIZE] }
            });

        Ok(Self { disassembly, internal_header, levels, secondary_entrances, gfx, map16_tilesets, overworld })
    }

    fn parse_levels(disasm: &mut RomDisassembly) -> anyhow::Result<Vec<Level>> {
        let mut levels = Vec::with_capacity(LEVEL_COUNT);
        for level_num in 0..LEVEL_COUNT as u32 {
            let level = Level::parse(disasm, level_num)?;
            levels.push(level);
        }
        Ok(levels)
    }

    fn parse_secondary_entrances(disasm: &mut RomDisassembly) -> anyhow::Result<Vec<SecondaryEntrance>> {
        let mut secondary_entrances = Vec::with_capacity(SECONDARY_ENTRANCE_TABLE.size);
        for entrance_id in 0..SECONDARY_ENTRANCE_TABLE.size {
            let entrance = SecondaryEntrance::read_from_rom(disasm, entrance_id)?;
            secondary_entrances.push(entrance);
        }
        Ok(secondary_entrances)
    }

    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        use std::io::Write;
        let bytes = self.disassembly.rom.0.to_vec();
        let mut f = std::fs::File::create(path)?;
        f.write_all(&bytes)?;
        Ok(())
    }

    /// Create a BPS patch from the original ROM to the current modified ROM
    ///
    /// Takes the original ROM bytes and generates a binary patch that can be applied
    /// with tools like Flips. This is useful for distributing ROM hacks without
    /// shipping the full ROM file.
    pub fn create_bps_patch(&self, original_rom: &[u8]) -> anyhow::Result<Vec<u8>> {
        let modified_rom = self.disassembly.rom.0.to_vec();
        let config = smwe_bps::BpsConfig::default();
        let patch = smwe_bps::create_patch(original_rom, &modified_rom, config)?;
        Ok(patch)
    }

    /// Create a BPS patch from the original ROM with metadata
    ///
    /// The metadata should be valid UTF-8 XML following the BPS specification.
    /// Example metadata structure:
    /// ```xml
    /// <?xml version="1.0" encoding="UTF-8"?>
    /// <patch>
    ///   <name>My Level Hack</name>
    ///   <author>Your Name</author>
    ///   <description>A description of your ROM hack</description>
    /// </patch>
    /// ```
    pub fn create_bps_patch_with_metadata(
        &self,
        original_rom: &[u8],
        metadata: Vec<u8>,
    ) -> anyhow::Result<Vec<u8>> {
        let modified_rom = self.disassembly.rom.0.to_vec();
        let config = smwe_bps::BpsConfig { metadata };
        let patch = smwe_bps::create_patch(original_rom, &modified_rom, config)?;
        Ok(patch)
    }

    /// Create an IPS patch from the original ROM to the current modified ROM
    ///
    /// IPS format is simpler and older than BPS but limited to 16MB files.
    /// This is still suitable for SMW ROM hacks. The patch can be applied
    /// with Flips or other ROM patching tools.
    pub fn create_ips_patch(&self, original_rom: &[u8]) -> anyhow::Result<Vec<u8>> {
        let modified_rom = self.disassembly.rom.0.to_vec();
        let patch = smwe_ips::create_patch(original_rom, &modified_rom)?;
        Ok(patch)
    }
}
