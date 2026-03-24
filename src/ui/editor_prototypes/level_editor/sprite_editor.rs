use smwe_rom::level::SpriteInstance;

/// Read all sprites from the decompressed sprite list in WRAM.
/// The list starts at 0x7EC901 and is terminated by 0xFF.
/// Reads directly from WRAM bytes to avoid needing &mut Cpu.
pub(super) fn read_sprites_from_wram(wram: &[u8]) -> Vec<SpriteInstance> {
    let mut sprites = Vec::new();
    let base = (0x7EC901 & 0x1FFFF) as usize; // WRAM offset
    let mut off = 0usize;
    loop {
        if base + off + 2 >= wram.len() {
            break;
        }
        let b0 = wram[base + off];
        if b0 == 0xFF {
            break;
        }
        let b1 = wram[base + off + 1];
        let b2 = wram[base + off + 2];
        sprites.push(SpriteInstance::from_raw([b0, b1, b2]));
        off += 3;
    }
    sprites
}

/// Write the sprite list back to WRAM, terminated by 0xFF.
pub(super) fn write_sprites_to_wram(wram: &mut [u8], sprites: &[SpriteInstance]) {
    let base = (0x7EC901 & 0x1FFFF) as usize;
    for (i, spr) in sprites.iter().enumerate() {
        let off = i * 3;
        let raw = spr.as_bytes();
        if base + off + 2 < wram.len() {
            wram[base + off] = raw[0];
            wram[base + off + 1] = raw[1];
            wram[base + off + 2] = raw[2];
        }
    }
    // Write terminator
    let term_off = sprites.len() * 3;
    if base + term_off < wram.len() {
        wram[base + term_off] = 0xFF;
    }
}

/// Compute the pixel anchor position for a sprite.
pub(super) fn sprite_pixel_pos(spr: &SpriteInstance, vertical: bool) -> (u32, u32) {
    let (x_tile, y_tile) = spr.xy_pos();
    let screen = spr.screen_number() as u32;
    if vertical {
        let anchor_x = (screen % 2) * 256 + (x_tile as u32) * 16;
        let anchor_y = (screen / 2) * 512 + (y_tile as u32) * 16;
        (anchor_x, anchor_y)
    } else {
        let anchor_x = screen * 256 + (x_tile as u32) * 16;
        let anchor_y = (y_tile as u32) * 16;
        (anchor_x, anchor_y)
    }
}

/// A color for a sprite overlay, deterministic from sprite ID.
pub(super) fn sprite_color(id: u8) -> egui::Color32 {
    let r = 40 + (id.wrapping_mul(53)) as u8;
    let g = 40 + (id.wrapping_mul(97)) as u8;
    let b = 40 + (id.wrapping_mul(151)) as u8;
    egui::Color32::from_rgba_unmultiplied(r, g, b, 100)
}

/// Handle sprite editing interactions.
pub(super) fn handle_sprite_interaction(
    wram: &mut [u8], sprites: &[SpriteInstance], resp: &egui::Response, origin: egui::Pos2, tile_sz: f32,
    vertical: bool, editing_mode: crate::ui::editing_mode::EditingMode, selected_sprite: &mut Option<usize>,
    place_sprite_id: u8,
) -> bool {
    let mut changed = false;

    match editing_mode {
        crate::ui::editing_mode::EditingMode::Select => {
            if resp.clicked_by(egui::PointerButton::Secondary) {
                if let Some(pos) = resp.hover_pos() {
                    let rel = (pos - origin) / tile_sz;
                    let tx = rel.x.floor() as u32;
                    let ty = rel.y.floor() as u32;
                    *selected_sprite = find_sprite_at(sprites, tx, ty, vertical);
                }
            }
        }
        crate::ui::editing_mode::EditingMode::Erase => {
            if resp.clicked_by(egui::PointerButton::Secondary) {
                if let Some(pos) = resp.hover_pos() {
                    let rel = (pos - origin) / tile_sz;
                    let tx = rel.x.floor() as u32;
                    let ty = rel.y.floor() as u32;
                    if let Some(idx) = find_sprite_at(sprites, tx, ty, vertical) {
                        let mut new_sprites = sprites.to_vec();
                        new_sprites.remove(idx);
                        write_sprites_to_wram(wram, &new_sprites);
                        *selected_sprite = None;
                        changed = true;
                    }
                }
            }
        }
        crate::ui::editing_mode::EditingMode::Draw => {
            if resp.clicked_by(egui::PointerButton::Secondary) {
                if let Some(pos) = resp.hover_pos() {
                    let rel = (pos - origin) / tile_sz;
                    let tx = rel.x.floor() as u32;
                    let ty = rel.y.floor() as u32;
                    let mut new_sprites = sprites.to_vec();
                    let new_sprite = SpriteInstance::new(place_sprite_id, tx, ty, 0, vertical);
                    new_sprites.push(new_sprite);
                    write_sprites_to_wram(wram, &new_sprites);
                    *selected_sprite = Some(new_sprites.len() - 1);
                    changed = true;
                }
            }
        }
        _ => {}
    }
    changed
}

fn find_sprite_at(sprites: &[SpriteInstance], tx: u32, ty: u32, vertical: bool) -> Option<usize> {
    // Search in reverse so topmost sprites are hit first
    for (i, spr) in sprites.iter().enumerate().rev() {
        let (ax, ay) = sprite_pixel_pos(spr, vertical);
        // Sprites are roughly 16x16 (one tile). Some are bigger but this is a good default.
        if tx >= ax / 16 && tx < (ax / 16) + 1 && ty >= ay / 16 && ty < (ay / 16) + 1 {
            return Some(i);
        }
    }
    None
}
