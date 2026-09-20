#![allow(clippy::enum_variant_names)]

use anyhow::Result;
use eframe::egui::Ui;
use egui::WidgetText;

pub trait DockableEditorTool {
    fn update(&mut self, ui: &mut Ui);
    fn title(&self) -> WidgetText;
    fn on_closed(&mut self) {}
    fn on_close_attempt_blocked(&mut self) {}
    fn save_to_rom(&self, _rom_bytes: &mut [u8], _has_smc_header: bool) -> Result<()> {
        Ok(())
    }
    /// Check if this tool is requesting a ROM save (and clear the flag)
    fn take_save_request(&mut self) -> bool {
        false
    }
    /// Check if this tool has unsaved changes
    fn has_unsaved_changes(&self) -> bool {
        false
    }
    /// Called by the main window after a ROM save completes successfully.
    /// Implementations should clear their unsaved-changes flag here.
    fn on_save_succeeded(&mut self) {}
    /// SMW translevel number this tab edits, if it is a level editor.
    /// Used by File > Export Level to PNG to pick the current level.
    fn level_number(&self) -> Option<u16> {
        None
    }
    fn placement_issues(&self) -> Vec<crate::placement_check::PlacementIssue> {
        Vec::new()
    }
    /// Ask a level-editor tab to open `level`, reusing its unsaved-changes
    /// confirmation flow. No-op for tabs that are not level editors. Used by
    /// Tools > Analyze Resources in Levels... for its jump links.
    fn request_level_jump(&mut self, _level: u16) {}
    /// Lunar Magic v1.11 "Open Level from Address": decode the Layer-1 object
    /// stream at the given headerless PC address into this tool. Sprites,
    /// entrances and background are intentionally not touched.
    /// Returns `Ok(None)` when this tool is not a level editor, otherwise
    /// `Ok(Some((object_count, bytes_consumed)))`.
    fn open_layer1_from_address(&mut self, _pc: u32) -> Result<Option<(usize, usize)>> {
        Ok(None)
    }
}
