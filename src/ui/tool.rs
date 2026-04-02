#![allow(clippy::enum_variant_names)]

use anyhow::Result;
use eframe::egui::Ui;
use egui::WidgetText;

pub trait DockableEditorTool {
    fn update(&mut self, ui: &mut Ui);
    fn title(&self) -> WidgetText;
    fn on_closed(&mut self) {}
    fn save_to_rom(&self, _rom_bytes: &mut [u8], _has_smc_header: bool) -> Result<()> {
        Ok(())
    }
}
