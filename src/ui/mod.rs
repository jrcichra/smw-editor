mod dev_utils;
mod editing_mode;
mod editor_prototypes;
mod ow_tile_picker;
mod style;
mod tab_viewer;
mod tool;
mod welcome;
mod world_editor;
mod world_editor_editing;

use std::{path::PathBuf, sync::Arc};

use anyhow::Context as _;
use eframe::{CreationContext, Frame};
use egui::*;
use egui_dock::{DockArea, DockState, Style as DockStyle};
use egui_file_dialog::FileDialog;
use egui_phosphor::Variant;
use smwe_rom::SmwRom;

use crate::{
    project::Project,
    ui::{
        dev_utils::address_converter::UiAddressConverter,
        editor_prototypes::{
            block_editor::UiBlockEditor, level_editor::UiLevelEditor, sprite_map_editor::UiSpriteMapEditor,
        },
        tab_viewer::EditorToolTabViewer,
        tool::DockableEditorTool,
        world_editor::UiWorldEditor,
    },
};

pub struct UiMainWindow {
    gl: Arc<glow::Context>,
    dock_style: DockStyle,
    dock_state: DockState<Box<dyn DockableEditorTool>>,
    /// Path of the currently-open ROM (for Save).
    rom_path: Option<PathBuf>,
    /// Set when a Save error needs to be shown.
    save_error: Option<String>,
    /// In-egui file dialog for Open ROM.
    open_dialog: FileDialog,
    /// In-egui file dialog for Save As.
    save_as_dialog: FileDialog,
    /// In-egui file dialog for BPS patch export.
    bps_export_dialog: FileDialog,
    /// In-egui file dialog for IPS patch export.
    ips_export_dialog: FileDialog,
    /// Set when user tries to close the app with unsaved changes
    show_exit_dialog: bool,
}

impl UiMainWindow {
    pub fn new(cc: &CreationContext) -> Self {
        let mut fonts = FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, Variant::Regular);
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_visuals(Visuals::dark());

        let mut dock_style = DockStyle::from_egui(&cc.egui_ctx.style());
        dock_style.tab.tab_body.inner_margin = Margin::ZERO;

        Self {
            gl: Arc::clone(cc.gl.as_ref().expect("must use the glow renderer")),
            dock_style,
            dock_state: DockState::new(vec![]),
            rom_path: None,
            save_error: None,
            open_dialog: FileDialog::new(),
            save_as_dialog: FileDialog::new(),
            bps_export_dialog: FileDialog::new(),
            ips_export_dialog: FileDialog::new(),
            show_exit_dialog: false,
        }
    }
}

impl eframe::App for UiMainWindow {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        let rom: Option<Arc<SmwRom>> = ctx.data(|data| data.get_temp(Id::new("rom")));

        // Check if user is trying to close the app
        let is_finishing = ctx.input(|i| i.viewport().close_requested());
        if is_finishing && !self.show_exit_dialog && self.has_any_unsaved_changes() {
            self.show_exit_dialog = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }

        // Menu bar always on top.
        self.main_menu_bar(ctx, rom.as_ref());

        // Open dialog.
        self.show_open_dialog(ctx);

        // Save As dialog (egui-native, no native file picker needed).
        self.show_save_as_dialog(ctx);

        // BPS export dialog.
        self.show_bps_export_dialog(ctx, rom.as_ref());

        // IPS export dialog.
        self.show_ips_export_dialog(ctx, rom.as_ref());

        // Save error toast.
        if let Some(err) = &self.save_error.clone() {
            let mut open = true;
            Window::new("Save Error").open(&mut open).show(ctx, |ui| {
                ui.label(err);
                if ui.button("OK").clicked() {
                    self.save_error = None;
                }
            });
            if !open {
                self.save_error = None;
            }
        }

        // Welcome / splash when no ROM is open and no tabs.
        if rom.is_none() && self.dock_state.iter_all_tabs().count() == 0 {
            CentralPanel::default().show(ctx, |ui| {
                let mut open_requested = false;
                let chosen = welcome::draw_welcome(ui, &mut open_requested);
                if open_requested {
                    self.open_dialog = FileDialog::new();
                    self.open_dialog.pick_file();
                }
                if let Some(path) = chosen {
                    self.load_rom_from_path(ctx, path);
                }
            });
        } else {
            CentralPanel::default().show(ctx, |_ui| {});
        }

        DockArea::new(&mut self.dock_state).style(self.dock_style.clone()).show(ctx, &mut EditorToolTabViewer);

        // Check if any level editor is requesting a save
        self.check_for_save_requests();

        // Exit confirmation dialog
        if self.show_exit_dialog {
            egui::Window::new("⚠️  Unsaved Changes")
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("You have unsaved changes in open editors.");
                    ui.label("Do you want to save before exiting?");
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("💾 Save & Exit").clicked() {
                            // Save all editors before closing
                            if self.rom_path.is_some() {
                                let path = self.rom_path.clone().unwrap();
                                if self.write_rom_to_path(&path, &path).is_ok() {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                } else {
                                    self.show_exit_dialog = false;
                                }
                            } else {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        if ui.button("❌ Exit Without Saving").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button("⏸️ Cancel").clicked() {
                            self.show_exit_dialog = false;
                        }
                    });
                });
        }
    }
}

impl UiMainWindow {
    fn open_tool<ToolType>(&mut self, tool: ToolType)
    where
        ToolType: 'static + DockableEditorTool,
    {
        log::info!("Opened {}", tool.title().text());
        self.dock_state.push_to_focused_leaf(Box::new(tool));
    }

    fn show_open_dialog(&mut self, ctx: &Context) {
        self.open_dialog.update(ctx);
        if let Some(path) = self.open_dialog.take_picked() {
            self.load_rom_from_path(ctx, path);
        }
    }

    fn load_rom_from_path(&mut self, ctx: &Context, path: PathBuf) {
        match Project::new(&path) {
            Ok(project) => {
                Project::add_to_recent(&path);
                ctx.data_mut(|data| {
                    data.insert_temp(Project::project_title_id(), project.title.clone());
                    data.insert_temp(Project::rom_id(), Arc::clone(&project.rom));
                });
                self.rom_path = Some(path.clone());
                let rom: Arc<SmwRom> = Arc::clone(&project.rom);
                match UiLevelEditor::new(Arc::clone(&self.gl), rom, path) {
                    Ok(editor) => self.open_tool(editor),
                    Err(e) => self.save_error = Some(format!("Failed to open level editor: {e}")),
                }
            }
            Err(e) => self.save_error = Some(format!("Failed to open ROM: {e}")),
        }
    }

    fn save_rom(&mut self, ctx: &Context) {
        let Some(path) = &self.rom_path else {
            self.save_error = Some("No ROM path — open a ROM first.".into());
            return;
        };
        let rom: Option<Arc<SmwRom>> = ctx.data(|d| d.get_temp(Id::new("rom")));
        let Some(_) = rom else {
            self.save_error = Some("No ROM loaded.".into());
            return;
        };
        match self.write_rom_to_path(path, path) {
            Ok(()) => {
                if let Err(e) = self.reload_rom_into_context(ctx, path) {
                    self.save_error = Some(format!("Saved ROM, but reload failed: {e}"));
                } else {
                    log::info!("Saved ROM to {}", path.display());
                }
            }
            Err(e) => self.save_error = Some(format!("Save failed: {e}")),
        }
    }

    fn save_rom_as(&mut self) {
        let initial_dir = self
            .rom_path
            .as_deref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let initial_name = self
            .rom_path
            .as_deref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "output.smc".to_string());

        self.save_as_dialog = FileDialog::new()
            .initial_directory(initial_dir)
            .default_file_name(&initial_name);
        self.save_as_dialog.save_file();
    }

    fn export_bps_patch(&mut self) {
        let initial_dir = self
            .rom_path
            .as_deref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let initial_name = self
            .rom_path
            .as_deref()
            .and_then(|p| p.file_stem())
            .map(|n| format!("{}.bps", n.to_string_lossy()))
            .unwrap_or_else(|| "output.bps".to_string());

        self.bps_export_dialog = FileDialog::new()
            .initial_directory(initial_dir)
            .default_file_name(&initial_name);
        self.bps_export_dialog.save_file();
    }

    fn export_ips_patch(&mut self) {
        let initial_dir = self
            .rom_path
            .as_deref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let initial_name = self
            .rom_path
            .as_deref()
            .and_then(|p| p.file_stem())
            .map(|n| format!("{}.ips", n.to_string_lossy()))
            .unwrap_or_else(|| "output.ips".to_string());

        self.ips_export_dialog = FileDialog::new()
            .initial_directory(initial_dir)
            .default_file_name(&initial_name);
        self.ips_export_dialog.save_file();
    }

    fn show_save_as_dialog(&mut self, ctx: &Context) {
        self.save_as_dialog.update(ctx);
        if let Some(dest) = self.save_as_dialog.take_picked() {
            let Some(src) = self.rom_path.clone() else {
                return;
            };
            match self.write_rom_to_path(&src, &dest) {
                Ok(_) => {
                    log::info!("Saved ROM as {}", dest.display());
                    if let Err(e) = self.reload_rom_into_context(ctx, &dest) {
                        self.save_error = Some(format!("Saved ROM As, but reload failed: {e}"));
                        return;
                    }
                    self.rom_path = Some(dest.to_path_buf());
                }
                Err(e) => self.save_error = Some(format!("Save As failed: {e}")),
            }
        }
    }

    fn show_bps_export_dialog(&mut self, ctx: &Context, rom: Option<&Arc<SmwRom>>) {
        self.bps_export_dialog.update(ctx);
        if let Some(patch_dest) = self.bps_export_dialog.take_picked() {
            let Some(rom) = rom else {
                self.save_error = Some("No ROM loaded.".into());
                return;
            };
            let Some(src) = self.rom_path.clone() else {
                self.save_error = Some("No ROM path — open a ROM first.".into());
                return;
            };

            match self.create_bps_patch(rom, &src, &patch_dest) {
                Ok(_) => {
                    log::info!("Exported BPS patch to {}", patch_dest.display());
                }
                Err(e) => self.save_error = Some(format!("BPS export failed: {e}")),
            }
        }
    }

    fn create_bps_patch(
        &self,
        _rom: &Arc<SmwRom>,
        original_rom_path: &std::path::Path,
        patch_dest: &std::path::Path,
    ) -> anyhow::Result<()> {
        // Read the original ROM to generate patch against it
        let original_bytes = std::fs::read(original_rom_path)
            .with_context(|| format!("Failed to read original ROM from {}", original_rom_path.display()))?;

        // Create the modified ROM (with all current edits applied)
        let mut modified_bytes = original_bytes.clone();
        let has_smc_header = modified_bytes.len() % 0x400 == 0x200;
        for (_, tab) in self.dock_state.iter_all_tabs() {
            tab.save_to_rom(&mut modified_bytes, has_smc_header)?;
        }

        // Create BPS patch
        let patch = smwe_bps::create_patch(&original_bytes, &modified_bytes, smwe_bps::BpsConfig::default())?;

        // Write patch to file
        std::fs::write(patch_dest, patch)
            .with_context(|| format!("Failed to write BPS patch to {}", patch_dest.display()))?;

        Ok(())
    }

    fn show_ips_export_dialog(&mut self, ctx: &Context, rom: Option<&Arc<SmwRom>>) {
        self.ips_export_dialog.update(ctx);
        if let Some(patch_dest) = self.ips_export_dialog.take_picked() {
            let Some(rom) = rom else {
                self.save_error = Some("No ROM loaded.".into());
                return;
            };
            let Some(src) = self.rom_path.clone() else {
                self.save_error = Some("No ROM path — open a ROM first.".into());
                return;
            };

            match self.create_ips_patch(rom, &src, &patch_dest) {
                Ok(_) => {
                    log::info!("Exported IPS patch to {}", patch_dest.display());
                }
                Err(e) => self.save_error = Some(format!("IPS export failed: {e}")),
            }
        }
    }

    fn create_ips_patch(
        &self,
        _rom: &Arc<SmwRom>,
        original_rom_path: &std::path::Path,
        patch_dest: &std::path::Path,
    ) -> anyhow::Result<()> {
        // Read the original ROM to generate patch against it
        let original_bytes = std::fs::read(original_rom_path)
            .with_context(|| format!("Failed to read original ROM from {}", original_rom_path.display()))?;

        // Create the modified ROM (with all current edits applied)
        let mut modified_bytes = original_bytes.clone();
        let has_smc_header = modified_bytes.len() % 0x400 == 0x200;
        for (_, tab) in self.dock_state.iter_all_tabs() {
            tab.save_to_rom(&mut modified_bytes, has_smc_header)?;
        }

        // Create IPS patch
        let patch = smwe_ips::create_patch(&original_bytes, &modified_bytes)?;

        // Write patch to file
        std::fs::write(patch_dest, patch)
            .with_context(|| format!("Failed to write IPS patch to {}", patch_dest.display()))?;

        Ok(())
    }

    fn main_menu_bar(&mut self, ctx: &Context, rom: Option<&Arc<SmwRom>>) {
        let has_rom = rom.is_some();
        // Ctrl+S shortcut.
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, Key::S))) {
            let ctx2 = ctx.clone();
            self.save_rom(&ctx2);
        }

        TopBottomPanel::top("main_top_bar").show(ctx, |ui| {
            menu::bar(ui, |ui| {
                // ── File ──
                ui.menu_button("File", |ui| {
                    if ui.button("Open ROM...").clicked() {
                        self.open_dialog = FileDialog::new();
                        self.open_dialog.pick_file();
                        ui.close_menu();
                    }
                    ui.add_enabled_ui(has_rom, |ui| {
                        if ui.button("Save ROM        Ctrl+S").clicked() {
                            let ctx2 = ctx.clone();
                            self.save_rom(&ctx2);
                            ui.close_menu();
                        }
                        if ui.button("Save ROM As...").clicked() {
                            self.save_rom_as();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Export BPS Patch...").clicked() {
                            self.export_bps_patch();
                            ui.close_menu();
                        }
                        if ui.button("Export IPS Patch...").clicked() {
                            self.export_ips_patch();
                            ui.close_menu();
                        }
                    });
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                });

                // ── Editors ──
                ui.menu_button("Editors", |ui| {
                    ui.add_enabled_ui(has_rom, |ui| {
                        if ui.button("Level Editor").clicked() {
                            let path = self.rom_path.clone().unwrap_or_default();
                            match UiLevelEditor::new(Arc::clone(&self.gl), Arc::clone(rom.unwrap()), path) {
                                Ok(editor) => self.open_tool(editor),
                                Err(e) => self.save_error = Some(format!("Failed to open level editor: {e}")),
                            }
                            ui.close_menu();
                        }
                        if ui.button("World Map Editor").clicked() {
                            let Some(path) = self.rom_path.clone() else {
                                self.save_error =
                                    Some("No ROM path available for emulator-backed overworld view.".into());
                                ui.close_menu();
                                return;
                            };
                            self.open_tool(UiWorldEditor::new(Arc::clone(&self.gl), Arc::clone(rom.unwrap()), path));
                            ui.close_menu();
                        }
                        if ui.button("Sprite Tile Editor").clicked() {
                            self.open_tool(UiSpriteMapEditor::new(Arc::clone(&self.gl), Arc::clone(rom.unwrap())));
                            ui.close_menu();
                        }
                        if ui.button("Block Editor").clicked() {
                            self.open_tool(UiBlockEditor::default());
                            ui.close_menu();
                        }
                    });
                });

                // ── Tools ──
                ui.menu_button("Tools", |ui| {
                    if ui.button("Address Converter").clicked() {
                        self.open_tool(UiAddressConverter::default());
                        ui.close_menu();
                    }
                });

                // Right-aligned ROM name.
                if has_rom {
                    let title: String =
                        ctx.data(|d| d.get_temp(Project::project_title_id()).unwrap_or_else(|| "ROM".to_string()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("📁 {title}")).small());
                    });
                }
            });
        });
    }

    fn write_rom_to_path(&self, source_path: &std::path::Path, dest_path: &std::path::Path) -> anyhow::Result<()> {
        let mut rom_bytes = std::fs::read(source_path)
            .with_context(|| format!("Failed to read ROM from {}", source_path.display()))?;
        let has_smc_header = rom_bytes.len() % 0x400 == 0x200;
        for (_, tab) in self.dock_state.iter_all_tabs() {
            tab.save_to_rom(&mut rom_bytes, has_smc_header)?;
        }
        std::fs::write(dest_path, rom_bytes)
            .with_context(|| format!("Failed to write ROM to {}", dest_path.display()))?;
        Ok(())
    }

    fn reload_rom_into_context(&self, ctx: &Context, path: &std::path::Path) -> anyhow::Result<()> {
        let project = Project::new(path)?;
        ctx.data_mut(|data| {
            data.insert_temp(Project::project_title_id(), project.title.clone());
            data.insert_temp(Project::rom_id(), Arc::clone(&project.rom));
        });
        Ok(())
    }

    fn has_any_unsaved_changes(&self) -> bool {
        for (_, tab) in self.dock_state.iter_all_tabs() {
            if tab.has_unsaved_changes() {
                return true;
            }
        }
        false
    }

    fn check_for_save_requests(&mut self) {
        let mut should_save = false;
        for (_, tab) in self.dock_state.iter_all_tabs_mut() {
            if tab.take_save_request() {
                should_save = true;
            }
        }
        if should_save {
            if let Some(path) = &self.rom_path.clone() {
                if let Err(e) = self.write_rom_to_path(path, path) {
                    self.save_error = Some(format!("Save failed: {e}"));
                } else {
                    log::info!("Saved ROM to {}", path.display());
                }
            }
        }
    }
}
