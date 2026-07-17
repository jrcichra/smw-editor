//! Lunar Magic-style horizontal toolbar for the level editor.
//!
//! Mirrors the feel of Lunar Magic's main window: a top strip of icon buttons
//! for file/nav actions, the level-number box, edit-target and tool toggles,
//! layer-visibility toggles, and sub-editor launchers — plus a bottom status
//! bar. All of this drives the existing `UiLevelEditor` state; the left panel
//! keeps the tile/sprite palette and the selection inspector.

use egui::{vec2, Button, Color32, Key, Modifiers, RichText, Ui};
use egui_phosphor::regular as icon;
use smwe_widgets::value_switcher::{ValueSwitcher, ValueSwitcherButtons};

use super::UiLevelEditor;
use crate::ui::editing_mode::EditingMode;

/// Highlight color for a pressed/active toolbar button (matches `style::toggle_button`).
const ACTIVE_FILL: Color32 = Color32::from_rgb(70, 130, 200);

/// A fixed-size icon toolbar button with a hover tooltip. Returns `true` when clicked.
fn tbtn(ui: &mut Ui, glyph: &str, tip: &str, active: bool) -> bool {
    let btn = Button::new(RichText::new(glyph).size(16.0)).min_size(vec2(30.0, 26.0));
    let btn = if active { btn.fill(ACTIVE_FILL) } else { btn };
    ui.add(btn).on_hover_text(tip).clicked()
}

impl UiLevelEditor {
    pub(super) fn toolbar(&mut self, ui: &mut Ui) {
        self.handle_shortcuts(ui);
        ui.add_space(2.0);

        // ── Row 1: file · level number · zoom ────────────────────────────────
        ui.horizontal(|ui| {
            if tbtn(ui, icon::FLOPPY_DISK, "Save level to ROM (Ctrl+S)", false) {
                self.request_rom_save = true;
            }
            if tbtn(ui, icon::ARROW_CLOCKWISE, "Reload level from ROM (discards unsaved edits)", false) {
                self.load_level();
            }
            ui.separator();

            // The iconic Lunar Magic level-number box.
            let old_level = self.level_num;
            let changed = ui
                .add(
                    ValueSwitcher::new(&mut self.level_num, "Level", ValueSwitcherButtons::MinusPlus)
                        .range(0..=0x1FF)
                        .hexadecimal(3, false, true),
                )
                .changed();
            if changed {
                self.request_level_change(old_level);
            }
            ui.separator();

            if tbtn(ui, icon::MAGNIFYING_GLASS_MINUS, "Zoom out (Ctrl+-)", false) {
                self.zoom = (self.zoom - 0.25).max(1.0);
            }
            ui.label(RichText::new(format!("{:.0}%", self.zoom * 100.0)).monospace());
            if tbtn(ui, icon::MAGNIFYING_GLASS_PLUS, "Zoom in (Ctrl+=)", false) {
                self.zoom = (self.zoom + 0.25).min(3.0);
            }
        });

        ui.add_space(1.0);

        // ── Row 2: edit target · tool · view · editors ───────────────────────
        ui.horizontal(|ui| {
            // What is being edited — Lunar Magic's Layer 1 / Layer 2 / sprite toggles.
            let on_l1 = !self.edit_sprites && self.edit_layer == 1;
            let on_l2 = !self.edit_sprites && self.edit_layer == 2;
            if tbtn(ui, icon::SQUARES_FOUR, "Enable editing of Layer 1 objects (` toggles L1/L2)", on_l1) {
                self.set_edit_target(false, 1);
            }
            let l2_tip = if self.level_properties.has_layer2 {
                "Enable editing of Layer 2 (` toggles L1/L2)"
            } else {
                "Enable editing of Layer 2 — this level has no Layer 2 objects (` toggles L1/L2)"
            };
            if tbtn(ui, icon::STACK_SIMPLE, l2_tip, on_l2) {
                self.set_edit_target(false, 2);
            }
            if tbtn(ui, icon::BUG, "Enable editing of sprites", self.edit_sprites) {
                self.set_edit_target(true, self.edit_layer);
            }
            ui.separator();

            // Tool (editing mode).
            for (glyph, tip, mode) in [
                (icon::CURSOR, "Select [1]", EditingMode::Select),
                (icon::PENCIL_SIMPLE, "Insert / draw [2]", EditingMode::Draw),
                (icon::ERASER, "Erase [3]", EditingMode::Erase),
                (icon::EYEDROPPER, "Probe / pick tile [4]", EditingMode::Probe),
            ] {
                if tbtn(ui, glyph, tip, self.editing_mode == mode) {
                    self.editing_mode = mode;
                }
            }
            ui.separator();

            // Layer / overlay visibility toggles.
            if tbtn(ui, icon::GRID_FOUR, "Always show grid (F8)", self.always_show_grid) {
                self.always_show_grid = !self.always_show_grid;
            }
            if tbtn(ui, icon::SELECTION, "Show object overlay", self.show_object_overlay) {
                self.show_object_overlay = !self.show_object_overlay;
            }
            if tbtn(ui, icon::DIAMOND, "Show sprite overlay", self.show_sprite_overlay) {
                self.show_sprite_overlay = !self.show_sprite_overlay;
            }
            if tbtn(ui, icon::TEXT_T, "Show object labels", self.show_object_labels) {
                self.show_object_labels = !self.show_object_labels;
            }
            ui.separator();

            // Sub-editor launchers (toggle the floating editor windows).
            if tbtn(ui, icon::GEAR, "Change properties of level (header)", self.show_level_header) {
                self.show_level_header = !self.show_level_header;
            }
            if tbtn(ui, icon::GRID_NINE, "Edit 16×16 tile map (Map16)", self.show_map16_editor) {
                self.show_map16_editor = !self.show_map16_editor;
            }
            if tbtn(ui, icon::IMAGE, "Edit 8×8 tiles (GFX / ExGFX)", self.show_gfx_editor) {
                self.show_gfx_editor = !self.show_gfx_editor;
            }
            if tbtn(ui, icon::PALETTE, "Edit colors (palette)", self.show_palette_editor) {
                self.show_palette_editor = !self.show_palette_editor;
            }
            if tbtn(ui, icon::WRENCH, "Sprite behavior (Sprite Header Editor)", self.show_sprite_tweaker_editor) {
                self.show_sprite_tweaker_editor = !self.show_sprite_tweaker_editor;
            }
            if tbtn(ui, icon::CHAT, "Edit message box text", self.show_message_editor) {
                self.show_message_editor = !self.show_message_editor;
            }
            if tbtn(ui, icon::DOOR, "Secondary entrances", self.show_secondary_entrances) {
                self.show_secondary_entrances = !self.show_secondary_entrances;
            }
            if tbtn(ui, icon::CROWN, "Title screen / credits", self.show_title_credits_editor) {
                self.show_title_credits_editor = !self.show_title_credits_editor;
            }
        });

        ui.add_space(2.0);
    }

    pub(super) fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let mode = match self.editing_mode {
                EditingMode::Select => "SELECT",
                EditingMode::Draw => "INSERT",
                EditingMode::Erase => "ERASE",
                EditingMode::Probe => "PROBE",
                _ => "—",
            };
            ui.label(RichText::new(mode).strong().color(ACTIVE_FILL));
            ui.separator();

            let target = if self.edit_sprites { "Sprites".to_string() } else { format!("Layer {}", self.edit_layer) };
            ui.label(format!("Editing: {target}"));
            ui.separator();

            ui.monospace(format!("Level {:03X}", self.level_num));
            ui.separator();

            let (w, h) = self.level_properties.level_dimensions_in_tiles();
            ui.monospace(format!("{w}×{h} tiles · {} screens", self.level_properties.num_screens()));
            ui.separator();

            if let Some((x, y)) = self.selected_tile {
                ui.monospace(format!("Tile ({x}, {y})"));
            } else if self.edit_sprites {
                ui.monospace(format!("{} sprite(s) selected", self.selected_sprite_indices.len()));
            } else {
                ui.monospace(format!("{} object(s) selected", self.selected_object_indices.len()));
            }
        });
    }

    /// Switch what is being edited (Layer 1 / Layer 2 / sprites), clearing the
    /// current selection and forcing a preview refresh — matching the behavior
    /// the left panel used before these controls moved to the toolbar.
    fn set_edit_target(&mut self, sprites: bool, layer: u8) {
        self.edit_sprites = sprites;
        self.edit_layer = layer;
        self.selected_object_indices.clear();
        self.selected_sprite_indices.clear();
        self.preview_for = None;
    }

    /// Handle a level-number change from the toolbar, guarding unsaved edits with
    /// the same confirmation flow the left panel used.
    fn request_level_change(&mut self, old_level: u16) {
        if self.has_unsaved_changes() {
            self.show_unsaved_dialog = true;
            self.pending_level_num = Some(self.level_num);
            self.level_num = old_level; // restore until the user decides
        } else {
            self.load_level();
        }
    }

    /// Lunar Magic keyboard shortcuts that operate on the level editor:
    /// PageUp/PageDown step the level number, backtick toggles Layer 1/2, F8
    /// toggles the grid, and Ctrl -/= zoom out/in.
    fn handle_shortcuts(&mut self, ui: &mut Ui) {
        // Don't steal keys while a widget (e.g. the level-number box) is focused.
        if ui.ctx().memory(|m| m.focused().is_some()) {
            return;
        }

        let (mut lvl_next, mut lvl_prev, mut toggle_l12, mut toggle_grid, mut zoom_in, mut zoom_out) =
            (false, false, false, false, false, false);
        ui.input_mut(|i| {
            lvl_next = i.consume_key(Modifiers::NONE, Key::PageUp);
            lvl_prev = i.consume_key(Modifiers::NONE, Key::PageDown);
            toggle_l12 = i.consume_key(Modifiers::NONE, Key::Backtick);
            toggle_grid = i.consume_key(Modifiers::NONE, Key::F8);
            zoom_in = i.consume_key(Modifiers::COMMAND, Key::Equals) || i.consume_key(Modifiers::COMMAND, Key::Plus);
            zoom_out = i.consume_key(Modifiers::COMMAND, Key::Minus);
        });

        if lvl_next {
            let old = self.level_num;
            self.level_num = (self.level_num + 1).min(0x1FF);
            if self.level_num != old {
                self.request_level_change(old);
            }
        }
        if lvl_prev {
            let old = self.level_num;
            self.level_num = self.level_num.saturating_sub(1);
            if self.level_num != old {
                self.request_level_change(old);
            }
        }
        if toggle_l12 {
            self.set_edit_target(false, if self.edit_layer == 2 { 1 } else { 2 });
        }
        if toggle_grid {
            self.always_show_grid = !self.always_show_grid;
        }
        if zoom_in {
            self.zoom = (self.zoom + 0.25).min(3.0);
        }
        if zoom_out {
            self.zoom = (self.zoom - 0.25).max(1.0);
        }
    }
}
