use egui::{Context, ScrollArea};

use super::UiLevelEditor;

/// Editor for SMW's boss sequence text ("Edit Boss Sequence Text" in Lunar
/// Magic's overworld editor, v1.50; clear-text / clear-all buttons v3.20).
///
/// After defeating each of the 7 Koopalings, SMW plays a boss-sequence
/// cutscene whose text ("Mario  has  defeated the / demented  Iggy Koopa /
/// ...") is stored as pre-composed Layer 3 stripe image data in bank $0C
/// (see `smwe_rom::boss_text` for the format). Each of the 53 messages is a
/// `$FF`-terminated stripe blob at a fixed ROM address; the blob byte length
/// is immutable, so the text field encodes live and refuses input longer
/// than the slot (shorter text is space-padded).
///
/// Character set is the real SMW message-box font (`FontMap::real()`) plus
/// `'#'` (`0x5A`) and `'0'-'9'` (`0x63-0x6C`) for the "castle #N" numbers —
/// verified against the real ROM (GFX2A tiles render `#` and `1`).
///
/// The raster preview renders the message's tiles left-to-right with the
/// real GFX2A font graphics, exactly as the stripe would place them.
impl UiLevelEditor {
    pub(super) fn boss_text_editor_window(&mut self, ctx: &Context) {
        if !self.show_boss_text_editor {
            return;
        }
        let mut open = self.show_boss_text_editor;
        egui::Window::new("Boss Sequence Text Editor")
            .open(&mut open)
            .resizable(true)
            .default_size([620.0, 560.0])
            .show(ctx, |ui| {
                ui.label("Text shown during the 7 boss-defeat cutscenes. Type real text — it encodes to stripe tiles live.");
                ui.small("Each message slot has a fixed length (pre-composed stripe data); shorter text is space-padded, longer text is rejected.");
                ui.horizontal(|ui| {
                    if ui.button("Clear Text").clicked() {
                        let (b, m) = (self.boss_text_boss, self.boss_text_msg);
                        if let Some(msg) = self.boss_text.messages.get_mut(b).and_then(|v| v.get_mut(m)) {
                            msg.clear();
                            self.boss_text_dirty = true;
                            self.has_edits = true;
                            self.boss_text_edit_for = None; // force re-decode
                        }
                    }
                    if ui.button("Clear All Text").clicked() {
                        self.boss_text.clear_all();
                        self.boss_text_dirty = true;
                        self.has_edits = true;
                        self.boss_text_edit_for = None;
                    }
                    ui.small("(LM v3.20: clear-text / clear-all buttons)");
                });
                // Lunar Magic v3.40 "Custom Table File" (.lmtbl) support:
                // a table replaces the built-in tile↔character mapping for
                // this dialog.
                ui.horizontal(|ui| {
                    match (&self.boss_table_name, &self.boss_table) {
                        (Some(name), Some(t)) => {
                            ui.label(format!("Table file: {name} ({} entries)", t.len()));
                        }
                        _ => {
                            ui.label("Table file: built-in boss font");
                        }
                    }
                    if ui.button("Load Table File...").clicked() {
                        if let Some(path) =
                            rfd::FileDialog::new().add_filter("Lunar Magic table file", &["lmtbl"]).pick_file()
                        {
                            match smwe_rom::table_file::load_table_file_for_dialog(
                                &path,
                                smwe_rom::table_file::TableDialog::BossSequence,
                            ) {
                                Ok((table, name, warnings)) => {
                                    self.boss_table = Some(table);
                                    self.boss_table_name = Some(name);
                                    // Force the text buffer to re-decode
                                    // through the new mapping.
                                    self.boss_text_edit_for = None;
                                    self.boss_table_error =
                                        if warnings.is_empty() { None } else { Some(warnings.join("\n")) };
                                }
                                Err(e) => {
                                    self.boss_table_error = Some(e.to_string());
                                }
                            }
                        }
                    }
                    if self.boss_table.is_some() && ui.button("Clear Table").clicked() {
                        self.boss_table = None;
                        self.boss_table_name = None;
                        self.boss_table_error = None;
                        self.boss_text_edit_for = None;
                    }
                });
                if let Some(err) = self.boss_table_error.as_deref() {
                    ui.colored_label(egui::Color32::from_rgb(220, 160, 60), err);
                }
                ui.separator();

                ui.horizontal(|ui| {
                    // Boss selector.
                    ScrollArea::vertical().max_height(380.0).id_salt("boss_list").show(ui, |ui| {
                        for (b, name) in smwe_rom::boss_text::BOSS_NAMES.iter().enumerate() {
                            let count = self.boss_text.messages.get(b).map(Vec::len).unwrap_or(0);
                            let label = format!("{name} ({count})");
                            if ui.selectable_value(&mut self.boss_text_boss, b, label).clicked() {
                                self.boss_text_msg = 0;
                                self.boss_text_edit_for = None;
                            }
                        }
                    });

                    ui.separator();

                    // Message list for the selected boss.
                    let table = self.boss_table.clone();
                    ScrollArea::vertical().max_height(380.0).id_salt("boss_msg_list").show(ui, |ui| {
                        let b = self.boss_text_boss;
                        let count = self.boss_text.messages.get(b).map(Vec::len).unwrap_or(0);
                        for m in 0..count {
                            let preview: String = self
                                .boss_text
                                .messages[b][m]
                                .text_with_table(table.as_ref())
                                .chars()
                                .take(24)
                                .collect();
                            let label = format!("M{}: {}", m + 1, preview.trim());
                            if ui.selectable_value(&mut self.boss_text_msg, m, label).clicked() {
                                self.boss_text_edit_for = None;
                            }
                        }
                    });

                    ui.separator();

                    ui.vertical(|ui| {
                        let (b, m) = (self.boss_text_boss, self.boss_text_msg);
                        let Some(msg) = self.boss_text.messages.get(b).and_then(|v| v.get(m)) else {
                            ui.label("No message selected.");
                            return;
                        };
                        let slot_len = msg.len();
                        let snes = msg.snes.0;
                        ui.label(format!(
                            "Editing: {} M{} (${snes:06X}, {slot_len} tiles)",
                            smwe_rom::boss_text::BOSS_NAMES[b],
                            m + 1
                        ));

                        // Keep the text buffer synced with the message bytes.
                        let key = (b, m, byte_hash_msg(msg));
                        if self.boss_text_edit_for != Some(key) {
                            self.boss_text_edit = msg.text_with_table(table.as_ref());
                            // Trim trailing padding spaces for editing comfort;
                            // set_text re-pads on encode.
                            self.boss_text_edit = self.boss_text_edit.trim_end().to_string();
                            self.boss_text_edit_for = Some(key);
                            self.boss_text_error = None;
                        }

                        ui.colored_label(
                            if self.boss_text_error.is_some() {
                                egui::Color32::from_rgb(220, 60, 60)
                            } else {
                                ui.style().visuals.text_color()
                            },
                            format!(
                                "Text encodes to {} / {slot_len} tiles",
                                self.boss_text.messages[b][m].len()
                            ),
                        );
                        if table.is_some() {
                            ui.small(
                                "Table active (LM v3.40): the tile budget counts encoded bytes; \
                                 unmapped typed characters are skipped.",
                            );
                        } else {
                            ui.small("Characters: A-Z a-z 0-9 # ! . \" , ? ' and space.");
                        }

                        let text_resp = ui.add(
                            egui::TextEdit::singleline(&mut self.boss_text_edit)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY),
                        );
                        if text_resp.changed() {
                            match self.boss_text.messages[b][m]
                                .set_text_with_table(&self.boss_text_edit, table.as_ref())
                            {
                                Ok(()) => {
                                    self.boss_text_error = None;
                                    self.boss_text_dirty = true;
                                    self.has_edits = true;
                                    // Re-key so the raster cache below rebuilds.
                                    self.boss_text_edit_for =
                                        Some((b, m, byte_hash_msg(&self.boss_text.messages[b][m])));
                                }
                                Err(e) => {
                                    self.boss_text_error = Some(e.to_string());
                                }
                            }
                        }
                        if let Some(err) = self.boss_text_error.as_ref() {
                            ui.colored_label(egui::Color32::from_rgb(220, 60, 60), err.as_str());
                        }

                        ui.separator();
                        ui.label("Preview (true SMW font)");

                        // Raster preview: render the message's tiles
                        // left-to-right with the real GFX2A font graphics.
                        if self.message_font.is_none() {
                            self.message_font =
                                smwe_rom::message_raster::decompress_message_font(&self.rom.rom).ok();
                        }
                        if let Some(font) = &self.message_font {
                            let msg = &self.boss_text.messages[b][m];
                            let hash = byte_hash_msg(msg);
                            if self.boss_text_raster_for != Some((b, m, hash)) {
                                let tiles: Vec<u8> = msg.char_bytes();
                                let img = rasterize_tiles_strip(&tiles, font);
                                // 3x scale for visibility.
                                let (w, h) = (img.width() * 3, img.height() * 3);
                                let mut pixels = Vec::with_capacity((w * h) as usize);
                                for y in 0..h {
                                    for x in 0..w {
                                        let p = img.get_pixel(x / 3, y / 3);
                                        pixels.push(egui::Color32::from_rgb(p[0], p[1], p[2]));
                                    }
                                }
                                let color_img =
                                    egui::ColorImage { size: [w as usize, h as usize], pixels };
                                let tex = ui.ctx().load_texture(
                                    format!("boss_text_raster_{b}_{m}"),
                                    color_img,
                                    egui::TextureOptions::NEAREST,
                                );
                                self.boss_text_raster_texture = Some(tex);
                                self.boss_text_raster_for = Some((b, m, hash));
                            }
                            if let Some(tex) = &self.boss_text_raster_texture {
                                let tiles = self.boss_text.messages[b][m].len() as f32;
                                ui.image((tex.id(), egui::vec2(tiles * 8.0 * 2.0, 16.0 * 2.0)));
                            }
                        }

                        // Readable text (decoded with the boss font map, or the
                        // active table file).
                        ui.separator();
                        ui.label("Decoded text:");
                        let decoded = self.boss_text.messages[b][m].text_with_table(table.as_ref());
                        ui.label(egui::RichText::new(decoded).text_style(egui::TextStyle::Monospace));
                    });
                });
            },
        );
        self.show_boss_text_editor = open;
    }
}

/// Hash a message's tiles for preview-cache invalidation.
fn byte_hash_msg(msg: &smwe_rom::boss_text::BossMessage) -> u64 {
    let mut hash: u64 = 0;
    for cmd in &msg.commands {
        for &t in &cmd.tiles {
            hash = hash.wrapping_mul(31).wrapping_add(t as u64);
        }
    }
    hash
}

/// Rasterize a horizontal strip of font tiles (8×8 each) to an RGB image.
fn rasterize_tiles_strip(tiles: &[u8], font: &[Box<[u8]>]) -> image::RgbImage {
    use image::{Rgb, RgbImage};
    let w = (tiles.len() * 8) as u32;
    let img_w = w.max(8);
    let mut img = RgbImage::new(img_w, 8);
    let palette = [Rgb([0, 0, 0]), Rgb([255, 255, 255]), Rgb([128, 128, 128]), Rgb([192, 192, 192])];
    for (i, &tile_idx) in tiles.iter().enumerate() {
        let tile = &font[(tile_idx & 0x7F) as usize % font.len()];
        for y in 0..8 {
            for x in 0..8 {
                let c = tile[y * 8 + x] as usize;
                img.put_pixel((i * 8 + x) as u32, y as u32, palette[c.min(3)]);
            }
        }
    }
    img
}
