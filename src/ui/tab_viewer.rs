use egui::{Ui, WidgetText};
use egui_dock::TabViewer;

use crate::ui::tool::DockableEditorTool;

pub struct EditorToolTabViewer;

impl TabViewer for EditorToolTabViewer {
    type Tab = Box<dyn DockableEditorTool>;

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.title()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        tab.update(ui);
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> bool {
        // Prevent closing if there are unsaved changes
        if tab.has_unsaved_changes() {
            log::info!("Cannot close {}: unsaved changes", tab.title().text());
            tab.on_close_attempt_blocked();
            return false;
        }
        tab.on_closed();
        log::info!("Closed {}", tab.title().text());
        true
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }
}
