mod dev_utils;
mod editing_mode;
mod editor_prototypes;
mod ow_tile_picker;
mod project_creator;
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
    project::{Project, ProjectRef},
    ui::{
        dev_utils::address_converter::UiAddressConverter,
        editor_prototypes::{
            block_editor::UiBlockEditor, level_editor::UiLevelEditor, sprite_map_editor::UiSpriteMapEditor,
        },
        project_creator::UiProjectCreator,
        tab_viewer::EditorToolTabViewer,
        tool::DockableEditorTool,
        world_editor::UiWorldEditor,
    },
};

pub struct UiMainWindow {
    gl: Arc<glow::Context>,
    project_creator: Option<UiProjectCreator>,
    dock_style: DockStyle,
    dock_state: DockState<Box<dyn DockableEditorTool>>,
    /// Path of the currently-open ROM (for Save).
    rom_path: Option<PathBuf>,
    /// Set when a Save error needs to be shown.
    save_error: Option<String>,
    /// True when a ROM was pre-loaded at startup and we still need to open the default editor tab.
    pending_initial_editor: bool,
    /// Full in-egui file dialog for Save As.
    save_as_dialog: FileDialog,
}

impl UiMainWindow {
    pub fn new(project: Option<ProjectRef>, cc: &CreationContext) -> Self {
        let mut fonts = FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, Variant::Regular);
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_visuals(Visuals::dark());

        let mut rom_path = None;
        let mut pending_initial_editor = false;
        if let Some(project) = project {
            cc.egui_ctx.data_mut(|data| {
                let project = project.borrow();
                data.insert_temp(Project::project_title_id(), project.title.clone());
                data.insert_temp(Project::rom_id(), Arc::clone(&project.rom));
            });
            rom_path = Some(project.borrow().path.clone());
            pending_initial_editor = true;
        }

        let mut dock_style = DockStyle::from_egui(&cc.egui_ctx.style());
        dock_style.tab.tab_body.inner_margin = Margin::ZERO;

        Self {
            gl: Arc::clone(cc.gl.as_ref().expect("must use the glow renderer")),
            project_creator: None,
            dock_style,
            dock_state: DockState::new(vec![]),
            rom_path,
            save_error: None,
            pending_initial_editor,
            save_as_dialog: FileDialog::new(),
        }
    }
}

impl eframe::App for UiMainWindow {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        let rom: Option<Arc<SmwRom>> = ctx.data(|data| data.get_temp(Id::new("rom")));

        // Open the level editor automatically whenever a ROM just became available
        // and there are no tabs yet (startup pre-load OR direct recent-file click).
        if self.pending_initial_editor
            || (rom.is_some() && self.dock_state.iter_all_tabs().count() == 0 && self.project_creator.is_none())
        {
            self.pending_initial_editor = false;
            if let Some(ref rom) = rom {
                let path = self.rom_path.clone().unwrap_or_default();
                match UiLevelEditor::new(Arc::clone(&self.gl), Arc::clone(rom), path) {
                    Ok(editor) => self.open_tool(editor),
                    Err(e) => self.save_error = Some(format!("Failed to open level editor: {e}")),
                }
            }
        }

        // Menu bar always on top.
        self.main_menu_bar(ctx, rom.as_ref());

        // Save As dialog (egui-native, no native file picker needed).
        self.show_save_as_dialog(ctx);

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
                welcome::draw_welcome(ui, &mut self.project_creator);
            });
        } else {
            CentralPanel::default().show(ctx, |_ui| {});
        }

        DockArea::new(&mut self.dock_state).style(self.dock_style.clone()).show(ctx, &mut EditorToolTabViewer);

        // Project creator dialog.
        if let Some(project_creator) = &mut self.project_creator {
            // project_creator.update() already opens a Window internally — no CentralPanel needed.
            let still_open = project_creator.update_ctx(ctx);
            if !still_open {
                // If a ROM was just loaded, grab its path and auto-open level editor.
                let new_rom: Option<Arc<SmwRom>> = ctx.data(|d| d.get_temp(Id::new("rom")));
                if let Some(rom) = new_rom {
                    if self.rom_path.is_none() {
                        // First load — open the level editor automatically.
                        let path: Option<String> = ctx.data(|d| d.get_temp(Id::new("rom_path")));
                        if let Some(p) = path {
                            self.rom_path = Some(PathBuf::from(p));
                        }
                        let path = self.rom_path.clone().unwrap_or_default();
                        match UiLevelEditor::new(Arc::clone(&self.gl), Arc::clone(&rom), path) {
                            Ok(editor) => self.open_tool(editor),
                            Err(e) => self.save_error = Some(format!("Failed to open level editor: {e}")),
                        }
                    }
                }
                self.project_creator = None;
            }
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

    fn show_save_as_dialog(&mut self, ctx: &Context) {
        self.save_as_dialog.update(ctx);
        if let Some(dest) = self.save_as_dialog.take_selected() {
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
                    self.rom_path = Some(dest);
                }
                Err(e) => self.save_error = Some(format!("Save As failed: {e}")),
            }
        }
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
                        self.project_creator = Some(UiProjectCreator::default());
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
}
