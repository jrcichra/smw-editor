use std::sync::Arc;

use egui::*;

use crate::project::Project;

pub fn draw_welcome(ui: &mut Ui, open_requested: &mut bool) {
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
                    .rounding(Rounding::same(8.0)),
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
                            .rounding(Rounding::same(5.0)),
                    )
                    .on_hover_text(&full);
                if resp.clicked() {
                    chosen = Some(path.clone());
                }
            }
            if let Some(path) = chosen {
                // Load the ROM directly — UiMainWindow will handle it via the rom egui data store.
                match Project::new(&path) {
                    Ok(project) => {
                        Project::add_to_recent(&path);
                        ui.data_mut(|data| {
                            data.insert_temp(Project::project_title_id(), project.title.clone());
                            data.insert_temp(Project::rom_id(), Arc::clone(&project.rom));
                            data.insert_temp(Id::new("rom_path"), path.to_string_lossy().into_owned());
                        });
                    }
                    Err(e) => {
                        log::error!("Failed to open recent ROM: {e}");
                        // Open the file browser so the user can pick a replacement.
                        *open_requested = true;
                    }
                }
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
                Frame::none()
                    .stroke(Stroke::new(1.0, Color32::from_gray(55)))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(Margin::same(12.0))
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
}
