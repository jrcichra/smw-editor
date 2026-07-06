use egui::{Context, ScrollArea, Slider, TextEdit};
use smwe_rom::message_boxes::{
    byte_to_char,
    decode_text,
    encode_text,
    MESSAGE_BOXES_MAX_SIZE,
    MESSAGE_LINE_WIDTH,
    MESSAGE_NAMES,
};

use super::UiLevelEditor;

/// Editor for SMW's vanilla message box text: 22 global messages. Text is
/// edited WYSIWYG — the byte<->character chart was derived from the actual
/// message font tiles in VRAM (see `smwe_rom::message_boxes::byte_to_char`)
/// and validated by decoding every vanilla message. A raw byte grid remains
/// available for the few special tiles that aren't ordinary characters.
///
/// Edits are global (every level shares the same 22 messages) and size-
/// constrained: the vanilla ROM already uses the full byte budget, so making
/// one message longer requires shrinking another (see
/// `smwe_rom::message_boxes` module docs for why this data isn't repointable).
impl UiLevelEditor {
    pub(super) fn message_editor_window(&mut self, ctx: &Context) {
        if !self.show_message_editor {
            return;
        }
        let mut open = self.show_message_editor;
        egui::Window::new("Message Box Editor").open(&mut open).resizable(true).default_size([620.0, 460.0]).show(
            ctx,
            |ui| {
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
                            if ui.selectable_value(&mut self.message_editor_selected, i, label).changed() {
                                self.message_editor_text = None;
                            }
                        }
                    });

                    ui.separator();

                    ui.vertical(|ui| {
                        let i = self.message_editor_selected;
                        ui.label(format!("Editing: {}", MESSAGE_NAMES[i]));

                        // WYSIWYG text panel. Lines are game rows: exactly
                        // MESSAGE_LINE_WIDTH characters each (padded with
                        // spaces on apply).
                        let has_special_tiles =
                            self.message_boxes.messages[i].iter().any(|&b| byte_to_char(b).is_none());
                        if has_special_tiles {
                            ui.small("This message uses special (non-text) tiles, shown as ¤ — editing text would lose them, so use the byte grid below.");
                            ui.add_enabled(
                                false,
                                TextEdit::multiline(&mut decode_text(&self.message_boxes.messages[i]))
                                    .font(egui::TextStyle::Monospace)
                                    .desired_rows(8),
                            );
                        } else {
                            let text =
                                self.message_editor_text.get_or_insert_with(|| decode_text(&self.message_boxes.messages[i]));
                            let response = ui.add(
                                TextEdit::multiline(text)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_rows(8)
                                    .char_limit(MESSAGE_LINE_WIDTH * 16),
                            );
                            ui.small(format!("{MESSAGE_LINE_WIDTH} characters per line; lines are padded with spaces."));
                            if response.changed() {
                                match encode_text(text) {
                                    Ok(bytes) => {
                                        if bytes != self.message_boxes.messages[i] {
                                            self.message_boxes.messages[i] = bytes;
                                            self.message_boxes_dirty = true;
                                            self.has_edits = true;
                                        }
                                    }
                                    Err(c) => {
                                        ui.colored_label(
                                            egui::Color32::from_rgb(220, 60, 60),
                                            format!("Unsupported character {c:?} — supported: A-Z a-z 0-9 !.-,?#()' and space."),
                                        );
                                    }
                                }
                            }
                        }

                        ui.separator();
                        ui.collapsing("Raw tile bytes", |ui| {
                            ui.horizontal(|ui| {
                                if ui.button("+ Byte").clicked() {
                                    self.message_boxes.messages[i].push(0x1F); // 0x1F = space
                                    self.message_boxes_dirty = true;
                                    self.has_edits = true;
                                    self.message_editor_text = None;
                                }
                                if ui.button("- Byte").clicked() && !self.message_boxes.messages[i].is_empty() {
                                    self.message_boxes.messages[i].pop();
                                    self.message_boxes_dirty = true;
                                    self.has_edits = true;
                                    self.message_editor_text = None;
                                }
                            });
                            ScrollArea::vertical().max_height(200.0).id_salt("message_bytes").show(ui, |ui| {
                                egui::Grid::new("message_byte_grid").num_columns(8).spacing([4.0, 4.0]).show(
                                    ui,
                                    |ui| {
                                        let mut changed = false;
                                        for (byte_i, byte) in self.message_boxes.messages[i].iter_mut().enumerate() {
                                            let mut v = *byte as i32;
                                            let glyph = byte_to_char(*byte).unwrap_or('¤');
                                            if ui
                                                .add(
                                                    Slider::new(&mut v, 0..=0x7F)
                                                        .hexadecimal(2, false, false)
                                                        .text(glyph.to_string()),
                                                )
                                                .changed()
                                            {
                                                *byte = v as u8;
                                                changed = true;
                                            }
                                            if (byte_i + 1) % 4 == 0 {
                                                ui.end_row();
                                            }
                                        }
                                        if changed {
                                            self.message_boxes_dirty = true;
                                            self.has_edits = true;
                                            self.message_editor_text = None;
                                        }
                                    },
                                );
                            });
                        });
                    });
                });
            },
        );
        self.show_message_editor = open;
    }
}
