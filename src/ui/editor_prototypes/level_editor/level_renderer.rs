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
        let tileset_idx = (level.primary_header.fg_bg_gfx() as usize) % smwe_rom::objects::tilesets::TILESETS_COUNT;

        let raw_bytes = level.layer1.as_bytes();
        if raw_bytes.is_empty() {
            self.layer1.set_tiles(gl, Vec::new());
            self.layer2.set_tiles(gl, Vec::new());
            self.sprites.set_tiles(gl, Vec::new());
            return;
        }

        // SMW level dimensions: each horizontal screen is 16×27 tiles; vertical screens are 16×16.
        // We build a flat tile grid (map) at 16×16px per tile, then emit Tile structs.
        const SCREEN_W_H: u32 = 16;
        const SCREEN_W_V: u32 = 32;
        const SCREEN_H_H: u32 = 27;
        const SCREEN_H_V: u32 = 16;
        let screen_w = if is_vertical { SCREEN_W_V } else { SCREEN_W_H };
        let screen_h = if is_vertical { SCREEN_H_V } else { SCREEN_H_H };
        let num_screens = level.primary_header.level_length() as u32 + 1;
        let (level_w, level_h) = if is_vertical {
            (screen_w, screen_h * num_screens)
        } else {
            (screen_w * num_screens, screen_h)
        };

        // key = (tile_x, tile_y) in 16x16 tile coords, value = map16 block index
        let mut tile_map: std::collections::HashMap<(u32, u32), usize> = std::collections::HashMap::with_capacity(1024);

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
        let mut i = 0usize;
        let mut lm_out_of_range = 0usize;
        let mut obj_count = 0usize;

        while i < raw_bytes.len() {
            let b0 = raw_bytes[i];
            if b0 == 0xFF {
                break;
            }
            if i + 1 >= raw_bytes.len() {
                break;
            }
            let b1 = raw_bytes[i + 1];

            // Lunar Magic direct Map16 objects (22/23/27/29): N10YYYYY
            if (b0 & 0x60) == 0x40 {
                if i + 3 >= raw_bytes.len() {
                    break;
                }
                let b2 = raw_bytes[i + 2];
                let obj_kind = b1 & 0xF0;

                // N-bit: new screen before placing
                if (b0 & 0x80) != 0 {
                    current_screen = current_screen.saturating_add(1);
                }

                let (local_x, local_y) = if is_vertical {
                    (b0 as u32 & 0x1F, b1 as u32 & 0x0F)
                } else {
                    (b1 as u32 & 0x0F, b0 as u32 & 0x1F)
                };
                let abs_x = local_x + if is_vertical { 0 } else { current_screen * screen_w };
                let abs_y = local_y + if is_vertical { current_screen * screen_h } else { 0 };

                let h = (b2 >> 4) as u32 + 1;
                let w = (b2 & 0x0F) as u32 + 1;

                if obj_kind == 0x20 || obj_kind == 0x30 {
                    // Object 22/23: direct Map16 pages 0-1
                    let page = (b1 >> 4) & 1;
                    let b3 = raw_bytes[i + 3];
                    let tile = ((page as u16) << 8) | (b3 as u16);
                    let tile_idx = tile as usize;
                    for dy in 0..h {
                        for dx in 0..w {
                            let tx = abs_x + dx;
                            let ty = abs_y + dy;
                            if tx < level_w && ty < level_h {
                                tile_map.insert((tx, ty), tile_idx);
                            }
                        }
                    }
                    obj_count += 1;
                    i += 4;
                    continue;
                }

                if obj_kind == 0x70 || obj_kind == 0x90 {
                    // Object 27/29: direct Map16 pages 00-3F / 40-7F
                    if i + 4 >= raw_bytes.len() {
                        break;
                    }
                    let b3 = raw_bytes[i + 3];
                    let b4 = raw_bytes[i + 4];
                    let mode = (b3 >> 6) & 0x03;
                    let mut tile = (((b3 & 0x3F) as u16) << 8) | (b4 as u16);
                    if obj_kind == 0x90 {
                        tile = tile.wrapping_add(0x400);
                    }
                    let mut tile_idx = tile as usize;
                    if tile_idx >= 0x200 {
                        lm_out_of_range += 1;
                        tile_idx %= 0x200;
                    }
                    for dy in 0..h {
                        for dx in 0..w {
                            let tx = abs_x + dx;
                            let ty = abs_y + dy;
                            if tx < level_w && ty < level_h {
                                tile_map.insert((tx, ty), tile_idx);
                            }
                        }
                    }
                    obj_count += 1;
                    i += match mode {
                        0 => 5,
                        1 | 2 => 6,
                        _ => 7,
                    };
                    continue;
                }

                // Unknown LM object type; skip three bytes to avoid stalling.
                i += 3;
                continue;
            }

            // Exit (4 bytes)
            if i + 3 < raw_bytes.len() && b0 & 0x50 == 0 && b1 & 0xF0 == 0 && raw_bytes[i + 2] == 0 {
                i += 4;
                continue;
            }

            // Standard/extended object (3 bytes)
            if i + 2 >= raw_bytes.len() {
                break;
            }
            let b2 = raw_bytes[i + 2];
            let obj = smwe_rom::objects::Object(u32::from_be_bytes([b0, b1, b2, 0]));

            if obj.is_screen_jump() {
                current_screen = obj.screen_number() as u32;
                i += 3;
                continue;
            }
            if obj.is_new_screen() {
                current_screen = current_screen.saturating_add(1);
            }

            let (local_x, local_y) = if is_vertical {
                (obj.y() as u32, obj.x() as u32)
            } else {
                (obj.x() as u32, obj.y() as u32)
            };
            let abs_x = local_x + if is_vertical { 0 } else { current_screen * screen_w };
            let abs_y = local_y + if is_vertical { current_screen * screen_h } else { 0 };

            if obj.is_extended() {
                let map16_tile = obj.settings() as usize;
                tile_map.insert((abs_x, abs_y), map16_tile);
                obj_count += 1;
                i += 3;
                continue;
            }

            let settings = obj.settings() as u32;
            let s_lo = settings & 0x0F;
            let s_hi = (settings >> 4) & 0x0F;
            let obj_id = obj.standard_object_number() as u32;
            let expand = expand_object(obj_id, s_lo, s_hi);
            for (dx, dy, map16_tile) in expand {
                let tx = abs_x + dx;
                let ty = abs_y + dy;
                if tx < level_w && ty < level_h {
                    tile_map.insert((tx, ty), map16_tile);
                }
            }
            obj_count += 1;
            i += 3;
        }

        if lm_out_of_range > 0 {
            log::warn!(
                "Level {:#X}: {} LM map16 tiles were beyond 0x1FF (wrapped to fit current tileset)",
                level_num,
                lm_out_of_range
            );
        }

        // Now convert the tile map to Tile structs for the renderer
        let mut l1_tiles: Vec<Tile> = Vec::with_capacity(tile_map.len() * 4);
        for ((tx, ty), map16_idx) in &tile_map {
            let px = tx * 16;
            let py = ty * 16;
            if let Some(block) = rom.map16_tilesets.get_map16_tile(*map16_idx, tileset_idx) {
                for (sub, (ox, oy)) in [
                    (block.upper_left, (0u32, 0u32)),
                    (block.upper_right, (8u32, 0u32)),
                    (block.lower_left, (0u32, 8u32)),
                    (block.lower_right, (8u32, 8u32)),
                ] {
                    if sub.tile_number() != 0 {
                        l1_tiles.push(bg_tile(px + ox, py + oy, sub.0));
                    }
                }
            }
        }

        if l1_tiles.is_empty() && obj_count > 0 {
            log::warn!(
                "Level {:#X}: no layer1 tiles built (objects={}, tile_map={})",
                level_num,
                obj_count,
                tile_map.len()
            );
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

    let mut hline = |out: &mut Vec<(u32, u32, usize)>, tile: usize, cols: u32, row: u32| {
        for dx in 0..cols {
            out.push((dx, row, tile));
        }
    };
    let mut vline = |out: &mut Vec<(u32, u32, usize)>, tile: usize, col: u32, rows: u32| {
        for dy in 0..rows {
            out.push((col, dy, tile));
        }
    };

    // SMW standard object IDs 0x00-0x3F
    // Source: SMW level data format (SnesLab / smwspeedruns).
    // Note: We still approximate tile mapping; this aligns object IDs to the correct
    // semantics so levels resemble the real layout more closely.
    match obj_id {
        // 0x00 – Extended objects (handled elsewhere)
        0x00 => {}
        // 0x01 – Water (Blue)
        0x01 => {
            hline(&mut out, 0x00E, w + 2, 0); // surface
            for dy in 1..=h {
                hline(&mut out, 0x00F, w + 2, dy);
            }
        }
        // 0x02 – Invisible coin blocks
        0x02 => {
            hline(&mut out, 0x007, w + 1, 0);
        }
        // 0x03 – Invisible note blocks
        0x03 => {
            hline(&mut out, 0x007, w + 1, 0);
        }
        // 0x04 – Invisible POW coins
        0x04 => {
            hline(&mut out, 0x007, w + 1, 0);
        }
        // 0x05 – Coins
        0x05 => {
            hline(&mut out, 0x02A, w + 1, 0);
        }
        // 0x06 – Walk-through dirt
        0x06 => {
            out.push((0, 0, 0x025));
            for dx in 1..=w {
                out.push((dx, 0, 0x025));
            }
            for dy in 1..=h {
                for dx in 0..=w {
                    out.push((dx, dy, 0x025));
                }
            }
        }
        // 0x07 – Water (other color)
        0x07 => {
            hline(&mut out, 0x00E, w + 2, 0);
            for dy in 1..=h {
                hline(&mut out, 0x00F, w + 2, dy);
            }
        }
        // 0x08 – Note blocks
        0x08 => {
            hline(&mut out, 0x02C, w + 1, 0);
        }
        // 0x09 – Turn blocks
        0x09 => {
            hline(&mut out, 0x02E, w + 1, 0);
        }
        // 0x0A – Coin ? blocks
        0x0A => {
            hline(&mut out, 0x024, w + 1, 0);
        }
        // 0x0B – Throw blocks
        0x0B => {
            hline(&mut out, 0x02D, w + 1, 0);
        }
        // 0x0C – Black piranha plants
        0x0C => {
            hline(&mut out, 0x034, w + 1, 0);
        }
        // 0x0D – Cement blocks
        0x0D => {
            for dy in 0..=h {
                hline(&mut out, 0x128, w + 1, dy);
            }
        }
        // 0x0E – Brown blocks
        0x0E => {
            for dy in 0..=h {
                hline(&mut out, 0x002, w + 1, dy);
            }
        }
        // 0x0F – Vertical pipes (type in high nibble)
        0x0F => {
            out.push((0, 0, 0x10C));
            out.push((1, 0, 0x10D));
            for dy in 1..=(h + 1) {
                out.push((0, dy, 0x10E));
                out.push((1, dy, 0x10F));
            }
        }
        // 0x10 – Horizontal pipes (type in high nibble)
        0x10 => {
            out.push((0, 0, 0x110));
            out.push((0, 1, 0x111));
            for dx in 1..=w {
                out.push((dx, 0, 0x112));
                out.push((dx, 1, 0x113));
            }
            out.push((w + 1, 0, 0x114));
            out.push((w + 1, 1, 0x115));
        }
        // 0x11 – Bullet shooter
        0x11 => {
            out.push((0, 0, 0x118));
            for dy in 1..=(h + 1) {
                out.push((0, dy, 0x119));
            }
        }
        // 0x12 – Slopes (approx)
        0x12 => {
            for dx in 0..w {
                out.push((dx, w - 1 - dx, 0x040));
                for dy in w - dx..w {
                    out.push((dx, dy, 0x025));
                }
            }
        }
        // 0x13 – Ledge edges (approx)
        0x13 => {
            vline(&mut out, 0x023, 0, h + 1);
        }
        // 0x14 – Ground ledge
        0x14 => {
            out.push((0, 0, 0x020));
            for dx in 1..=w {
                out.push((dx, 0, 0x021));
            }
            out.push((w + 1, 0, 0x022));
            for dy in 1..=h {
                out.push((0, dy, 0x023));
                for dx in 1..=w {
                    out.push((dx, dy, 0x025));
                }
                out.push((w + 1, dy, 0x024));
            }
        }
        // 0x15 – Midway/Goal point
        0x15 => {
            out.push((0, 0, 0x1F0));
        }
        // 0x16 – Blue coins
        0x16 => {
            hline(&mut out, 0x02A, w + 1, 0);
        }
        // 0x17 – Rope/Clouds (type in high nibble)
        0x17 => {
            let ty = s_hi & 0xF;
            if ty == 0 {
                hline(&mut out, 0x167, w + 1, 0);
            } else {
                out.push((0, 0, 0x134));
                for dx in 1..=w {
                    out.push((dx, 0, 0x135));
                }
                out.push((w + 1, 0, 0x136));
            }
        }
        // 0x18 – Water surface (animated)
        0x18 => {
            hline(&mut out, 0x00E, w + 2, 0);
        }
        // 0x19 – Water surface (non-animated)
        0x19 => {
            hline(&mut out, 0x00E, w + 2, 0);
        }
        // 0x1A – Lava surface
        0x1A => {
            hline(&mut out, 0x00D, w + 2, 0);
        }
        // 0x1B – Net top edge
        0x1B => {
            hline(&mut out, 0x168, w + 1, 0);
        }
        // 0x1C – Donut bridge
        0x1C => {
            hline(&mut out, 0x163, w + 1, 0);
        }
        // 0x1D – Net bottom edge
        0x1D => {
            hline(&mut out, 0x168, w + 1, 0);
        }
        // 0x1E – Net vertical edge
        0x1E => {
            vline(&mut out, 0x168, 0, h + 1);
        }
        // 0x1F – Vertical pipe/bone/log
        0x1F => {
            vline(&mut out, 0x10E, 0, h + 1);
        }
        // 0x20 – Horizontal pipe/bone/log
        0x20 => {
            hline(&mut out, 0x112, w + 1, 0);
        }
        // 0x21 – Long ground ledge
        0x21 => {
            out.push((0, 0, 0x020));
            for dx in 1..=w {
                out.push((dx, 0, 0x021));
            }
            out.push((w + 1, 0, 0x022));
        }
        // 0x22-0x2D – LM reserved/special (no visual)
        0x22..=0x2D => {}
        // 0x2E-0x3F – Tileset-specific (use the first tileset-specific map16 range)
        0x2E..=0x3F => {
            let idx = (obj_id - 0x2E) as usize;
            let tile = 0x073 + idx;
            out.push((0, 0, tile));
        }
        _ => {}
    }

    if out.is_empty() && obj_id != 0x3F {
        // Fallback tile so unmapped objects still appear visually.
        out.push((0, 0, 0x025));
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
