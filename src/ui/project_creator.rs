use std::path::{Path, PathBuf};

use eframe::egui::{Button, RichText, ScrollArea, Ui, Window};

use crate::{
    project::Project,
    ui::style::{EditorStyle, ErrorStyle},
};

#[derive(Debug)]
pub struct UiProjectCreator {
    base_rom_path:        String,
    recent_files:         Vec<PathBuf>,
    err_base_rom_path:    String,
    err_project_creation: String,
}

impl Default for UiProjectCreator {
    fn default() -> Self {
        log::info!("Opened Project Creator");
        UiProjectCreator {
            base_rom_path:        String::new(),
            recent_files:         Project::load_recent_files(),
            err_base_rom_path:    String::new(),
            err_project_creation: String::new(),
        }
    }
}

impl UiProjectCreator {
    /// Returns false when the creator should be closed.
    pub fn update(&mut self, ui: &Ui) -> bool {
        let mut opened = true;
        let mut created_or_cancelled = false;

        Window::new("Open ROM").auto_sized().resizable(false).collapsible(false).open(&mut opened).show(
            ui.ctx(),
            |ui| {
                // Recent files
                if !self.recent_files.is_empty() {
                    ui.label(RichText::new("Recent files").strong());
                    ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                        ui.set_min_width(360.0);
                        let mut chosen = None;
                        for path in &self.recent_files {
                            let name = path.file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let full = path.to_string_lossy().into_owned();
                            if ui.selectable_label(false, format!("{name}  —  {full}")).clicked() {
                                chosen = Some(path.clone());
                            }
                        }
                        if let Some(path) = chosen {
                            self.open_project_at(path, ui, &mut created_or_cancelled);
                        }
                    });
                    ui.separator();
                }

                // Manual path entry
                ui.label(RichText::new("ROM file").strong());
                ui.horizontal(|ui| {
                    if ui.text_edit_singleline(&mut self.base_rom_path).changed() {
                        self.validate_path();
                    }
                    if ui.small_button("Browse...").clicked() {
                        self.pick_file();
                    }
                });
                if !self.err_base_rom_path.is_empty() {
                    ui.colored_label(
                        ErrorStyle::get_from_egui(ui.ctx(), |s| s.text_color),
                        &self.err_base_rom_path,
                    );
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let can_open = self.err_base_rom_path.is_empty() && !self.base_rom_path.is_empty();
                    if ui.add_enabled(can_open, Button::new("Open").small()).clicked() {
                        let path = PathBuf::from(&self.base_rom_path);
                        self.open_project_at(path, ui, &mut created_or_cancelled);
                    }
                    if ui.small_button("Cancel").clicked() {
                        created_or_cancelled = true;
                    }
                });
                if !self.err_project_creation.is_empty() {
                    ui.colored_label(
                        ErrorStyle::get_from_egui(ui.ctx(), |s| s.text_color),
                        &self.err_project_creation,
                    );
                }
            },
        );

        let running = opened && !created_or_cancelled;
        if !running {
            log::info!("Closed Project Creator");
        }
        running
    }

    fn validate_path(&mut self) {
        let p = Path::new(&self.base_rom_path);
        if self.base_rom_path.is_empty() {
            self.err_base_rom_path.clear();
        } else if !p.exists() {
            self.err_base_rom_path = format!("File '{}' does not exist.", self.base_rom_path);
        } else if p.is_dir() {
            self.err_base_rom_path = format!("'{}' is a directory, not a file.", self.base_rom_path);
        } else {
            self.err_base_rom_path.clear();
        }
    }

    fn pick_file(&mut self) {
        // On WSL the XDG portal is broken; use the GTK/native backend directly.
        // rfd's FileDialog falls back to GTK when the portal isn't available.
        std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("SNES ROM", &["smc", "sfc"])
            .pick_file()
        {
            self.base_rom_path = path.to_string_lossy().into_owned();
            self.validate_path();
        }
    }

    fn open_project_at(&mut self, path: PathBuf, ui: &Ui, done: &mut bool) {
        log::info!("Opening project from: {}", path.display());
        match Project::new(&path) {
            Ok(project) => {
                log::info!("Success opening project");
                Project::add_to_recent(&path);
                ui.data_mut(|data| {
                    data.insert_temp(Project::project_title_id(), project.title.clone());
                    data.insert_temp(Project::rom_id(), project.rom);
                });
                self.err_project_creation.clear();
                *done = true;
            }
            Err(err) => {
                log::error!("Failed to open project: {err}");
                self.err_project_creation = format!("Error: {err}");
            }
        }
    }
}
