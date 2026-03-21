use egui::{vec2, Slider, Ui};
use smwe_widgets::value_switcher::{ValueSwitcher, ValueSwitcherButtons};

use super::UiLevelEditor;

impl UiLevelEditor {
    pub(super) fn left_panel(&mut self, ui: &mut Ui) {
        ui.add_space(ui.spacing().item_spacing.y);
        ui.group(|ui| {
            ui.allocate_space(vec2(ui.available_width(), 0.));
            self.controls_panel(ui);
        });
    }

    fn controls_panel(&mut self, ui: &mut Ui) {
        let level_changed = {
            let switcher = ValueSwitcher::new(&mut self.level_num, "Level", ValueSwitcherButtons::MinusPlus)
                .range(0..=0x1FF)
                .hexadecimal(3, false, true);
            ui.add(switcher).changed()
        };
        if level_changed {
            self.load_level();
        }

        ui.add(Slider::new(&mut self.zoom, 1.0..=3.0).step_by(0.25).text("Zoom"));
        ui.checkbox(&mut self.always_show_grid, "Always show grid");
        ui.checkbox(&mut self.show_object_overlay, "Show object overlay");
        ui.checkbox(&mut self.show_object_labels, "Show object labels");

        ui.separator();
        ui.label(format!("Level {:03X}", self.level_num));
        let is_vertical = {
            let props = &self.level_properties;
            ui.label(format!("Mode: {:02X}  GFX: {:X}", props.level_mode, props.fg_bg_gfx));
            ui.label(format!("Music: {}  Timer: {}", props.music, props.timer));
            ui.label(if props.is_vertical { "Vertical" } else { "Horizontal" });
            ui.label(format!("Screens: {}", props.num_screens()));
            let (w, h) = props.level_dimensions_in_tiles();
            ui.label(format!("Size: {}x{} tiles", w, h));
            props.is_vertical
        };

        ui.separator();

        // Selected tile info
        if let Some((x, y)) = self.selected_tile {
            ui.label(format!("Selected: ({x}, {y})"));
            if let Some(block_id) = self.block_id_at(x, y) {
                ui.monospace(format!("  Block ID: {block_id:#04X}"));
                let screen = if is_vertical { y / 512 } else { x / 256 };
                ui.monospace(format!("  Screen: {screen:X}"));
            }
        } else {
            ui.label("Selected: (none)");
        }
    }
}
