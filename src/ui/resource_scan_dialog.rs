//! Lunar Magic parity: Tools > "Analyze Resources in Levels..." (LM v3.03;
//! music-track reporting added in LM v3.20).
//!
//! Like "Scan for Undefined Exits", the scan walks all 512 levels through
//! the real emulator (for the Map16 tile walk), so it runs on a worker
//! thread behind a progress bar. The results window presents the report two
//! ways: by resource (pick a music track, sprite, Map16 tile, or GFX/ExGFX
//! file and see every level using it) and by level (a per-level summary
//! table). Double-clicking a level jumps it open in the level editor.
//! "Save Text Report..." writes the LM-style text report to a file.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
    Arc,
};

use egui::{ComboBox, Context, RichText, ScrollArea, Window};
use smwe_rom::{exgfx::BYPASS_SLOT_NAMES, music::format_music_track, SmwRom};

use crate::{
    project::Project,
    resource_scan::{format_text_report, scan_resources_with_progress, ResourceReport, ScanOptions},
    ui::{editor_prototypes::level_editor::UiLevelEditor, tool::DockableEditorTool, UiMainWindow},
};

/// Messages from the scan worker thread to the UI thread.
enum ResourceScanMsg {
    /// Number of levels scanned so far (out of 512).
    Progress(u32),
    /// The scan finished; `Err` carries a displayable message.
    Done(Result<ResourceReport, String>),
}

struct RunningScan {
    rx:      Receiver<ResourceScanMsg>,
    cancel:  Arc<AtomicBool>,
    scanned: u32,
}

enum ResourceScanState {
    Idle,
    Running(RunningScan),
    Done(ResourceReport),
    Failed(String),
}

/// Which side of the report the results window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ScanView {
    #[default]
    ByResource,
    ByLevel,
}

/// Resource class picker in the by-resource view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ResourceClass {
    #[default]
    Music,
    Sprites,
    Map16Tiles,
    GfxFiles,
    CustomPalettes,
}

impl ResourceClass {
    fn label(self) -> &'static str {
        match self {
            ResourceClass::Music => "Music tracks",
            ResourceClass::Sprites => "Sprites",
            ResourceClass::Map16Tiles => "Map16 tiles",
            ResourceClass::GfxFiles => "GFX / ExGFX files",
            ResourceClass::CustomPalettes => "Custom palettes",
        }
    }

    fn all() -> [ResourceClass; 5] {
        [
            ResourceClass::Music,
            ResourceClass::Sprites,
            ResourceClass::Map16Tiles,
            ResourceClass::GfxFiles,
            ResourceClass::CustomPalettes,
        ]
    }
}

/// One selectable resource in the by-resource view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceKey {
    Music(u8),
    Sprite(u8),
    Map16(u16),
    GfxFile(u16),
    CustomPalettes,
}

/// Display label for a resource key, e.g. `"Track 3: Castle"`, `"Sprite $3F"`.
fn resource_label(report: &ResourceReport, key: ResourceKey) -> String {
    match key {
        ResourceKey::Music(t) => format!("Track {}", format_music_track(t)),
        ResourceKey::Sprite(s) => format!("Sprite ${s:02X}"),
        ResourceKey::Map16(t) => format!("Tile ${t:04X}"),
        ResourceKey::GfxFile(f) => {
            let label = if f >= 0x80 { format!("ExGFX file ${f:03X}") } else { format!("GFX file ${f:02X}") };
            // Show which slots reference it anywhere in the report.
            let mut slots = Vec::new();
            for (i, name) in BYPASS_SLOT_NAMES.iter().enumerate() {
                if report.per_level.iter().any(|r| r.scanned && r.all_slot_files()[i] == f) {
                    slots.push(*name);
                }
            }
            if slots.is_empty() {
                label
            } else {
                format!("{label} ({})", slots.join("/"))
            }
        }
        ResourceKey::CustomPalettes => "Levels with a custom palette".to_string(),
    }
}

/// Write the LM-style text report to a user-picked `.txt` file.
/// Returns the status line for the window on success.
fn save_report_to_file(report: &ResourceReport) -> Result<String, String> {
    let Some(path) =
        rfd::FileDialog::new().add_filter("Text report", &["txt"]).set_file_name("smw-resource-report.txt").save_file()
    else {
        return Err(String::new());
    };
    std::fs::write(&path, format_text_report(report))
        .map(|()| format!("Report saved to {}", path.display()))
        .map_err(|e| format!("Could not write report: {e}"))
}

// -------------------------------------------------------------------------------------------------

/// State for the Tools > Analyze Resources in Levels window. Owned by
/// [`UiMainWindow`].
pub struct ResourceScanUi {
    state:            ResourceScanState,
    options:          ScanOptions,
    view:             ScanView,
    class:            ResourceClass,
    filter:           String,
    selected:         Option<ResourceKey>,
    level_filter:     String,
    /// Consumed by the main window: jump this level open in the level editor.
    pub pending_jump: Option<u16>,
    save_error:       Option<String>,
    save_ok:          Option<String>,
}

impl ResourceScanUi {
    pub fn new() -> Self {
        ResourceScanUi {
            state:        ResourceScanState::Idle,
            options:      ScanOptions::all(),
            view:         ScanView::ByResource,
            class:        ResourceClass::Music,
            filter:       String::new(),
            selected:     None,
            level_filter: String::new(),
            pending_jump: None,
            save_error:   None,
            save_ok:      None,
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.state, ResourceScanState::Running(_))
    }

    pub fn has_report(&self) -> bool {
        matches!(self.state, ResourceScanState::Done(_))
    }

    fn set_failed(&mut self, msg: String) {
        self.state = ResourceScanState::Failed(msg);
    }

    /// Start a scan over `rom_bytes` (already merged with unsaved tab edits
    /// by the caller). No-op while a scan is already running.
    pub fn start(&mut self, rom_bytes: Vec<u8>) {
        if self.is_running() {
            return;
        }
        let options = self.options;
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_flag = Arc::clone(&cancel);
        std::thread::Builder::new()
            .name("resource-scan".to_string())
            .spawn(move || {
                let mut last_sent = 0u32;
                let result = scan_resources_with_progress(&rom_bytes, options, &mut |n| {
                    if n - last_sent >= 4 || n >= 512 {
                        last_sent = n;
                        if tx.send(ResourceScanMsg::Progress(n)).is_err() {
                            return false;
                        }
                    }
                    !cancel_flag.load(Ordering::Relaxed)
                })
                .map_err(|e| e.to_string());
                let _ = tx.send(ResourceScanMsg::Done(result));
            })
            .expect("failed to spawn resource-scan thread");
        self.state = ResourceScanState::Running(RunningScan { rx, cancel, scanned: 0 });
        self.save_error = None;
        self.save_ok = None;
    }

    /// Drop a running scan; the worker thread notices the cancel flag (or the
    /// closed channel) and stops at the next level boundary.
    pub fn cancel(&mut self) {
        if let ResourceScanState::Running(running) = &self.state {
            running.cancel.store(true, Ordering::Relaxed);
        }
        self.state = ResourceScanState::Idle;
    }

    /// Drain pending worker messages. Called every frame while the window is
    /// open.
    fn poll(&mut self) {
        let running = match &mut self.state {
            ResourceScanState::Running(r) => r,
            _ => return,
        };
        while let Ok(msg) = running.rx.try_recv() {
            match msg {
                ResourceScanMsg::Progress(n) => running.scanned = n,
                ResourceScanMsg::Done(result) => {
                    self.state = match result {
                        Ok(report) => {
                            self.selected = None;
                            ResourceScanState::Done(report)
                        }
                        Err(e) if e == "scan cancelled" => ResourceScanState::Idle,
                        Err(e) => ResourceScanState::Failed(e),
                    };
                    return;
                }
            }
        }
    }

    fn summary_text(report: &ResourceReport) -> String {
        let mut s = format!("Scanned {} levels", report.levels_scanned);
        if report.levels_skipped > 0 {
            s.push_str(&format!(" ({} skipped: unparseable data)", report.levels_skipped));
        }
        s.push('.');
        if report.options.music {
            s.push_str(&format!(" {} music tracks in use.", report.used_music_tracks().len()));
        }
        if report.options.sprites {
            s.push_str(&format!(" {} distinct sprites.", report.used_sprites().len()));
        }
        if report.options.map16 {
            s.push_str(&format!(" {} distinct Map16 tiles.", report.used_map16_tiles().len()));
        }
        if report.options.gfx {
            s.push_str(&format!(" {} GFX/ExGFX files referenced.", report.used_gfx_files().len()));
            let unused = report.unused_exgfx_files();
            if !unused.is_empty() {
                s.push_str(&format!(
                    " {} installed ExGFX file{} unused.",
                    unused.len(),
                    if unused.len() == 1 { "" } else { "s" }
                ));
            }
        }
        if report.options.palettes {
            s.push_str(&format!(" {} levels with a custom palette.", report.levels_with_custom_palette().len()));
        }
        s
    }

    /// Levels using the currently selected resource.
    fn levels_for(report: &ResourceReport, key: ResourceKey) -> Vec<u16> {
        match key {
            ResourceKey::Music(t) => report.levels_using_music(t),
            ResourceKey::Sprite(s) => report.levels_using_sprite(s),
            ResourceKey::Map16(t) => report.levels_using_map16(t),
            ResourceKey::GfxFile(f) => report.levels_using_gfx_file(f),
            ResourceKey::CustomPalettes => report.levels_with_custom_palette(),
        }
    }

    /// One row in the by-level table.
    fn level_summary(report: &ResourceReport, level: u16) -> String {
        let Some(r) = report.per_level.get(level as usize) else {
            return "unknown level".to_string();
        };
        if !r.scanned {
            return "could not parse".to_string();
        }
        let mut parts = Vec::new();
        if report.options.music {
            parts.push(format!("music {}", format_music_track(r.music)));
        }
        if report.options.gfx {
            parts.push(format!(
                "FG1-3/BG1 {:02X}/{:02X}/{:02X}/{:02X} · SP1-4 {:02X}/{:02X}/{:02X}/{:02X}",
                r.fg_files[0],
                r.fg_files[1],
                r.fg_files[2],
                r.fg_files[3],
                r.sp_files[0],
                r.sp_files[1],
                r.sp_files[2],
                r.sp_files[3]
            ));
        }
        if report.options.sprites {
            parts.push(format!("{} sprites", r.sprites.len()));
        }
        if report.options.map16 {
            parts.push(if r.map16_ok {
                format!("{} Map16 tiles", r.map16.len())
            } else {
                "Map16 walk failed".to_string()
            });
        }
        if report.options.palettes && r.custom_palette {
            parts.push("custom palette".to_string());
        }
        parts.join(" · ")
    }

    /// The options form shown before the first scan.
    fn options_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut scan_now = false;
        ui.label(
            "Check which Map16 tiles, GFX/ExGFX files, sprites, and music tracks are used in which \
             levels (LM v3.03; music added in LM v3.20), then browse the results or save a text report.",
        );
        ui.separator();
        ui.label("Resource classes to scan:");
        ui.checkbox(&mut self.options.music, "Music tracks (per level + overworld submaps)");
        ui.checkbox(&mut self.options.sprites, "Sprites");
        ui.checkbox(&mut self.options.map16, "Map16 tiles (walks every level's block map — the slow part)");
        ui.checkbox(&mut self.options.gfx, "GFX / ExGFX files per level");
        ui.checkbox(&mut self.options.palettes, "Custom palettes");
        ui.separator();
        ui.label("The scan merges unsaved tab edits first, then walks all 512 levels.");
        ui.horizontal(|ui| {
            if ui.button("Scan now").clicked() {
                scan_now = true;
            }
        });
        scan_now
    }

    /// The results half of the window: view switcher + by-resource or
    /// by-level content + the report buttons. Returns whether to re-scan.
    fn results_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut rescan = false;
        let ResourceScanState::Done(report) = &self.state else {
            return false;
        };
        ui.label(Self::summary_text(report));
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("View:");
            ui.selectable_value(&mut self.view, ScanView::ByResource, "By resource");
            ui.selectable_value(&mut self.view, ScanView::ByLevel, "By level");
        });
        ui.separator();

        // Re-borrow through disjoint fields for the two views.
        let state = &self.state;
        let ResourceScanState::Done(report) = state else {
            return false;
        };
        match self.view {
            ScanView::ByResource => {
                Self::by_resource_ui(
                    ui,
                    report,
                    &mut self.class,
                    &mut self.filter,
                    &mut self.selected,
                    &mut self.pending_jump,
                );
            }
            ScanView::ByLevel => {
                Self::by_level_ui(ui, report, &mut self.level_filter, &mut self.pending_jump);
            }
        }

        ui.separator();
        if let Some(err) = &self.save_error {
            ui.label(RichText::new(err).color(egui::Color32::RED));
        }
        if let Some(ok) = &self.save_ok {
            ui.label(RichText::new(ok).color(egui::Color32::GREEN));
        }
        ui.horizontal(|ui| {
            if ui.button("Save Text Report...").clicked() {
                let ResourceScanState::Done(report) = &self.state else {
                    return;
                };
                match save_report_to_file(report) {
                    Ok(msg) => {
                        self.save_ok = Some(msg);
                        self.save_error = None;
                    }
                    Err(e) => {
                        self.save_error = if e.is_empty() { None } else { Some(e) };
                        self.save_ok = None;
                    }
                }
            }
            if ui.button("Re-scan").clicked() {
                rescan = true;
            }
        });
        rescan
    }

    /// By-resource view: pick a class, filter, pick a resource, see the
    /// levels using it. Double-click a level to open it in the level editor.
    #[allow(clippy::too_many_arguments)]
    fn by_resource_ui(
        ui: &mut egui::Ui, report: &ResourceReport, class: &mut ResourceClass, filter: &mut String,
        selected: &mut Option<ResourceKey>, pending_jump: &mut Option<u16>,
    ) {
        ui.horizontal(|ui| {
            ui.label("Resource class:");
            ComboBox::from_id_salt("resource_class").selected_text(class.label()).show_ui(ui, |ui| {
                for c in ResourceClass::all() {
                    ui.selectable_value(class, c, c.label());
                }
            });
            ui.label("Filter:");
            if ui.text_edit_singleline(filter).changed() {
                *selected = None;
            }
        });

        // Collect the filtered resource list (shared borrows only).
        let filter_lc = filter.trim().to_lowercase();
        let matches = |label: &str| filter_lc.is_empty() || label.to_lowercase().contains(&filter_lc);
        let mut resources = Vec::new();
        match *class {
            ResourceClass::Music => {
                for track in report.used_music_tracks() {
                    let key = ResourceKey::Music(track);
                    if matches(&resource_label(report, key)) {
                        resources.push(key);
                    }
                }
            }
            ResourceClass::Sprites => {
                for sprite in report.used_sprites() {
                    let key = ResourceKey::Sprite(sprite);
                    if matches(&resource_label(report, key)) {
                        resources.push(key);
                    }
                }
            }
            ResourceClass::Map16Tiles => {
                for tile in report.used_map16_tiles() {
                    let key = ResourceKey::Map16(tile);
                    if matches(&resource_label(report, key)) {
                        resources.push(key);
                    }
                }
            }
            ResourceClass::GfxFiles => {
                for file in report.used_gfx_files() {
                    let key = ResourceKey::GfxFile(file);
                    if matches(&resource_label(report, key)) {
                        resources.push(key);
                    }
                }
            }
            ResourceClass::CustomPalettes => {
                if !report.levels_with_custom_palette().is_empty() {
                    resources.push(ResourceKey::CustomPalettes);
                }
            }
        }

        let shown = resources.len().min(2000);
        ui.small(format!(
            "{} resource{} (showing {}{}) — double-click a level to open it in the level editor.",
            resources.len(),
            if resources.len() == 1 { "" } else { "s" },
            shown,
            if resources.len() > shown { ", filter to narrow" } else { "" }
        ));

        ui.columns(2, |cols| {
            ScrollArea::vertical().max_height(300.0).id_salt("resource_list").show(&mut cols[0], |ui| {
                for key in resources.iter().take(shown) {
                    let label = resource_label(report, *key);
                    if ui.selectable_label(*selected == Some(*key), label).clicked() {
                        *selected = Some(*key);
                    }
                }
            });
            ScrollArea::vertical().max_height(300.0).id_salt("resource_levels").show(
                &mut cols[1],
                |ui| match *selected {
                    None => {
                        ui.small("Select a resource to see the levels using it.");
                    }
                    Some(key) => {
                        let levels = Self::levels_for(report, key);
                        ui.small(format!(
                            "{} — used by {} level{}:",
                            resource_label(report, key),
                            levels.len(),
                            if levels.len() == 1 { "" } else { "s" }
                        ));
                        for level in levels {
                            if ui.button(format!("Level ${level:03X}")).double_clicked() {
                                *pending_jump = Some(level);
                            }
                        }
                    }
                },
            );
        });
    }

    /// By-level view: one summary row per level; double-click jumps to it.
    fn by_level_ui(
        ui: &mut egui::Ui, report: &ResourceReport, level_filter: &mut String, pending_jump: &mut Option<u16>,
    ) {
        ui.horizontal(|ui| {
            ui.label("Level filter (hex):");
            ui.text_edit_singleline(level_filter);
        });
        let filter = level_filter.trim().to_lowercase();
        let filter_ok = |level: u16| {
            filter.is_empty() || format!("{level:03x}").contains(&filter) || format!("${level:03x}").contains(&filter)
        };
        ui.small("Double-click a level to open it in the level editor.");
        ScrollArea::vertical().max_height(340.0).id_salt("resource_by_level").show(ui, |ui| {
            for level in 0..512u16 {
                if !filter_ok(level) {
                    continue;
                }
                let summary = Self::level_summary(report, level);
                ui.horizontal(|ui| {
                    if ui.button(format!("${level:03X}")).double_clicked() {
                        *pending_jump = Some(level);
                    }
                    ui.label(summary);
                });
            }
        });
    }
}

// -------------------------------------------------------------------------------------------------

impl UiMainWindow {
    /// Tools > Analyze Resources in Levels... — start the scan (ROM bytes
    /// with unsaved tab edits merged, like the PNG export) and open the
    /// window.
    pub(crate) fn open_resource_scan(&mut self) {
        self.show_resource_scan_dialog = true;
        if !self.resource_scan.is_running() && !self.resource_scan.has_report() {
            match self.rom_bytes_with_tab_edits() {
                Ok(bytes) => self.resource_scan.start(bytes),
                Err(e) => self.resource_scan.set_failed(format!("Could not read ROM: {e:#}")),
            }
        }
    }

    /// Re-run the scan over the current ROM image (with tab edits merged).
    fn rescan_resources(&mut self) {
        match self.rom_bytes_with_tab_edits() {
            Ok(bytes) => self.resource_scan.start(bytes),
            Err(e) => self.resource_scan.set_failed(format!("Could not read ROM: {e:#}")),
        }
    }

    /// Consume a pending level jump: send it to the first open level editor
    /// tab (reusing its unsaved-changes flow), or open a fresh level editor
    /// when none is open.
    fn consume_resource_scan_jump(&mut self, ctx: &Context, level: u16) {
        for (_, tab) in self.dock_state.iter_all_tabs_mut() {
            if tab.level_number().is_some() {
                tab.request_level_jump(level);
                return;
            }
        }
        // No level editor open: open one on the target level.
        let rom: Option<Arc<SmwRom>> = ctx.data(|d| d.get_temp(Project::rom_id()));
        let Some(rom) = rom else {
            self.save_error = Some("No ROM is open.".to_string());
            return;
        };
        let path = self.rom_path.clone().unwrap_or_default();
        match UiLevelEditor::new(Arc::clone(&self.gl), rom, path) {
            Ok(mut editor) => {
                editor.request_level_jump(level);
                self.open_tool(editor);
            }
            Err(e) => self.save_error = Some(format!("Failed to open level editor: {e:#}")),
        }
    }

    pub(crate) fn resource_scan_window(&mut self, ctx: &Context) {
        // Pick up any double-clicked level from the results first so the
        // jump happens even if the window closes this frame.
        if let Some(level) = self.resource_scan.pending_jump.take() {
            self.consume_resource_scan_jump(ctx, level);
        }
        self.resource_scan.poll();
        let mut open = true;
        let mut start_scan = false;
        let mut rescan = false;
        Window::new("Analyze Resources in Levels")
            .collapsible(false)
            .resizable(true)
            .default_size([640.0, 560.0])
            .show(ctx, |ui| {
                match &self.resource_scan.state {
                    ResourceScanState::Idle => {
                        if self.resource_scan.options_ui(ui) {
                            start_scan = true;
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Close").clicked() {
                                open = false;
                            }
                        });
                    }
                    ResourceScanState::Running(running) => {
                        let scanned = running.scanned;
                        ui.label(format!("Scanning levels… {scanned} / 512"));
                        ui.add(egui::ProgressBar::new(scanned as f32 / 512.0).show_percentage());
                        ui.label(
                            "Each level is decompressed through the real emulator; this takes a couple of minutes.",
                        );
                        // Keep the progress bar moving while the worker runs.
                        ctx.request_repaint();
                        if ui.button("Cancel").clicked() {
                            self.resource_scan.cancel();
                        }
                    }
                    ResourceScanState::Failed(msg) => {
                        ui.label(RichText::new(format!("Scan failed: {msg}")).color(egui::Color32::RED));
                        ui.horizontal(|ui| {
                            if ui.button("Re-scan").clicked() {
                                rescan = true;
                            }
                            if ui.button("Close").clicked() {
                                open = false;
                            }
                        });
                    }
                    ResourceScanState::Done(_) => {
                        if self.resource_scan.results_ui(ui) {
                            rescan = true;
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Close").clicked() {
                                open = false;
                            }
                        });
                    }
                }
            });
        if start_scan || rescan {
            self.rescan_resources();
        }
        if !open {
            self.show_resource_scan_dialog = false;
        }
    }
}
