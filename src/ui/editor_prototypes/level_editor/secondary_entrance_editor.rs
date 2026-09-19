use egui::{Context, Grid, ScrollArea, Slider};
use smwe_rom::level::secondary_entrance::{
    OverworldExit,
    OwExitKind,
    OwPlayerSwitch,
    SecondaryExitOptions,
    SECONDARY_ENTRANCE_COUNT_MAX,
    SECONDARY_ENTRANCE_COUNT_VANILLA,
};

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

// ── Index helpers ────────────────────────────────────────────────────────────

impl UiLevelEditor {
    /// Vanilla-format bytes for `idx`, or `None` when the index has no stored
    /// entry (indices ≥ 0x200 live in the editor's RATS block).
    fn se_bytes(&self, idx: u16) -> Option<[u8; 4]> {
        if (idx as usize) < SECONDARY_ENTRANCE_COUNT_VANILLA {
            self.secondary_entrance_data.get(idx as usize).copied()
        } else {
            self.secondary_exit_ext.extended_entry(idx)
        }
    }

    /// Mutable bytes for a *stored* entry; `None` for unstored extended indices.
    fn se_bytes_mut(&mut self, idx: u16) -> Option<&mut [u8; 4]> {
        if (idx as usize) < SECONDARY_ENTRANCE_COUNT_VANILLA {
            self.secondary_entrance_data.get_mut(idx as usize)
        } else {
            self.secondary_exit_ext.extended_entries.get_mut(&idx)
        }
    }

    /// All indices shown in the grid: the vanilla 0x200 plus any stored
    /// extended entries, filtered by the search box.
    fn se_row_indices(&self, filter: Option<u16>) -> Vec<u16> {
        let mut out: Vec<u16> = (0..SECONDARY_ENTRANCE_COUNT_VANILLA as u16).collect();
        out.extend(self.secondary_exit_ext.extended_entries.keys().copied());
        out.sort_unstable();
        out.dedup();
        if let Some(f) = filter {
            out.retain(|&idx| idx == f || self.se_bytes(idx).is_some_and(|b| se_destination_level(&b) == f));
        }
        out
    }

    /// Short badge string for the extended options on `idx` ("W", "OW", "→M",
    /// "FL", "FG").
    fn se_flag_badges(&self, idx: u16) -> String {
        let o = self.secondary_exit_ext.options_for(idx);
        let mut s = String::new();
        if o.water_level {
            s.push_str("W ");
        }
        if o.exit_to_overworld.is_some() {
            s.push_str("OW ");
        }
        if o.midway_redirect.is_some() {
            s.push_str("→M ");
        }
        if o.face_left {
            s.push_str("FL ");
        }
        if o.new_fg_bg_init {
            s.push_str("FG ");
        }
        s.pop();
        s
    }

    fn teleport_label(&self, t: u8) -> String {
        let e = self.secondary_exit_ext.teleport_table[t as usize];
        let sub = smwe_rom::overworld::SUBMAP_NAMES.get(e.submap as usize).copied().unwrap_or("???");
        format!("0x{t:02X} — {sub} ({}, {})", e.x, e.y)
    }

    /// Store `opts` for `idx` (removing the entry when it is default), marking
    /// the extended-data block dirty when anything changed.
    fn set_se_options(&mut self, idx: u16, opts: SecondaryExitOptions) {
        let old = self.secondary_exit_ext.options_for(idx);
        if old == opts {
            return;
        }
        if opts == SecondaryExitOptions::default() {
            self.secondary_exit_ext.options.remove(&idx);
        } else {
            self.secondary_exit_ext.options.insert(idx, opts);
        }
        self.secondary_exit_ext_dirty = true;
        self.mark_edited();
    }

    fn parse_se_index(text: &str) -> Option<u16> {
        let t = text.trim();
        let v = t
            .strip_prefix("0x")
            .or_else(|| t.strip_prefix("0X"))
            .and_then(|s| u16::from_str_radix(s, 16).ok())
            .or_else(|| t.parse::<u16>().ok())?;
        (v as usize).lt(&SECONDARY_ENTRANCE_COUNT_MAX).then_some(v)
    }
}

// ── UI ───────────────────────────────────────────────────────────────────────

impl UiLevelEditor {
    pub(super) fn secondary_entrance_editor_window(&mut self, ctx: &Context) {
        if !self.show_secondary_entrances {
            return;
        }
        let mut open = self.show_secondary_entrances;
        egui::Window::new("Secondary Entrances").open(&mut open).resizable(true).default_size([780.0, 620.0]).show(
            ctx,
            |ui| {
                ui.horizontal(|ui| {
                    ui.label("Filter:");
                    ui.text_edit_singleline(&mut self.secondary_entrance_search);
                    if ui.small_button("Clear").clicked() {
                        self.secondary_entrance_search.clear();
                    }
                    ui.separator();
                    // LM v2.50: type full values directly into the index combo.
                    ui.label("Go to:");
                    let goto = ui.text_edit_singleline(&mut self.se_goto_text);
                    if (goto.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        || ui.small_button("Go").clicked()
                    {
                        if let Some(idx) = Self::parse_se_index(&self.se_goto_text.clone()) {
                            self.selected_secondary_entrance = idx;
                        }
                    }
                });
                ui.label("Editing entrances will be saved with Ctrl+S.");
                ui.separator();

                let search = self.secondary_entrance_search.clone();
                let filter: Option<u16> = search
                    .trim()
                    .strip_prefix("0x")
                    .and_then(|s| u16::from_str_radix(s, 16).ok())
                    .or_else(|| search.trim().parse::<u16>().ok());

                let rows = self.se_row_indices(filter);
                let selected = self.selected_secondary_entrance;
                ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    Grid::new("se_grid").num_columns(9).spacing([8.0, 4.0]).striped(true).show(ui, |ui| {
                        // Header row
                        ui.strong("ID");
                        ui.strong("Dest Level");
                        ui.strong("Screen");
                        ui.strong("X");
                        ui.strong("Y");
                        ui.strong("FG Pos");
                        ui.strong("BG Pos");
                        ui.strong("Flags");
                        ui.strong("Jump");
                        ui.end_row();

                        let mut dirty = false;
                        for idx in rows {
                            let Some(b) = self.se_bytes(idx) else { continue };
                            let dest = se_destination_level(&b);

                            let is_sel = idx == selected;
                            if ui.selectable_label(is_sel, format!("{:03X}", idx)).clicked() {
                                self.selected_secondary_entrance = idx;
                            }

                            // Destination level
                            {
                                let mut v = dest as i32;
                                if ui
                                    .add(
                                        Slider::new(&mut v, 0..=0x1FF)
                                            .hexadecimal(3, false, true)
                                            .clamping(egui::SliderClamping::Always),
                                    )
                                    .changed()
                                {
                                    if let Some(slot) = self.se_bytes_mut(idx) {
                                        se_set_destination_level(slot, v as u16);
                                        dirty = true;
                                    }
                                }
                            }

                            // Screen
                            {
                                let mut v = se_screen(&b) as i32;
                                if ui.add(Slider::new(&mut v, 0..=31)).changed() {
                                    if let Some(slot) = self.se_bytes_mut(idx) {
                                        se_set_screen(slot, v as u8);
                                        dirty = true;
                                    }
                                }
                            }

                            // X
                            {
                                let mut v = se_x(&b) as i32;
                                if ui.add(Slider::new(&mut v, 0..=7)).changed() {
                                    if let Some(slot) = self.se_bytes_mut(idx) {
                                        let y = se_y(slot);
                                        se_set_xy(slot, v as u8, y);
                                        dirty = true;
                                    }
                                }
                            }

                            // Y
                            {
                                let mut v = se_y(&b) as i32;
                                if ui.add(Slider::new(&mut v, 0..=15)).changed() {
                                    if let Some(slot) = self.se_bytes_mut(idx) {
                                        let x = se_x(slot);
                                        se_set_xy(slot, x, v as u8);
                                        dirty = true;
                                    }
                                }
                            }

                            // FG initial pos
                            {
                                let mut v = se_fg_initial_pos(&b) as i32;
                                if ui.add(Slider::new(&mut v, 0..=3)).changed() {
                                    if let Some(slot) = self.se_bytes_mut(idx) {
                                        se_set_fg_initial_pos(slot, v as u8);
                                        dirty = true;
                                    }
                                }
                            }

                            // BG initial pos
                            {
                                let mut v = se_bg_initial_pos(&b) as i32;
                                if ui.add(Slider::new(&mut v, 0..=3)).changed() {
                                    if let Some(slot) = self.se_bytes_mut(idx) {
                                        se_set_bg_initial_pos(slot, v as u8);
                                        dirty = true;
                                    }
                                }
                            }

                            // LM v3.00 option badges
                            ui.weak(self.se_flag_badges(idx));

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

                ui.separator();
                self.se_extended_options_panel(ui);
            },
        );
        self.show_secondary_entrances = open;
    }

    /// LM v3.00 per-entrance options for the selected entrance: water-level
    /// flag, exit-to-overworld, midway-entrance redirect.
    fn se_extended_options_panel(&mut self, ui: &mut egui::Ui) {
        let idx = self.selected_secondary_entrance;
        ui.heading(format!("Entrance 0x{idx:03X} — extended options (LM v3.00)"));

        // Extended indices (≥ 0x200) need a stored entry before their bytes
        // can be edited in the grid.
        if (idx as usize) >= SECONDARY_ENTRANCE_COUNT_VANILLA && self.secondary_exit_ext.extended_entry(idx).is_none() {
            ui.horizontal(|ui| {
                ui.weak("No stored entry at this index (LM v2.50 expanded table).");
                if ui.button("Create entry").clicked() {
                    self.secondary_exit_ext.extended_entries.insert(idx, [0; 4]);
                    self.secondary_exit_ext_dirty = true;
                    self.mark_edited();
                }
            });
        }

        let mut opts = self.secondary_exit_ext.options_for(idx);

        // ── Water level ──
        if ui.checkbox(&mut opts.water_level, "Water level — the destination plays as a water level").changed() {
            self.set_se_options(idx, opts);
            return;
        }

        // ── Face left / new FG/BG init (LM v3.00) ──
        if ui.checkbox(&mut opts.face_left, "Face left — Mario faces the left direction on this entrance").changed() {
            self.set_se_options(idx, opts);
            return;
        }
        if ui
            .checkbox(
                &mut opts.new_fg_bg_init,
                "New FG/BG init system — FG relative to player, BG computed from FG/scroll/height",
            )
            .changed()
        {
            self.set_se_options(idx, opts);
            return;
        }

        // ── Exit to overworld ──
        let mut exit_ow = opts.exit_to_overworld.is_some();
        if ui.checkbox(&mut exit_ow, "Exit to overworld instead of entering a level").changed() {
            opts.exit_to_overworld = exit_ow.then(OverworldExit::default);
            self.set_se_options(idx, opts);
            return;
        }
        if let Some(mut ow) = opts.exit_to_overworld {
            ui.indent("se_exit_ow", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Exit:");
                    if ui.radio(ow.exit_kind == OwExitKind::Normal, "Normal").clicked() {
                        ow.exit_kind = OwExitKind::Normal;
                    }
                    if ui.radio(ow.exit_kind == OwExitKind::Secret, "Secret").clicked() {
                        ow.exit_kind = OwExitKind::Secret;
                    }
                    ui.separator();
                    ui.label("Player:");
                    egui::ComboBox::from_id_salt(("se_player", idx))
                        .selected_text(match ow.player {
                            OwPlayerSwitch::Keep => "Don't switch",
                            OwPlayerSwitch::Mario => "Mario",
                            OwPlayerSwitch::Luigi => "Luigi",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut ow.player, OwPlayerSwitch::Keep, "Don't switch");
                            ui.selectable_value(&mut ow.player, OwPlayerSwitch::Mario, "Mario");
                            ui.selectable_value(&mut ow.player, OwPlayerSwitch::Luigi, "Luigi");
                        });
                });
                ui.horizontal(|ui| {
                    let mut ev = ow.base_event as i32;
                    if ui
                        .add(
                            Slider::new(&mut ev, 0..=smwe_rom::overworld::OW_EVENT_COUNT as i32 - 1).text("Base event"),
                        )
                        .changed()
                    {
                        ow.base_event = ev as u8;
                    }
                    let mut tp = ow.teleport as i32;
                    if ui.add(Slider::new(&mut tp, 0..=0xFF).hexadecimal(2, false, true).text("Teleport")).changed() {
                        ow.teleport = tp as u8;
                    }
                });
                ui.weak(format!("Teleport target: {}", self.teleport_label(ow.teleport)));
                ui.weak("Edit teleport locations from the overworld editor toolbar.");
            });
            if ow != opts.exit_to_overworld.unwrap_or_default() {
                opts.exit_to_overworld = Some(ow);
                self.set_se_options(idx, opts);
                return;
            }
        }

        // ── Midway redirect ──
        let mut redirect = opts.midway_redirect.is_some();
        if ui.checkbox(&mut redirect, "Midway entrance redirects to another level's midway entrance").changed() {
            opts.midway_redirect = redirect.then_some(0);
            self.set_se_options(idx, opts);
            return;
        }
        if let Some(mut target) = opts.midway_redirect {
            ui.indent("se_midway", |ui| {
                ui.horizontal(|ui| {
                    let mut v = target as i32;
                    if ui
                        .add(Slider::new(&mut v, 0..=0x1FF).hexadecimal(3, false, true).text("Redirect to level"))
                        .changed()
                    {
                        target = v as u16;
                    }
                    if ui.small_button(format!("→ {:03X}", target)).clicked() {
                        self.jump_to_level(target);
                    }
                });
            });
            if Some(target) != opts.midway_redirect {
                opts.midway_redirect = Some(target);
                self.set_se_options(idx, opts);
                return;
            }
        }

        ui.separator();
        ui.weak(
            "Options above and entrances ≥ 0x200 are stored in the editor's RATS block \
             (SMWESEX2). In-game playback needs Lunar Magic's ASM hacks, which this \
             editor does not install.",
        );
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
