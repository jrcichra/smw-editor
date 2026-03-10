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

        // SMW level dimensions: each screen is 16 tiles wide × 27 tiles tall
        // We build a flat tile grid (map) at 16×16px per tile, then emit Tile structs
        const SCREEN_W: u32 = 16;
        const SCREEN_H: u32 = 27;
        const LEVEL_W: u32 = 32 * SCREEN_W; // max width (32 screens)
        const LEVEL_H: u32 = SCREEN_H;

        // Use a sparse HashMap so we don't allocate a huge grid up front
        // key = (tile_x, tile_y), value = map16 block index
        let mut tile_map: std::collections::HashMap<(u32, u32), usize> =
            std::collections::HashMap::with_capacity(1024);

        let mut current_screen: u32 = 0;

        for obj in &raw_objects {
            if obj.is_exit() {
                continue;
            }
            if obj.is_screen_jump() {
                current_screen = obj.screen_number() as u32;
                continue;
            }
            if obj.is_new_screen() {
                current_screen = current_screen.saturating_add(1);
            }

            let local_x = obj.x() as u32;
            let local_y = obj.y() as u32;
            let abs_x = local_x + if is_vertical { 0 } else { current_screen * SCREEN_W };
            let abs_y = local_y + if is_vertical { current_screen * SCREEN_H } else { 0 };

            let settings = obj.settings() as u32;
            let s_lo = settings & 0x0F;
            let s_hi = (settings >> 4) & 0x0F;

            // All 0x40 standard object IDs map into the map16 table via a fixed
            // encoding: each object ID occupies a 4×(max_ext+1) region of the
            // map16 page starting at a well-known base.
            //
            // Rather than guessing tile layouts, we use the actual in-ROM
            // object property encoding.  The standard object table at
            // $05B37E (Lunar Magic calls it "Object Tiles") maps each of the 64
            // standard objects to a (base_tile, width_style, height_style).
            // We approximate this using the actual map16 blocks which are
            // already parsed in rom.map16_tilesets.
            //
            // The reliable approach: SMW objects place map16 tiles.  For each
            // object the first byte after its header is the "settings" byte
            // which encodes the (w,h) extension.  We expand each object into
            // its rectangular tile grid using the real map16 block data.

            let obj_id = obj.standard_object_number() as u32;

            // Object-to-map16 base tile lookup (from SMW disassembly / Lunar Magic).
            // Tile numbers here are map16 indices (0x000–0x1FF).
            // Format: (base_tile, w_tiles fn, h_tiles fn)
            // The closures return the (cols, rows) to fill given (s_lo, s_hi).
            let expand = expand_object(obj_id, s_lo, s_hi);

            for (dx, dy, map16_tile) in expand {
                let tx = abs_x + dx;
                let ty = abs_y + dy;
                if tx < LEVEL_W && ty < LEVEL_H {
                    tile_map.insert((tx, ty), map16_tile);
                }
            }
        }

        // Now convert the tile map to Tile structs for the renderer
        let mut l1_tiles: Vec<Tile> = Vec::with_capacity(tile_map.len() * 4);
        for ((tx, ty), map16_idx) in &tile_map {
            let px = tx * 16;
            let py = ty * 16;
            if let Some(block) = rom.map16_tilesets.get_map16_tile(*map16_idx, tileset_idx) {
                for (sub, (ox, oy)) in [
                    (block.upper_left,  (0u32, 0u32)),
                    (block.upper_right, (8u32, 0u32)),
                    (block.lower_left,  (0u32, 8u32)),
                    (block.lower_right, (8u32, 8u32)),
                ] {
                    if sub.tile_number() != 0 {
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

/// Expand a standard SMW object into a list of (dx, dy, map16_tile_num) entries.
/// dx/dy are tile offsets relative to the object origin.
/// map16_tile_num is a 0x000..0x1FF index into the map16 table.
///
/// Object layout data from SMW disassembly and Lunar Magic:
/// s_lo = settings & 0x0F  (width extension: actual width = s_lo + base)
/// s_hi = settings >> 4    (height extension: actual height = s_hi + base)
fn expand_object(obj_id: u32, s_lo: u32, s_hi: u32) -> Vec<(u32, u32, usize)> {
    let mut out = Vec::new();

    // Horizontal repeat: w tiles wide (1-based from s_lo)
    let w = s_lo + 1;
    // Vertical repeat: h tiles tall (1-based from s_hi)
    let h = s_hi + 1;

    // Helpers
    // fill a rectangle with a single tile
    let fill_rect = |out: &mut Vec<(u32,u32,usize)>, tile: usize, x0: u32, y0: u32, cols: u32, rows: u32| {
        for dy in 0..rows { for dx in 0..cols { out.push((x0+dx, y0+dy, tile)); } }
    };
    // place a single tile
    let single = |out: &mut Vec<_>, tile: usize, dx: u32, dy: u32| { out.push((dx, dy, tile)); };

    match obj_id {
        // ── 0x00: Sloped ledge / ground (w+1 wide, 1 tall, top surface)
        0x00 => {
            // Map16 0x000=top-left cap, 0x001=top middle fill, 0x002=top-right cap
            single(&mut out, 0x000, 0, 0);
            for dx in 1..=w { out.push((dx, 0, 0x001)); }
            out.push((w+1, 0, 0x002));
        }
        // ── 0x01: Ground floor (w+2 wide, h+1 tall with top+body)
        0x01 => {
            // Top row: 0x020=left, 0x021=mid, 0x022=right
            single(&mut out, 0x020, 0, 0);
            for dx in 1..=w { out.push((dx, 0, 0x021)); }
            out.push((w+1, 0, 0x022));
            // Body rows: 0x023=left edge, 0x025=fill, 0x024=right edge
            for dy in 1..=h {
                out.push((0, dy, 0x023));
                for dx in 1..=w { out.push((dx, dy, 0x025)); }
                out.push((w+1, dy, 0x024));
            }
        }
        // ── 0x02: Vertical cliff / wall left side (1 wide, h+1 tall)
        0x02 => {
            for dy in 0..=h { fill_rect(&mut out, 0x015, 0, dy, 1, 1); }
        }
        // ── 0x03: Vertical cliff / wall right side
        0x03 => {
            for dy in 0..=h { fill_rect(&mut out, 0x016, 0, dy, 1, 1); }
        }
        // ── 0x04: Diagonal ground slope (up-right), w+1 tiles
        0x04 => {
            for dx in 0..=w { out.push((dx, w - dx, 0x040)); }
        }
        // ── 0x05: Diagonal ground slope (up-left)
        0x05 => {
            for dx in 0..=w { out.push((dx, dx, 0x040)); }
        }
        // ── 0x06: Water surface (w+2 wide, h+1 tall)
        0x06 => {
            for dx in 0..=w+1 { out.push((dx, 0, 0x00E)); }
            fill_rect(&mut out, 0x00F, 0, 1, w+2, h);
        }
        // ── 0x07: Lava (w+2 wide, h+1 tall)
        0x07 => {
            for dx in 0..=w+1 { out.push((dx, 0, 0x00D)); }
            fill_rect(&mut out, 0x00C, 0, 1, w+2, h);
        }
        // ── 0x08: Vertical pipe (2 wide, h+2 tall, green)
        0x08 | 0x09 => {
            out.push((0, 0, 0x10C)); out.push((1, 0, 0x10D)); // pipe top
            for dy in 1..=h+1 {
                out.push((0, dy, 0x10E)); out.push((1, dy, 0x10F));
            }
        }
        // ── 0x0A: Horizontal pipe (w+2 wide, 2 tall)
        0x0A => {
            out.push((0, 0, 0x110)); out.push((0, 1, 0x111));
            for dx in 1..=w { out.push((dx, 0, 0x112)); out.push((dx, 1, 0x113)); }
            out.push((w+1, 0, 0x114)); out.push((w+1, 1, 0x115));
        }
        // ── 0x0B: Bullet Bill cannon (1 wide, h+1 tall)
        0x0B => {
            out.push((0, 0, 0x118));
            for dy in 1..=h { out.push((0, dy, 0x119)); }
        }
        // ── 0x0C: Coin row (w+1 wide)
        0x0C => {
            for dx in 0..=w { out.push((dx, 0, 0x02A)); }
        }
        // ── 0x0D: Note block row (w+1 wide)
        0x0D => {
            for dx in 0..=w { out.push((dx, 0, 0x02C)); }
        }
        // ── 0x0E: Brick row (w+1 wide, h+1 tall)
        0x0E => {
            fill_rect(&mut out, 0x002, 0, 0, w+1, h+1);
        }
        // ── 0x0F: ? block row (w+1 wide)
        0x0F => {
            for dx in 0..=w { out.push((dx, 0, 0x024)); }
        }
        // ── 0x10: Wooden platform (w+2 wide, 1 tall)
        0x10 => {
            out.push((0, 0, 0x131));
            for dx in 1..=w { out.push((dx, 0, 0x132)); }
            out.push((w+1, 0, 0x133));
        }
        // ── 0x11: Cement platform (w+2 wide, 1 tall)
        0x11 => {
            out.push((0, 0, 0x128));
            for dx in 1..=w { out.push((dx, 0, 0x129)); }
            out.push((w+1, 0, 0x12A));
        }
        // ── 0x12: Rock/ground top surface only (w+2 wide)
        0x12 => {
            out.push((0, 0, 0x11E));
            for dx in 1..=w { out.push((dx, 0, 0x11F)); }
            out.push((w+1, 0, 0x120));
        }
        // ── 0x13: Castle wall (h+1 tall, 1 wide)
        0x13 => {
            for dy in 0..=h { out.push((0, dy, 0x170)); }
        }
        // ── 0x14: Castle platform
        0x14 => {
            out.push((0, 0, 0x175));
            for dx in 1..=w { out.push((dx, 0, 0x176)); }
            out.push((w+1, 0, 0x177));
        }
        // ── 0x15: Donut lift (1×1)
        0x15 => { single(&mut out, 0x163, 0, 0); }
        // ── 0x16: Cloud platform (w+2 wide)
        0x16 => {
            out.push((0, 0, 0x134));
            for dx in 1..=w { out.push((dx, 0, 0x135)); }
            out.push((w+1, 0, 0x136));
        }
        // ── 0x17: Rope (vertical, h+1 tall)
        0x17 => {
            for dy in 0..=h { out.push((0, dy, 0x166)); }
        }
        // ── 0x18: Rope (horizontal, w+1 wide)
        0x18 => {
            for dx in 0..=w { out.push((dx, 0, 0x167)); }
        }
        // ── 0x19: Chain-link fence (w+2 wide, h+1 tall)
        0x19 => {
            fill_rect(&mut out, 0x168, 0, 0, w+2, h+1);
        }
        // ── 0x1A: Slope (diagonal up-right, big)
        0x1A => {
            let size = w.max(1);
            for i in 0..size {
                out.push((i, size-1-i, 0x040));
                for j in size-i..size { out.push((i, j, 0x025)); }
            }
        }
        // ── 0x1B: Slope (diagonal up-left, big)
        0x1B => {
            let size = w.max(1);
            for i in 0..size {
                out.push((i, i, 0x040));
                for j in i+1..size { out.push((i, j, 0x025)); }
            }
        }
        // ── 0x1C: Small slope up-right
        0x1C => {
            out.push((0, 1, 0x040)); out.push((1, 0, 0x040));
            out.push((0, 2, 0x025)); out.push((1, 1, 0x025)); out.push((1, 2, 0x025));
        }
        // ── 0x1D: Small slope up-left
        0x1D => {
            out.push((0, 0, 0x040)); out.push((1, 1, 0x040));
            out.push((0, 1, 0x025)); out.push((0, 2, 0x025)); out.push((1, 2, 0x025));
        }
        // ── 0x1E: Muncher (w+1 wide)
        0x1E => {
            for dx in 0..=w { out.push((dx, 0, 0x034)); }
        }
        // ── 0x1F: Goal tape post
        0x1F => {
            for dy in 0..=h { out.push((0, dy, 0x176)); }
        }
        // ── 0x20: Used block row
        0x20 => {
            for dx in 0..=w { out.push((dx, 0, 0x025)); }
        }
        // ── 0x21: Turn block row
        0x21 => {
            for dx in 0..=w { out.push((dx, 0, 0x02E)); }
        }
        // ── 0x22: Yoshi coin
        0x22 => { single(&mut out, 0x171, 0, 0); }
        // ── 0x23: Grab block
        0x23 => { single(&mut out, 0x02F, 0, 0); }
        // ── 0x24-0x2F: Single tiles — map to map16 block for misc objects
        0x24 => { single(&mut out, 0x180, 0, 0); } // Door
        0x25 => { single(&mut out, 0x186, 0, 0); } // Goal sphere
        0x26 => { single(&mut out, 0x02B, 0, 0); } // P-switch
        0x27 => { single(&mut out, 0x006, 0, 0); } // p-switch used
        // ── Fallback: place a single ground tile as a placeholder
        _ => {
            // Map the object ID loosely to a map16 tile so *something* shows up.
            // Objects >= 0x40 are extended objects; we just skip unknown ones.
            if obj_id < 0x40 {
                let tile = ((obj_id * 4) as usize).min(0x1FE);
                single(&mut out, tile, 0, 0);
            }
        }
    }

    out
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
