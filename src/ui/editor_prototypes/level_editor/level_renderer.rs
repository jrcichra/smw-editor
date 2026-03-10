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
        let object_tileset = level.primary_header.fg_bg_gfx() as usize;
        let map16_tileset = smwe_rom::objects::tilesets::object_tileset_to_map16_tileset(object_tileset);

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
                let ext_id = obj.settings() as usize;
                let map16_tile = extended_object_map16(ext_id).unwrap_or(ext_id as u16) as usize;
                tile_map.insert((abs_x, abs_y), map16_tile);
                obj_count += 1;
                i += 3;
                continue;
            }

            let obj_id = obj.standard_object_number() as u32;

            // Lunar Magic direct Map16 objects are encoded as standard objects with IDs 0x22/0x23/0x27/0x29.
            if matches!(obj_id, 0x22 | 0x23 | 0x27 | 0x29) {
                if i + 3 >= raw_bytes.len() {
                    break;
                }
                let b2 = raw_bytes[i + 2];
                let obj_kind = b1 & 0xF0;
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
                    let tile_idx = tile as usize;
                    if tile_idx >= 0x8000 {
                        lm_out_of_range += 1;
                        i += match mode {
                            0 => 5,
                            1 | 2 => 6,
                            _ => 7,
                        };
                        continue;
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
            }
            let settings = obj.settings() as u32;
            let s_lo = settings & 0x0F;
            let s_hi = (settings >> 4) & 0x0F;

            let handled = place_object_disx(
                &mut tile_map,
                obj_id,
                s_lo,
                s_hi,
                abs_x as i32,
                abs_y as i32,
                level_w,
                level_h,
            );
            if !handled {
                let expand = expand_object(obj_id, s_lo, s_hi);
                for (dx, dy, map16_tile) in expand {
                    let tx = abs_x + dx;
                    let ty = abs_y + dy;
                    if tx < level_w && ty < level_h {
                        tile_map.insert((tx, ty), map16_tile);
                    }
                }
            }
            obj_count += 1;
            i += 3;
        }

        if lm_out_of_range > 0 {
            log::warn!(
                "Level {:#X}: {} LM map16 tiles were beyond 0x7FFF and were skipped",
                level_num,
                lm_out_of_range
            );
        }

        // Now convert the tile map to Tile structs for the renderer
        let mut l1_tiles: Vec<Tile> = Vec::with_capacity(tile_map.len() * 4);
        let mut missing_map16 = 0usize;
        for ((tx, ty), map16_idx) in &tile_map {
            let px = tx * 16;
            let py = ty * 16;
            if let Some(block) = rom.map16_tilesets.get_map16_tile(*map16_idx, map16_tileset) {
                for (sub, (ox, oy)) in [
                    (block.upper_left, (0u32, 0u32)),
                    (block.upper_right, (8u32, 0u32)),
                    (block.lower_left, (0u32, 8u32)),
                    (block.lower_right, (8u32, 8u32)),
                ] {
                    l1_tiles.push(bg_tile(px + ox, py + oy, sub.0));
                }
            } else {
                missing_map16 += 1;
            }
        }

        if missing_map16 > 0 {
            log::warn!(
                "Level {:#X}: {} map16 tiles missing (tileset={}, tile_map={})",
                level_num,
                missing_map16,
                map16_tileset,
                tile_map.len()
            );
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

    let hline = |out: &mut Vec<(u32, u32, usize)>, tile: usize, cols: u32, row: u32| {
        for dx in 0..cols {
            out.push((dx, row, tile));
        }
    };
    let vline = |out: &mut Vec<(u32, u32, usize)>, tile: usize, col: u32, rows: u32| {
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
            for dy in 1..=h {
                out.push((0, dy, 0x023));
                for dx in 1..=w {
                    out.push((dx, dy, 0x025));
                }
                out.push((w + 1, dy, 0x024));
            }
        }
        // 0x22-0x2D – LM reserved/special (no visual)
        0x22..=0x2D => {}
        // 0x2E-0x3F – Tileset-specific (use the first tileset-specific map16 range)
        0x2E..=0x3F => {
            let idx = (obj_id - 0x2E) as usize;
            let tile = 0x073 + idx;
            for dy in 0..h {
                for dx in 0..w {
                    out.push((dx, dy, tile));
                }
            }
        }
        _ => {}
    }

    if out.is_empty() && obj_id != 0x3F {
        // Fallback tile so unmapped objects still appear visually.
        out.push((0, 0, 0x025));
    }

    out
}

// SMWDisX-derived object renderers for tileset 7 (used by level 0x105).
// This directly places Map16 tiles based on the object-specific routines.
fn place_object_disx(
    tile_map: &mut std::collections::HashMap<(u32, u32), usize>,
    obj_id: u32,
    s_lo: u32,
    s_hi: u32,
    base_x: i32,
    base_y: i32,
    level_w: u32,
    level_h: u32,
) -> bool {
    match obj_id {
        0x01..=0x0F => {
            place_obj_0da8c3(tile_map, obj_id, s_lo, s_hi, base_x, base_y, level_w, level_h);
            true
        }
        0x0F => {
            place_obj_0daa26(tile_map, s_lo, s_hi, base_x, base_y, level_w, level_h);
            true
        }
        0x13 => {
            place_obj_0db075(tile_map, s_lo, s_hi, base_x, base_y, level_w, level_h);
            true
        }
        0x39 => {
            place_obj_0xdb73f(tile_map, s_hi, base_x, base_y, level_w, level_h);
            true
        }
        0x3A => {
            place_obj_0xdb7aa(tile_map, s_lo, s_hi, base_x, base_y, level_w, level_h);
            true
        }
        0x3F => {
            place_obj_0xdb5b7(tile_map, s_lo, s_hi, base_x, base_y, level_w, level_h);
            true
        }
        0x14 => {
            place_obj_0db1d4(tile_map, s_lo, s_hi, base_x, base_y, level_w, level_h);
            true
        }
        0x21 => {
            place_obj_0db1c8(tile_map, s_lo, s_hi, base_x, base_y, level_w, level_h);
            true
        }
        _ => false,
    }
}

struct ObjCursor<'a> {
    tile_map: &'a mut std::collections::HashMap<(u32, u32), usize>,
    level_w: u32,
    level_h: u32,
    base_x: i32,
    base_y: i32,
    x: i32,
    y: i32,
    map16_hi: u16,
    saved: Option<(i32, i32)>,
}

impl<'a> ObjCursor<'a> {
    fn new(
        tile_map: &'a mut std::collections::HashMap<(u32, u32), usize>,
        base_x: i32,
        base_y: i32,
        level_w: u32,
        level_h: u32,
    ) -> Self {
        Self {
            tile_map,
            level_w,
            level_h,
            base_x,
            base_y,
            x: base_x,
            y: base_y,
            map16_hi: 0,
            saved: None,
        }
    }

    fn set_hi(&mut self, hi: u16) {
        self.map16_hi = hi & 1;
    }

    fn place_low(&mut self, low: u8, advance: bool) {
        let tx = self.x;
        let ty = self.y;
        if tx >= 0 && ty >= 0 {
            let txu = tx as u32;
            let tyu = ty as u32;
            if txu < self.level_w && tyu < self.level_h {
                let tile = ((self.map16_hi << 8) | low as u16) as usize;
                self.tile_map.insert((txu, tyu), tile);
            }
        }
        if advance {
            self.x += 1;
        }
    }

    fn place_low_adjust_abfd(&mut self, low: u8) {
        let mut out = low;
        let cur_low = self.get_cur_low();
        if cur_low == 0x3F {
            out = out.wrapping_add(0x01);
        } else if cur_low == 0x01 {
            out = out.wrapping_add(0x03);
        } else if cur_low == 0x03 {
            out = out.wrapping_add(0x04);
        }
        self.place_low(out, true);
    }

    fn place_low_adjust_b84e(&mut self, low: u8) {
        let mut out = low;
        let cur_low = self.get_cur_low();
        if cur_low == 0x3F {
            out = out.wrapping_add(0x01);
        } else if cur_low != 0x25 {
            out = out.wrapping_add(0x02);
        }
        self.place_low(out, true);
    }

    fn get_cur_low(&self) -> u8 {
        let tx = self.x;
        let ty = self.y;
        if tx >= 0 && ty >= 0 {
            let txu = tx as u32;
            let tyu = ty as u32;
            if let Some(tile) = self.tile_map.get(&(txu, tyu)) {
                return (*tile as u16 & 0xFF) as u8;
            }
        }
        0x25
    }

    fn save_pos(&mut self) {
        self.saved = Some((self.x, self.y));
    }

    fn restore_pos(&mut self) {
        if let Some((x, y)) = self.saved {
            self.x = x;
            self.y = y;
        }
    }

    fn step_down(&mut self) {
        self.y += 1;
    }

    fn step_down_left(&mut self) {
        self.x -= 1;
        self.y += 1;
    }

    fn step_down_right(&mut self) {
        self.x += 1;
        self.y += 1;
    }
}

fn place_obj_0xdb5b7(
    tile_map: &mut std::collections::HashMap<(u32, u32), usize>,
    s_lo: u32,
    s_hi: u32,
    base_x: i32,
    base_y: i32,
    level_w: u32,
    level_h: u32,
) {
    const A: [u8; 5] = [0x73, 0x7A, 0x85, 0x88, 0xC3];
    const B: [u8; 5] = [0x74, 0x7B, 0x86, 0x89, 0xC3];
    const C: [u8; 5] = [0x79, 0x80, 0x87, 0x8E, 0xC3];
    let style = (s_hi as usize).min(A.len() - 1);
    let mut width = s_lo as i32 + 1;
    if width < 2 {
        width = 2;
    }
    let mut cur = ObjCursor::new(tile_map, base_x, base_y, level_w, level_h);
    cur.set_hi(0);
    cur.place_low(A[style], true);
    for _ in 0..(width - 2) {
        cur.place_low(B[style], true);
    }
    cur.place_low(C[style], false);
}

fn place_obj_0xdb73f(
    tile_map: &mut std::collections::HashMap<(u32, u32), usize>,
    s_hi: u32,
    base_x: i32,
    base_y: i32,
    level_w: u32,
    level_h: u32,
) {
    const DATA: [u8; 16] = [0xC4, 0xC5, 0xC7, 0xEC, 0xED, 0xC6, 0xC7, 0xEE, 0x59, 0x5A, 0xEF, 0xC7, 0xEE, 0x59, 0x5B, 0x5C];
    let mut cur = ObjCursor::new(tile_map, base_x, base_y, level_w, level_h);
    cur.set_hi(1);
    let mut height = s_hi as i32 + 1;
    let mut row_len = 1i32;
    let mut idx: i32 = 0;
    cur.save_pos();

    while height > 0 {
        let mut count = row_len;
        while count >= 0 && idx < DATA.len() as i32 {
            cur.place_low(DATA[idx as usize], true);
            idx += 1;
            count -= 1;
        }
        cur.restore_pos();
        cur.step_down_left();
        row_len += 2;
        height -= 1;
        if idx == 6 {
            break;
        }
    }

    if height > 0 {
        row_len -= 1;
        while height > 0 {
            let mut count = row_len;
            while count >= 0 && idx < DATA.len() as i32 {
                cur.place_low(DATA[idx as usize], true);
                idx += 1;
                count -= 1;
            }
            cur.restore_pos();
            cur.step_down_left();
            if idx == 0x10 {
                idx -= 5;
            }
            height -= 1;
        }
    }

    cur.place_low(0xEB, false);
}

fn place_obj_0xdb7aa(
    tile_map: &mut std::collections::HashMap<(u32, u32), usize>,
    s_lo: u32,
    s_hi: u32,
    base_x: i32,
    base_y: i32,
    level_w: u32,
    level_h: u32,
) {
    let mut cur = ObjCursor::new(tile_map, base_x, base_y, level_w, level_h);
    let mut width = s_lo as i32 + 1;
    let mut height = s_hi as i32 + 1;
    if width < 1 {
        width = 1;
    }
    if height < 1 {
        height = 1;
    }

    // Top section (approximation of CODE_0DB7AA): alternating solid and fill rows
    cur.save_pos();
    let mut row = 0;
    while row < height {
        cur.restore_pos();
        cur.y += row;
        cur.x -= row;
        cur.set_hi(1);
        cur.place_low_adjust_abfd(0xAA);
        for _ in 0..width {
            cur.set_hi(1);
            cur.place_low(0xE2, true);
            cur.set_hi(0);
            cur.place_low(0x3F, true);
        }
        cur.set_hi(0);
        cur.place_low_adjust_b84e(0xA6);
        row += 1;
    }

    // Bottom cap
    cur.restore_pos();
    cur.y += height;
    cur.x -= height;
    cur.set_hi(1);
    cur.place_low_adjust_abfd(0xF7);
    for _ in 0..width {
        cur.set_hi(0);
        cur.place_low(0x3F, true);
    }
    cur.set_hi(0);
    cur.place_low_adjust_b84e(0xA6);
}

fn place_obj_0da8c3(
    tile_map: &mut std::collections::HashMap<(u32, u32), usize>,
    obj_id: u32,
    s_lo: u32,
    s_hi: u32,
    base_x: i32,
    base_y: i32,
    level_w: u32,
    level_h: u32,
) {
    const DATA: [u8; 15] = [0x02, 0x21, 0x23, 0x2A, 0x2B, 0x3F, 0x03, 0x13, 0x1E, 0x24, 0x2E, 0x2F, 0x30, 0x32, 0x65];
    let idx = obj_id.saturating_sub(1) as usize;
    if idx >= DATA.len() {
        return;
    }
    let tile_low = DATA[idx] as u16;
    let hi = if idx >= 7 { 1u16 } else { 0u16 };
    let width = s_lo as i32 + 1;
    let height = s_hi as i32 + 1;
    for dy in 0..height {
        for dx in 0..width {
            let x = base_x + dx;
            let y = base_y + dy;
            if x >= 0 && y >= 0 {
                let tx = x as u32;
                let ty = y as u32;
                if tx < level_w && ty < level_h {
                    tile_map.insert((tx, ty), ((hi << 8) | tile_low) as usize);
                }
            }
        }
    }
}

fn place_obj_0daa26(
    tile_map: &mut std::collections::HashMap<(u32, u32), usize>,
    s_lo: u32,
    s_hi: u32,
    base_x: i32,
    base_y: i32,
    level_w: u32,
    level_h: u32,
) {
    const TOP_L: [u8; 3] = [0x33, 0x37, 0x39];
    const TOP_R: [u8; 3] = [0x34, 0x38, 0x3A];
    const BOT_L: [u8; 5] = [0x00, 0x00, 0x39, 0x33, 0x37];
    const BOT_R: [u8; 5] = [0x00, 0x00, 0x3A, 0x34, 0x38];

    let kind = s_lo as usize;
    let height = s_hi as i32 + 1;
    let mut cur = ObjCursor::new(tile_map, base_x, base_y, level_w, level_h);
    cur.set_hi(1);

    let (top_l, top_r) = if kind < 3 {
        (TOP_L[kind], TOP_R[kind])
    } else if kind == 5 {
        (0x68, 0x69)
    } else {
        (0x35, 0x36)
    };

    cur.x = base_x;
    cur.y = base_y;
    cur.place_low(top_l, false);
    cur.x = base_x + 1;
    cur.place_low(top_r, false);

    if height <= 1 {
        return;
    }

    for row in 1..(height - 1) {
        cur.y = base_y + row;
        cur.x = base_x;
        cur.place_low(0x35, false);
        cur.x = base_x + 1;
        cur.place_low(0x36, false);
    }

    cur.y = base_y + height - 1;
    let (bot_l, bot_r) = if kind < BOT_L.len() { (BOT_L[kind], BOT_R[kind]) } else { (0x35, 0x36) };
    if bot_l != 0 || bot_r != 0 {
        cur.x = base_x;
        cur.place_low(bot_l, false);
        cur.x = base_x + 1;
        cur.place_low(bot_r, false);
    }
}

fn place_obj_0db075(
    tile_map: &mut std::collections::HashMap<(u32, u32), usize>,
    s_lo: u32,
    s_hi: u32,
    base_x: i32,
    base_y: i32,
    level_w: u32,
    level_h: u32,
) {
    const TOP: [u8; 15] = [0x40, 0x41, 0x06, 0x45, 0x4B, 0x48, 0x4C, 0x01, 0x03, 0xB6, 0xB7, 0x45, 0x4B, 0x48, 0x4C];
    const MID1: [u8; 15] = [0x40, 0x41, 0x06, 0x4B, 0x4B, 0x4C, 0x4C, 0x40, 0x41, 0x4B, 0x4C, 0x4B, 0x4B, 0x4C, 0x4C];
    const MID2: [u8; 15] = [0x40, 0x41, 0x06, 0x4B, 0x4B, 0x4C, 0x4C, 0x40, 0x41, 0x4B, 0x4C, 0x4B, 0x4B, 0x4C, 0x4C];
    const BOT: [u8; 15] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xE2, 0xE2, 0xE4, 0xE4];

    let kind = s_lo as usize;
    if kind >= TOP.len() {
        return;
    }
    let width = 1;
    let height = s_hi as i32 + 1;
    let mut cur = ObjCursor::new(tile_map, base_x, base_y, level_w, level_h);
    cur.set_hi(0);
    for row in 0..height {
        cur.x = base_x;
        cur.y = base_y + row;
        let tile = if row == 0 {
            TOP[kind]
        } else if row == 1 {
            MID1[kind]
        } else if row + 1 == height && BOT[kind] != 0xFF {
            BOT[kind]
        } else {
            MID2[kind]
        };
        for _ in 0..width {
            cur.place_low(tile, true);
        }
    }
}

fn place_obj_0db1d4(
    tile_map: &mut std::collections::HashMap<(u32, u32), usize>,
    s_lo: u32,
    s_hi: u32,
    base_x: i32,
    base_y: i32,
    level_w: u32,
    level_h: u32,
) {
    let width = s_lo as i32 + 1;
    let height = s_hi as i32 + 1;
    for dy in 0..height {
        for dx in 0..width {
            let x = base_x + dx;
            let y = base_y + dy;
            if x >= 0 && y >= 0 {
                let tx = x as u32;
                let ty = y as u32;
                if tx < level_w && ty < level_h {
                    let tile = if dy == 0 { 0x100 } else { 0x3F };
                    tile_map.insert((tx, ty), tile);
                }
            }
        }
    }
}

fn place_obj_0db1c8(
    tile_map: &mut std::collections::HashMap<(u32, u32), usize>,
    s_lo: u32,
    s_hi: u32,
    base_x: i32,
    base_y: i32,
    level_w: u32,
    level_h: u32,
) {
    let width = ((s_hi << 4) | s_lo) as i32 + 1;
    let height = 3;
    for dy in 0..height {
        for dx in 0..width {
            let x = base_x + dx;
            let y = base_y + dy;
            if x >= 0 && y >= 0 {
                let tx = x as u32;
                let ty = y as u32;
                if tx < level_w && ty < level_h {
                    let tile = if dy == 0 { 0x100 } else { 0x3F };
                    tile_map.insert((tx, ty), tile);
                }
            }
        }
    }
}

fn place_obj_0xdb224(
    tile_map: &mut std::collections::HashMap<(u32, u32), usize>,
    s_lo: u32,
    s_hi: u32,
    base_x: i32,
    base_y: i32,
    level_w: u32,
    level_h: u32,
) {
    const TOP: [u8; 3] = [0x2F, 0x25, 0x32];
    const MID: [u8; 3] = [0x30, 0x25, 0x33];
    const BOT: [u8; 3] = [0x31, 0x25, 0x34];
    const TOP_ALT: [u8; 3] = [0x39, 0x25, 0x3C];
    const MID_ALT: [u8; 3] = [0x3A, 0x25, 0x3D];
    const BOT_ALT: [u8; 3] = [0x3B, 0x25, 0x3E];

    let height = s_hi as i32 + 2;
    let use_alt = s_lo != 0;
    let mut cur = ObjCursor::new(tile_map, base_x, base_y, level_w, level_h);
    cur.set_hi(0);

    for col in 0..3 {
        let top_tile = if use_alt { TOP_ALT[col] } else { TOP[col] };
        let mid_tile = if use_alt { MID_ALT[col] } else { MID[col] };
        let bot_tile = if use_alt { BOT_ALT[col] } else { BOT[col] };
        cur.x = base_x + col as i32;
        cur.y = base_y;
        cur.place_low(top_tile, false);
        for row in 1..(height - 1) {
            cur.y = base_y + row;
            cur.place_low(mid_tile, false);
        }
        cur.y = base_y + height - 1;
        cur.place_low(bot_tile, false);
    }
}

fn place_obj_ground_21(
    tile_map: &mut std::collections::HashMap<(u32, u32), usize>,
    s_lo: u32,
    s_hi: u32,
    base_x: i32,
    base_y: i32,
    level_w: u32,
    level_h: u32,
) {
    // Long ground ledge: draw from the placement row upward.
    let w = s_lo as i32 + 1;
    let h = s_hi as i32 + 1;
    let mut place = |x: i32, y: i32, tile: usize| {
        if x >= 0 && y >= 0 {
            let tx = x as u32;
            let ty = y as u32;
            if tx < level_w && ty < level_h {
                tile_map.insert((tx, ty), tile);
            }
        }
    };

    // Top row
    place(base_x, base_y, 0x020);
    for dx in 1..=w {
        place(base_x + dx, base_y, 0x021);
    }
    place(base_x + w + 1, base_y, 0x022);

    for dy in 1..=h {
        let y = base_y - dy;
        place(base_x, y, 0x023);
        for dx in 1..=w {
            place(base_x + dx, y, 0x025);
        }
        place(base_x + w + 1, y, 0x024);
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

fn extended_object_map16(ext_id: usize) -> Option<u16> {
    // SMWDisX DATA_0DA548: extended object tile list (for IDs 0x10..)
    const DATA_0DA548: [u8; 51] = [
        0x1F, 0x22, 0x24, 0x42, 0x43, 0x27, 0x29, 0x25, 0x6E, 0x6F, 0x70, 0x71, 0x72, 0x45, 0x46, 0x47,
        0x48, 0x36, 0x37, 0x11, 0x12, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x29, 0x1D,
        0x1F, 0x20, 0x21, 0x22, 0x23, 0x25, 0x26, 0x27, 0x28, 0x2A, 0xDE, 0xE0, 0xE2, 0xE4, 0xEC, 0xED,
        0x2C, 0x25, 0x2D,
    ];
    if ext_id < 0x10 {
        return None;
    }
    let idx = ext_id - 0x10;
    if idx < DATA_0DA548.len() {
        Some(DATA_0DA548[idx] as u16)
    } else {
        None
    }
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
