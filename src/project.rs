use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use egui::Id;
use smwe_rom::SmwRom;

// Recent files are persisted here
const RECENT_FILES_PATH: &str = ".smw-editor-recent.json";
const MAX_RECENT_FILES: usize = 10;

#[derive(Debug)]
pub struct Project {
    pub title: String,
    pub rom:   Arc<SmwRom>,
    pub path:  PathBuf,
}

pub type ProjectRef = Rc<RefCell<Project>>;

impl Project {
    pub fn new(rom_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = rom_path.as_ref().to_path_buf();
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("Unnamed"));
        let rom = SmwRom::from_file(&path)?;
        Ok(Self { title, rom: Arc::new(rom), path })
    }

    pub fn rom_id() -> Id {
        Id::new("rom")
    }

    pub fn project_title_id() -> Id {
        Id::new("project_title")
    }

    pub fn load_recent_files() -> Vec<PathBuf> {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = PathBuf::from(home).join(RECENT_FILES_PATH);
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(files) = serde_json::from_str::<Vec<String>>(&data) {
                return files
                    .into_iter()
                    .map(PathBuf::from)
                    .filter(|p| p.exists())
                    .take(MAX_RECENT_FILES)
                    .collect();
            }
        }
        Vec::new()
    }

    pub fn save_recent_files(files: &[PathBuf]) {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = PathBuf::from(home).join(RECENT_FILES_PATH);
        let strings: Vec<String> = files.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        if let Ok(json) = serde_json::to_string_pretty(&strings) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn add_to_recent(path: &Path) {
        let mut recent = Self::load_recent_files();
        recent.retain(|p| p != path);
        recent.insert(0, path.to_path_buf());
        recent.truncate(MAX_RECENT_FILES);
        Self::save_recent_files(&recent);
    }
}
