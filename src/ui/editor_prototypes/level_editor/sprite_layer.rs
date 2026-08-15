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

// Undo/redo needs a lossless round-trip of `EditableSprite`, independent of level
// orientation — it is NOT the packed ROM sprite format (see `serialize_bytes`, used only
// when actually writing to ROM). Using the packed format here previously hardcoded
// horizontal-level screen math in `to_bytes`, silently truncating/scrambling sprite
// Y/screen on every undo of a vertical level.
const UNDO_SPRITE_STRIDE: usize = 10;

impl Undo for EditableSpriteLayer {
    fn from_bytes(bytes: Vec<u8>) -> Self {
        let sprites = bytes
            .chunks_exact(UNDO_SPRITE_STRIDE)
            .map(|c| {
                let sprite_id = c[0];
                let extra_bits = c[1];
                let x = u32::from_le_bytes([c[2], c[3], c[4], c[5]]);
                let y = u32::from_le_bytes([c[6], c[7], c[8], c[9]]);
                EditableSprite { x, y, sprite_id, extra_bits }
            })
            .collect();
        Self { sprites }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.sprites.len() * UNDO_SPRITE_STRIDE);
        for spr in &self.sprites {
            out.push(spr.sprite_id);
            out.push(spr.extra_bits);
            out.extend_from_slice(&spr.x.to_le_bytes());
            out.extend_from_slice(&spr.y.to_le_bytes());
        }
        out
    }

    fn size_bytes(&self) -> usize {
        self.sprites.len() * UNDO_SPRITE_STRIDE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_snapshot_round_trips_vertical_sprite_coordinates() {
        let layer = EditableSpriteLayer {
            sprites: vec![
                EditableSprite { x: 31, y: 511, sprite_id: 0x35, extra_bits: 3 },
                EditableSprite { x: 0, y: 0, sprite_id: 0x01, extra_bits: 0 },
            ],
        };

        let restored = EditableSpriteLayer::from_bytes(layer.to_bytes());
        assert_eq!(restored.sprites, layer.sprites);
    }
}
