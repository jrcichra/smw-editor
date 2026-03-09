use egui::Vec2;
use glow::*;
use smwe_render::{
    gfx_buffers::GfxBuffers,
    tile_renderer::{Tile, TileRenderer, TileUniforms},
};
use smwe_rom::SmwRom;

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

    pub(super) fn upload_level_from_rom(&mut self, gl: &Context, rom: &SmwRom, level_num: u16) {
        if self.destroyed {
            return;
        }
        let level_idx = level_num as usize;
        if level_idx >= rom.levels.len() {
            self.layer1.set_tiles(gl, Vec::new());
            self.layer2.set_tiles(gl, Vec::new());
            self.sprites.set_tiles(gl, Vec::new());
            return;
        }
        let level = &rom.levels[level_idx];
        let is_vertical = level.secondary_header.vertical_level();
        let tileset_idx = (level.primary_header.fg_bg_gfx() as usize)
            % smwe_rom::objects::tilesets::TILESETS_COUNT;

        // Parse object layer to get placed objects with absolute tile coords
        let raw_bytes = level.layer1.as_bytes();
        let raw_objects = match smwe_rom::objects::Object::parse_from_ram(raw_bytes) {
            Some(o) => o,
            None => {
                self.layer1.set_tiles(gl, Vec::new());
                self.layer2.set_tiles(gl, Vec::new());
                self.sprites.set_tiles(gl, Vec::new());
                return;
            }
        };

        const SCREEN_WIDTH: u32 = 16;
        let mut l1_tiles: Vec<Tile> = Vec::new();
        let mut current_screen: u32 = 0;

        for obj in &raw_objects {
            if obj.is_exit() || obj.is_screen_jump() {
                if obj.is_screen_jump() {
                    current_screen = obj.screen_number() as u32;
                }
                continue;
            }
            if obj.is_new_screen() {
                current_screen = current_screen.saturating_add(1);
            }

            let local_x = obj.x() as u32;
            let local_y = obj.y() as u32;
            let abs_x = local_x + if is_vertical { 0 } else { current_screen * SCREEN_WIDTH };
            let abs_y = local_y + if is_vertical { current_screen * SCREEN_WIDTH } else { 0 };

            let obj_id = obj.standard_object_number() as usize;
            let settings = obj.settings() as u32;

            let mut blocks: Vec<(usize, u32, u32)> = Vec::new();
            object_tiles(obj_id, settings, &mut blocks);

            for (tile_num, dx, dy) in blocks {
                let tile_num = tile_num.min(0x1FF); // clamp to valid range
                let px = (abs_x + dx) * 16;
                let py = (abs_y + dy) * 16;
                if let Some(block) = rom.map16_tilesets.get_map16_tile(tile_num, tileset_idx) {
                    for (sub, (ox, oy)) in [
                        (block.upper_left,  (0u32, 0u32)),
                        (block.upper_right, (8,    0   )),
                        (block.lower_left,  (0,    8   )),
                        (block.lower_right, (8,    8   )),
                    ] {
                        l1_tiles.push(bg_tile(px + ox, py + oy, sub.0));
                    }
                }
            }
        }

        self.layer1.set_tiles(gl, l1_tiles);
        self.layer2.set_tiles(gl, Vec::new());
        self.sprites.set_tiles(gl, Vec::new());
    }

    pub(super) fn set_offset(&mut self, offset: Vec2) {
        if self.destroyed {
            return;
        }
        self.offset = offset;
    }
}

/// For each object, emit (tile_num, rel_x, rel_y) for every 16x16 block to place.
/// tile_num is a map16 index. rel_x/y are in blocks relative to the object's origin.
/// settings byte encodes size: low nibble = width ext, high nibble = height ext.
fn object_tiles(obj_id: usize, settings: u32, out: &mut Vec<(usize, u32, u32)>) {
    let s_lo = (settings & 0x0F) as u32;
    let s_hi = ((settings >> 4) & 0x0F) as u32;
    let w = s_lo + 1; // number of interior/repeat tiles horizontally
    let h = s_hi + 1;

    // Helper closures
    let fill = |out: &mut Vec<_>, tile: usize, cols: u32, rows: u32| {
        for dy in 0..rows { for dx in 0..cols { out.push((tile, dx, dy)); } }
    };
    let row = |out: &mut Vec<_>, tile: usize, cols: u32, row: u32| {
        for dx in 0..cols { out.push((tile, dx, row)); }
    };

    match obj_id {
        // ── Ground / terrain ─────────────────────────────────────────────────
        0x00 => { // Slanted ground top (left cap + fill + right cap)
            out.push((0x000, 0,   0)); // left edge
            row(out, 0x001, w, 0);     // middle fill
            out.push((0x002, w+1, 0)); // right edge (approx)
        }
        0x01 => { // Ground top — straight line
            out.push((0x010, 0,   0));
            row(out, 0x011, w, 0);
            out.push((0x012, w+1, 0));
        }
        0x02 => { // Ground fill (solid block)
            out.push((0x100, 0,   0)); out.push((0x101, 1, 0)); // top row
            fill(out, 0x102, 2, h);                               // body
        }
        0x04 => { // Left wall, extends down
            fill(out, 0x015, 1, h+1);
        }
        0x05 => { // Right wall
            fill(out, 0x016, 1, h+1);
        }
        0x06 => { // Ledge (horizontal)
            out.push((0x026, 0, 0));
            row(out, 0x027, w, 0);
            out.push((0x028, w+1, 0));
        }
        0x07 => { // Block fill
            fill(out, 0x130, w+2, h+1);
        }
        // ── Pipes ─────────────────────────────────────────────────────────────
        0x08 | 0x09 => { // Vertical pipe
            let (top_l, top_r, body_l, body_r) = (0x10C, 0x10D, 0x10E, 0x10F);
            out.push((top_l, 0, 0)); out.push((top_r, 1, 0));
            for dy in 1..=h { out.push((body_l, 0, dy)); out.push((body_r, 1, dy)); }
        }
        0x0A => { // Horizontal pipe top
            out.push((0x110, 0, 0)); row(out, 0x112, w, 0); out.push((0x114, w+1, 0));
        }
        0x0B => { // Horizontal pipe bottom
            out.push((0x111, 0, 0)); row(out, 0x113, w, 0); out.push((0x115, w+1, 0));
        }
        // ── Water / lava ─────────────────────────────────────────────────────
        0x0C => { fill(out, 0x00E, w+2, h+1); } // Water
        0x0D => { fill(out, 0x00D, w+2, h+1); } // Lava
        // ── Platforms ────────────────────────────────────────────────────────
        0x0E => { // Floating platform
            out.push((0x131, 0, 0));
            row(out, 0x132, w, 0);
            out.push((0x133, w+1, 0));
        }
        0x0F => { // Cement platform
            out.push((0x128, 0, 0));
            row(out, 0x129, w, 0);
            out.push((0x12A, w+1, 0));
        }
        // ── Cave / underground ────────────────────────────────────────────────
        0x12 => {
            out.push((0x11E, 0, 0)); row(out, 0x11F, w, 0); out.push((0x120, w+1, 0));
        }
        // ── Items / coins ─────────────────────────────────────────────────────
        0x13 => { row(out, 0x02A, w+1, 0); } // Coins
        0x14 => { row(out, 0x02B, w+1, 0); } // Note blocks
        0x24 => { row(out, 0x024, w+1, 0); } // ?-blocks
        0x25 => { row(out, 0x002, w+1, 0); } // Bricks
        0x26 => { row(out, 0x02A, w+1, 0); } // Coin blocks
        0x29 => { out.push((0x024, 0, 0)); }  // Single ?
        0x2A => { out.push((0x002, 0, 0)); }  // Single brick
        0x2B => { row(out, 0x02D, w+1, 0); }  // Note blocks alt
        0x31 => { out.push((0x171.min(0x1FF), 0, 0)); } // Yoshi coin
        // ── Slopes ───────────────────────────────────────────────────────────
        0x1A | 0x1B | 0x18 | 0x1C | 0x1D => {
            // Just show the base tile for now
            out.push((0x040.min(0x1FF), 0, 0));
        }
        // ── Doors / misc ─────────────────────────────────────────────────────
        0x74 => {
            for dy in 0..3 { for dx in 0..2 {
                out.push((0x060 + dy*2 + dx, dx as u32, dy as u32));
            }}
        }
        // ── Fallback: render a single tile of the object's base type ─────────
        _ => {
            let tile = (obj_id * 4).min(0x1FF);
            out.push((tile, 0, 0));
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

#[allow(dead_code)]
fn sp_tile(x: u32, y: u32, t: u16) -> Tile {
    let t = t as u32;
    let tile = (t & 0x1FF) + 0x600;
    let scale = 8;
    let pal = ((t >> 9) & 0x7) + 8;
    let params = scale | (pal << 8) | (t & 0xC000);
    Tile([x, y, tile, params])
}
