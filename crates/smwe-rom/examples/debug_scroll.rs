use smwe_rom::SmwRom;

fn main() {
    let smw = SmwRom::from_file("/home/justin/git/smw-editor/smw.smc").unwrap();

    for (i, tilemap) in smw.overworld.layer2.iter().enumerate() {
        println!(
            "Submap {} ({:?}): scroll=({}, {}) -> origin({}, {})",
            i,
            tilemap.submap_info.name,
            tilemap.submap_info.scroll_x,
            tilemap.submap_info.scroll_y,
            tilemap.submap_info.scroll_x / 8,
            tilemap.submap_info.scroll_y / 8
        );
    }
}
