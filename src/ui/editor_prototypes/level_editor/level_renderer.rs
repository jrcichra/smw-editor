use egui::Vec2;
use glow::*;
use smwe_render::{
    gfx_buffers::GfxBuffers,
    tile_renderer::{Tile, TileRenderer, TileUniforms},
};
use smwe_rom::SmwRom;

#[derive(Debug)]
pub(super) struct LevelRenderer {
    layer1:   TileRenderer,
    layer2:   TileRenderer,
    sprites:  TileRenderer,
    gfx_bufs: GfxBuffers,

    offset:    Vec2,
    destroyed: bool,
}

impl LevelRenderer {
    pub(super) fn new(gl: &Context) -> Self {
        let layer1 = TileRenderer::new(gl);
        let layer2 = TileRenderer::new(gl);
        let sprites = TileRenderer::new(gl);
        let gfx_bufs = GfxBuffers::new(gl);
        Self { layer1, layer2, sprites, gfx_bufs, offset: Vec2::splat(0.), destroyed: false }
    }

    pub(super) fn destroy(&mut self, gl: &Context) {
        self.gfx_bufs.destroy(gl);
        self.layer1.destroy(gl);
        self.layer2.destroy(gl);
        self.destroyed = true;
    }

    pub(super) fn paint(&self, gl: &Context, screen_size: Vec2, zoom: f32) {
        if self.destroyed {
            return;
        }
        let uniforms = TileUniforms { gfx_bufs: self.gfx_bufs, screen_size, offset: self.offset, zoom };
        self.layer2.paint(gl, &uniforms);
        self.layer1.paint(gl, &uniforms);
        self.sprites.paint(gl, &uniforms);
    }

    pub(super) fn upload_gfx(&self, gl: &Context, data: &[u8]) {
        if self.destroyed {
            return;
        }
        self.gfx_bufs.upload_vram(gl, data);
    }

    pub(super) fn upload_palette(&self, gl: &Context, data: &[u8]) {
        if self.destroyed {
            return;
        }
        self.gfx_bufs.upload_palette(gl, data);
    }

    pub(super) fn upload_level_from_rom(&mut self, gl: &Context, rom: &SmwRom, level_num: u16) {
        if self.destroyed {
            return;
        }
        let level_idx = level_num as usize;
        if level_idx >= rom.levels.len() {
            return;
        }
        let level = &rom.levels[level_idx];
        let is_vertical = level.secondary_header.vertical_level();
        let has_layer2 = matches!(level.layer2, smwe_rom::level::Layer2Data::Objects(_));
        let scr_len: u32 = match (is_vertical, has_layer2) {
            (false, false) => 0x20,
            (true,  false) => 0x1C,
            (false, true)  => 0x10,
            (true,  true)  => 0x0E,
        };
        let (scr_w, scr_h): (u32, u32) = if is_vertical { (16, 32) } else { (16, 27) };

        // Build layer1 tile list from the map16 tileset data.
        // The map16 tiles tell us which 8x8 sub-tiles to use.
        let tileset_idx = level.primary_header.fg_bg_gfx() as usize
            % smwe_rom::objects::tilesets::TILESETS_COUNT;

        let mut l1_tiles: Vec<Tile> = Vec::new();
        let total_blocks = (scr_len * scr_w * scr_h) as usize;

        for block_idx in 0..total_blocks.min(rom.map16_tilesets.tiles.len()) {
            use smwe_rom::objects::tilesets::Tile as TilesetTile;
            let map16 = &rom.map16_tilesets.tiles[block_idx];
            let block = match map16 {
                TilesetTile::Shared(b) => b,
                TilesetTile::TilesetSpecific(arr) => &arr[tileset_idx.min(arr.len() - 1)],
            };

            let (block_x, block_y) = if is_vertical {
                let screen  = block_idx / (scr_w * scr_h) as usize;
                let in_scr  = block_idx % (scr_w * scr_h) as usize;
                let col     = in_scr % scr_w as usize;
                let row     = in_scr / scr_w as usize;
                let sub_y   = screen / 2;
                let sub_x   = screen % 2;
                (col as u32 * 16 + sub_x as u32 * 256, row as u32 * 16 + sub_y as u32 * 256)
            } else {
                let screen = block_idx / (scr_w * scr_h) as usize;
                let in_scr = block_idx % (scr_w * scr_h) as usize;
                let col    = in_scr % scr_w as usize;
                let row    = in_scr / scr_w as usize;
                (col as u32 * 16 + screen as u32 * 256, row as u32 * 16)
            };

            for (sub_tile, (ox, oy)) in [
                (block.upper_left,  (0u32, 0u32)),
                (block.upper_right, (8,    0   )),
                (block.lower_left,  (0,    8   )),
                (block.lower_right, (8,    8   )),
            ] {
                l1_tiles.push(bg_tile(block_x + ox, block_y + oy, sub_tile.0));
            }
        }

        self.layer1.set_tiles(gl, l1_tiles);
        self.layer2.set_tiles(gl, Vec::new()); // layer2 rendering deferred
        self.sprites.set_tiles(gl, Vec::new());
    }

    pub(super) fn set_offset(&mut self, offset: Vec2) {
        if self.destroyed {
            return;
        }
        self.offset = offset;
    }

}

fn bg_tile(x: u32, y: u32, t: u16) -> Tile {
    let t = t as u32;
    let tile = t & 0x3FF;
    let scale = 8;
    let pal = (t >> 10) & 0x7;
    let params = scale | (pal << 8) | (t & 0xC000);
    Tile([x, y, tile, params])
}

#[allow(dead_code)]
fn sp_tile(x: u32, y: u32, t: u16) -> Tile {
    let t = t as u32;
    let tile = (t & 0x1FF) + 0x600;
    let scale = 8;
    let pal = ((t >> 9) & 0x7) + 8;
    let params = scale | (pal << 8) | (t & 0xC000);
    Tile([x, y, tile, params])
}
