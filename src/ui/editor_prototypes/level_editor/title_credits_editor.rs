use egui::{Context, DragValue, Slider};
use smwe_rom::title_credits::{self, ENEMY_NAME_COUNT, ENEMY_NAME_LABELS};

use super::UiLevelEditor;

/// Global editor for title-screen and ending enemy-name data that lives in
/// fixed vanilla ROM slots.
impl UiLevelEditor {
    pub(super) fn title_credits_editor_window(&mut self, ctx: &Context) {
        if !self.show_title_credits_editor {
            return;
        }

        let mut open = self.show_title_credits_editor;
        egui::Window::new("Title Screen / Credits").open(&mut open).resizable(true).default_size([620.0, 520.0]).show(
            ctx,
            |ui| {
                ui.label("Edits here are global and use vanilla fixed-size data slots.");
                ui.separator();

                ui.heading("Title screen");
                ui.horizontal(|ui| {
                    ui.label("Opening overworld submap:");
                    let mut submap = self.title_credits.title_submap as i32;
                    if ui.add(Slider::new(&mut submap, 0..=6).hexadecimal(1, false, false)).changed() {
                        self.title_credits.title_submap = submap as u8;
                        self.title_credits_dirty = true;
                        self.has_edits = true;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Demo input: {} / {} bytes",
                        self.title_credits.title_demo_inputs.len() * 2 + 1,
                        title_credits::TITLE_INPUT_SEQ_MAX_SIZE
                    ));
                    if ui.button("+ Step").clicked()
                        && self.title_credits.title_demo_inputs.len() * 2 + 3 <= title_credits::TITLE_INPUT_SEQ_MAX_SIZE
                    {
                        self.title_credits
                            .title_demo_inputs
                            .push(title_credits::TitleDemoInput { buttons: 0x00, duration: 0x10 });
                        self.title_credits_dirty = true;
                        self.has_edits = true;
                    }
                    if ui.button("- Step").clicked() && !self.title_credits.title_demo_inputs.is_empty() {
                        self.title_credits.title_demo_inputs.pop();
                        self.title_credits_dirty = true;
                        self.has_edits = true;
                    }
                });

                egui::ScrollArea::vertical().max_height(150.0).id_salt("title_demo_inputs").show(ui, |ui| {
                    egui::Grid::new("title_demo_input_grid").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
                        ui.label("#");
                        ui.label("Buttons");
                        ui.label("Duration");
                        ui.label("Held");
                        ui.end_row();
                        for (i, input) in self.title_credits.title_demo_inputs.iter_mut().enumerate() {
                            ui.label(format!("{i:02}"));
                            let mut buttons = input.buttons as i32;
                            if ui
                                .add(DragValue::new(&mut buttons).range(0..=0xFF).hexadecimal(2, false, false))
                                .changed()
                            {
                                input.buttons = buttons as u8;
                                self.title_credits_dirty = true;
                                self.has_edits = true;
                            }
                            let mut duration = input.duration as i32;
                            if ui
                                .add(DragValue::new(&mut duration).range(0..=0xFF).hexadecimal(2, false, false))
                                .changed()
                            {
                                input.duration = duration as u8;
                                self.title_credits_dirty = true;
                                self.has_edits = true;
                            }
                            ui.label(button_summary(input.buttons));
                            ui.end_row();
                        }
                    });
                });

                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Title stripe: {} / {} bytes",
                        self.title_credits.title_screen_stripe.len(),
                        title_credits::TITLE_SCREEN_STRIPE_MAX_SIZE
                    ));
                    if ui.button("+ Byte").clicked()
                        && self.title_credits.title_screen_stripe.len() < title_credits::TITLE_SCREEN_STRIPE_MAX_SIZE
                    {
                        let insert_at = self.title_credits.title_screen_stripe.len().saturating_sub(1);
                        self.title_credits.title_screen_stripe.insert(insert_at, 0xFC);
                        self.title_credits_dirty = true;
                        self.has_edits = true;
                    }
                    if ui.button("- Byte").clicked() && self.title_credits.title_screen_stripe.len() > 1 {
                        let remove_at = self.title_credits.title_screen_stripe.len() - 2;
                        self.title_credits.title_screen_stripe.remove(remove_at);
                        self.title_credits_dirty = true;
                        self.has_edits = true;
                    }
                });
                if !self.title_credits.title_screen_stripe.ends_with(&[0xFF]) {
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 70), "Title stripe must end with FF.");
                }
                egui::CollapsingHeader::new("Raw title stripe bytes").show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(160.0).id_salt("title_stripe_bytes").show(ui, |ui| {
                        egui::Grid::new("title_stripe_byte_grid").num_columns(8).spacing([4.0, 4.0]).show(ui, |ui| {
                            for (byte_i, byte) in self.title_credits.title_screen_stripe.iter_mut().enumerate() {
                                let mut v = *byte as i32;
                                if ui.add(DragValue::new(&mut v).range(0..=0xFF).hexadecimal(2, false, false)).changed()
                                {
                                    *byte = v as u8;
                                    self.title_credits_dirty = true;
                                    self.has_edits = true;
                                }
                                if byte_i % 8 == 7 {
                                    ui.end_row();
                                }
                            }
                        });
                    });
                });

                ui.separator();
                ui.heading("Ending enemy-name stripes");
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical().max_height(240.0).id_salt("credits_enemy_list").show(ui, |ui| {
                            for i in 0..ENEMY_NAME_COUNT {
                                let used = self.title_credits.enemy_name_stripes[i].len();
                                let max = title_credits::TitleCreditsData::enemy_name_slot_size(i);
                                ui.selectable_value(
                                    &mut self.credits_editor_selected,
                                    i,
                                    format!("{i:02X} {} ({used}/{max} B)", ENEMY_NAME_LABELS[i]),
                                );
                            }
                        });
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        let i = self.credits_editor_selected.min(ENEMY_NAME_COUNT - 1);
                        let slot_size = title_credits::TitleCreditsData::enemy_name_slot_size(i);
                        let summary =
                            title_credits::summarize_enemy_name_stripe(&self.title_credits.enemy_name_stripes[i]);
                        if summary.is_empty() {
                            ui.label("Decoded text: (none detected)");
                        } else {
                            ui.label(format!("Decoded text: {summary}"));
                        }
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "Raw bytes: {} / {slot_size}",
                                self.title_credits.enemy_name_stripes[i].len()
                            ));
                            if ui.button("+ Byte").clicked()
                                && self.title_credits.enemy_name_stripes[i].len() < slot_size
                            {
                                let insert_at = self.title_credits.enemy_name_stripes[i].len().saturating_sub(1);
                                self.title_credits.enemy_name_stripes[i].insert(insert_at, 0xFC);
                                self.title_credits_dirty = true;
                                self.has_edits = true;
                            }
                            if ui.button("- Byte").clicked() && self.title_credits.enemy_name_stripes[i].len() > 1 {
                                let remove_at = self.title_credits.enemy_name_stripes[i].len() - 2;
                                self.title_credits.enemy_name_stripes[i].remove(remove_at);
                                self.title_credits_dirty = true;
                                self.has_edits = true;
                            }
                        });
                        if !self.title_credits.enemy_name_stripes[i].ends_with(&[0xFF]) {
                            ui.colored_label(egui::Color32::from_rgb(220, 80, 70), "Stripe must end with FF.");
                        }
                        egui::ScrollArea::vertical().max_height(220.0).id_salt("credits_enemy_bytes").show(ui, |ui| {
                            egui::Grid::new("credits_enemy_byte_grid").num_columns(8).spacing([4.0, 4.0]).show(
                                ui,
                                |ui| {
                                    for (byte_i, byte) in
                                        self.title_credits.enemy_name_stripes[i].iter_mut().enumerate()
                                    {
                                        let mut v = *byte as i32;
                                        if ui
                                            .add(DragValue::new(&mut v).range(0..=0xFF).hexadecimal(2, false, false))
                                            .changed()
                                        {
                                            *byte = v as u8;
                                            self.title_credits_dirty = true;
                                            self.has_edits = true;
                                        }
                                        if byte_i % 8 == 7 {
                                            ui.end_row();
                                        }
                                    }
                                },
                            );
                        });
                    });
                });
            },
        );

        self.show_title_credits_editor = open;
    }
}

fn button_summary(buttons: u8) -> String {
    let mut names = Vec::new();
    for (mask, name) in [
        (0x80, "B"),
        (0x40, "Y"),
        (0x20, "Select"),
        (0x10, "Start"),
        (0x08, "Up"),
        (0x04, "Down"),
        (0x02, "Left"),
        (0x01, "Right"),
    ] {
        if buttons & mask != 0 {
            names.push(name);
        }
    }
    if names.is_empty() {
        "-".to_string()
    } else {
        names.join("+")
    }
}
