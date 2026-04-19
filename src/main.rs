use std::env;

use eframe::{NativeOptions, Renderer};
use egui::{vec2, ViewportBuilder};
use smw_editor::{
    project::Project,
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

    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--nogui") {
        return run_nogui(&args);
    }

    let native_options = NativeOptions {
        renderer: Renderer::Glow,
        viewport: ViewportBuilder::default().with_min_inner_size(vec2(1280., 720.)),
        ..NativeOptions::default()
    };
    eframe::run_native("SMW Editor v0.1.0", native_options, Box::new(|cc| Box::new(UiMainWindow::new(cc))))
}

fn run_nogui(args: &[String]) -> eframe::Result<()> {
    let level_num = args
        .iter()
        .find_map(|a| a.strip_prefix("--level="))
        .and_then(parse_level_arg)
        .unwrap_or(0x000);

    let rom_path = resolve_rom_path(args).unwrap_or_else(|| {
        log::error!("No ROM path defined (ROM_PATH not set, --rom missing, ./smw.smc not found)");
        String::new()
    });
    if rom_path.is_empty() {
        return Ok(());
    }

    log::info!("Opening ROM from: {rom_path}");
    let project = match Project::new(&rom_path) {
        Ok(p) => p,
        Err(e) => {
            log::error!("Cannot create project: {e}");
            return Ok(());
        }
    };

    let rom = &project.rom;
    let level_idx = level_num as usize;
    if level_idx >= rom.levels.len() {
        log::error!("Level {:#X} out of range (max {:#X})", level_num, rom.levels.len().saturating_sub(1));
        return Ok(());
    }

    let level = &rom.levels[level_idx];
    let is_vertical = level.secondary_header.vertical_level();
    let level_length = level.primary_header.level_length();
    let screens = level_length as u32 + 1;
    let (screen_w, screen_h) = if is_vertical { (32_u32, 16_u32) } else { (16_u32, 27_u32) };
    let (level_w, level_h) = if is_vertical {
        (screen_w, screen_h * screens)
    } else {
        (screen_w * screens, screen_h)
    };

    println!("SMW Editor nogui");
    println!("ROM: {}", project.path.display());
    println!(
        "Level {:03X}: vertical={} length={} screens={} size={}x{} tiles",
        level_num, is_vertical, level_length, screens, level_w, level_h
    );
    println!(
        "Header: mode={:02X} fg_bg_gfx={:X} palette_fg={} palette_bg={} music={} timer={}",
        level.primary_header.level_mode(),
        level.primary_header.fg_bg_gfx(),
        level.primary_header.palette_fg(),
        level.primary_header.palette_bg(),
        level.primary_header.music(),
        level.primary_header.timer()
    );
    let object_tileset = level.primary_header.fg_bg_gfx() as usize;
    let map16_tileset = smwe_rom::objects::tilesets::object_tileset_to_map16_tileset(object_tileset);
    println!("Tileset: object_tileset={} map16_tileset={}", object_tileset, map16_tileset);

    let raw = level.layer1.as_bytes();
    let objects = smwe_rom::objects::Object::parse_from_layer(raw).unwrap_or_default();
    println!("Layer1 objects: {}", objects.len());

    let mut current_screen: u32 = 0;
    let mut standard = 0;
    let mut extended = 0;
    let mut exits = 0;
    let mut screen_jumps = 0;
    let mut printed = 0;

    for obj in &objects {
        if obj.is_exit() {
            exits += 1;
            continue;
        }
        if obj.is_screen_jump() {
            screen_jumps += 1;
            current_screen = obj.screen_number() as u32;
            continue;
        }
        if obj.is_new_screen() {
            current_screen = current_screen.saturating_add(1);
        }

        let (local_x, local_y) = if is_vertical {
            (obj.y() as u32, obj.x() as u32)
        } else {
            (obj.x() as u32, obj.y() as u32)
        };
        let abs_x = local_x + if is_vertical { 0 } else { current_screen * 16 };
        let abs_y = local_y + if is_vertical { current_screen * 16 } else { 0 };

        if obj.is_extended() {
            extended += 1;
        } else {
            standard += 1;
        }

        if printed < 25 {
            let label = if obj.is_extended() {
                format!("E{:02X}", obj.settings())
            } else {
                format!("{:02X}", obj.standard_object_number())
            };
            println!(
                "  obj {:>3}: screen={:02X} pos=({:02},{:02}) abs=({:03},{:03}) id={} settings={:02X}",
                printed + 1,
                current_screen,
                local_x,
                local_y,
                abs_x,
                abs_y,
                label,
                obj.settings()
            );
            printed += 1;
        }
    }

    println!(
        "Counts: standard={} extended={} exits={} screen_jumps={}",
        standard, extended, exits, screen_jumps
    );
    Ok(())
}

fn parse_level_arg(value: &str) -> Option<u16> {
    if let Some(hex) = value.strip_prefix("0x") {
        u16::from_str_radix(hex, 16).ok()
    } else {
        value.parse::<u16>().ok()
    }
}


fn resolve_rom_path(args: &[String]) -> Option<String> {
    if let Some(arg) = args.iter().find_map(|a| a.strip_prefix("--rom=")) {
        if !arg.is_empty() {
            return Some(arg.to_string());
        }
    }
    if let Ok(p) = env::var("ROM_PATH") {
        if !p.is_empty() {
            return Some(p);
        }
    }
    None
}
