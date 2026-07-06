#![allow(clippy::identity_op)]

pub mod block_behavior;
pub mod compression;
pub mod disassembler;
pub mod graphics;
pub mod internal_header;
pub mod level;
pub mod message_boxes;
pub mod objects;
pub mod overworld;
pub mod snes_utils;
pub mod sprite_categories;
pub mod sprite_tweakers;
pub mod title_credits;

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
        Level,
        LEVEL_COUNT,
    },
    message_boxes::MessageBoxes,
    objects::tilesets::Tilesets,
    overworld::{level_names::OwLevelNames, OverworldData, OverworldEvents, TranslevelEvents},
    snes_utils::{
        addr::AddrSnes,
        rom::{Rom, RomError},
        rom_slice::SnesSlice,
    },
    sprite_tweakers::SpriteTweakers,
    title_credits::TitleCreditsData,
};

// -------------------------------------------------------------------------------------------------

#[derive(Debug)]
pub struct SmwRom {
    pub disassembly:         RomDisassembly,
    pub internal_header:     RomInternalHeader,
    pub levels:              Vec<Level>,
    pub secondary_entrances: Vec<SecondaryEntrance>,
    pub gfx:                 Gfx,
    pub map16_tilesets:      Tilesets,
    pub overworld:           OverworldData,
    pub overworld_events:    OverworldEvents,
    pub translevel_events:   TranslevelEvents,
    pub ow_level_names:      OwLevelNames,
    pub sprite_tweakers:     SpriteTweakers,
    pub message_boxes:       MessageBoxes,
    pub title_credits:       TitleCreditsData,
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
                kind:  DataKind::InternalRomHeader,
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
        let overworld = OverworldData::parse(&disassembly.rom).unwrap_or_else(|e| {
            log::warn!("Could not parse overworld data: {e}");
            OverworldData { layer1_tiles: vec![0u8; overworld::OWL1_TILE_DATA_SIZE] }
        });

        log::info!("Parsing overworld event data");
        let overworld_events = OverworldEvents::parse(&disassembly.rom).unwrap_or_else(|e| {
            log::warn!("Could not parse overworld event data: {e}");
            OverworldEvents {
                tile_offsets:  vec![0u16; overworld::OW_EVENT_COUNT],
                reveal_before: vec![0u8; overworld::OW_EVENT_REVEAL_COUNT],
                reveal_after:  vec![0u8; overworld::OW_EVENT_REVEAL_COUNT],
            }
        });

        log::info!("Parsing translevel event table");
        let translevel_events = TranslevelEvents::parse(&disassembly.rom).unwrap_or_else(|e| {
            log::warn!("Could not parse translevel event table: {e}");
            TranslevelEvents { events: vec![overworld::TRANSLEVEL_NO_EVENT; overworld::TRANSLEVEL_EVENTS_COUNT] }
        });

        log::info!("Parsing overworld level names");
        let ow_level_names = OwLevelNames::parse(&disassembly.rom).unwrap_or_else(|e| {
            log::warn!("Could not parse overworld level names: {e}");
            OwLevelNames {
                piece1:  vec![vec![overworld::level_names::BLANK_PIECE_BYTE]],
                piece2:  vec![vec![overworld::level_names::BLANK_PIECE_BYTE]],
                piece3:  vec![vec![overworld::level_names::BLANK_PIECE_BYTE]],
                entries: vec![
                    overworld::level_names::LevelNameEntry { piece1: 0, piece2: 0, piece3: 0 };
                    overworld::level_names::LEVEL_NAMES_COUNT
                ],
            }
        });

        log::info!("Parsing sprite tweaker bytes");
        let sprite_tweakers = SpriteTweakers::parse(&disassembly.rom).unwrap_or_else(|e| {
            log::warn!("Could not parse sprite tweaker bytes: {e}");
            SpriteTweakers {
                tweaker_a: vec![0u8; sprite_tweakers::SPRITE_TWEAKER_COUNT],
                tweaker_b: vec![0u8; sprite_tweakers::SPRITE_TWEAKER_COUNT],
                tweaker_c: vec![0u8; sprite_tweakers::SPRITE_TWEAKER_COUNT],
                tweaker_d: vec![0u8; sprite_tweakers::SPRITE_TWEAKER_COUNT],
                tweaker_e: vec![0u8; sprite_tweakers::SPRITE_TWEAKER_COUNT],
                tweaker_f: vec![0u8; sprite_tweakers::SPRITE_TWEAKER_COUNT],
            }
        });

        log::info!("Parsing message box text");
        let message_boxes = MessageBoxes::parse(&disassembly.rom).unwrap_or_else(|e| {
            log::warn!("Could not parse message box text: {e}");
            MessageBoxes { messages: vec![Vec::new(); message_boxes::MESSAGE_COUNT] }
        });

        log::info!("Parsing title screen / credits data");
        let title_credits = TitleCreditsData::parse(&disassembly.rom).unwrap_or_else(|e| {
            log::warn!("Could not parse title screen / credits data: {e}");
            TitleCreditsData::empty()
        });

        Ok(Self {
            disassembly,
            internal_header,
            levels,
            secondary_entrances,
            gfx,
            map16_tilesets,
            overworld,
            overworld_events,
            translevel_events,
            ow_level_names,
            sprite_tweakers,
            message_boxes,
            title_credits,
        })
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
    pub fn create_bps_patch_with_metadata(&self, original_rom: &[u8], metadata: Vec<u8>) -> anyhow::Result<Vec<u8>> {
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
