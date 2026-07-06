use egui::{Context, Slider};
use rfd::{MessageButtons, MessageDialog, MessageLevel};
use smwe_rom::graphics::gfx_file::{self, GfxFile, Tile};

use super::UiLevelEditor;

/// Export/import raw GFX file tile data as lossless grayscale PNGs (pixel
/// intensity = color index, scaled to fill 0-255 for the format's bit depth),
/// and stage edits for `save_to_rom`. Grayscale-of-index (rather than a
/// palette-colored preview) is deliberate: it round-trips exactly regardless
/// of which palette a given file happens to be viewed with in-game, and
/// avoids introducing palette-matching ambiguity into the import path.
impl UiLevelEditor {
    pub(super) fn gfx_editor_window(&mut self, ctx: &Context) {
        if !self.show_gfx_editor {
            return;
        }
        let mut open = self.show_gfx_editor;
        egui::Window::new("GFX Editor (ExGFX)").open(&mut open).resizable(true).default_size([420.0, 320.0]).show(
            ctx,
            |ui| {
                ui.label("Export/import raw tile data for a GFX file slot as a grayscale PNG.");
                ui.label("Pixel intensity encodes the color index directly — it's not a colored preview.");
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("GFX file:");
                    let mut file_num = self.gfx_editor_file_num as i32;
                    let max = gfx_file::gfx_file_count() as i32 - 1;
                    if ui.add(Slider::new(&mut file_num, 0..=max).hexadecimal(2, false, false)).changed() {
                        self.gfx_editor_file_num = file_num as usize;
                    }
                });

                let file_num = self.gfx_editor_file_num;
                let format = gfx_file::tile_format_of(file_num);
                let n_tiles = match self.gfx_edits.get(&file_num) {
                    Some(raw_bytes) => raw_bytes.len() / tile_byte_size(format).max(1),
                    None => self.rom.gfx.files.get(file_num).map(|f| f.tiles.len()).unwrap_or(0),
                };
                ui.label(format!("Format: {format}  •  {n_tiles} tiles"));
                if self.gfx_edits.contains_key(&file_num) {
                    ui.colored_label(egui::Color32::from_rgb(220, 160, 60), "Pending unsaved import for this file.");
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Export...").clicked() {
                        self.export_gfx_file(file_num);
                    }
                    if ui.button("Import...").clicked() {
                        self.import_gfx_file(file_num);
                    }
                });
            },
        );
        self.show_gfx_editor = open;
    }

    fn export_gfx_file(&mut self, file_num: usize) {
        let Some(file) = self.rom.gfx.files.get(file_num) else { return };
        let Some(img) = tiles_to_image(&file.tiles, file.tile_format) else { return };

        if let Some(path) = rfd::FileDialog::new().set_file_name(format!("gfx{file_num:02X}.png")).save_file() {
            if let Err(e) = img.save(&path) {
                MessageDialog::new()
                    .set_level(MessageLevel::Error)
                    .set_buttons(MessageButtons::Ok)
                    .set_description(format!("Failed to save PNG: {e}"))
                    .show();
            }
        }
    }

    fn import_gfx_file(&mut self, file_num: usize) {
        let Some(path) = rfd::FileDialog::new().add_filter("PNG image", &["png"]).pick_file() else { return };
        let img = match image::open(&path) {
            Ok(img) => img.to_luma8(),
            Err(e) => {
                MessageDialog::new()
                    .set_level(MessageLevel::Error)
                    .set_buttons(MessageButtons::Ok)
                    .set_description(format!("Failed to open PNG: {e}"))
                    .show();
                return;
            }
        };

        let format = gfx_file::tile_format_of(file_num);
        let tiles = match image_to_tiles(&img, format) {
            Ok(tiles) => tiles,
            Err(msg) => {
                MessageDialog::new().set_level(MessageLevel::Error).set_buttons(MessageButtons::Ok).set_description(msg).show();
                return;
            }
        };

        let raw_bytes = GfxFile { tile_format: format, tiles }.to_raw_bytes();
        self.gfx_edits.insert(file_num, raw_bytes);
        self.has_edits = true;
    }
}

fn tile_byte_size(format: gfx_file::TileFormat) -> usize {
    use gfx_file::TileFormat::*;
    match format {
        Tile2bpp => 16,
        Tile3bpp | Tile3bppMode7 => 24,
        Tile4bpp => 32,
        Tile8bpp => 64,
    }
}

const GFX_IMAGE_COLS: usize = 16;

/// Render tiles into a lossless grayscale sheet (pixel intensity = color
/// index, scaled to fill 0-255), `GFX_IMAGE_COLS` tiles per row.
fn tiles_to_image(tiles: &[Tile], format: gfx_file::TileFormat) -> Option<image::GrayImage> {
    if tiles.is_empty() {
        return None;
    }
    let scale = index_scale(format);
    let rows = tiles.len().div_ceil(GFX_IMAGE_COLS);
    let mut img = image::GrayImage::new((GFX_IMAGE_COLS * 8) as u32, (rows * 8) as u32);
    for (i, tile) in tiles.iter().enumerate() {
        let (tx, ty) = ((i % GFX_IMAGE_COLS) * 8, (i / GFX_IMAGE_COLS) * 8);
        for py in 0..8usize {
            for px in 0..8usize {
                let idx = tile.color_indices[py * 8 + px];
                img.put_pixel((tx + px) as u32, (ty + py) as u32, image::Luma([idx * scale]));
            }
        }
    }
    Some(img)
}

/// Inverse of `tiles_to_image`. Errors (as a user-facing message) if the
/// image dimensions don't match the expected `GFX_IMAGE_COLS`-wide, 8px-tile
/// grid layout.
fn image_to_tiles(img: &image::GrayImage, format: gfx_file::TileFormat) -> Result<Vec<Tile>, String> {
    let scale = index_scale(format);
    let max_index = 255 / scale;
    let (w, h) = img.dimensions();
    if w as usize != GFX_IMAGE_COLS * 8 || h as usize % 8 != 0 {
        return Err(format!(
            "Image must be {}px wide ({GFX_IMAGE_COLS} tiles/row) and a multiple of 8px tall; got {w}x{h}.",
            GFX_IMAGE_COLS * 8
        ));
    }
    let rows = h as usize / 8;
    let n_tiles = rows * GFX_IMAGE_COLS;

    let mut tiles = Vec::with_capacity(n_tiles);
    for i in 0..n_tiles {
        let (tx, ty) = ((i % GFX_IMAGE_COLS) * 8, (i / GFX_IMAGE_COLS) * 8);
        let mut color_indices = [0u8; 64];
        for py in 0..8usize {
            for px in 0..8usize {
                let px_val = img.get_pixel((tx + px) as u32, (ty + py) as u32).0[0];
                color_indices[py * 8 + px] = (px_val / scale).min(max_index);
            }
        }
        tiles.push(Tile { color_indices: Box::new(color_indices) });
    }
    Ok(tiles)
}

/// Scale factor mapping a color index (0..max) to a full 0-255 grayscale
/// range, so exported images use the full brightness range and are easy to
/// eyeball, while still round-tripping exactly on import.
fn index_scale(format: gfx_file::TileFormat) -> u8 {
    use gfx_file::TileFormat::*;
    match format {
        Tile2bpp => 85,                 // 255 / 3
        Tile3bpp | Tile3bppMode7 => 36, // 255 / 7 (rounded down)
        Tile4bpp => 17,                 // 255 / 15
        Tile8bpp => 1,                  // 255 / 255
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile_with_indices(color_indices: [u8; 64]) -> Tile {
        Tile { color_indices: Box::new(color_indices) }
    }

    #[test]
    fn tiles_to_image_and_back_round_trips_4bpp() {
        let mut indices_a = [0u8; 64];
        let mut indices_b = [0u8; 64];
        for i in 0..64 {
            indices_a[i] = (i % 16) as u8;
            indices_b[i] = ((i * 3) % 16) as u8;
        }
        let tiles = vec![tile_with_indices(indices_a), tile_with_indices(indices_b)];
        let img = tiles_to_image(&tiles, gfx_file::TileFormat::Tile4bpp).unwrap();
        let round_tripped = image_to_tiles(&img, gfx_file::TileFormat::Tile4bpp).unwrap();

        assert_eq!(round_tripped.len(), GFX_IMAGE_COLS * 1); // padded to one full row
        assert_eq!(round_tripped[0].color_indices, tiles[0].color_indices);
        assert_eq!(round_tripped[1].color_indices, tiles[1].color_indices);
    }

    #[test]
    fn tiles_to_image_and_back_round_trips_all_formats() {
        use gfx_file::TileFormat::*;
        for format in [Tile2bpp, Tile3bpp, Tile4bpp, Tile8bpp, Tile3bppMode7] {
            let max = 255u16 / index_scale(format) as u16;
            let mut indices = [0u8; 64];
            for (i, v) in indices.iter_mut().enumerate() {
                *v = (i as u16 % (max + 1)) as u8;
            }
            let tiles = vec![tile_with_indices(indices)];
            let img = tiles_to_image(&tiles, format).unwrap();
            let round_tripped = image_to_tiles(&img, format).unwrap();
            assert_eq!(round_tripped[0].color_indices, tiles[0].color_indices, "format {format:?}");
        }
    }

    #[test]
    fn image_to_tiles_rejects_wrong_width() {
        let img = image::GrayImage::new(100, 8);
        let err = image_to_tiles(&img, gfx_file::TileFormat::Tile4bpp).unwrap_err();
        assert!(err.contains("must be"));
    }

    #[test]
    fn empty_tiles_produce_no_image() {
        assert!(tiles_to_image(&[], gfx_file::TileFormat::Tile4bpp).is_none());
    }
}

#[cfg(test)]
mod real_rom_tests {
    use smwe_rom::{compression::lc_lz2, SmwRom};

    use super::*;

    /// Full-pipeline check against a real ROM: real GFX file -> image ->
    /// tiles -> raw bytes -> compress -> decompress -> compare to the
    /// original file's own re-encoded raw bytes. Run with
    /// `ROM_PATH=/path/to/smw.smc cargo test --lib -- --ignored
    /// gfx_export_import_round_trip`.
    #[test]
    #[ignore]
    fn gfx_export_import_round_trip() {
        let rom_path = std::env::var("ROM_PATH").expect("set ROM_PATH");
        let rom = SmwRom::from_file(rom_path).expect("parse ROM");
        let file = &rom.gfx.files[0];

        let img = tiles_to_image(&file.tiles, file.tile_format).expect("nonempty file");
        let round_tripped_tiles = image_to_tiles(&img, file.tile_format).expect("valid image");

        let original_raw = file.to_raw_bytes();
        let round_tripped_raw =
            smwe_rom::graphics::gfx_file::GfxFile { tile_format: file.tile_format, tiles: round_tripped_tiles }
                .to_raw_bytes();
        assert_eq!(round_tripped_raw, original_raw);

        let compressed = lc_lz2::compress(&round_tripped_raw);
        let decompressed = lc_lz2::decompress(&compressed, false).unwrap();
        assert_eq!(decompressed, original_raw);
    }
}
