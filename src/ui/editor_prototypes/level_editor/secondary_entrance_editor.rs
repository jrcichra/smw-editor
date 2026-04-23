use egui::{Context, Grid, ScrollArea, Slider};

use super::UiLevelEditor;

// ── Byte accessors ────────────────────────────────────────────────────────────

pub(super) fn se_destination_level(b: &[u8; 4]) -> u16 {
    let hi = (b[3] as u16 & 0b1000) << 5;
    let lo = b[0] as u16;
    hi | lo
}

pub(super) fn se_screen(b: &[u8; 4]) -> u8 {
    b[2] & 0b11111
}

pub(super) fn se_x(b: &[u8; 4]) -> u8 {
    b[2] >> 5
}

pub(super) fn se_y(b: &[u8; 4]) -> u8 {
    b[1] & 0b1111
}

pub(super) fn se_fg_initial_pos(b: &[u8; 4]) -> u8 {
    (b[1] >> 4) & 0b11
}

pub(super) fn se_bg_initial_pos(b: &[u8; 4]) -> u8 {
    b[1] >> 6
}

// ── Byte setters ─────────────────────────────────────────────────────────────

fn se_set_destination_level(b: &mut [u8; 4], level: u16) {
    b[0] = (level & 0xFF) as u8;
    b[3] = (b[3] & !0x08) | (((level >> 8) & 1) as u8) << 3;
}

fn se_set_screen(b: &mut [u8; 4], screen: u8) {
    b[2] = (b[2] & 0xE0) | (screen & 0x1F);
}

fn se_set_xy(b: &mut [u8; 4], x: u8, y: u8) {
    b[2] = (b[2] & 0x1F) | ((x & 0x07) << 5);
    b[1] = (b[1] & 0xF0) | (y & 0x0F);
}

fn se_set_fg_initial_pos(b: &mut [u8; 4], fg: u8) {
    b[1] = (b[1] & !(0b11 << 4)) | ((fg & 0b11) << 4);
}

fn se_set_bg_initial_pos(b: &mut [u8; 4], bg: u8) {
    b[1] = (b[1] & !(0b11 << 6)) | ((bg & 0b11) << 6);
}

// ── UI ───────────────────────────────────────────────────────────────────────

impl UiLevelEditor {
    pub(super) fn secondary_entrance_editor_window(&mut self, ctx: &Context) {
        if !self.show_secondary_entrances {
            return;
        }
        let mut open = self.show_secondary_entrances;
        egui::Window::new("Secondary Entrances")
            .open(&mut open)
            .resizable(true)
            .default_size([640.0, 480.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Filter:");
                    ui.text_edit_singleline(&mut self.secondary_entrance_search);
                    if ui.small_button("Clear").clicked() {
                        self.secondary_entrance_search.clear();
                    }
                });
                ui.label("Editing entrances will be saved with Ctrl+S.");
                ui.separator();

                let search = self.secondary_entrance_search.clone();
                let filter: Option<u16> = search.trim().strip_prefix("0x")
                    .and_then(|s| u16::from_str_radix(s, 16).ok())
                    .or_else(|| search.trim().parse::<u16>().ok());

                ScrollArea::vertical().show(ui, |ui| {
                    Grid::new("se_grid")
                        .num_columns(8)
                        .spacing([8.0, 4.0])
                        .striped(true)
                        .show(ui, |ui| {
                            // Header row
                            ui.strong("ID");
                            ui.strong("Dest Level");
                            ui.strong("Screen");
                            ui.strong("X");
                            ui.strong("Y");
                            ui.strong("FG Pos");
                            ui.strong("BG Pos");
                            ui.strong("Jump");
                            ui.end_row();

                            let len = self.secondary_entrance_data.len();
                            let mut dirty = false;
                            for idx in 0..len {
                                let b = self.secondary_entrance_data[idx];
                                let dest = se_destination_level(&b);

                                // Filter
                                if let Some(f) = filter {
                                    if idx as u16 != f && dest != f {
                                        continue;
                                    }
                                }

                                ui.monospace(format!("{:03X}", idx));

                                // Destination level
                                {
                                    let mut v = dest as i32;
                                    if ui.add(Slider::new(&mut v, 0..=0x1FF)
                                        .hexadecimal(3, false, true)
                                        .clamping(egui::SliderClamping::Always))
                                        .changed()
                                    {
                                        se_set_destination_level(&mut self.secondary_entrance_data[idx], v as u16);
                                        dirty = true;
                                    }
                                }

                                // Screen
                                {
                                    let mut v = se_screen(&b) as i32;
                                    if ui.add(Slider::new(&mut v, 0..=31)).changed() {
                                        se_set_screen(&mut self.secondary_entrance_data[idx], v as u8);
                                        dirty = true;
                                    }
                                }

                                // X
                                {
                                    let mut v = se_x(&b) as i32;
                                    if ui.add(Slider::new(&mut v, 0..=7)).changed() {
                                        let y = se_y(&self.secondary_entrance_data[idx]);
                                        se_set_xy(&mut self.secondary_entrance_data[idx], v as u8, y);
                                        dirty = true;
                                    }
                                }

                                // Y
                                {
                                    let mut v = se_y(&b) as i32;
                                    if ui.add(Slider::new(&mut v, 0..=15)).changed() {
                                        let x = se_x(&self.secondary_entrance_data[idx]);
                                        se_set_xy(&mut self.secondary_entrance_data[idx], x, v as u8);
                                        dirty = true;
                                    }
                                }

                                // FG initial pos
                                {
                                    let mut v = se_fg_initial_pos(&b) as i32;
                                    if ui.add(Slider::new(&mut v, 0..=3)).changed() {
                                        se_set_fg_initial_pos(&mut self.secondary_entrance_data[idx], v as u8);
                                        dirty = true;
                                    }
                                }

                                // BG initial pos
                                {
                                    let mut v = se_bg_initial_pos(&b) as i32;
                                    if ui.add(Slider::new(&mut v, 0..=3)).changed() {
                                        se_set_bg_initial_pos(&mut self.secondary_entrance_data[idx], v as u8);
                                        dirty = true;
                                    }
                                }

                                // Jump to destination level button
                                if ui.small_button(format!("→ {:03X}", dest)).clicked() {
                                    self.jump_to_level(dest);
                                }

                                ui.end_row();
                            }

                            if dirty {
                                self.mark_edited();
                            }
                        });
                });
            });
        self.show_secondary_entrances = open;
    }

    fn jump_to_level(&mut self, level: u16) {
        if level < 0x200 && (level as usize) < self.rom.levels.len() {
            if self.has_unsaved_changes() {
                self.show_unsaved_dialog = true;
                self.pending_level_num = Some(level);
            } else {
                self.level_num = level;
                self.load_level();
            }
        }
    }
}
