use std::env;
use smwe_rom::{SmwRom, objects::Object};

fn place_obj_0db1c8(
    tile_map: &mut std::collections::HashMap<(u32, u32), usize>,
    s_lo: u32,
    s_hi: u32,
    base_x: i32,
    base_y: i32,
    level_w: u32,
    level_h: u32,
) {
    let width = ((s_hi << 4) | s_lo) as i32;
    let height = 2i32;
    println!("  place_obj_0db1c8: width={} height={} base=({},{})", width, height, base_x, base_y);
    let mut place = |x: i32, y: i32, tile: usize| {
        if x >= 0 && y >= 0 {
            let tx = x as u32; let ty = y as u32;
            if tx < level_w && ty < level_h { tile_map.insert((tx, ty), tile); }
        }
    };
    for dx in 0..=width {
        place(base_x + dx, base_y, 0x100);
    }
    for dy in 1..=height {
        for dx in 0..=width {
            place(base_x + dx, base_y + dy, 0x03F);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: dump_level <rom.sfc> <level_hex>");
        std::process::exit(1);
    }
    let rom_path = &args[1];
    let level_num = u16::from_str_radix(args[2].trim_start_matches("0x"), 16).expect("bad level");

    let rom = SmwRom::from_file(rom_path).expect("failed to load ROM");
    let level = &rom.levels[level_num as usize];

    let is_vertical = level.secondary_header.vertical_level();
    let fg_bg_gfx = level.primary_header.fg_bg_gfx() as usize;
    let map16_tileset = smwe_rom::objects::tilesets::object_tileset_to_map16_tileset(fg_bg_gfx);
    let num_screens = level.primary_header.level_length() as u32 + 1;
    let screen_w = 16u32;
    let screen_h = 27u32;
    let level_w = screen_w * num_screens;
    let level_h = screen_h;

    println!("Level {:#X}  fg_bg_gfx={} map16_tileset={} level_w={} level_h={}",
        level_num, fg_bg_gfx, map16_tileset, level_w, level_h);

    // Check map16 tile availability
    println!("\nMap16 tile checks:");
    for tile_idx in [0x03F, 0x100usize] {
        let result = rom.map16_tilesets.get_map16_tile(tile_idx, map16_tileset);
        println!("  tile {:#05X} tileset {}: {:?}", tile_idx, map16_tileset,
            if result.is_some() { "PRESENT" } else { "MISSING" });
        if let Some(block) = result {
            println!("    UL={:#06X} UR={:#06X} LL={:#06X} LR={:#06X}",
                block.upper_left.0, block.upper_right.0,
                block.lower_left.0, block.lower_right.0);
        }
    }

    // Simulate just the first object (0x21 ground fill)
    let raw = level.layer1.as_bytes();
    let b0 = raw[0]; let b1 = raw[1]; let b2 = raw[2];
    let obj = Object(u32::from_be_bytes([b0, b1, b2, 0]));
    let obj_id = obj.standard_object_number() as u32;
    let settings = obj.settings() as u32;
    let s_lo = settings & 0x0F;
    let s_hi = (settings >> 4) & 0x0F;
    let abs_x = obj.x() as i32;
    let abs_y = obj.y() as i32;

    println!("\nFirst object: id={:#04x} x={} y={} s_lo={} s_hi={}", obj_id, abs_x, abs_y, s_lo, s_hi);

    let mut tile_map = std::collections::HashMap::new();
    place_obj_0db1c8(&mut tile_map, s_lo, s_hi, abs_x, abs_y, level_w, level_h);

    println!("Tiles placed: {}", tile_map.len());
    let mut ys: Vec<u32> = tile_map.keys().map(|k| k.1).collect();
    ys.sort(); ys.dedup();
    for y in &ys {
        let count = tile_map.keys().filter(|k| k.1 == *y).count();
        let tile = tile_map.iter().find(|(k,_)| k.1 == *y).map(|(_,v)| *v).unwrap();
        println!("  y={}: {} tiles, tile={:#05X}", y, count, tile);
    }

    // Check those tiles in the ROM
    println!("\nRendering check for placed tiles:");
    for tile_idx in [0x100, 0x03F] {
        let block = rom.map16_tilesets.get_map16_tile(tile_idx, map16_tileset);
        println!("  map16[{:#05X}] → {:?}", tile_idx,
            if block.is_some() { "FOUND" } else { "MISSING (will not render!)" });
    }
}
