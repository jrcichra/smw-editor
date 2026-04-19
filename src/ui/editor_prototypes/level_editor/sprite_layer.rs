use smwe_rom::level::{Level, SpriteLayer as RomSpriteLayer};

use crate::undo::Undo;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct EditableSprite {
    pub x: u32,
    pub y: u32,
    pub sprite_id: u8,
    pub extra_bits: u8,
}

#[derive(Clone, Debug, Default)]
pub(super) struct EditableSpriteLayer {
    pub sprites: Vec<EditableSprite>,
}

impl EditableSpriteLayer {
    pub fn from_level(level: &Level) -> Self {
        Self::from_rom_sprite_layer(&level.sprite_layer, level.secondary_header.vertical_level())
    }

    pub fn from_rom_sprite_layer(layer: &RomSpriteLayer, vertical_level: bool) -> Self {
        let sprites = layer
            .sprites
            .iter()
            .map(|spr| {
                let (x_tile, y_tile) = spr.xy_pos();
                let screen = spr.screen_number() as u32;
                let (x, y) = if vertical_level {
                    let sx = screen % 2;
                    let sy = screen / 2;
                    (sx * 16 + x_tile as u32, sy * 32 + y_tile as u32)
                } else {
                    (screen * 16 + x_tile as u32, y_tile as u32)
                };
                EditableSprite { x, y, sprite_id: spr.sprite_id(), extra_bits: spr.extra_bits() }
            })
            .collect();
        Self { sprites }
    }

    pub fn serialize_bytes(&self, vertical_level: bool) -> anyhow::Result<Vec<u8>> {
        let mut out = Vec::with_capacity(self.sprites.len() * 3 + 1);
        for spr in &self.sprites {
            // Skip editor-only Mario spawn point marker (0xFF)
            if spr.sprite_id == 0xFF {
                continue;
            }
            let (screen, x_tile, y_tile) = sprite_screen_and_local(*spr, vertical_level)?;
            let y_low = (y_tile & 0x0F) << 4;
            let y_high = (y_tile >> 4) & 0x01;
            let screen_high = ((screen >> 4) & 0x01) << 1;
            let b0 = y_low | screen_high | ((spr.extra_bits & 0x03) << 2) | y_high;
            let b1 = ((x_tile & 0x0F) << 4) | (screen & 0x0F);
            out.extend_from_slice(&[b0, b1, spr.sprite_id]);
        }
        out.push(0xFF);
        Ok(out)
    }
}

fn sprite_screen_and_local(spr: EditableSprite, vertical_level: bool) -> anyhow::Result<(u8, u8, u8)> {
    if vertical_level {
        let sub_x = spr.x / 16;
        let sub_y = spr.y / 32;
        let screen = u8::try_from(sub_y * 2 + sub_x)?;
        let x = u8::try_from(spr.x % 16)?;
        let y = u8::try_from(spr.y % 32)?;
        Ok((screen, x, y))
    } else {
        let screen = u8::try_from(spr.x / 16)?;
        let x = u8::try_from(spr.x % 16)?;
        let y = u8::try_from(spr.y % 32)?;
        Ok((screen, x, y))
    }
}

impl Undo for EditableSpriteLayer {
    fn from_bytes(bytes: Vec<u8>) -> Self {
        let mut sprites = Vec::new();
        let mut i = 0usize;
        while i + 2 < bytes.len() {
            if bytes[i] == 0xFF {
                break;
            }
            let b0 = bytes[i];
            let b1 = bytes[i + 1];
            let sprite_id = bytes[i + 2];
            let y = ((b0 >> 4) & 0x0F) | ((b0 & 0x01) << 4);
            let x = b1 >> 4;
            let screen = (b1 & 0x0F) | ((b0 & 0x02) << 3);
            let extra_bits = (b0 >> 2) & 0x03;
            sprites.push(EditableSprite {
                x: (screen as u32) * 16 + x as u32,
                y: y as u32,
                sprite_id,
                extra_bits,
            });
            i += 3;
        }
        Self { sprites }
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.serialize_bytes(false).unwrap_or_else(|_| vec![0xFF])
    }

    fn size_bytes(&self) -> usize {
        self.sprites.len() * 3 + 1
    }
}
