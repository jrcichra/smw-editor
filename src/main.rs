use std::{cell::RefCell, env, rc::Rc};

use eframe::{NativeOptions, Renderer};
use egui::{vec2, ViewportBuilder};
use smw_editor::{
    project::{Project, ProjectRef},
    ui::UiMainWindow,
};

fn main() -> eframe::Result<()> {
    log4rs::init_file("log4rs.yaml", Default::default()).expect("Failed to initialize log4rs");

    // In WSL environments the Wayland socket exists but is non-functional,
    // causing an immediate crash. Force X11 and software rendering if we detect WSL.
    if std::fs::exists("/proc/sys/fs/binfmt_misc/WSLInterop").unwrap_or(false) {
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
    }

    let project = dev_open_rom();
    let native_options = NativeOptions {
        renderer: Renderer::Glow,
        viewport: ViewportBuilder::default().with_min_inner_size(vec2(1280., 720.)),
        ..NativeOptions::default()
    };
    eframe::run_native("SMW Editor v0.1.0", native_options, Box::new(|cc| Box::new(UiMainWindow::new(project, cc))))
}

fn dev_open_rom() -> Option<ProjectRef> {
    let rom_path = match env::var("ROM_PATH") {
        Ok(p) if !p.is_empty() => p,
        _ => {
            log::info!("No ROM path defined (ROM_PATH not set)");
            return None;
        }
    };

    log::info!("Opening ROM from ROM_PATH: {rom_path}");
    let project = Project::new(&rom_path)
        .map_err(|e| {
            log::error!("Cannot create project: {e}");
            e
        })
        .ok()?;

    Project::add_to_recent(std::path::Path::new(&rom_path));
    Some(Rc::new(RefCell::new(project)))
}
