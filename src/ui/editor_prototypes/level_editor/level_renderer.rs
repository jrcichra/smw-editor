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
        const MAX_SCREENS: u32 = 32;
        const LEVEL_W: u32 = MAX_SCREENS * SCREEN_W;
        const LEVEL_H: u32 = SCREEN_H * 2; // some vertical levels use 2 rows

        // key = (tile_x, tile_y) in 16x16 tile coords, value = map16 block index
        let mut tile_map: std::collections::HashMap<(u32, u32), usize> =
            std::collections::HashMap::with_capacity(1024);

        // In SMW's object format, the N (new-screen) bit on an object means
        // "increment the current screen counter before placing this object".
        // It does NOT fire for every object — only specific "new screen" marker
        // objects. Standard objects use it as a flag that belongs to the current
        // screen boundary crossing. We must only increment once per crossing,
        // not once per object with the N bit set.
        //
        // Correct algorithm: screen starts at 0. When an object has N=1,
        // increment the screen counter ONCE for that boundary, then place the
        // object using the NEW screen number. Objects without N=1 stay on the
        // same screen.
        let mut current_screen: u32 = 0;

        for obj in &raw_objects {
            if obj.is_exit() {
                continue;
            }
            if obj.is_screen_jump() {
                current_screen = obj.screen_number() as u32;
                continue;
            }

            // N-bit: this object is the first on a new screen; increment before placing.
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

/// Expand a standard SMW object into (dx, dy, map16_tile) triples.
///
/// Tile numbers and object shapes from the SMW disassembly (asar/Lunar Magic).
/// s_lo = settings & 0x0F, s_hi = settings >> 4
/// Most objects use only s_lo for width and s_hi for height extension.
fn expand_object(obj_id: u32, s_lo: u32, s_hi: u32) -> Vec<(u32, u32, usize)> {
    let mut out = Vec::new();

    // Width/height extensions (1-based: 0 means 1 tile, 15 means 16 tiles)
    let w = s_lo + 1;
    let h = s_hi + 1;

    let mut hline = |out: &mut Vec<(u32,u32,usize)>, tile: usize, cols: u32, row: u32| {
        for dx in 0..cols { out.push((dx, row, tile)); }
    };
    let mut vline = |out: &mut Vec<(u32,u32,usize)>, tile: usize, col: u32, rows: u32| {
        for dy in 0..rows { out.push((col, dy, tile)); }
    };

    // SMW standard object IDs 0x00-0x3F
    // Source: SMW disassembly + Lunar Magic object list
    // Map16 tile numbers (0x000-0x1FF) are the SNES page*256+index values
    match obj_id {
        // 0x00 – Sloped terrain (uses s_lo as width of the slope top)
        // Actually: screen-wide ground ledge variant, 1 row tall
        0x00 => {
            // Left cap + (w) middle + right cap on row 0
            out.push((0, 0, 0x000));
            for dx in 1..=w { out.push((dx, 0, 0x001)); }
            out.push((w+1, 0, 0x002));
        }
        // 0x01 – Ground (top edge + body fill), w+2 wide, h+2 tall
        0x01 => {
            // top row
            out.push((0, 0, 0x020));
            for dx in 1..=w { out.push((dx, 0, 0x021)); }
            out.push((w+1, 0, 0x022));
            // body rows
            for dy in 1..=h {
                out.push((0, dy, 0x023));
                for dx in 1..=w { out.push((dx, dy, 0x025)); }
                out.push((w+1, dy, 0x024));
            }
        }
        // 0x02 – Vertical cliff left (1 wide, h+1 tall)
        0x02 => { vline(&mut out, 0x015, 0, h+1); }
        // 0x03 – Vertical cliff right (1 wide, h+1 tall)
        0x03 => { vline(&mut out, 0x016, 0, h+1); }
        // 0x04 – Diagonal slope up-right (staircase, w+1 wide)
        0x04 => {
            for dx in 0..=w { out.push((dx, w - dx.min(w), 0x040)); }
        }
        // 0x05 – Diagonal slope up-left
        0x05 => {
            for dx in 0..=w { out.push((dx, dx, 0x040)); }
        }
        // 0x06 – Water (w+2 wide, h+1 tall)
        0x06 => {
            hline(&mut out, 0x00E, w+2, 0);     // surface row
            for dy in 1..=h { hline(&mut out, 0x00F, w+2, dy); } // body
        }
        // 0x07 – Lava (w+2 wide, h+1 tall)
        0x07 => {
            hline(&mut out, 0x00D, w+2, 0);
            for dy in 1..=h { hline(&mut out, 0x00C, w+2, dy); }
        }
        // 0x08 – Pipe (green, upward entrance), 2 wide, h+2 tall
        0x08 => {
            out.push((0, 0, 0x10C)); out.push((1, 0, 0x10D));
            for dy in 1..=(h+1) { out.push((0, dy, 0x10E)); out.push((1, dy, 0x10F)); }
        }
        // 0x09 – Pipe (green, downward entrance), 2 wide, h+2 tall
        0x09 => {
            for dy in 0..=h { out.push((0, dy, 0x10E)); out.push((1, dy, 0x10F)); }
            out.push((0, h+1, 0x10C)); out.push((1, h+1, 0x10D));
        }
        // 0x0A – Horizontal pipe (left entrance), w+2 wide, 2 tall
        0x0A => {
            out.push((0, 0, 0x110)); out.push((0, 1, 0x111));
            for dx in 1..=w { out.push((dx, 0, 0x112)); out.push((dx, 1, 0x113)); }
            out.push((w+1, 0, 0x114)); out.push((w+1, 1, 0x115));
        }
        // 0x0B – Bullet Bill launcher, 1 wide, h+2 tall
        0x0B => {
            out.push((0, 0, 0x118));
            for dy in 1..=(h+1) { out.push((0, dy, 0x119)); }
        }
        // 0x0C – Coin row, w+1 wide
        0x0C => { hline(&mut out, 0x02A, w+1, 0); }
        // 0x0D – Note block row, w+1 wide
        0x0D => { hline(&mut out, 0x02C, w+1, 0); }
        // 0x0E – Brick row, w+1 wide
        0x0E => { hline(&mut out, 0x002, w+1, 0); }
        // 0x0F – ? block row, w+1 wide
        0x0F => { hline(&mut out, 0x024, w+1, 0); }
        // 0x10 – Wooden platform, w+2 wide
        0x10 => {
            out.push((0, 0, 0x131));
            for dx in 1..=w { out.push((dx, 0, 0x132)); }
            out.push((w+1, 0, 0x133));
        }
        // 0x11 – Cement platform, w+2 wide (tileset-specific)
        0x11 => {
            out.push((0, 0, 0x128));
            for dx in 1..=w { out.push((dx, 0, 0x129)); }
            out.push((w+1, 0, 0x12A));
        }
        // 0x12 – Ground (top edge only, no body), w+2 wide
        0x12 => {
            out.push((0, 0, 0x020));
            for dx in 1..=w { out.push((dx, 0, 0x021)); }
            out.push((w+1, 0, 0x022));
        }
        // 0x13 – Vertical solid block column, 1 wide, h+1 tall
        0x13 => { vline(&mut out, 0x025, 0, h+1); }
        // 0x14 – Horizontal solid block row, w+2 wide
        0x14 => {
            out.push((0, 0, 0x025));
            for dx in 1..=w { out.push((dx, 0, 0x025)); }
            out.push((w+1, 0, 0x025));
        }
        // 0x15 – Donut lift platform, 1×1
        0x15 => { out.push((0, 0, 0x163)); }
        // 0x16 – Cloud platform, w+2 wide
        0x16 => {
            out.push((0, 0, 0x134));
            for dx in 1..=w { out.push((dx, 0, 0x135)); }
            out.push((w+1, 0, 0x136));
        }
        // 0x17 – Net/fence tile (1×1)
        0x17 => { out.push((0, 0, 0x168)); }
        // 0x18 – Rope horizontal, w+1 wide
        0x18 => { hline(&mut out, 0x167, w+1, 0); }
        // 0x19 – Rope vertical, h+1 tall
        0x19 => { vline(&mut out, 0x166, 0, h+1); }
        // 0x1A – Slope (45° up-right), s_lo+1 tiles wide
        0x1A => {
            for dx in 0..w {
                out.push((dx, w-1-dx, 0x040));
                for dy in w-dx..w { out.push((dx, dy, 0x025)); }
            }
        }
        // 0x1B – Slope (45° up-left)
        0x1B => {
            for dx in 0..w {
                out.push((dx, dx, 0x040));
                for dy in (dx+1)..w { out.push((dx, dy, 0x025)); }
            }
        }
        // 0x1C – Small slope (2:1 up-right) – 2 tiles wide, 2 tall
        0x1C => {
            out.push((0, 1, 0x040)); out.push((1, 0, 0x040));
            out.push((0, 2, 0x025)); out.push((1, 1, 0x025)); out.push((1, 2, 0x025));
        }
        // 0x1D – Small slope (2:1 up-left)
        0x1D => {
            out.push((0, 0, 0x040)); out.push((1, 1, 0x040));
            out.push((0, 1, 0x025)); out.push((0, 2, 0x025)); out.push((1, 2, 0x025));
        }
        // 0x1E – Muncher row, w+1 wide
        0x1E => { hline(&mut out, 0x034, w+1, 0); }
        // 0x1F – Goal point (1×1 orb)
        0x1F => { out.push((0, 0, 0x1F0)); }
        // 0x20 – Used block row, w+1 wide
        0x20 => { hline(&mut out, 0x025, w+1, 0); }
        // 0x21 – Turn block row, w+1 wide
        0x21 => { hline(&mut out, 0x02E, w+1, 0); }
        // 0x22 – Yoshi coin (1×1)
        0x22 => { out.push((0, 0, 0x171)); }
        // 0x23 – P-balloon block
        0x23 => { out.push((0, 0, 0x02F)); }
        // 0x24 – Spring board
        0x24 => { out.push((0, 0, 0x028)); }
        // 0x25 – Goal sphere / ball
        0x25 => { out.push((0, 0, 0x029)); }
        // 0x26 – P-switch
        0x26 => { out.push((0, 0, 0x02B)); }
        // 0x27 – Used P-switch box
        0x27 => { out.push((0, 0, 0x006)); }
        // 0x28 – Pow block
        0x28 => { out.push((0, 0, 0x02D)); }
        // 0x29 – Message block
        0x29 => { out.push((0, 0, 0x005)); }
        // 0x2A – Invisible coin block
        0x2A => { out.push((0, 0, 0x007)); }
        // 0x2B – Invisible 1-up block
        0x2B => { out.push((0, 0, 0x007)); }
        // 0x2C – Invisible running course
        0x2C => { out.push((0, 0, 0x007)); }
        // 0x2D – Invisible Yoshi egg
        0x2D => { out.push((0, 0, 0x007)); }
        // 0x2E – Brick block (single)
        0x2E => { out.push((0, 0, 0x002)); }
        // 0x2F – ? block (single)
        0x2F => { out.push((0, 0, 0x024)); }
        // 0x30 – Solid block (single)
        0x30 => { out.push((0, 0, 0x025)); }
        // 0x31 – Moving coin (single visible coin)
        0x31 => { out.push((0, 0, 0x02A)); }
        // 0x32 – Blue coin block
        0x32 => { out.push((0, 0, 0x002)); }
        // 0x33 – Boo block
        0x33 => { out.push((0, 0, 0x025)); }
        // 0x34 – Trampoline
        0x34 => { out.push((0, 0, 0x028)); }
        // 0x35 – 3-up moon
        0x35 => { out.push((0, 0, 0x02A)); }
        // 0x36 – Checkpoint
        0x36 => { out.push((0, 0, 0x025)); }
        // 0x37 – Skull raft
        0x37 => { out.push((0, 0, 0x025)); }
        // 0x38 – Lava/water fall (1 wide, h+1 tall)
        0x38 => { vline(&mut out, 0x00E, 0, h+1); }
        // 0x39 – Horizontal net, w+1 wide
        0x39 => { hline(&mut out, 0x168, w+1, 0); }
        // 0x3A – Diagonal moving platform
        0x3A => { out.push((0, 0, 0x131)); }
        // 0x3B – Auto-scroll platform
        0x3B => { out.push((0, 0, 0x131)); }
        // 0x3C – Layer 2 smash
        0x3C => { out.push((0, 0, 0x025)); }
        // 0x3D – Floating island
        0x3D => { out.push((0, 0, 0x025)); }
        // 0x3E – Net platform
        0x3E => { out.push((0, 0, 0x025)); }
        // 0x3F – Screen exit (no visual)
        0x3F => {}
        _ => {}
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
