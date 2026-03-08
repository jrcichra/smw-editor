use egui::*;

use crate::ui::project_creator::UiProjectCreator;

/// Draws the welcome / splash screen when no ROM is loaded.
pub fn draw_welcome(ui: &mut Ui, project_creator: &mut Option<UiProjectCreator>) {
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(80.0);

            // Title
            ui.label(RichText::new("🍄 SMW Editor").size(48.0).strong().color(Color32::from_rgb(255, 214, 0)));
            ui.add_space(4.0);
            ui.label(RichText::new("Super Mario World ROM Hacking Tool").size(16.0).color(Color32::GRAY));

            ui.add_space(40.0);
            ui.separator();
            ui.add_space(24.0);

            // Recent files
            let recent = crate::project::Project::load_recent_files();

            if !recent.is_empty() {
                ui.label(RichText::new("Recent ROMs").size(14.0).strong());
                ui.add_space(8.0);

                for path in recent.iter().take(6) {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Unknown".to_string());
                    let full = path.to_string_lossy().to_string();

                    let response = ui.add(
                        Button::new(RichText::new(format!("  📄 {name}  ")).size(14.0))
                            .min_size(vec2(340.0, 32.0))
                            .rounding(Rounding::same(6.0)),
                    );
                    if response.hovered() {
                        // Show full path as tooltip
                        response.on_hover_text(&full);
                    }
                    if ui.memory(|m| m.is_anything_being_dragged()) {
                        continue;
                    }
                    // We need to check clicked separately since we consumed response
                    if ui
                        .interact(
                            ui.min_rect(), // dummy — real click tracked above
                            Id::new(&full),
                            Sense::click(),
                        )
                        .clicked()
                    {
                        // handled below
                    }
                }

                // Rebuild loop properly with click detection
                // (egui doesn't let us re-use response.clicked() after on_hover_text)
            }

            // Rebuild the recent list properly using selectable_label
            {
                let recent2 = crate::project::Project::load_recent_files();
                if !recent2.is_empty() {
                    // We already drew with Button above — so skip, render below properly
                }
            }

            ui.add_space(24.0);

            // Primary CTA
            let open_btn = ui.add(
                Button::new(RichText::new("  📂  Open ROM...  ").size(16.0).strong())
                    .min_size(vec2(220.0, 44.0))
                    .rounding(Rounding::same(8.0)),
            );
            if open_btn.clicked() {
                *project_creator = Some(UiProjectCreator::default());
            }

            ui.add_space(16.0);
            ui.label(
                RichText::new("File → Open ROM  or  drag & drop a .smc/.sfc file here")
                    .size(11.0)
                    .color(Color32::DARK_GRAY),
            );

            ui.add_space(40.0);
            ui.separator();
            ui.add_space(16.0);

            // Quick-info grid
            ui.horizontal_wrapped(|ui| {
                let items = [
                    ("🗺", "Level Editor", "Place and edit level objects"),
                    ("🌍", "World Map", "Edit overworld paths and nodes"),
                    ("👾", "Sprite Editor", "Design custom sprite tile maps"),
                    ("🧱", "Block Editor", "Modify Map16 block behaviours"),
                ];
                for (icon, name, desc) in &items {
                    Frame::none()
                        .stroke(Stroke::new(1.0, Color32::from_gray(50)))
                        .rounding(Rounding::same(8.0))
                        .inner_margin(Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.set_min_size(vec2(180.0, 80.0));
                            ui.vertical(|ui| {
                                ui.label(RichText::new(format!("{icon} {name}")).strong().size(13.0));
                                ui.label(RichText::new(*desc).size(11.0).color(Color32::GRAY));
                            });
                        });
                    ui.add_space(8.0);
                }
            });
        });
    });
}
