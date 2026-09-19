use nom::{
    combinator::map,
    multi::count,
    number::complete::{le_u16, le_u24},
};
use thiserror::Error;

pub use self::{
    background::{BackgroundData, BackgroundTileID},
    headers::{PrimaryHeader, SecondaryHeader, SpriteHeader, PRIMARY_HEADER_SIZE, SPRITE_HEADER_SIZE},
    object_layer::ObjectLayer,
    sprite_layer::SpriteLayer,
};
use crate::{
    compression::DecompressionError,
    level::background::bg_high_byte_for_pointer,
    snes_utils::{
        addr::AddrSnes,
        rom::{parse_bytes, Rom},
        rom_slice::SnesSlice,
    },
    RomError,
};

pub mod background;
pub mod dimensions;
pub mod entrance_extras;
pub mod headers;
pub mod object_layer;
pub mod scroll;
pub mod secondary_entrance;
pub mod sprite_header_ext;
pub mod sprite_layer;

// -------------------------------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum LevelParseError {
    #[error("Reading address of Layer1:\n- {0}")]
    Layer1AddressRead(RomError),
    #[error("Reading address of Layer2:\n- {0}")]
    Layer2AddressRead(RomError),
    #[error("Reading address of Sprite data:\n- {0}")]
    SpriteAddressRead(RomError),

    #[error("Isolating Layer2 data:\n- {0}")]
    Layer2Isolate(RomError),

    #[error("Reading Primary Header:\n- {0}")]
    PrimaryHeaderRead(RomError),
    #[error("Reading Secondary Header:\n- {0}")]
    SecondaryHeaderRead(RomError),
    #[error("Reading Sprite Header:\n- {0}")]
    SpriteHeaderRead(RomError),

    #[error("Reading Layer1 object data:\n- {0}")]
    Layer1Read(RomError),
    #[error("Parsing Layer2 object data:\n- {0}")]
    Layer2Read(RomError),
    #[error("Reading Layer2 background:\n- {0}")]
    Layer2BackgroundRead(DecompressionError),
    #[error("Reading Sprite data:\n- {0}")]
    SpriteRead(RomError),
}

// -------------------------------------------------------------------------------------------------

pub const LEVEL_COUNT: usize = 0x200;

// -------------------------------------------------------------------------------------------------

/// Size of the Layer 2 object-data header in bytes.
///
/// When a level's Layer 2 pointer does not have bank `$FF`, the pointer leads
/// with a 5-byte header that the game skips outright (`SMWDisX bank_05.asm`:
/// `ADC #$05` — "to ignore Layer 2's header") before loading the object
/// stream. In the vanilla ROM it usually mirrors the level's own primary
/// header, but the game never reads it, so the editor treats it as
/// user-editable bytes instead of copying it verbatim on save.
pub const LAYER2_HEADER_SIZE: usize = 5;

#[derive(Debug, Clone)]
pub enum Layer2Data {
    Background(BackgroundData),
    Objects { header: [u8; LAYER2_HEADER_SIZE], objects: ObjectLayer },
}

#[derive(Debug, Clone)]
pub struct Level {
    pub primary_header:   PrimaryHeader,
    pub secondary_header: SecondaryHeader,
    pub sprite_header:    SpriteHeader,
    pub layer1:           ObjectLayer,
    pub layer2:           Layer2Data,
    pub sprite_layer:     SpriteLayer,
}

// -------------------------------------------------------------------------------------------------

impl Level {
    pub fn parse(rom: &Rom, level_num: u32) -> Result<Self, LevelParseError> {
        let (primary_header, layer1) = Self::parse_ph_and_l1(rom, level_num)?;
        let layer2 = Self::parse_l2(rom, level_num)?;
        let (sprite_header, sprite_layer) = Self::parse_sh_and_sl(rom, level_num)?;
        let secondary_header =
            SecondaryHeader::read_from_rom(rom, level_num).map_err(LevelParseError::SecondaryHeaderRead)?;

        Ok(Level { primary_header, secondary_header, sprite_header, layer1, layer2, sprite_layer })
    }

    fn parse_ph_and_l1(rom: &Rom, level_num: u32) -> Result<(PrimaryHeader, ObjectLayer), LevelParseError> {
        let l1_ptr_slice = SnesSlice::new(AddrSnes(0x05E000), 0x200 * 3);
        let ph_addr = rom
            .parse_lorom(l1_ptr_slice, count(map(le_u24, AddrSnes), 0x200))
            .map_err(LevelParseError::Layer1AddressRead)?[level_num as usize];

        let primary_header = {
            let ph_slice = SnesSlice::new(ph_addr, PRIMARY_HEADER_SIZE);
            PrimaryHeader::new(rom.slice_lorom(ph_slice).map_err(LevelParseError::PrimaryHeaderRead)?)
        };

        let layer1 = {
            let bytes = rom.slice_from(ph_addr + PRIMARY_HEADER_SIZE as u32).map_err(LevelParseError::Layer1Read)?;
            parse_bytes(bytes, ObjectLayer::parse).map_err(LevelParseError::Layer1Read)?.0
        };

        Ok((primary_header, layer1))
    }

    fn parse_l2(rom: &Rom, level_num: u32) -> Result<Layer2Data, LevelParseError> {
        const LAYER2_DATA: AddrSnes = AddrSnes(0x05E600);

        let l2_addr_slice = SnesSlice::new(LAYER2_DATA + (3 * level_num), 3);
        let l2_ptr =
            rom.parse_lorom(l2_addr_slice, map(le_u24, AddrSnes)).map_err(LevelParseError::Layer2AddressRead)?;

        if l2_ptr.bank() == 0xFF {
            let bytes = rom.slice_from(l2_ptr.with_bank(0x0C)).map_err(LevelParseError::Layer2Isolate)?;
            let (mut background, _) =
                BackgroundData::read_from(bytes).map_err(LevelParseError::Layer2BackgroundRead)?;
            // The game's Map16 bank for this background comes from the
            // pointer itself (bank_05.asm CODE_058126): below $0CE8FE is
            // page 0, at/above is page 1.
            background.set_high_byte(bg_high_byte_for_pointer(l2_ptr.0));
            Ok(Layer2Data::Background(background))
        } else {
            let header_slice = SnesSlice::new(l2_ptr, LAYER2_HEADER_SIZE);
            let header_bytes = rom.slice_lorom(header_slice).map_err(LevelParseError::Layer2Read)?;
            let mut header = [0u8; LAYER2_HEADER_SIZE];
            header.copy_from_slice(header_bytes);
            let bytes = rom.slice_from(l2_ptr + LAYER2_HEADER_SIZE as u32).map_err(LevelParseError::Layer2Read)?;
            let (objects, _) = parse_bytes(bytes, ObjectLayer::parse).map_err(LevelParseError::Layer2Read)?;
            Ok(Layer2Data::Objects { header, objects })
        }
    }

    fn parse_sh_and_sl(rom: &Rom, level_num: u32) -> Result<(SpriteHeader, SpriteLayer), LevelParseError> {
        const SPRITE_DATA: AddrSnes = AddrSnes(0x05EC00);

        let sprite_ptr_slice = SnesSlice::new(SPRITE_DATA + (2 * level_num), 2);
        let sh_addr = rom.parse_lorom(sprite_ptr_slice, le_u16).map_err(LevelParseError::SpriteAddressRead)?;
        let sh_addr = AddrSnes(sh_addr as _).with_bank(0x07);

        let sh_slice = SnesSlice::new(sh_addr, SPRITE_HEADER_SIZE);
        let sprite_header =
            rom.parse_lorom(sh_slice, SpriteHeader::read_from).map_err(LevelParseError::SpriteHeaderRead)?;

        let sprite_layer = {
            let bytes = rom.slice_from(sh_addr + 1).map_err(LevelParseError::SpriteRead)?;
            parse_bytes(bytes, SpriteLayer::parse).map_err(LevelParseError::SpriteRead)?.0
        };

        Ok((sprite_header, sprite_layer))
    }
}

// -------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        snes_utils::addr::{AddrPc, AddrSnes},
        SmwRom,
    };

    /// Real-ROM tests: need `ROM_PATH` pointing at a headerless SMW ROM.
    fn test_rom() -> Option<SmwRom> {
        let path = std::env::var("ROM_PATH").ok()?;
        SmwRom::from_file(&path).ok()
    }

    fn l2_pointer_pc(rom_bytes: &[u8], level_num: u32) -> u32 {
        let tbl_pc = AddrPc::try_from_lorom(AddrSnes(0x05E600 + level_num * 3)).unwrap().as_index() as u32;
        let s = &rom_bytes[tbl_pc as usize..tbl_pc as usize + 3];
        u32::from_le_bytes([s[0], s[1], s[2], 0])
    }

    /// The 5-byte Layer 2 object header extracted by `parse_l2` must be
    /// exactly the bytes at the level's Layer 2 pointer in the vanilla ROM.
    #[test]
    #[ignore]
    fn real_rom_l2_object_headers_match_pointer_bytes() {
        let rom = test_rom().expect("ROM_PATH must point at a headerless SMW ROM");
        let rom_bytes = rom.rom.bytes();
        let mut object_levels = 0u32;
        for (level_num, level) in rom.levels.iter().enumerate() {
            let Layer2Data::Objects { header, .. } = &level.layer2 else { continue };
            object_levels += 1;
            let l2_ptr = l2_pointer_pc(rom_bytes, level_num as u32);
            assert_ne!(l2_ptr >> 16, 0xFF, "level {level_num:03X}: parsed as objects but pointer bank is $FF");
            let data_pc = AddrPc::try_from_lorom(AddrSnes(l2_ptr)).unwrap().as_index() as usize;
            assert_eq!(
                &rom_bytes[data_pc..data_pc + LAYER2_HEADER_SIZE],
                header,
                "level {level_num:03X}: parsed L2 header != ROM bytes at the Layer 2 pointer",
            );
        }
        assert!(object_levels > 0, "expected some levels with Layer 2 objects in the vanilla ROM");
    }

    /// Simulate the editor flow: edit the L2 header bytes, write them at the
    /// Layer 2 pointer (as `save_to_rom` does), re-parse — the new header
    /// must come back and the object stream must be untouched.
    #[test]
    #[ignore]
    fn real_rom_l2_header_edit_round_trips() {
        let rom = test_rom().expect("ROM_PATH must point at a headerless SMW ROM");
        // Level 0x9 has Layer 2 objects in the vanilla ROM.
        let Layer2Data::Objects { header, objects } = &rom.levels[0x9].layer2 else {
            panic!("level 009 should have Layer 2 objects in the vanilla ROM");
        };

        let edited = [0xDEu8, 0xAD, 0xBE, 0xEF, 0x00];
        assert_ne!(&edited, header, "test edit must differ from the vanilla header");

        let mut bytes = rom.rom.bytes().to_vec();
        let l2_ptr = l2_pointer_pc(&bytes, 0x9);
        let data_pc = AddrPc::try_from_lorom(AddrSnes(l2_ptr)).unwrap().as_index() as usize;
        bytes[data_pc..data_pc + LAYER2_HEADER_SIZE].copy_from_slice(&edited);

        let reparsed = Level::parse(&Rom::new(bytes).unwrap(), 0x9).unwrap();
        let Layer2Data::Objects { header: header2, objects: objects2 } = &reparsed.layer2 else {
            panic!("level 009 lost its Layer 2 objects after the header edit");
        };
        assert_eq!(&edited, header2, "edited L2 header did not round-trip through re-parse");
        assert_eq!(objects.as_bytes(), objects2.as_bytes(), "object stream changed by the header edit");
    }
}
