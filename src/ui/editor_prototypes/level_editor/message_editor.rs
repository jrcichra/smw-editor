use std::sync::Arc;

use egui::{Context, ScrollArea, Slider};
use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};
use smwe_rom::{
    font_map::{decode_editable_text, encode_message_checked, FontMap},
    message_boxes::{
        pointer_slot_for_message,
        MESSAGE_BOXES_MAX_SIZE,
        MESSAGE_BOXES_SNES,
        MESSAGE_NAMES,
        MESSAGE_POINTER_COUNT,
        MESSAGE_POINTER_TABLE_SNES,
    },
    snes_utils::addr::{AddrPc, AddrSnes},
};

use super::UiLevelEditor;

/// Editor for SMW's vanilla message box text: 22 global messages.
///
/// Phase 2 (editable text): each message shows a multiline text field with
/// the decoded text — type real characters and the field re-encodes to
/// font-tile-index bytes live. `\n` is the line-break representation (8 rows
/// × 18 cells, matching the game's message window); there are no other
/// control codes because the real `CODE_05B208` has none.
///
/// Byte↔character mapping is the real SMW (U) font map (`FontMap::real()`,
/// verified 2026-09-10 by running all 22 messages through the genuine
/// `CODE_05B1BC`). Bytes with no font glyph (non-text graphic tiles such as
/// Yoshi's signature or the bonus-star icons) show as `�` (U+FFFD): leave
/// the placeholder in place to keep the graphic, delete it to drop it.
/// Graphics can't be moved or inserted via the text field — the raw byte grid
/// below remains for byte-level surgery.
///
/// Size constraint: the 22-message blob isn't repointable (addressed directly
/// by ASM), so each message's encoded text must fit within its vanilla byte
/// span. The field shows `used / budget` bytes and refuses over-budget input
/// with an explanatory message instead of silently truncating.
///
/// The raster preview below the text field is live: it re-renders from the
/// current bytes with the real GFX2A message-font graphics on every edit.
/// The `CODE_05B1BC` readout also re-runs on every edit — the edited bytes
/// are patched into a scratch ROM image (message blob + recomputed pointer
/// table, exactly what saving would write) so it shows what the game will
/// actually render.
impl UiLevelEditor {
    pub(super) fn message_editor_window(&mut self, ctx: &Context) {
        if !self.show_message_editor {
            return;
        }
        let mut open = self.show_message_editor;
        egui::Window::new("Message Box Editor")
            .open(&mut open)
            .resizable(true)
            .default_size([560.0, 700.0])
            .show(ctx, |ui| {
                ui.label("Type real text — it encodes to font-tile bytes live. 8 rows × 18 cells; longer lines are rejected.");
                // Lunar Magic v3.40 "Custom Table File" (.lmtbl) support:
                // a table replaces the built-in tile↔character mapping for
                // this dialog, so text can be displayed/edited in a
                // different language.
                ui.horizontal(|ui| {
                    match (&self.message_table_name, &self.message_table) {
                        (Some(name), Some(t)) => {
                            ui.label(format!("Table file: {name} ({} entries)", t.len()));
                        }
                        _ => {
                            ui.label("Table file: built-in font map");
                        }
                    }
                    if ui.button("Load Table File...").clicked() {
                        if let Some(path) =
                            rfd::FileDialog::new().add_filter("Lunar Magic table file", &["lmtbl"]).pick_file()
                        {
                            match smwe_rom::table_file::load_table_file_for_dialog(
                                &path,
                                smwe_rom::table_file::TableDialog::MessageBox,
                            ) {
                                Ok((table, name, warnings)) => {
                                    self.message_table = Some(table);
                                    self.message_table_name = Some(name);
                                    // Force the text buffer to re-decode
                                    // through the new mapping.
                                    self.message_text_for = None;
                                    self.message_table_error =
                                        if warnings.is_empty() { None } else { Some(warnings.join("\n")) };
                                }
                                Err(e) => {
                                    self.message_table_error = Some(e.to_string());
                                }
                            }
                        }
                    }
                    if self.message_table.is_some() && ui.button("Clear Table").clicked() {
                        self.message_table = None;
                        self.message_table_name = None;
                        self.message_table_error = None;
                        self.message_text_for = None;
                    }
                });
                if let Some(err) = self.message_table_error.as_deref() {
                    ui.colored_label(egui::Color32::from_rgb(220, 160, 60), err);
                }
                let total = self.message_boxes.total_size();
                let over_budget = total > MESSAGE_BOXES_MAX_SIZE;
                let color = if over_budget {
                    egui::Color32::from_rgb(220, 60, 60)
                } else if total == MESSAGE_BOXES_MAX_SIZE {
                    egui::Color32::from_rgb(220, 160, 60)
                } else {
                    ui.style().visuals.text_color()
                };
                ui.colored_label(color, format!("Total: {total} / {MESSAGE_BOXES_MAX_SIZE} bytes"));
                if total == MESSAGE_BOXES_MAX_SIZE {
                    ui.small(
                        "Vanilla already uses the full budget — lengthening one message requires shortening another.",
                    );
                }
                ui.separator();

                ui.horizontal(|ui| {
                    ScrollArea::vertical().max_height(340.0).id_salt("message_list").show(ui, |ui| {
                        for (i, name) in MESSAGE_NAMES.iter().enumerate() {
                            let label = format!("{name} ({} B)", self.message_boxes.messages[i].len());
                            ui.selectable_value(&mut self.message_editor_selected, i, label);
                        }
                    });

                    ui.separator();

                    ui.vertical(|ui| {
                        let i = self.message_editor_selected;
                        let map = FontMap::real();
                        // Active custom table (cloned so the encode path
                        // below can mutate `self` without borrow issues).
                        let table = self.message_table.clone();
                        ui.label(format!("Editing: {}", MESSAGE_NAMES[i]));

                        // Keep the text buffer synced: a selection change, a
                        // table load/clear, or an edit via the raw byte grid
                        // below, re-decodes it.
                        let cur_hash = byte_hash(&self.message_boxes.messages[i]);
                        if self.message_text_for != Some(i) || cur_hash != self.message_text_bytes_hash
                        {
                            self.message_text_edit = match &table {
                                Some(t) => {
                                    smwe_rom::font_map::decode_message_with_table(t, &self.message_boxes.messages[i])
                                }
                                None => decode_editable_text(&map, &self.message_boxes.messages[i]),
                            };
                            self.message_text_for = Some(i);
                            self.message_text_bytes_hash = cur_hash;
                            self.message_text_error = None;
                        }

                        let budget = self.message_budgets[i];
                        let used = self.message_boxes.messages[i].len();
                        let budget_color = if self.message_text_error.is_some() || used > budget {
                            egui::Color32::from_rgb(220, 60, 60)
                        } else {
                            ui.style().visuals.text_color()
                        };
                        ui.colored_label(budget_color, format!("Text encodes to {used} / {budget} bytes"));
                        if table.is_some() {
                            ui.small(
                                "Table active (LM v3.40): unmapped bytes show as <XX> (display-only); \
                                 unmapped typed characters are skipped.",
                            );
                        } else {
                            ui.small("'�' marks a graphic tile: keep it in place to preserve the graphic, delete it to drop it.");
                        }

                        let text_resp = ui.add(
                            egui::TextEdit::multiline(&mut self.message_text_edit)
                                .font(egui::TextStyle::Monospace)
                                .desired_rows(8)
                                .desired_width(f32::INFINITY),
                        );
                        if text_resp.changed() {
                            let original = self.message_boxes.messages[i].clone();
                            let result = match &table {
                                Some(t) => {
                                    smwe_rom::font_map::encode_message_with_table(t, budget, &self.message_text_edit)
                                }
                                None => encode_message_checked(&map, &original, budget, &self.message_text_edit),
                            };
                            match result {
                                Ok(bytes) => {
                                    self.message_boxes.messages[i] = bytes;
                                    self.message_text_bytes_hash =
                                        byte_hash(&self.message_boxes.messages[i]);
                                    self.message_text_error = None;
                                    self.message_boxes_dirty = true;
                                    self.has_edits = true;
                                }
                                Err(e) => {
                                    self.message_text_error = Some(e.to_string());
                                }
                            }
                        }
                        if let Some(err) = self.message_text_error.as_ref() {
                            ui.colored_label(egui::Color32::from_rgb(220, 60, 60), err.as_str());
                        }

                        ui.separator();
                        ui.label("Preview (live, 8×18)");

                        // True raster preview: decompress GFX2A ("Message Box
                        // Letters") once, rasterize the 8×18 grid with the real
                        // SMW font graphics. The cache key includes a hash of
                        // the message bytes so typing rebuilds the preview.
                        if self.message_font.is_none() {
                            self.message_font =
                                smwe_rom::message_raster::decompress_message_font(&self.rom.rom).ok();
                        }
                        if let Some(font) = &self.message_font {
                            let msg_bytes = &self.message_boxes.messages[i];
                            let hash = byte_hash(msg_bytes);
                            if self.message_raster_for != Some((i, hash)) {
                                let cells = smwe_rom::font_map::message_cells(msg_bytes);
                                let img = smwe_rom::message_raster::rasterize_message(cells, font);
                                // 3x scale for visibility.
                                let (w, h) = (img.width() * 3, img.height() * 3);
                                let mut pixels = Vec::with_capacity((w * h) as usize);
                                for y in 0..h {
                                    for x in 0..w {
                                        let p = img.get_pixel(x / 3, y / 3);
                                        pixels.push(egui::Color32::from_rgb(p[0], p[1], p[2]));
                                    }
                                }
                                let color_img = egui::ColorImage { size: [w as usize, h as usize], pixels };
                                let tex = ui.ctx().load_texture(
                                    format!("message_raster_{i}"),
                                    color_img,
                                    egui::TextureOptions::NEAREST,
                                );
                                self.message_raster_texture = Some(tex);
                                self.message_raster_for = Some((i, hash));
                            }
                            if let Some(tex) = &self.message_raster_texture {
                                ui.label("Raster (true SMW font):");
                                ui.image((tex.id(), egui::vec2(432.0, 192.0)));
                            }
                        }

                        // Live game-routine check: patch the CURRENT bytes
                        // into a scratch ROM image (message blob + recomputed
                        // pointer table — exactly what saving writes) and run
                        // the real CODE_05B1BC, so this reflects the edited
                        // text, not the vanilla bytes.
                        let slot = pointer_slot_for_message(i);
                        let stripe_hash = byte_hash(&self.message_boxes.messages[i]);
                        if self.message_preview_for != Some((i, stripe_hash)) {
                            if let Ok((blob, pointers)) = self.message_boxes.to_blob_and_pointers() {
                                let mut patched = self.cpu.mem.cart.as_slice().to_vec();
                                let base = snes_to_pc(MESSAGE_BOXES_SNES);
                                let tab = snes_to_pc(MESSAGE_POINTER_TABLE_SNES);
                                if base + blob.len() <= patched.len()
                                    && tab + 2 * MESSAGE_POINTER_COUNT <= patched.len()
                                {
                                    patched[base..base + blob.len()].copy_from_slice(&blob);
                                    for (s, p) in pointers.iter().enumerate() {
                                        patched[tab + 2 * s..tab + 2 * s + 2]
                                            .copy_from_slice(&p.to_le_bytes());
                                    }
                                    let mut emu_rom = EmuRom::new(patched);
                                    emu_rom.load_symbols(include_str!(
                                        "../../../../symbols/SMW_U.sym"
                                    ));
                                    let mut scratch =
                                        Cpu::new(CheckedMem::new(Arc::new(emu_rom)));
                                    self.message_preview =
                                        Some(smwe_emu::emu::render_message(&mut scratch, slot));
                                    self.message_preview_for = Some((i, stripe_hash));
                                }
                            }
                        }
                        if let Some(stripe) = &self.message_preview {
                            ui.small(format!(
                                "CODE_05B1BC on edited bytes ({} cycles): {} stripe bytes (8 rows × 18 tiles).",
                                stripe.cycles,
                                stripe.stripe.len()
                            ));
                        }

                        ui.separator();
                        ui.collapsing("Raw bytes (advanced)", |ui| {
                            ui.small("Direct byte surgery. The text field above re-decodes from these bytes.");
                            ui.horizontal(|ui| {
                                if ui.button("+ Byte").clicked() {
                                    self.message_boxes.messages[i].push(0x1F); // 0x1F = vanilla space code
                                    self.message_boxes_dirty = true;
                                    self.has_edits = true;
                                }
                                if ui.button("- Byte").clicked()
                                    && !self.message_boxes.messages[i].is_empty()
                                {
                                    self.message_boxes.messages[i].pop();
                                    self.message_boxes_dirty = true;
                                    self.has_edits = true;
                                }
                            });

                            ScrollArea::vertical()
                                .max_height(200.0)
                                .id_salt("message_bytes")
                                .show(ui, |ui| {
                                    egui::Grid::new("message_byte_grid")
                                        .num_columns(8)
                                        .spacing([4.0, 4.0])
                                        .show(ui, |ui| {
                                            let mut changed = false;
                                            for (byte_i, byte) in
                                                self.message_boxes.messages[i].iter_mut().enumerate()
                                            {
                                                let mut v = *byte as i32;
                                                if ui
                                                    .add(Slider::new(&mut v, 0..=0xFF).hexadecimal(2, false, false))
                                                    .changed()
                                                {
                                                    *byte = v as u8;
                                                    changed = true;
                                                }
                                                if (byte_i + 1) % 8 == 0 {
                                                    ui.end_row();
                                                }
                                            }
                                            if changed {
                                                self.message_boxes_dirty = true;
                                                self.has_edits = true;
                                            }
                                        });
                                });
                        });
                    });
                });
            },
        );
        self.show_message_editor = open;
    }
}

/// Simple byte hash for preview-cache invalidation.
fn byte_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0;
    for &b in bytes {
        hash = hash.wrapping_mul(31).wrapping_add(b as u64);
    }
    hash
}

/// LoROM SNES address → file offset, for patching the scratch CPU's ROM.
fn snes_to_pc(snes: AddrSnes) -> usize {
    AddrPc::try_from_lorom(snes).expect("message box SNES address").0 as usize
}
