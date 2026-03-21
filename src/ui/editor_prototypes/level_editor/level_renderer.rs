use egui::Vec2;
use glow::*;
use smwe_emu::{
    emu::{RawOamEntry, SpriteOamTile},
    Cpu,
};
use smwe_render::{
    gfx_buffers::GfxBuffers,
    tile_renderer::{Tile, TileRenderer, TileUniforms},
};

#[allow(dead_code)]
#[derive(Debug)]
pub(super) struct LevelRenderer {
    layer1: TileRenderer,
    layer2: TileRenderer,
    sprites: TileRenderer,
    gfx_bufs: GfxBuffers,

    offset: Vec2,
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

    pub(super) fn upload_level(&mut self, gl: &Context, cpu: &mut Cpu) {
        if self.destroyed {
            return;
        }
        self.load_layer(gl, cpu, false);
        self.load_layer(gl, cpu, true);
    }

    /// Build sprite tiles from ROM sprite positions + emulator OAM tile data.
    ///
    /// `oam_map`: sprite_id → Vec<SpriteOamTile> collected by sprite_oam_tiles().
    /// Each SpriteOamTile has dx/dy offsets from the emulator's fixed anchor
    /// (x=0xD0, y=0x80). We apply those offsets to the ROM-decoded pixel position
    /// so every tile of multi-tile sprites (Wiggler body, Dragon Coin frame) is
    /// placed correctly relative to the sprite's actual in-level position.
    pub(super) fn upload_sprites(
        &mut self, gl: &Context, sprite_layer: &smwe_rom::level::SpriteLayer,
        oam_map: &std::collections::HashMap<u8, Vec<SpriteOamTile>>, vertical: bool,
    ) {
        if self.destroyed {
            return;
        }
        let mut tiles = Vec::new();

        for spr in &sprite_layer.sprites {
            let id = spr.sprite_id();
            let oam_tiles = match oam_map.get(&id) {
                Some(v) if !v.is_empty() => v,
                _ => continue,
            };

            // Decode ROM pixel position for the sprite's anchor point
            let (x_tile, y_tile) = spr.xy_pos();
            let screen = spr.screen_number() as i32;
            let (anchor_x, anchor_y) = if vertical {
                let sx = screen % 2;
                let sy = screen / 2;
                (sx * 256 + x_tile as i32 * 16, sy * 512 + y_tile as i32 * 16)
            } else {
                (screen * 256 + x_tile as i32 * 16, y_tile as i32 * 16)
            };

            // Emit each OAM tile at anchor + its emulator-derived offset
            for oam in oam_tiles {
                let px = (anchor_x + oam.dx) as u32;
                let py = (anchor_y + oam.dy) as u32;
                let t = oam.tile_word;

                if oam.is_16x16 {
                    let (xn, xf) = if t & 0x4000 == 0 { (0u32, 8u32) } else { (8, 0) };
                    let (yn, yf) = if t & 0x8000 == 0 { (0u32, 8u32) } else { (8, 0) };
                    // Preserve attribute bits (palette, flip, priority); only increment
                    // the 9-bit tile number field. Adding to the raw u16 would corrupt
                    // the attribute byte whenever the tile number wraps past 0x100.
                    let attr = t & 0xFE00;
                    let base = t & 0x01FF;
                    tiles.push(sp_tile(px + xn, py + yn, attr | (base & 0x1FF)));
                    tiles.push(sp_tile(px + xf, py + yn, attr | ((base + 1) & 0x1FF)));
                    tiles.push(sp_tile(px + xn, py + yf, attr | ((base + 16) & 0x1FF)));
                    tiles.push(sp_tile(px + xf, py + yf, attr | ((base + 17) & 0x1FF)));
                } else {
                    tiles.push(sp_tile(px, py, t));
                }
            }
        }

        self.sprites.set_tiles(gl, tiles);
    }

    /// Render sprites directly from a raw OAM snapshot taken after exec_sprites().
    /// Since the camera starts at (0,0) after decompress_sublevel, OAM X/Y are
    /// already level-space pixel coordinates — no anchor math needed.
    pub(super) fn upload_sprites_oam(&mut self, gl: &Context, entries: &[RawOamEntry], _vertical: bool) {
        if self.destroyed {
            return;
        }
        let mut tiles = Vec::new();

        for entry in entries {
            let px = entry.x as u32;
            let py = entry.y as u32;
            let t = entry.tile_word;

            if entry.is_16x16 {
                let (xn, xf) = if t & 0x4000 == 0 { (0u32, 8u32) } else { (8, 0) };
                let (yn, yf) = if t & 0x8000 == 0 { (0u32, 8u32) } else { (8, 0) };
                let attr = t & 0xFE00;
                let base = t & 0x01FF;
                tiles.push(sp_tile(px + xn, py + yn, attr | (base & 0x1FF)));
                tiles.push(sp_tile(px + xf, py + yn, attr | ((base + 1) & 0x1FF)));
                tiles.push(sp_tile(px + xn, py + yf, attr | ((base + 16) & 0x1FF)));
                tiles.push(sp_tile(px + xf, py + yf, attr | ((base + 17) & 0x1FF)));
            } else {
                tiles.push(sp_tile(px, py, t));
            }
        }

        self.sprites.set_tiles(gl, tiles);
    }

    pub(super) fn set_offset(&mut self, offset: Vec2) {
        if self.destroyed {
            return;
        }
        self.offset = offset;
    }

    fn load_layer(&mut self, gl: &Context, cpu: &mut Cpu, bg: bool) {
        let mut tiles = Vec::new();

        let map16_bank = cpu.mem.cart.resolve("Map16Common").expect("Cannot resolve Map16Common") & 0xFF0000;
        let map16_bg = cpu.mem.cart.resolve("Map16BGTiles").expect("Cannot resolve Map16BGTiles");
        let vertical = cpu.mem.load_u8(0x5B) & if bg { 2 } else { 1 } != 0;

        let has_layer2 = {
            let mode = cpu.mem.load_u8(0x1925);
            let renderer_table = cpu.mem.cart.resolve("CODE_058955").unwrap() + 9;
            let renderer = cpu.mem.load_u24(renderer_table + (mode as u32) * 3);
            let l2_renderers = [cpu.mem.cart.resolve("CODE_058B8D"), cpu.mem.cart.resolve("CODE_058C71")];
            l2_renderers.contains(&Some(renderer))
        };

        let scr_len = match (vertical, has_layer2) {
            (false, false) => 0x20,
            (true, false) => 0x1C,
            (false, true) => 0x10,
            (true, true) => 0x0E,
        };
        let scr_size = if vertical { 16 * 32 } else { 16 * 27 };

        let (blocks_lo_addr, blocks_hi_addr) = match (bg, has_layer2) {
            (true, true) => {
                let o = scr_len * scr_size;
                (0x7EC800 + o, 0x7FC800 + o)
            }
            (true, false) => (0x7EB900, 0x7EBD00),
            (false, _) => (0x7EC800, 0x7FC800),
        };

        let len = if has_layer2 { 256 * 27 } else { 512 * 27 };

        for idx in 0..len {
            let (block_x, block_y) = if vertical {
                let (screen, sidx) = (idx / (16 * 16), idx % (16 * 16));
                let (row, column) = (sidx / 16, sidx % 16);
                let (sub_y, sub_x) = (screen / 2, screen % 2);
                (column * 16 + sub_x * 256, row * 16 + sub_y * 256)
            } else {
                let (screen, sidx) = (idx / (16 * 27), idx % (16 * 27));
                let (row, column) = (sidx / 16, sidx % 16);
                (column * 16 + screen * 256, row * 16)
            };

            let idx_adj = if bg && !has_layer2 { idx % (16 * 27 * 2) } else { idx };
            let block_id = cpu.mem.load_u8(blocks_lo_addr + idx_adj) as u16
                | (((cpu.mem.load_u8(blocks_hi_addr + idx_adj) as u16) & 0x01) << 8);

            let block_ptr = if bg && !has_layer2 {
                block_id as u32 * 8 + map16_bg
            } else {
                cpu.mem.load_u16(0x0FBE + block_id as u32 * 2) as u32 + map16_bank
            };

            for (tile_id, (off_x, off_y)) in (0..4).zip([(0u32, 0u32), (0, 8), (8, 0), (8, 8)]) {
                let tile_id = cpu.mem.load_u16(block_ptr + tile_id * 2);
                tiles.push(bg_tile(block_x + off_x, block_y + off_y, tile_id));
            }
        }

        if bg {
            self.layer2.set_tiles(gl, tiles);
        } else {
            self.layer1.set_tiles(gl, tiles);
        }
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

fn sp_tile(x: u32, y: u32, t: u16) -> Tile {
    let t = t as u32;
    let tile = (t & 0x1FF) + 0x600;
    let scale = 8;
    let pal = ((t >> 9) & 0x7) + 8;
    let params = scale | (pal << 8) | (t & 0xC000);
    Tile([x, y, tile, params])
}
