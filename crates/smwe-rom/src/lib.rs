#![allow(clippy::identity_op)]

pub mod block_behavior;
pub mod boss_text;
pub mod compression;
pub mod direct_map16;
pub mod exanimation;
pub mod exgfx;

pub mod font_map;
pub mod freespace;
pub mod graphics;
pub mod internal_header;
pub mod layer3;
pub mod level;
pub mod level_deletion;
pub mod map16_expanded;
pub mod map16_file;
pub mod message_boxes;
pub mod message_raster;
pub mod music;
pub mod mwl;
pub mod objects;
pub mod overworld;
pub mod rom_expansion;
pub mod snes_utils;
pub mod sprite_tweakers;
pub mod title_credits;
pub mod title_stripe;
pub mod xref;

use std::{fs, path::Path};

use crate::{
    boss_text::BossText,
    direct_map16::DirectMap16Data,
    exanimation::ExAnimationData,
    graphics::Gfx,
    internal_header::{InternalHeaderParseError, RegionCode, RomInternalHeader},
    level::{
        dimensions::LevelHeights,
        entrance_extras::{EntranceExtrasError, LevelEntranceExtrasData},
        secondary_entrance::{SecondaryEntrance, SecondaryExitExtData, SECONDARY_ENTRANCE_TABLE},
        sprite_header_ext::{SpriteHeaderExtData, SpriteHeaderExtError},
        Level,
        LEVEL_COUNT,
    },
    message_boxes::MessageBoxes,
    objects::tilesets::Tilesets,
    overworld::{OverworldData, OverworldEvents, OverworldL2Events},
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
    pub rom:                   Rom,
    pub internal_header:       RomInternalHeader,
    pub levels:                Vec<Level>,
    pub secondary_entrances:   Vec<SecondaryEntrance>,
    pub gfx:                   Gfx,
    pub map16_tilesets:        Tilesets,
    pub overworld:             OverworldData,
    pub overworld_events:      OverworldEvents,
    pub overworld_l2_events:   OverworldL2Events,
    pub sprite_tweakers:       SpriteTweakers,
    pub message_boxes:         MessageBoxes,
    pub boss_text:             BossText,
    pub title_credits:         TitleCreditsData,
    pub exanimation:           ExAnimationData,
    pub secondary_exit_ext:    SecondaryExitExtData,
    pub sprite_header_ext:     SpriteHeaderExtData,
    pub exgfx:                 exgfx::ExGfxData,
    pub gfx_bypass:            exgfx::BypassData,
    pub direct_map16:          DirectMap16Data,
    pub level_heights:         LevelHeights,
    pub level_entrance_extras: LevelEntranceExtrasData,
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

        rom.slice_lorom(SnesSlice::new(AddrSnes(0x00FFC0), internal_header::sizes::INTERNAL_HEADER))
            .map_err(|_| InternalHeaderParseError::NotFound)?;

        log::info!("Parsing level data");
        let levels = Self::parse_levels(&rom)?;

        log::info!("Parsing secondary entrances");
        let secondary_entrances = Self::parse_secondary_entrances(&rom)?;

        log::info!("Parsing GFX files");
        let gfx = Gfx::parse(&rom, &levels, &internal_header)?;

        log::info!("Parsing Map16 tilesets");
        let map16_tilesets = Tilesets::parse(&rom)?;

        log::info!("Parsing overworld data");
        let overworld = OverworldData::parse(&rom).unwrap_or_else(|e| {
            log::warn!("Could not parse overworld data: {e}");
            OverworldData { layer1_tiles: vec![0u8; overworld::OWL1_TILE_DATA_SIZE] }
        });

        log::info!("Parsing overworld event data");
        let overworld_events = OverworldEvents::parse(&rom).unwrap_or_else(|e| {
            log::warn!("Could not parse overworld event data: {e}");
            OverworldEvents {
                tile_offsets:  vec![0u16; overworld::OW_EVENT_COUNT],
                reveal_before: vec![0u8; overworld::OW_EVENT_REVEAL_COUNT],
                reveal_after:  vec![0u8; overworld::OW_EVENT_REVEAL_COUNT],
            }
        });

        log::info!("Parsing overworld Layer 2 event data");
        let overworld_l2_events = OverworldL2Events::parse(&rom).unwrap_or_else(|e| {
            log::warn!("Could not parse overworld Layer 2 event data: {e}");
            OverworldL2Events { entries: Vec::new(), boundaries: Vec::new(), silent_events: Vec::new() }
        });

        log::info!("Parsing sprite tweaker bytes");
        let sprite_tweakers = SpriteTweakers::parse(&rom).unwrap_or_else(|e| {
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
        let message_boxes = MessageBoxes::parse(&rom).unwrap_or_else(|e| {
            log::warn!("Could not parse message box text: {e}");
            MessageBoxes { messages: vec![Vec::new(); message_boxes::MESSAGE_COUNT] }
        });

        log::info!("Parsing title screen / credits data");
        let title_credits = TitleCreditsData::parse(&rom).unwrap_or_else(|e| {
            log::warn!("Could not parse title screen / credits data: {e}");
            TitleCreditsData::empty()
        });

        log::info!("Parsing boss sequence text");
        let boss_text = BossText::parse(&rom).unwrap_or_else(|e| {
            log::warn!("Could not parse boss sequence text: {e}");
            BossText { messages: vec![Vec::new(); 7] }
        });

        log::info!("Parsing ExAnimation data");
        let exanimation = ExAnimationData::parse(rom.bytes()).unwrap_or_else(|e| {
            // NotFound is the normal case: a ROM nobody has authored
            // ExAnimation for yet simply has no block.
            if !matches!(e, exanimation::ExAnimError::NotFound) {
                log::warn!("Could not parse ExAnimation data: {e}");
            }
            ExAnimationData::default()
        });

        log::info!("Parsing secondary-exit extended data");
        let secondary_exit_ext = SecondaryExitExtData::parse(rom.bytes()).unwrap_or_else(|e| {
            // NotFound is the normal case: a ROM nobody has authored
            // extended secondary-exit options for yet simply has no block.
            if !matches!(e, level::secondary_entrance::SecExitExtError::NotFound) {
                log::warn!("Could not parse secondary-exit extended data: {e}");
            }
            SecondaryExitExtData::default()
        });

        log::info!("Parsing sprite-header-ext data");
        let sprite_header_ext = SpriteHeaderExtData::parse(rom.bytes()).unwrap_or_else(|e| {
            // NotFound is the normal case: a ROM nobody has set LM 3.00
            // sprite-header options on yet simply has no block.
            if !matches!(e, SpriteHeaderExtError::NotFound) {
                log::warn!("Could not parse sprite-header-ext data: {e}");
            }
            SpriteHeaderExtData::default()
        });

        log::info!("Parsing ExGFX files and Super GFX Bypass table");
        let exgfx = exgfx::ExGfxData::parse(rom.bytes());
        let gfx_bypass = exgfx::BypassData::parse(rom.bytes()).unwrap_or_default();
        log::info!("Parsing Direct Map16 data");
        let direct_map16 = DirectMap16Data::parse(rom.bytes()).unwrap_or_else(|e| {
            // NotFound is the normal case: a ROM nobody has authored
            // Direct Map16 objects for yet simply has no block.
            if !matches!(e, direct_map16::Dm16Error::NotFound) {
                log::warn!("Could not parse Direct Map16 data: {e}");
            }
            DirectMap16Data::default()
        });
        log::info!("Parsing per-level entrance extras");
        let level_entrance_extras = LevelEntranceExtrasData::parse(rom.bytes()).unwrap_or_else(|e| {
            // NotFound is the normal case: a ROM nobody has authored
            // entrance extras for yet simply has no block.
            if !matches!(e, EntranceExtrasError::NotFound) {
                log::warn!("Could not parse entrance-extras data: {e}");
            }
            LevelEntranceExtrasData::default()
        });

        log::info!("Parsing dynamic level heights");
        let level_heights = LevelHeights::parse(rom.bytes()).unwrap_or_else(|e| {
            // NotFound is the normal case: a ROM nobody has set a custom
            // level height for yet simply has no block.
            if !matches!(e, level::dimensions::LevelHeightError::NotFound) {
                log::warn!("Could not parse level height data: {e}");
            }
            LevelHeights::default()
        });

        Ok(Self {
            rom,
            internal_header,
            levels,
            secondary_entrances,
            gfx,
            map16_tilesets,
            overworld,
            overworld_events,
            overworld_l2_events,
            sprite_tweakers,
            message_boxes,
            boss_text,
            title_credits,
            exanimation,
            secondary_exit_ext,
            sprite_header_ext,
            exgfx,
            gfx_bypass,
            direct_map16,
            level_heights,
            level_entrance_extras,
        })
    }

    fn parse_levels(rom: &Rom) -> anyhow::Result<Vec<Level>> {
        let mut levels = Vec::with_capacity(LEVEL_COUNT);
        for level_num in 0..LEVEL_COUNT as u32 {
            let level = Level::parse(rom, level_num)?;
            levels.push(level);
        }
        Ok(levels)
    }

    fn parse_secondary_entrances(rom: &Rom) -> anyhow::Result<Vec<SecondaryEntrance>> {
        let mut secondary_entrances = Vec::with_capacity(SECONDARY_ENTRANCE_TABLE.size);
        for entrance_id in 0..SECONDARY_ENTRANCE_TABLE.size {
            let entrance = SecondaryEntrance::read_from_rom(rom, entrance_id)?;
            secondary_entrances.push(entrance);
        }
        Ok(secondary_entrances)
    }

    pub fn rom_bytes(&self) -> &[u8] {
        &self.rom.0
    }

    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        use std::io::Write;
        let bytes = self.rom.0.to_vec();
        let mut f = std::fs::File::create(path)?;
        f.write_all(&bytes)?;
        Ok(())
    }
}
