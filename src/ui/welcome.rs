use std::path::PathBuf;

use egui::*;

/// Returns the path of a recent ROM the user clicked on, if any.
pub fn draw_welcome(ui: &mut Ui, open_requested: &mut bool) -> Option<PathBuf> {
    let mut chosen_recent: Option<PathBuf> = None;
    let total_h = ui.available_height();
    ui.add_space((total_h * 0.08).max(20.0));

    ui.vertical_centered(|ui| {
        ui.label(RichText::new("🍄  SMW Editor").size(48.0).strong().color(Color32::from_rgb(255, 214, 0)));
        ui.add_space(4.0);
        ui.label(RichText::new("Super Mario World ROM Hacking Tool").size(15.0).color(Color32::GRAY));
        ui.add_space(32.0);

        if ui
            .add(
                Button::new(RichText::new("  📂  Open ROM…  ").size(16.0).strong())
                    .min_size(vec2(240.0, 44.0))
                    .corner_radius(8),
            )
            .clicked()
        {
            *open_requested = true;
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new("File → Open ROM  or drag & drop a .smc / .sfc file").size(11.0).color(Color32::DARK_GRAY),
        );

        let recent = crate::project::Project::load_recent_files();
        if !recent.is_empty() {
            ui.add_space(28.0);
            ui.label(RichText::new("Recent ROMs").size(13.0).strong());
            ui.add_space(6.0);

            let mut chosen: Option<std::path::PathBuf> = None;
            for path in recent.iter().take(6) {
                let name =
                    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Unknown".into());
                let full = path.to_string_lossy().to_string();
                let resp = ui
                    .add(
                        Button::new(RichText::new(format!("  📄  {name}")).size(13.0))
                            .min_size(vec2(360.0, 28.0))
                            .corner_radius(5),
                    )
                    .on_hover_text(&full);
                if resp.clicked() {
                    chosen = Some(path.clone());
                }
            }
            if let Some(path) = chosen {
                chosen_recent = Some(path);
            }
        }

        ui.add_space(36.0);
        ui.separator();
        ui.add_space(16.0);
        ui.label(RichText::new("Editors available after opening a ROM").size(12.0).color(Color32::GRAY));
        ui.add_space(10.0);

        ui.horizontal_wrapped(|ui| {
            let features: &[(&str, &str, &str)] = &[
                ("🗺", "Level Editor", "Place and edit level objects"),
                ("🌍", "World Map", "Edit overworld nodes and paths"),
                ("👾", "Sprite Tiles", "Design custom sprite tile maps"),
                ("🧱", "Block Editor", "Modify Map16 blocks and behaviour"),
                ("🔢", "Address Converter", "Convert between ROM address formats"),
            ];
            for (icon, name, desc) in features {
                Frame::NONE
                    .stroke(Stroke::new(1.0, Color32::from_gray(55)))
                    .corner_radius(8)
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_min_size(vec2(195.0, 72.0));
                        ui.vertical(|ui| {
                            ui.label(RichText::new(format!("{icon}  {name}")).strong().size(13.0));
                            ui.add_space(3.0);
                            ui.label(RichText::new(*desc).size(11.0).color(Color32::GRAY));
                        });
                    });
                ui.add_space(6.0);
            }
        });
    });
    chosen_recent
}
