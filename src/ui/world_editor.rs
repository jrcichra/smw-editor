//! World Map Editor UI
//!
//! Placeholder - needs to be implemented from scratch.

use std::sync::Arc;

use egui::*;
use smwe_rom::SmwRom;

use crate::ui::tool::DockableEditorTool;

// ── Constants ─────────────────────────────────────────────────────────────────

const TILE_PX: f32 = 8.0;

// ── Struct ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct WorldEditor {
    pub rom: Arc<SmwRom>,
    pub submap: usize,
}

impl WorldEditor {
    pub fn new(rom: Arc<SmwRom>) -> Self {
        Self { rom, submap: 0 }
    }
}

impl DockableEditorTool for WorldEditor {
    fn title(&self) -> WidgetText {
        "World Map Editor".into()
    }

    fn update(&mut self, ui: &mut Ui) {
        ui.label("World Map Editor - Placeholder");
        ui.label("This editor needs to be implemented from scratch.");
        ui.label(&format!("ROM loaded: {} levels", self.rom.levels.len()));
    }
}
