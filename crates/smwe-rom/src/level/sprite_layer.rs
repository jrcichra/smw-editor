use std::convert::TryInto;

use nom::{
    bytes::complete::{tag, take},
    multi::many_till,
    IResult,
};

pub const SPRITE_INSTANCE_SIZE: usize = 3;

pub type SpriteID = u8;

#[derive(Debug, Clone)]
pub struct SpriteInstance([u8; SPRITE_INSTANCE_SIZE]);

#[derive(Debug, Clone)]
pub struct SpriteLayer {
    pub sprites: Vec<SpriteInstance>,
}

impl SpriteInstance {
    pub fn from_raw(bytes: [u8; SPRITE_INSTANCE_SIZE]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; SPRITE_INSTANCE_SIZE] {
        &self.0
    }

    pub fn new(id: u8, x_tile: u32, y_tile: u32, screen: u32, _vertical: bool) -> Self {
        let mut bytes = [0u8; 3];
        // Byte 2: sprite ID
        bytes[2] = id;
        // Byte 1: screen low nibble in bits 0-3, X tile in bits 4-7
        let x_local = (x_tile % 16) as u8;
        let screen_lo = (screen & 0x0F) as u8;
        bytes[1] = (x_local << 4) | screen_lo;
        // Byte 0: Y in bits 4-7 and bit 0, screen high bit in bit 1
        let y_local = (y_tile % 32) as u8;
        let y_hi = (y_local >> 4) & 1;
        let y_lo = y_local & 0x0F;
        let screen_hi = ((screen >> 4) & 1) as u8;
        bytes[0] = (y_lo << 4) | (screen_hi << 1) | y_hi;
        Self(bytes)
    }

    pub fn xy_pos(&self) -> (u8, u8) {
        // yyyy---Y XXXX---- --------
        // xy_pos = (XXXX, Yyyyy)
        let x = self.0[1] >> 4;
        let y = {
            let hi = (self.0[0] & 0b1) << 4;
            let lo = self.0[0] >> 4;
            hi | lo
        };
        (x, y)
    }

    pub fn extra_bits(&self) -> u8 {
        (self.0[0] >> 2) & 0b11
    }

    pub fn screen_number(&self) -> u8 {
        // ------S- ----ssss --------
        let hi = (self.0[0] & 0b10) << 3;
        let lo = self.0[1] & 0b1111;
        hi | lo
    }

    pub fn sprite_id(&self) -> SpriteID {
        self.0[2]
    }
}

impl SpriteLayer {
    pub fn parse(input: &[u8]) -> IResult<&[u8], (Self, usize)> {
        let mut read_sprite_layer = many_till(take(SPRITE_INSTANCE_SIZE), tag(&[0xFFu8]));
        let (rest, (sprites_raw, _)) = read_sprite_layer(input)?;
        let sprites = sprites_raw.into_iter().map(|spr| SpriteInstance(spr.try_into().unwrap())).collect();
        let bytes_consumed = input.len() - rest.len();
        Ok((rest, (Self { sprites }, bytes_consumed)))
    }
}
