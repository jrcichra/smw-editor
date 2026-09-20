mod dev_utils;
mod editing_mode;
mod editor_prototypes;
mod exanimation_dialog;
mod exit_scan_dialog;
mod resource_scan_dialog;
mod style;
mod tab_viewer;
mod tool;
mod welcome;
mod world_editor;

pub mod clipboard;
pub mod restore;
pub mod user_toolbar;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Context as _;
use eframe::{CreationContext, Frame};
use egui::*;
use egui_dock::{DockArea, DockState, Style as DockStyle};
use egui_file_dialog::FileDialog;
use egui_phosphor::Variant;
use smwe_rom::{
    level_deletion::{delete_levels, level_modified_vs, GAMEPLAY_CRITICAL_LEVELS},
    level_sharing::share_data_between_levels,
    overworld::level_number_for_index,
    rom_expansion::{expand_rom, expansion_targets, format_size, split_smc_header},
    snes_utils::rom::Rom,
    SmwRom,
};

use crate::{
    editor_options::EditorOptions,
    level_png_export::{level_export_filename, level_png_bytes, LevelPngOptions, LEVEL_COUNT},
    placement_check::{format_issue, PlacementIssue},
    project::Project,
    ui::{
        dev_utils::address_converter::UiAddressConverter,
        editor_prototypes::{level_editor::UiLevelEditor, sprite_map_editor::UiSpriteMapEditor},
        restore::RestoreManager,
        tab_viewer::EditorToolTabViewer,
        tool::DockableEditorTool,
        user_toolbar::{ToolbarAction, UserToolbarState},
        world_editor::UiWorldEditor,
    },
};

pub struct UiMainWindow {
    gl:                        Arc<glow::Context>,
    dock_style:                DockStyle,
    dock_state:                DockState<Box<dyn DockableEditorTool>>,
    /// Path of the currently-open ROM (for Save).
    rom_path:                  Option<PathBuf>,
    /// Set when a Save error needs to be shown.
    save_error:                Option<String>,
    /// In-egui file dialog for Open ROM.
    open_dialog:               FileDialog,
    /// In-egui file dialog for Save As.
    save_as_dialog:            FileDialog,
    /// In-egui file dialog for BPS patch export.
    bps_export_dialog:         FileDialog,
    /// In-egui file dialog for IPS patch export.
    ips_export_dialog:         FileDialog,
    /// Expand-ROM dialog (File > Expand ROM...).
    show_expand_dialog:        bool,
    /// Selected expansion target size in bytes.
    expand_target:             usize,
    /// Status line shown in the Expand-ROM dialog.
    expand_status:             Option<String>,
    /// In-egui file dialog for single-level PNG export (File > Export Level to PNG...).
    png_export_dialog:         FileDialog,
    /// Translevel chosen for the pending single-level PNG export.
    png_export_level:          Option<u16>,
    /// Status line for the last single-level PNG export.
    png_export_status:         Option<String>,
    /// Batch level-export dialog (File > Levels > Export Multiple Levels to Image Files...).
    show_batch_export_dialog:  bool,
    /// In-egui directory picker for the batch export output folder.
    batch_export_dir_dialog:   FileDialog,
    /// Hex strings for the batch export range (inclusive), e.g. "000"–"1FF".
    batch_from:                String,
    batch_to:                  String,
    /// Batch export output folder.
    batch_out_dir:             Option<PathBuf>,
    /// Batch export layer toggles (mirror the single-level options).
    batch_include_l1:          bool,
    batch_include_l2:          bool,
    batch_include_sprites:     bool,
    /// Status line shown in the batch-export dialog.
    batch_status:              Option<String>,
    /// Delete-levels dialog (File > Levels > Delete Levels from ROM..., LM v3.50 parity).
    show_delete_levels_dialog: bool,
    /// Per-level checkbox state for the delete dialog (index = level number).
    delete_levels_selected:    Vec<bool>,
    /// Per-level "modified vs ROM-as-opened" flags, refreshed when the dialog opens.
    delete_levels_modified:    Vec<bool>,
    /// Per-level gameplay-critical flags (title/demo + overworld-placed),
    /// refreshed when the dialog opens; selecting any shows a warning.
    delete_levels_critical:    Vec<bool>,
    /// Levels awaiting delete confirmation.
    delete_levels_pending:     Option<Vec<u16>>,
    /// Status line shown in the delete-levels dialog.
    delete_levels_status:      Option<String>,
    /// Open-Level-from-Address dialog (File > Open Level from Address...,
    /// LM v1.11 parity).
    show_level_addr_dialog:    bool,
    /// Hex text of the PC address field in the from-address dialog.
    level_addr_text:           String,
    /// Generic dialog error toast (title "Error").
    dialog_error:              Option<String>,
    /// Share-data dialog (File > Levels > Share Data Between Levels to Save Space...).
    show_share_data_dialog:    bool,
    /// Status line shown in the share-data dialog.
    share_data_status:         Option<String>,
    /// Exit-scan dialog (Tools > Scan for Undefined Exits..., LM v1.50/v1.60
    /// parity). The scan runs on a worker thread; this owns its state.
    show_exit_scan_dialog:     bool,
    exit_scan:                 exit_scan_dialog::ExitScanUi,
    /// Resource-analysis dialog (Tools > Analyze Resources in Levels...,
    /// LM v3.03/v3.20 parity). The scan runs on a worker thread; this owns
    /// its state.
    show_resource_scan_dialog: bool,
    resource_scan:             resource_scan_dialog::ResourceScanUi,
    /// Set when user tries to close the app with unsaved changes
    show_exit_dialog:          bool,
    /// Restore points + original-ROM reference copy (Restore menu, LM v1.80).
    restore_manager:           RestoreManager,
    /// In-egui file dialog for Apply IPS Patch.
    ips_apply_dialog:          FileDialog,
    /// "Create Restore Point" dialog state.
    show_restore_dialog:       bool,
    /// Name typed into the "Create Restore Point" dialog.
    restore_point_name:        String,
    /// Restore-point index awaiting revert confirmation.
    pending_revert:            Option<usize>,
    /// IPS patch path + preview awaiting apply confirmation.
    pending_ips_apply:         Option<PendingIpsApply>,
    /// IPS export destination awaiting same-directory-warning confirmation.
    pending_ips_export:        Option<PathBuf>,
    /// Status line for restore/IPS actions (shown in the Restore menu area).
    restore_status:            Option<String>,
    /// Lunar Magic-style custom user toolbar (LM v2.31+): second toolbar
    /// strip built from `usertoolbar.txt`, with external scripting buttons,
    /// internal `LM_…` commands, and keyboard shortcuts.
    user_toolbar:              UserToolbarState,
    /// Lunar Magic v1.91 "Check Object Placement on Save" (Options menu).
    /// Persisted per-user (`$HOME/.smw-editor-options.json`); never the ROM.
    check_placement_on_save:   bool,
    /// A save deferred by the placement-warning dialog, awaiting the user's
    /// answer ("Save anyway" resumes it; "Cancel" drops it).
    pending_placement_warning: Option<PendingPlacementWarning>,
    /// One-shot: set when the user answered "Save anyway", consumed by the
    /// next save so the check does not immediately re-trigger the dialog.
    placement_save_confirmed:  bool,
}

/// A save deferred by the LM v1.91 placement-warning dialog.
#[derive(Debug, Clone)]
enum DeferredSave {
    /// Normal save (Ctrl+S / File > Save ROM).
    Save,
    /// Save As with the picked destination.
    SaveAs(PathBuf),
}

/// Placement issues found by the LM v1.91 on-save check, awaiting the user's
/// answer in the warning dialog.
#[derive(Debug, Clone)]
struct PendingPlacementWarning {
    issues:   Vec<PlacementIssue>,
    deferred: DeferredSave,
}

/// An IPS patch the user picked, applied in-memory to the current ROM image,
/// waiting for the user to confirm installing it.
struct PendingIpsApply {
    patch_path:    PathBuf,
    patched_bytes: Vec<u8>,
    bytes_changed: usize,
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
            show_expand_dialog: false,
            expand_target: 0,
            expand_status: None,
            png_export_dialog: FileDialog::new(),
            png_export_level: None,
            png_export_status: None,
            show_batch_export_dialog: false,
            batch_export_dir_dialog: FileDialog::new(),
            batch_from: "000".to_string(),
            batch_to: "1FF".to_string(),
            batch_out_dir: None,
            batch_include_l1: true,
            batch_include_l2: true,
            batch_include_sprites: true,
            batch_status: None,
            show_delete_levels_dialog: false,
            delete_levels_selected: Vec::new(),
            delete_levels_modified: Vec::new(),
            delete_levels_critical: Vec::new(),
            delete_levels_pending: None,
            delete_levels_status: None,
            show_level_addr_dialog: false,
            level_addr_text: String::new(),
            dialog_error: None,
            show_share_data_dialog: false,
            share_data_status: None,
            show_exit_scan_dialog: false,
            exit_scan: exit_scan_dialog::ExitScanUi::new(),
            show_resource_scan_dialog: false,
            resource_scan: resource_scan_dialog::ResourceScanUi::new(),
            show_exit_dialog: false,
            restore_manager: RestoreManager::new(),
            ips_apply_dialog: FileDialog::new(),
            show_restore_dialog: false,
            restore_point_name: String::new(),
            pending_revert: None,
            pending_ips_apply: None,
            pending_ips_export: None,
            restore_status: None,
            user_toolbar: UserToolbarState::load(),
            check_placement_on_save: EditorOptions::load().check_placement_on_save,
            pending_placement_warning: None,
            placement_save_confirmed: false,
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

        // Lunar Magic-style custom user toolbar (v2.31+): poll shortcuts first
        // so user-defined shortcuts win over built-ins, then render the strip
        // below the menu bar.
        let rom_path_str = self.rom_path.as_ref().map(|p| p.to_string_lossy().to_string());
        let has_rom = rom.is_some();
        let mut toolbar_actions = self.user_toolbar.poll_shortcuts(ctx, rom_path_str.as_deref(), has_rom);
        self.user_toolbar.reap_children();

        // Menu bar always on top.
        self.main_menu_bar(ctx, rom.as_ref());

        // Second toolbar strip built from usertoolbar.txt.
        toolbar_actions.extend(self.user_toolbar.show_strip(ctx, rom_path_str.as_deref(), has_rom));
        for action in toolbar_actions {
            match action {
                ToolbarAction::OpenWorldEditor => {
                    if let Some(r) = rom.as_ref() {
                        if let Some(path) = self.rom_path.clone() {
                            self.open_tool(UiWorldEditor::new(Arc::clone(&self.gl), Arc::clone(r), path));
                        } else {
                            self.save_error = Some("No ROM path available for emulator-backed overworld view.".into());
                        }
                    }
                }
                ToolbarAction::OpenDeleteLevels => self.open_delete_levels_dialog(ctx),
                ToolbarAction::OpenBatchPngExport => {
                    self.batch_from = "000".to_string();
                    self.batch_to = format!("{:03X}", LEVEL_COUNT - 1);
                    self.batch_include_l1 = true;
                    self.batch_include_l2 = true;
                    self.batch_include_sprites = true;
                    self.batch_status = None;
                    self.show_batch_export_dialog = true;
                }
            }
        }

        // User-toolbar parse/launch errors (capped at LM_DISPLAY_ERRORS).
        self.user_toolbar.show_error_window(ctx);

        // Open dialog.
        self.show_open_dialog(ctx);

        // Save As dialog (egui-native, no native file picker needed).
        self.show_save_as_dialog(ctx);

        // BPS export dialog.
        self.show_bps_export_dialog(ctx, rom.as_ref());

        // IPS export dialog.
        self.show_ips_export_dialog(ctx, rom.as_ref());

        // Single-level PNG export dialog (File > Export Level to PNG...).
        self.show_png_export_dialog(ctx);

        // Batch level-export dialog (File > Levels > Export Multiple Levels to Image Files...).
        if self.show_batch_export_dialog {
            self.batch_export_window(ctx);
        }
        self.batch_export_dir_dialog.update(ctx);
        if let Some(dir) = self.batch_export_dir_dialog.take_picked() {
            self.batch_out_dir = Some(dir);
            self.batch_status = None;
        }

        // Delete-levels dialog (File > Levels > Delete Levels from ROM...).
        if self.show_delete_levels_dialog {
            self.delete_levels_window(ctx);
        }
        if self.delete_levels_pending.is_some() {
            self.delete_levels_confirm_window(ctx);
        }
        // Share-data dialog (File > Levels > Share Data Between Levels to Save Space...).
        if self.show_share_data_dialog {
            self.share_data_window(ctx);
        }
        // Exit-scan dialog (Tools > Scan for Undefined Exits..., LM v1.50/v1.60 parity).
        if self.show_exit_scan_dialog {
            self.exit_scan_window(ctx);
        }
        // Resource-analysis dialog (Tools > Analyze Resources in Levels...,
        // LM v3.03/v3.20 parity).
        if self.show_resource_scan_dialog {
            self.resource_scan_window(ctx);
        }
        // IPS export same-directory warning (LM v1.80).
        self.show_ips_export_warning_dialog(ctx, rom.as_ref());

        // Apply IPS dialog (Restore menu).
        self.show_ips_apply_dialog(ctx);

        // Create Restore Point dialog (Restore menu).
        self.show_restore_point_dialog(ctx);

        // Revert-to-restore-point confirmation (Restore menu).
        self.show_revert_confirm_dialog(ctx);

        // Object-placement warning dialog (LM v1.91 "Check Object Placement
        // on Save"): deferred saves wait here for the user's answer.
        self.show_placement_warning_dialog(ctx);

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

        // Open Level from Address dialog (File > Open Level from Address...,
        // Lunar Magic v1.11 parity).
        if self.show_level_addr_dialog {
            let mut open = true;
            let mut confirmed = false;
            Window::new("Open Level From Address (in hex)").open(&mut open).resizable(false).show(ctx, |ui| {
                ui.label("PC address to open level (in hex)");
                let resp = ui.text_edit_singleline(&mut self.level_addr_text);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_level_addr_dialog = false;
                    }
                });
                if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    confirmed = true;
                }
            });
            if !open {
                self.show_level_addr_dialog = false;
            } else if confirmed {
                self.show_level_addr_dialog = false;
                self.confirm_open_level_address(rom.as_ref());
            }
        }

        // Generic dialog error toast.
        if let Some(err) = &self.dialog_error.clone() {
            let mut open = true;
            Window::new("Error").open(&mut open).show(ctx, |ui| {
                ui.label(err);
                if ui.button("OK").clicked() {
                    self.dialog_error = None;
                }
            });
            if !open {
                self.dialog_error = None;
            }
        }

        // PNG export status toast.
        if let Some(status) = &self.png_export_status.clone() {
            let mut open = true;
            Window::new("Level PNG Export").open(&mut open).show(ctx, |ui| {
                ui.label(status);
                if ui.button("OK").clicked() {
                    self.png_export_status = None;
                }
            });
            if !open {
                self.png_export_status = None;
            }
        }

        // Expand ROM dialog.
        if self.show_expand_dialog {
            let mut open = true;
            let mut close_requested = false;
            let rom_len = rom.as_ref().map(|r| r.rom.bytes().len()).unwrap_or(0);
            let targets = expansion_targets(rom_len);
            Window::new("Expand ROM").open(&mut open).resizable(false).show(ctx, |ui| {
                if let Some(r) = rom.as_ref() {
                    ui.label(format!("Current size: {} ({})", format_size(rom_len), r.internal_header.map_mode));
                }
                ui.separator();
                if targets.is_empty() {
                    ui.label("This ROM is already at the maximum LoROM size (4 MB).");
                } else {
                    ui.label("Expand to:");
                    for t in &targets {
                        ui.radio_value(
                            &mut self.expand_target,
                            *t,
                            format!("{} ({} Mbit)", format_size(*t), t / 0x2_0000),
                        );
                    }
                    ui.separator();
                    ui.label(
                        "Appends $FF-filled banks and updates the internal header\n\
                         (ROM size byte + checksum). A .bak backup of the original\n\
                         file is kept next to the ROM. Unsaved edits are saved first.",
                    )
                    .on_hover_text("Same layout Lunar Magic produces for LoROM expansion");
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let can_expand = !targets.is_empty();
                    if ui.add_enabled(can_expand, Button::new("Expand")).clicked() {
                        let ctx2 = ctx.clone();
                        self.perform_rom_expansion(&ctx2);
                    }
                    if ui.button("Cancel").clicked() {
                        close_requested = true;
                    }
                });
                if let Some(status) = &self.expand_status.clone() {
                    ui.separator();
                    ui.label(status);
                }
            });
            if !open || close_requested {
                self.show_expand_dialog = false;
                self.expand_status = None;
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
        self.check_for_save_requests(ctx);

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
                // Capture the original-ROM reference copy for the Restore menu.
                self.restore_manager.open_rom(&path);
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
        let Some(path) = self.rom_path.clone() else {
            self.save_error = Some("No ROM path — open a ROM first.".into());
            return;
        };
        let rom: Option<Arc<SmwRom>> = ctx.data(|d| d.get_temp(Id::new("rom")));
        let Some(_) = rom else {
            self.save_error = Some("No ROM loaded.".into());
            return;
        };
        // Lunar Magic v1.91 "Check Object Placement on Save": check the
        // current tab edit states before writing; warn (don't block) when
        // anything sits outside the level boundaries.
        if !self.placement_check_passes(DeferredSave::Save) {
            return;
        }
        self.maybe_auto_restore_point(&path);
        match self.write_rom_to_path(&path, &path) {
            Ok(()) => {
                if let Err(e) = self.reload_rom_into_context(ctx, &path) {
                    self.save_error = Some(format!("Saved ROM, but reload failed: {e}"));
                } else {
                    log::info!("Saved ROM to {}", path.display());
                }
            }
            Err(e) => self.save_error = Some(format!("Save failed: {e}")),
        }
    }

    /// Collect placement issues from every open tab's current (unsaved) edit
    /// state (LM v1.91 "Check Object Placement on Save").
    fn collect_placement_issues(&self) -> Vec<PlacementIssue> {
        self.dock_state.iter_all_tabs().flat_map(|(_, tab)| tab.placement_issues()).collect()
    }

    /// Run the LM v1.91 on-save placement check. Returns `true` when the save
    /// may proceed; returns `false` and stashes the issues in
    /// `pending_placement_warning` when the warning dialog must ask the user
    /// first. A "Save anyway" answer sets `placement_save_confirmed`, which
    /// is consumed here so the resumed save does not re-trigger the dialog.
    fn placement_check_passes(&mut self, deferred: DeferredSave) -> bool {
        if !self.check_placement_on_save {
            return true;
        }
        if self.placement_save_confirmed {
            self.placement_save_confirmed = false;
            return true;
        }
        let issues = self.collect_placement_issues();
        if issues.is_empty() {
            return true;
        }
        self.pending_placement_warning = Some(PendingPlacementWarning { issues, deferred });
        false
    }

    /// "Object Placement Warning" dialog (LM v1.91): lists what the on-save
    /// check found and lets the user save anyway or cancel. LM warns, it
    /// does not block.
    fn show_placement_warning_dialog(&mut self, ctx: &Context) {
        if self.pending_placement_warning.is_none() {
            return;
        }
        let mut open = true;
        let mut save_anyway = false;
        let mut cancel = false;
        let issues = self.pending_placement_warning.as_ref().map(|p| p.issues.clone()).unwrap_or_default();
        let heading = crate::placement_check::warning_heading(issues.len());
        Window::new("Object Placement Warning").open(&mut open).resizable(true).show(ctx, |ui| {
            ui.label(RichText::new(&heading));
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for issue in &issues {
                    ui.label(format!("⚠ {}", format_issue(issue)));
                }
            });
            ui.label(RichText::new(crate::placement_check::SAVE_KEEPS_HINT).small().italics());
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Save anyway").clicked() {
                    save_anyway = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if save_anyway {
            if let Some(pending) = self.pending_placement_warning.take() {
                self.placement_save_confirmed = true;
                match pending.deferred {
                    DeferredSave::Save => self.save_rom(ctx),
                    DeferredSave::SaveAs(dest) => self.finish_save_as(ctx, &dest),
                }
            }
        } else if !open || cancel {
            self.pending_placement_warning = None;
            self.placement_save_confirmed = false;
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

        self.save_as_dialog = FileDialog::new().initial_directory(initial_dir).default_file_name(&initial_name);
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

        self.bps_export_dialog = FileDialog::new().initial_directory(initial_dir).default_file_name(&initial_name);
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

        self.ips_export_dialog = FileDialog::new().initial_directory(initial_dir).default_file_name(&initial_name);
        self.ips_export_dialog.save_file();
    }

    fn show_save_as_dialog(&mut self, ctx: &Context) {
        self.save_as_dialog.update(ctx);
        if let Some(dest) = self.save_as_dialog.take_picked() {
            // LM v1.91 "Check Object Placement on Save" applies to Save As
            // too; the picked destination rides along in the deferred save.
            if !self.placement_check_passes(DeferredSave::SaveAs(dest.clone())) {
                return;
            }
            self.finish_save_as(ctx, &dest);
        }
    }

    fn finish_save_as(&mut self, ctx: &Context, dest: &std::path::Path) {
        let Some(src) = self.rom_path.clone() else {
            return;
        };
        match self.write_rom_to_path(&src, dest) {
            Ok(_) => {
                log::info!("Saved ROM as {}", dest.display());
                if let Err(e) = self.reload_rom_into_context(ctx, dest) {
                    self.save_error = Some(format!("Saved ROM As, but reload failed: {e}"));
                    return;
                }
                self.rom_path = Some(dest.to_path_buf());
            }
            Err(e) => self.save_error = Some(format!("Save As failed: {e}")),
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
        &self, _rom: &Arc<SmwRom>, original_rom_path: &std::path::Path, patch_dest: &std::path::Path,
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
        let patch = smwe_bps::create_patch(&original_bytes, &modified_bytes)?;

        // Write patch to file
        std::fs::write(patch_dest, patch)
            .with_context(|| format!("Failed to write BPS patch to {}", patch_dest.display()))?;

        Ok(())
    }

    fn show_ips_export_dialog(&mut self, ctx: &Context, rom: Option<&Arc<SmwRom>>) {
        self.ips_export_dialog.update(ctx);
        if let Some(patch_dest) = self.ips_export_dialog.take_picked() {
            // LM v1.80 warns when the patch lands next to the ROM.
            let same_dir =
                self.rom_path.as_ref().and_then(|r| r.parent()).zip(patch_dest.parent()).is_some_and(|(a, b)| a == b);
            if same_dir {
                self.pending_ips_export = Some(patch_dest);
            } else {
                self.run_ips_export(rom, &patch_dest);
            }
        }
    }

    /// Confirm handler for the Open Level from Address dialog (Lunar Magic
    /// v1.11 parity). Imports the Layer-1 object stream at the typed PC
    /// address into the first open level editor tab, opening a level editor
    /// first when none is open. The displayed level number stays the current
    /// ordinary slot; sprites, entrances and background are not loaded from
    /// the address; the next save inserts the imported Layer 1 into that
    /// slot via the normal save path.
    fn confirm_open_level_address(&mut self, rom: Option<&Arc<SmwRom>>) {
        let text = self.level_addr_text.trim().to_string();
        let hex = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(&text);
        let pc = match u32::from_str_radix(hex, 16) {
            Ok(pc) => pc,
            Err(_) => {
                self.dialog_error = Some(format!("\"{text}\" is not a valid hex address."));
                return;
            }
        };
        let Some(rom) = rom else {
            self.dialog_error = Some("No ROM loaded.".to_string());
            return;
        };

        let mut import_err: Option<anyhow::Error> = None;
        let mut handled = false;
        for (_, tab) in self.dock_state.iter_all_tabs_mut() {
            match tab.open_layer1_from_address(pc) {
                Ok(Some((objects, bytes))) => {
                    log::info!("Opened level from address 0x{pc:05X}: {objects} objects ({bytes} bytes)");
                    handled = true;
                    break;
                }
                Ok(None) => {}
                Err(e) => {
                    import_err = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = import_err {
            self.dialog_error = Some(format!("Could not open level from address 0x{pc:X}: {e:#}"));
        } else if !handled {
            let path = self.rom_path.clone().unwrap_or_default();
            match UiLevelEditor::new(Arc::clone(&self.gl), Arc::clone(rom), path) {
                Ok(mut editor) => match editor.open_layer1_from_address(pc) {
                    Ok(_) => self.open_tool(editor),
                    Err(e) => {
                        self.dialog_error = Some(format!("Could not open level from address 0x{pc:X}: {e:#}"));
                    }
                },
                Err(e) => self.dialog_error = Some(format!("Failed to open level editor: {e:#}")),
            }
        }
    }

    fn create_ips_patch(
        &self, _rom: &Arc<SmwRom>, original_rom_path: &std::path::Path, patch_dest: &std::path::Path,
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

    /// Read the ROM from disk with every open tab's unsaved edits merged in —
    /// shared by the PNG export actions so exports reflect on-screen edits
    /// (same merge the BPS/IPS exports do).
    fn rom_bytes_with_tab_edits(&self) -> anyhow::Result<Vec<u8>> {
        let Some(src) = self.rom_path.clone() else { anyhow::bail!("No ROM path — open a ROM first.") };
        let mut rom_bytes =
            std::fs::read(&src).with_context(|| format!("Failed to read ROM from {}", src.display()))?;
        let has_smc_header = rom_bytes.len() % 0x400 == 0x200;
        for (_, tab) in self.dock_state.iter_all_tabs() {
            tab.save_to_rom(&mut rom_bytes, has_smc_header)?;
        }
        Ok(rom_bytes)
    }

    /// File > Export Level to PNG... — exports the focused level-editor tab
    /// (falling back to the first open level editor), like LM exports the
    /// active level window.
    fn export_level_png(&mut self) {
        let level = self
            .dock_state
            .find_active_focused()
            .and_then(|(_, tab)| tab.level_number())
            .or_else(|| self.dock_state.iter_all_tabs().find_map(|(_, tab)| tab.level_number()));
        let Some(level) = level else {
            self.save_error = Some("No level editor tab is open — open a level first.".to_string());
            return;
        };
        let initial_dir = self.rom_path.as_ref().and_then(|p| p.parent()).map(|d| d.to_path_buf()).unwrap_or_default();
        self.png_export_level = Some(level);
        self.png_export_status = None;
        self.png_export_dialog =
            FileDialog::new().initial_directory(initial_dir).default_file_name(&level_export_filename(level));
        self.png_export_dialog.save_file();
    }

    fn show_png_export_dialog(&mut self, ctx: &Context) {
        self.png_export_dialog.update(ctx);
        let Some(png_dest) = self.png_export_dialog.take_picked() else {
            return;
        };
        let Some(level) = self.png_export_level else {
            return;
        };
        self.png_export_level = None;
        let result = (|| -> anyhow::Result<()> {
            let rom_bytes = self.rom_bytes_with_tab_edits()?;
            let png = level_png_bytes(&rom_bytes, level, &LevelPngOptions::default())?;
            std::fs::write(&png_dest, &png)
                .with_context(|| format!("Failed to write PNG to {}", png_dest.display()))?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.png_export_status = Some(format!("Exported level {level:03X} to {}", png_dest.display()));
                log::info!("Exported level {level:03X} PNG to {}", png_dest.display());
            }
            Err(e) => {
                self.png_export_status = Some(format!("Level export failed: {e}"));
            }
        }
    }

    /// File > Levels > Export Multiple Levels to Image Files... dialog.
    /// Mirrors LM v3.20: a hex level range plus output folder, one PNG per
    /// level named `level_XXX.png`.
    fn batch_export_window(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_requested = false;
        Window::new("Export Multiple Levels to Image Files")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Renders each level in the range as a PNG image,\none file per level (level_000.png … level_{:03X}.png).",
                    LEVEL_COUNT - 1
                ));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("From level (hex):");
                    ui.text_edit_singleline(&mut self.batch_from);
                    ui.label("To level (hex):");
                    ui.text_edit_singleline(&mut self.batch_to);
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.batch_include_l1, "Layer 1");
                    ui.checkbox(&mut self.batch_include_l2, "Layer 2");
                    ui.checkbox(&mut self.batch_include_sprites, "Sprites");
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Output folder:");
                    ui.label(
                        self.batch_out_dir
                            .as_ref()
                            .map(|d| d.display().to_string())
                            .unwrap_or_else(|| "(not chosen)".to_string()),
                    );
                    if ui.button("Choose...").clicked() {
                        let initial = self
                            .batch_out_dir
                            .clone()
                            .or_else(|| {
                                self.rom_path.as_ref().and_then(|p| p.parent()).map(|d| d.to_path_buf())
                            })
                            .unwrap_or_default();
                        self.batch_export_dir_dialog = FileDialog::new().initial_directory(initial);
                        self.batch_export_dir_dialog.pick_directory();
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    let can_export = self.batch_out_dir.is_some();
                    if ui.add_enabled(can_export, Button::new("Export")).clicked() {
                        self.perform_batch_export();
                    }
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
                if let Some(status) = &self.batch_status.clone() {
                    ui.separator();
                    ui.label(status);
                }
            });
        if !open || close_requested {
            self.show_batch_export_dialog = false;
        }
    }

    fn perform_batch_export(&mut self) {
        fn parse_hex(s: &str) -> Option<u16> {
            u16::from_str_radix(s.trim().trim_start_matches("0x").trim_start_matches('$').trim_start_matches('#'), 16)
                .ok()
        }
        let (from, to) = (parse_hex(&self.batch_from), parse_hex(&self.batch_to));
        let (Some(from), Some(to)) = (from, to) else {
            self.batch_status = Some("Invalid level range — enter hex numbers like 000 and 1FF.".to_string());
            return;
        };
        if from > to || to >= LEVEL_COUNT {
            self.batch_status = Some(format!("Range must satisfy 000 ≤ from ≤ to ≤ {:03X}.", LEVEL_COUNT - 1));
            return;
        }
        let Some(out_dir) = self.batch_out_dir.clone() else {
            self.batch_status = Some("Choose an output folder first.".to_string());
            return;
        };
        let opts = LevelPngOptions {
            include_layer1:  self.batch_include_l1,
            include_layer2:  self.batch_include_l2,
            include_sprites: self.batch_include_sprites,
        };
        let rom_bytes = match self.rom_bytes_with_tab_edits() {
            Ok(b) => b,
            Err(e) => {
                self.batch_status = Some(format!("Batch export failed: {e}"));
                return;
            }
        };
        let mut exported = 0u32;
        for level in from..=to {
            match level_png_bytes(&rom_bytes, level, &opts) {
                Ok(png) => {
                    let dest = out_dir.join(level_export_filename(level));
                    if let Err(e) = std::fs::write(&dest, &png) {
                        self.batch_status = Some(format!("Failed writing {}: {e}", dest.display()));
                        return;
                    }
                    exported += 1;
                }
                Err(e) => {
                    self.batch_status = Some(format!("Failed rendering level {level:03X}: {e}"));
                    return;
                }
            }
        }
        self.batch_status = Some(format!("Exported {exported} level PNGs to {}", out_dir.display()));
        log::info!("Batch-exported {exported} level PNGs to {}", out_dir.display());
    }

    // ── Delete Levels from ROM (LM v3.50 parity) ───────────────────────

    /// Open the Delete Levels dialog, refreshing the modified/critical
    /// classifications against the ROM as currently opened.
    fn open_delete_levels_dialog(&mut self, ctx: &Context) {
        let count = LEVEL_COUNT as usize;
        self.delete_levels_selected = vec![false; count];
        self.delete_levels_modified = vec![false; count];
        self.delete_levels_critical = vec![false; count];

        // Modified flags: compare the current image (with tab edits) against
        // the ROM-as-opened reference copy. Pointer relocation and in-place
        // block edits both count as modified.
        if let (Some(original), Ok(current)) = (self.restore_manager.original(), self.current_rom_image()) {
            let header_offset = usize::from(current.len() % 0x400 == 0x200) * 0x200;
            for level in 0..count {
                self.delete_levels_modified[level] = level_modified_vs(&current, original, level as u16, header_offset);
            }
        }

        // Gameplay-critical: the title/demo sequence levels plus every level
        // placed as a tile on the overworld map (deleting one leaves the
        // overworld tile loading the test level instead).
        for &level in GAMEPLAY_CRITICAL_LEVELS {
            self.delete_levels_critical[level as usize] = true;
        }
        let rom: Option<Arc<SmwRom>> = ctx.data(|d| d.get_temp(Project::rom_id()));
        if let Some(rom) = rom {
            let tiles = &rom.overworld.layer1_tiles;
            for idx in 0..tiles.len() {
                if let Some(level) = level_number_for_index(tiles, idx) {
                    self.delete_levels_critical[level as usize] = true;
                }
            }
        }

        self.delete_levels_status = None;
        self.delete_levels_pending = None;
        self.show_delete_levels_dialog = true;
    }

    fn delete_levels_window(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_requested = false;
        Window::new("Delete Levels from ROM")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    "Replaces each selected level's data with the vanilla test level\n\
                     and erases the old data blocks, reclaiming them as free space.\n\
                     A restore point is created first, and the ROM checksum is repaired.",
                );
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Quick select:");
                    if ui.button("All").clicked() {
                        self.delete_levels_selected.fill(true);
                    }
                    if ui.button("Modified").clicked() {
                        for i in 0..self.delete_levels_selected.len() {
                            self.delete_levels_selected[i] = self.delete_levels_modified[i];
                        }
                    }
                    if ui.button("Unmodified").clicked() {
                        for i in 0..self.delete_levels_selected.len() {
                            self.delete_levels_selected[i] = !self.delete_levels_modified[i];
                        }
                    }
                    if ui.button("None").clicked() {
                        self.delete_levels_selected.fill(false);
                    }
                });
                let selected_count = self.delete_levels_selected.iter().filter(|&&s| s).count();
                ui.label(format!("{selected_count} level(s) selected"));
                ui.label(
                    RichText::new("Yellow = modified since the ROM was opened · Red = gameplay-critical")
                        .small()
                        .italics(),
                );
                ui.separator();
                ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                    Grid::new("delete_levels_grid").num_columns(8).spacing([8.0, 2.0]).show(ui, |ui| {
                        for level in 0..LEVEL_COUNT as usize {
                            let mut label = RichText::new(format!("{level:03X}")).monospace();
                            if self.delete_levels_critical[level] {
                                label = label.color(Color32::from_rgb(255, 120, 120));
                            } else if self.delete_levels_modified[level] {
                                label = label.color(Color32::from_rgb(255, 205, 90));
                            }
                            ui.checkbox(&mut self.delete_levels_selected[level], label);
                            if level % 8 == 7 {
                                ui.end_row();
                            }
                        }
                    });
                });
                ui.separator();
                let critical_selected: Vec<u16> = (0..LEVEL_COUNT)
                    .filter(|&l| self.delete_levels_selected[l as usize] && self.delete_levels_critical[l as usize])
                    .collect();
                if !critical_selected.is_empty() {
                    let shown: Vec<String> =
                        critical_selected.iter().take(8).map(|l| format!("{l:03X}")).collect();
                    let more = if critical_selected.len() > 8 { ", ..." } else { "" };
                    ui.label(
                        RichText::new(format!(
                            "⚠ Deleting gameplay-critical level(s) {}{} can break the game\n(title/demo or overworld-placed levels).",
                            shown.join(", "),
                            more
                        ))
                        .color(Color32::from_rgb(255, 120, 120)),
                    );
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            selected_count > 0,
                            Button::new(format!("Delete {selected_count} level(s)")),
                        )
                        .clicked()
                    {
                        self.delete_levels_pending = Some(
                            (0..LEVEL_COUNT)
                                .filter(|&l| self.delete_levels_selected[l as usize])
                                .collect(),
                        );
                    }
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
                if let Some(status) = &self.delete_levels_status.clone() {
                    ui.separator();
                    ui.label(status);
                }
            });
        if !open || close_requested {
            self.show_delete_levels_dialog = false;
            self.delete_levels_pending = None;
        }
    }

    fn delete_levels_confirm_window(&mut self, ctx: &Context) {
        let Some(levels) = self.delete_levels_pending.clone() else { return };
        let mut confirmed = false;
        let mut cancelled = false;
        Window::new("Confirm Delete Levels").collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(format!("Delete {} level(s)?", levels.len()));
            ui.label(
                "Each level's Layer 1, sprite, and Layer 2 data is replaced with\n\
                 the vanilla test level. The old data blocks are erased and\n\
                 reclaimed as free space. A restore point is created first so\n\
                 this can be undone.",
            );
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Delete").clicked() {
                    confirmed = true;
                }
                if ui.button("Cancel").clicked() {
                    cancelled = true;
                }
            });
        });
        if confirmed {
            self.perform_delete_levels(ctx);
        } else if cancelled {
            self.delete_levels_pending = None;
        }
    }

    fn perform_delete_levels(&mut self, ctx: &Context) {
        let Some(levels) = self.delete_levels_pending.take() else { return };
        let mut bytes = match self.current_rom_image() {
            Ok(b) => b,
            Err(e) => {
                self.delete_levels_status = Some(format!("Could not read ROM image: {e:#}"));
                return;
            }
        };
        let header_offset = usize::from(bytes.len() % 0x400 == 0x200) * 0x200;
        // A restore point first, so the deletion can always be undone.
        self.restore_manager.create_point(format!("Before deleting {} level(s)", levels.len()), bytes.clone());
        match delete_levels(&mut bytes, &levels, header_offset) {
            Ok(report) => match self.install_rom_image(ctx, &bytes) {
                Ok(()) => {
                    log::info!(
                        "Deleted {} level(s); reclaimed {} bytes in {} erased block(s)",
                        report.deleted.len(),
                        report.bytes_reclaimed,
                        report.blocks_erased
                    );
                    self.delete_levels_status = Some(format!(
                        "Deleted {} level(s); reclaimed {} bytes in {} erased block(s).",
                        report.deleted.len(),
                        report.bytes_reclaimed,
                        report.blocks_erased
                    ));
                    self.show_delete_levels_dialog = false;
                    self.delete_levels_selected.fill(false);
                }
                Err(e) => {
                    self.delete_levels_status = Some(format!("Levels deleted, but ROM reload failed: {e:#}"));
                }
            },
            Err(e) => {
                self.delete_levels_status = Some(format!("Delete failed: {e}"));
            }
        }
    }

    /// File > Levels > Share Data Between Levels to Save Space... dialog.
    /// Mirrors LM v3.50: scan all levels for byte-identical data blocks and
    /// share one copy between them, reclaiming the freed blocks as free space.
    fn share_data_window(&mut self, ctx: &Context) {
        let mut open = true;
        let mut close_requested = false;
        let mut share_requested = false;
        Window::new("Share Data Between Levels to Save Space").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.label(
                "Scans all 512 levels for byte-identical Layer 1, sprite,\n\
                     and Layer 2 data blocks. Levels holding identical blocks\n\
                     are repointed at a single shared copy, and the freed\n\
                     blocks are erased — reclaiming them as free space.",
            );
            ui.separator();
            ui.label(
                "Sharing is invisible to the game: every level loads\n\
                     exactly the same data afterwards. A restore point is\n\
                     created first, so you can undo from the Restore menu.",
            );
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Share Data").clicked() {
                    share_requested = true;
                }
                if ui.button("Close").clicked() {
                    close_requested = true;
                }
            });
            if let Some(status) = &self.share_data_status.clone() {
                ui.separator();
                ui.label(status);
            }
        });
        if !open || close_requested {
            self.show_share_data_dialog = false;
            self.share_data_status = None;
        } else if share_requested {
            let ctx2 = ctx.clone();
            self.perform_share_data(&ctx2);
        }
    }

    /// Run the share pass on the current ROM image (unsaved tab edits are
    /// merged in first), install the result atomically, and reload.
    fn perform_share_data(&mut self, ctx: &Context) {
        let result = (|| -> anyhow::Result<String> {
            let mut rom_bytes = self.current_rom_image()?;
            let has_smc_header = rom_bytes.len() % 0x400 == 0x200;
            let header_offset = usize::from(has_smc_header) * 0x200;
            // Keep the pre-share image for the restore point; only snapshotted
            // when the pass actually changes something.
            let before = rom_bytes.clone();
            let report =
                share_data_between_levels(&mut rom_bytes, header_offset).map_err(|e| anyhow::anyhow!("{e}"))?;
            if report.groups_merged == 0 {
                return Ok("No duplicate level data found — nothing changed.".to_string());
            }
            self.restore_manager.create_point("Before share data".to_string(), before);
            self.install_rom_image(ctx, &rom_bytes)?;
            Ok(format!(
                "Shared data across {} level(s): {} duplicate group(s) merged, {} block(s) erased, {} bytes \
                 reclaimed as free space.",
                report.levels_shared, report.groups_merged, report.blocks_erased, report.bytes_reclaimed
            ))
        })();
        match result {
            Ok(status) => {
                self.share_data_status = Some(status.clone());
                log::info!("{status}");
            }
            Err(e) => {
                self.share_data_status = Some(format!("Share data failed: {e:#}"));
            }
        }
    }

    // ── Restore menu (LM v1.80 parity) ──────────────────────────────

    /// Full current ROM image: the file on disk with all unsaved tab edits
    /// merged in — the same base Save and the patch exporters use.
    fn current_rom_image(&self) -> anyhow::Result<Vec<u8>> {
        let Some(src) = self.rom_path.clone() else { anyhow::bail!("No ROM path — open a ROM first.") };
        let mut bytes = std::fs::read(&src).with_context(|| format!("Failed to read ROM from {}", src.display()))?;
        let has_smc_header = bytes.len() % 0x400 == 0x200;
        for (_, tab) in self.dock_state.iter_all_tabs() {
            tab.save_to_rom(&mut bytes, has_smc_header)?;
        }
        Ok(bytes)
    }

    /// Snapshot the pre-save file image as an automatic restore point when
    /// change-tracking is on. Called before every in-place save.
    fn maybe_auto_restore_point(&mut self, path: &Path) {
        if !self.restore_manager.auto_track_on_save {
            return;
        }
        match std::fs::read(path) {
            Ok(before) => self.restore_manager.auto_point_before_save(before),
            Err(e) => log::warn!("auto restore point skipped: {e:#}"),
        }
    }

    /// "Create Restore Point..." dialog (Restore menu).
    fn show_restore_point_dialog(&mut self, ctx: &Context) {
        if !self.show_restore_dialog {
            return;
        }
        let mut open = true;
        let mut close_requested = false;
        let mut create = false;
        Window::new("Create Restore Point").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.label("Snapshot the current ROM, including unsaved edits.");
            ui.text_edit_singleline(&mut self.restore_point_name);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Create").clicked() {
                    create = true;
                }
                if ui.button("Cancel").clicked() {
                    close_requested = true;
                }
            });
        });
        if create {
            let name = self.restore_point_name.trim().to_string();
            let name = if name.is_empty() { self.restore_manager.suggested_name() } else { name };
            match self.current_rom_image() {
                Ok(bytes) => {
                    self.restore_manager.create_point(name.clone(), bytes);
                    self.restore_status = Some(format!("Restore point \"{name}\" created."));
                    log::info!("Created restore point \"{name}\"");
                }
                Err(e) => self.save_error = Some(format!("Could not snapshot ROM: {e:#}")),
            }
            self.show_restore_dialog = false;
        } else if !open || close_requested {
            self.show_restore_dialog = false;
        }
    }

    /// "Revert to Restore Point" confirmation dialog.
    fn show_revert_confirm_dialog(&mut self, ctx: &Context) {
        let Some(index) = self.pending_revert else { return };
        let Some(name) = self.restore_manager.points().get(index).map(|p| p.name.clone()) else {
            self.pending_revert = None;
            return;
        };
        let unsaved = self.has_any_unsaved_changes();
        let mut open = true;
        let mut close_requested = false;
        let mut revert = false;
        Window::new("Revert to Restore Point").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.label(format!("Revert the ROM to \"{name}\"?"));
            if unsaved {
                ui.label(
                    RichText::new("Open editors have unsaved changes — reverting discards them.")
                        .color(Color32::YELLOW),
                );
            }
            ui.label(
                "The current file is kept as a .bak backup. All open editors will be\n\
                 closed and reopened on the reverted ROM.",
            );
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Revert").clicked() {
                    revert = true;
                }
                if ui.button("Cancel").clicked() {
                    close_requested = true;
                }
            });
        });
        if revert {
            self.pending_revert = None;
            self.perform_revert(ctx, index, &name);
        } else if !open || close_requested {
            self.pending_revert = None;
        }
    }

    fn perform_revert(&mut self, ctx: &Context, index: usize, name: &str) {
        let Some(bytes) = self.restore_manager.revert_bytes(index).map(<[u8]>::to_vec) else {
            self.save_error = Some("Restore point no longer exists.".to_string());
            return;
        };
        match self.install_rom_image(ctx, &bytes) {
            Ok(()) => {
                self.restore_status = Some(format!("Reverted to \"{name}\"."));
                log::info!("Reverted ROM to restore point \"{name}\"");
            }
            Err(e) => self.save_error = Some(format!("Revert failed: {e:#}")),
        }
    }

    /// Write a full ROM image to the open ROM's path (`.bak` backup), reload
    /// it into the context, then close every open editor tab — tabs hold
    /// `Arc<SmwRom>`s parsed from the old image — and reopen the level
    /// editor on the new image.
    fn install_rom_image(&mut self, ctx: &Context, bytes: &[u8]) -> anyhow::Result<()> {
        let path = self.rom_path.clone().context("No ROM is open.")?;
        Self::atomic_write_with_backup(&path, bytes)?;
        self.reload_rom_into_context(ctx, &path)?;
        self.dock_state = DockState::new(vec![]);
        let rom: Option<Arc<SmwRom>> = ctx.data(|d| d.get_temp(Project::rom_id()));
        let Some(rom) = rom else { anyhow::bail!("Reloaded ROM missing from context") };
        match UiLevelEditor::new(Arc::clone(&self.gl), rom, path) {
            Ok(editor) => self.open_tool(editor),
            Err(e) => self.save_error = Some(format!("ROM installed, but the level editor failed to reopen: {e:#}")),
        }
        Ok(())
    }

    /// "Apply IPS Patch..." (Restore menu): pick a `.ips` file.
    fn pick_ips_to_apply(&mut self) {
        let initial_dir = self
            .rom_path
            .as_deref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        self.ips_apply_dialog =
            FileDialog::new().initial_directory(initial_dir).add_file_filter_extensions("IPS patches", vec!["ips"]);
        self.ips_apply_dialog.pick_file();
    }

    /// File-dialog pump for Apply IPS; on pick, apply the patch in-memory and
    /// stage the confirmation dialog.
    fn show_ips_apply_dialog(&mut self, ctx: &Context) {
        self.ips_apply_dialog.update(ctx);
        if let Some(patch_path) = self.ips_apply_dialog.take_picked() {
            match self.preview_ips_apply(&patch_path) {
                Ok(pending) => self.pending_ips_apply = Some(pending),
                Err(e) => self.save_error = Some(format!("Could not read IPS patch: {e:#}")),
            }
        }
        self.show_ips_apply_confirm_dialog(ctx);
    }

    /// Apply the picked patch to the current ROM image in memory so the
    /// confirmation dialog can show exactly what will change.
    fn preview_ips_apply(&self, patch_path: &Path) -> anyhow::Result<PendingIpsApply> {
        let patch_bytes =
            std::fs::read(patch_path).with_context(|| format!("Failed to read IPS patch {}", patch_path.display()))?;
        let current = self.current_rom_image()?;
        let patched_bytes = smwe_ips::apply_patch(&current, &patch_bytes).map_err(|e| anyhow::anyhow!("{e}"))?;
        let common = current.len().min(patched_bytes.len());
        let mut bytes_changed = current.iter().zip(patched_bytes.iter()).take(common).filter(|(a, b)| a != b).count();
        bytes_changed += current.len().abs_diff(patched_bytes.len());
        Ok(PendingIpsApply { patch_path: patch_path.to_path_buf(), patched_bytes, bytes_changed })
    }

    /// Confirmation dialog showing what the staged patch will do.
    fn show_ips_apply_confirm_dialog(&mut self, ctx: &Context) {
        let Some(pending) = self.pending_ips_apply.as_ref() else { return };
        let patch_name = pending.patch_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bytes_changed = pending.bytes_changed;
        let old_len = self.current_image_len_for_preview(pending);
        let new_len = pending.patched_bytes.len();
        let mut open = true;
        let mut close_requested = false;
        let mut apply = false;
        Window::new("Apply IPS Patch").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.label(format!("Patch: {patch_name}"));
            ui.label(format!(
                "{bytes_changed} byte(s) will change. ROM size: {} → {}.",
                format_size(old_len),
                format_size(new_len)
            ));
            ui.label(
                "The patched ROM is written to disk (a .bak backup is kept).\n\
                 All open editors will be closed and reopened on the patched ROM.",
            );
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    apply = true;
                }
                if ui.button("Cancel").clicked() {
                    close_requested = true;
                }
            });
        });
        if apply {
            let pending = self.pending_ips_apply.take().expect("staged above");
            match self.install_rom_image(ctx, &pending.patched_bytes) {
                Ok(()) => {
                    self.restore_status = Some(format!("Applied {patch_name}: {bytes_changed} byte(s) changed."));
                    log::info!("Applied IPS patch {patch_name}");
                }
                Err(e) => self.save_error = Some(format!("Apply IPS failed: {e:#}")),
            }
        } else if !open || close_requested {
            self.pending_ips_apply = None;
        }
    }

    /// Length of the current ROM image for the apply-preview line.
    /// `current_rom_image` can fail (no ROM); fall back to the staged size.
    fn current_image_len_for_preview(&self, pending: &PendingIpsApply) -> usize {
        self.current_rom_image().map(|b| b.len()).unwrap_or(pending.patched_bytes.len())
    }

    /// LM v1.80 warns when an IPS patch is created in the same directory as
    /// the ROM (patchers tend to auto-apply same-folder patches to that ROM).
    fn show_ips_export_warning_dialog(&mut self, ctx: &Context, rom: Option<&Arc<SmwRom>>) {
        if self.pending_ips_export.is_none() {
            return;
        }
        let mut open = true;
        let mut close_requested = false;
        let mut create_anyway = false;
        Window::new("IPS Patch Location").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.label("The patch will be saved in the same folder as the ROM.");
            ui.label(
                "Patching tools often apply a same-folder patch to that ROM\n\
                 automatically — make sure this is what you want.",
            );
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Create Anyway").clicked() {
                    create_anyway = true;
                }
                if ui.button("Cancel").clicked() {
                    close_requested = true;
                }
            });
        });
        if create_anyway {
            let dest = self.pending_ips_export.take().expect("staged above");
            self.run_ips_export(rom, &dest);
        } else if !open || close_requested {
            self.pending_ips_export = None;
        }
    }

    /// Shared IPS-create path for File > Export IPS and Restore > Create IPS.
    fn run_ips_export(&mut self, rom: Option<&Arc<SmwRom>>, patch_dest: &Path) {
        let Some(rom) = rom else {
            self.save_error = Some("No ROM loaded.".into());
            return;
        };
        let Some(src) = self.rom_path.clone() else {
            self.save_error = Some("No ROM path — open a ROM first.".into());
            return;
        };
        match self.create_ips_patch(rom, &src, patch_dest) {
            Ok(_) => {
                log::info!("Exported IPS patch to {}", patch_dest.display());
                self.restore_status = Some(format!("IPS patch written to {}", patch_dest.display()));
            }
            Err(e) => self.save_error = Some(format!("IPS export failed: {e}")),
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
                        // Lunar Magic v1.11 parity: open a Layer-1 object
                        // stream from a raw PC address into the current level.
                        if ui.button("Open Level from Address...").clicked() {
                            self.show_level_addr_dialog = true;
                            self.level_addr_text.clear();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Expand ROM...").clicked() {
                            // Default to the largest available target, like Lunar Magic.
                            if let Some(r) = rom {
                                let current = r.rom.bytes().len();
                                self.expand_target = expansion_targets(current).into_iter().last().unwrap_or(0);
                            }
                            self.expand_status = None;
                            self.show_expand_dialog = true;
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
                        ui.separator();
                        if ui.button("Export Level to PNG...").clicked() {
                            self.export_level_png();
                            ui.close_menu();
                        }
                        ui.menu_button("Levels", |ui| {
                            if ui.button("Export Multiple Levels to Image Files...").clicked() {
                                self.batch_from = "000".to_string();
                                self.batch_to = format!("{:03X}", LEVEL_COUNT - 1);
                                self.batch_include_l1 = true;
                                self.batch_include_l2 = true;
                                self.batch_include_sprites = true;
                                self.batch_status = None;
                                self.show_batch_export_dialog = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Delete Levels from ROM...").clicked() {
                                self.open_delete_levels_dialog(ctx);
                                ui.close_menu();
                            }
                            if ui.button("Share Data Between Levels to Save Space...").clicked() {
                                self.share_data_status = None;
                                self.show_share_data_dialog = true;
                                ui.close_menu();
                            }
                        });
                    });
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                });

                // ── Restore (LM v1.80 parity: restore points + create/apply IPS) ──
                ui.menu_button("Restore", |ui| {
                    ui.add_enabled_ui(has_rom, |ui| {
                        if ui.button("Create Restore Point...").clicked() {
                            self.restore_point_name = self.restore_manager.suggested_name();
                            self.show_restore_dialog = true;
                            ui.close_menu();
                        }
                        let point_count = self.restore_manager.points().len();
                        ui.add_enabled_ui(point_count > 0, |ui| {
                            // Collect summaries first: the submenu closure borrows
                            // `self` mutably when a revert is picked.
                            let summaries: Vec<(usize, String, String)> = self
                                .restore_manager
                                .points()
                                .iter()
                                .enumerate()
                                .map(|(i, p)| (i, p.name.clone(), p.stamp()))
                                .collect();
                            ui.menu_button("Revert to Restore Point", |ui| {
                                for (i, name, stamp) in summaries {
                                    if ui.button(format!("{name}  ({stamp})")).clicked() {
                                        self.pending_revert = Some(i);
                                        ui.close_menu();
                                    }
                                }
                            });
                        });
                        ui.separator();
                        if ui.button("Create IPS Patch...").clicked() {
                            self.export_ips_patch();
                            ui.close_menu();
                        }
                        if ui.button("Apply IPS Patch...").clicked() {
                            self.pick_ips_to_apply();
                            ui.close_menu();
                        }
                        ui.separator();
                        ui.checkbox(
                            &mut self.restore_manager.auto_track_on_save,
                            "Track changes (restore point before each save)",
                        )
                        .on_hover_text(
                            "When on, a restore point of the ROM is captured automatically \
                             before every save, so any save can be undone from this menu.",
                        );
                        if let Some(status) = self.restore_status.clone() {
                            ui.separator();
                            ui.label(RichText::new(status).small().italics());
                        }
                    });
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
                    });
                });

                // ── Tools ──
                ui.menu_button("Tools", |ui| {
                    if ui.button("Address Converter").clicked() {
                        self.open_tool(UiAddressConverter::default());
                        ui.close_menu();
                    }
                    if ui.button("Scan for Undefined Exits...").clicked() {
                        self.open_exit_scan();
                        ui.close_menu();
                    }
                    if ui.button("Analyze Resources in Levels...").clicked() {
                        self.open_resource_scan();
                        ui.close_menu();
                    }
                });

                // ── Options (LM v1.91 parity) ──
                ui.menu_button("Options", |ui| {
                    if ui
                        .checkbox(&mut self.check_placement_on_save, "Check Object Placement on Save")
                        .on_hover_text(
                            "When enabled, saving to the ROM warns about objects and sprites \
                             placed outside the level boundaries (Lunar Magic v1.91).",
                        )
                        .changed()
                    {
                        EditorOptions { check_placement_on_save: self.check_placement_on_save }.save();
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

    /// Write `bytes` to `dest_path` atomically: keep a `.bak` backup of the
    /// previous contents (if any), write via a temp file + rename so a
    /// crash/full-disk mid-write can't corrupt the user's only copy.
    fn atomic_write_with_backup(dest_path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
        if dest_path.exists() {
            let bak_path = dest_path
                .with_extension(format!("{}.bak", dest_path.extension().and_then(|e| e.to_str()).unwrap_or("smc")));
            std::fs::copy(dest_path, &bak_path)
                .with_context(|| format!("Failed to back up {} to {}", dest_path.display(), bak_path.display()))?;
        }

        let dest_dir = dest_path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let tmp_path =
            dest_dir.join(format!(".{}.tmp", dest_path.file_name().and_then(|n| n.to_str()).unwrap_or("rom_save")));
        {
            let mut tmp_file = std::fs::File::create(&tmp_path)
                .with_context(|| format!("Failed to create temp file {}", tmp_path.display()))?;
            use std::io::Write;
            tmp_file.write_all(bytes).with_context(|| format!("Failed to write temp file {}", tmp_path.display()))?;
            tmp_file.sync_all().with_context(|| format!("Failed to flush temp file {}", tmp_path.display()))?;
        }
        std::fs::rename(&tmp_path, dest_path).with_context(|| {
            format!("Failed to move temp file {} into place at {}", tmp_path.display(), dest_path.display())
        })?;
        Ok(())
    }

    fn write_rom_to_path(&self, source_path: &std::path::Path, dest_path: &std::path::Path) -> anyhow::Result<()> {
        let mut rom_bytes =
            std::fs::read(source_path).with_context(|| format!("Failed to read ROM from {}", source_path.display()))?;
        let has_smc_header = rom_bytes.len() % 0x400 == 0x200;
        for (_, tab) in self.dock_state.iter_all_tabs() {
            tab.save_to_rom(&mut rom_bytes, has_smc_header)?;
        }

        Self::atomic_write_with_backup(dest_path, &rom_bytes)
    }

    /// File > Expand ROM... action: merge unsaved tab edits (like Save does),
    /// grow the image to `self.expand_target`, preserve any SMC header, and
    /// reload the project so the new space is visible everywhere.
    fn perform_rom_expansion(&mut self, ctx: &Context) {
        let Some(path) = self.rom_path.clone() else {
            self.expand_status = Some("No ROM is open.".to_string());
            return;
        };
        let target = self.expand_target;
        let result = (|| -> anyhow::Result<usize> {
            let mut rom_bytes =
                std::fs::read(&path).with_context(|| format!("Failed to read ROM from {}", path.display()))?;
            let has_smc_header = rom_bytes.len() % 0x400 == 0x200;
            for (_, tab) in self.dock_state.iter_all_tabs() {
                tab.save_to_rom(&mut rom_bytes, has_smc_header)?;
            }
            let (smc_header, body) = split_smc_header(&rom_bytes);
            let expanded = expand_rom(&Rom::new(body.to_vec())?, target)?;
            let mut out = Vec::with_capacity(target + smc_header.map(|h| h.len()).unwrap_or(0));
            if let Some(h) = smc_header {
                out.extend_from_slice(h);
            }
            out.extend_from_slice(expanded.bytes());
            Self::atomic_write_with_backup(&path, &out)?;
            self.reload_rom_into_context(ctx, &path)?;
            Ok(target)
        })();
        match result {
            Ok(new_size) => {
                self.expand_status = Some(format!(
                    "Expanded to {}. The new $FF space is now available to the free-space scanner.",
                    format_size(new_size)
                ));
            }
            Err(e) => {
                self.expand_status = Some(format!("Expansion failed: {e:#}"));
            }
        }
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

    fn check_for_save_requests(&mut self, ctx: &Context) {
        let mut should_save = false;
        for (_, tab) in self.dock_state.iter_all_tabs_mut() {
            if tab.take_save_request() {
                should_save = true;
            }
        }
        if should_save {
            if let Some(path) = &self.rom_path.clone() {
                self.maybe_auto_restore_point(path);
                if let Err(e) = self.write_rom_to_path(path, path) {
                    self.save_error = Some(format!("Save failed: {e}"));
                } else {
                    log::info!("Saved ROM to {}", path.display());
                    if let Err(e) = self.reload_rom_into_context(ctx, path) {
                        self.save_error = Some(format!("Saved ROM, but reload failed: {e}"));
                    }
                    for (_, tab) in self.dock_state.iter_all_tabs_mut() {
                        tab.on_save_succeeded();
                    }
                }
            }
        }
    }
}
