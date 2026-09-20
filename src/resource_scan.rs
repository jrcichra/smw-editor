//! Lunar Magic parity: "Analyze Resources in Levels" (LM v3.03; the option to
//! report on music tracks used in levels was added in LM v3.20).
//!
//! LM's command "can check which Map16 tiles, GFX/ExGFX files, and sprites
//! are used in which levels then generate a text file report" (official LM
//! 3.63 `readme.txt`, version 3.03 entry, 2019-04-01). This module is the
//! scan engine behind Tools > Analyze Resources in Levels...: it walks all
//! 512 levels, summarizes each level's resource usage, and inverts the
//! summaries so the UI (and the text report) can answer "which levels use
//! resource X?" for each resource class:
//!
//! * **Music tracks** — per-level header music value (0–7, named like LM's
//!   track list; LM v3.20), plus per-submap overworld music.
//! * **Sprites** — sprite IDs placed in each level.
//! * **Map16 tiles** — distinct Map16 tile IDs in each level's Layer 1 block
//!   map (tile `$0000` is empty space and is excluded), decompressed through
//!   the real emulator — the slow part, shared with the undefined-exit scan.
//! * **GFX/ExGFX files** — per-level FG1/FG2/FG3/BG1 + SP1/SP2/SP3/SP4 slot
//!   file numbers, resolved through OBJECTGFXLIST/SPRITEGFXLIST with the
//!   Super GFX Bypass applied (same resolution as the Level GFX Slots
//!   browser), so ExGFX files show up under their real indices.
//! * **Custom palettes** — levels with an editor custom palette enabled
//!   (LM v3.30 `SMWECPLT`).
//!
//! `rom_bytes` is the raw ROM image (SMC header included if present);
//! callers merge unsaved tab edits first, like the PNG export does, so the
//! scan sees the current editor state.
//!
//! `progress` is called after each level with the number of levels scanned
//! so far; returning `false` aborts the scan early (reported as an error).

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use smwe_rom::{
    exgfx::{BypassData, ExGfxData, BYPASS_DEFAULT, BYPASS_SLOT_NAMES},
    level::{custom_palette::CustomPaletteData, Level, LEVEL_COUNT},
    music::format_music_track,
    objects::{object_gfx_list::ObjectGfxList, sprite_gfx_list::SpriteGfxList},
    overworld::submap_music::{SubmapMusic, SUBMAP_MUSIC_LEN},
    snes_utils::rom::Rom,
};

use crate::level_png_export::{block_at, level_geom_of, load_level_cpu, BLOCK_MAP_BASE};

// -------------------------------------------------------------------------------------------------

/// Which resource classes the scan collects. Defaults to everything, like
/// LM's dialog with all its options ticked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanOptions {
    /// Sprite IDs placed per level.
    pub sprites:  bool,
    /// Map16 tiles in each level's Layer 1 block map (emulator-backed; the
    /// slow part of the scan).
    pub map16:    bool,
    /// Per-level GFX slot file numbers (FG1-3/BG1/SP1-4, bypass applied).
    pub gfx:      bool,
    /// Per-level music track + per-submap overworld music (LM v3.20).
    pub music:    bool,
    /// Per-level custom-palette enable flag.
    pub palettes: bool,
}

impl ScanOptions {
    pub fn all() -> Self {
        ScanOptions { sprites: true, map16: true, gfx: true, music: true, palettes: true }
    }
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self::all()
    }
}

// -------------------------------------------------------------------------------------------------

/// Resource usage summary for one level.
#[derive(Debug, Clone, Default)]
pub struct LevelResources {
    /// Translevel (`0x000`–`0x1FF`).
    pub level:          u16,
    /// `false` when the level failed to parse at all (corrupt data).
    pub scanned:        bool,
    /// Level-header music value (0–7 vanilla), when [`ScanOptions::music`].
    pub music:          u8,
    /// Sprite IDs placed in the level, when [`ScanOptions::sprites`].
    pub sprites:        BTreeSet<u8>,
    /// Distinct Map16 tile IDs in the Layer 1 block map (tile `$0000`
    /// excluded), when [`ScanOptions::map16`].
    pub map16:          BTreeSet<u16>,
    /// `false` when the emulator-backed block-map walk failed for this level.
    pub map16_ok:       bool,
    /// FG/BG tileset nibble from the header (clamped to the 26-row tables).
    pub fg_tileset:     u8,
    /// Sprite tileset nibble from the header (clamped to the 26-row tables).
    pub sp_tileset:     u8,
    /// Resolved FG1/FG2/FG3/BG1 file numbers (bypass applied), when
    /// [`ScanOptions::gfx`].
    pub fg_files:       [u16; 4],
    /// Resolved SP1/SP2/SP3/SP4 file numbers (bypass applied), when
    /// [`ScanOptions::gfx`].
    pub sp_files:       [u16; 4],
    /// Whether a custom palette is enabled for the level, when
    /// [`ScanOptions::palettes`].
    pub custom_palette: bool,
}

impl LevelResources {
    /// All 8 resolved GFX slot file numbers, FG1-3/BG1 then SP1-4.
    pub fn all_slot_files(&self) -> [u16; 8] {
        [
            self.fg_files[0],
            self.fg_files[1],
            self.fg_files[2],
            self.fg_files[3],
            self.sp_files[0],
            self.sp_files[1],
            self.sp_files[2],
            self.sp_files[3],
        ]
    }
}

// -------------------------------------------------------------------------------------------------

/// Full result of scanning the ROM's levels.
#[derive(Debug)]
pub struct ResourceReport {
    /// Per-level summaries, indexed by level number (`0x000`–`0x1FF`).
    pub per_level:       Vec<LevelResources>,
    /// Levels successfully parsed (`0x000`–`0x1FF`).
    pub levels_scanned:  u32,
    /// Levels skipped because their data failed to parse.
    pub levels_skipped:  u32,
    /// SPC track per overworld submap 0–6 (`None` when the tables couldn't
    /// be read), when [`ScanOptions::music`].
    pub submap_music:    Option<[u8; SUBMAP_MUSIC_LEN]>,
    /// Installed ExGFX file indices (`0x80`–`0xFFF`) found in the ROM.
    pub exgfx_installed: Vec<u16>,
    /// The options this report was built with (classes that were off read
    /// as empty, never as "unused").
    pub options:         ScanOptions,
}

impl ResourceReport {
    /// Levels using the given music track value.
    pub fn levels_using_music(&self, track: u8) -> Vec<u16> {
        self.matching(|r| r.scanned && r.music == track)
    }

    /// Levels with the given sprite placed.
    pub fn levels_using_sprite(&self, sprite_id: u8) -> Vec<u16> {
        self.matching(|r| r.scanned && r.sprites.contains(&sprite_id))
    }

    /// Levels whose Layer 1 block map contains the given Map16 tile.
    pub fn levels_using_map16(&self, tile: u16) -> Vec<u16> {
        self.matching(|r| r.scanned && r.map16.contains(&tile))
    }

    /// Levels with the given GFX/ExGFX file in any of the 8 slots.
    pub fn levels_using_gfx_file(&self, file: u16) -> Vec<u16> {
        self.matching(|r| r.scanned && r.all_slot_files().contains(&file))
    }

    /// Levels with a custom palette enabled.
    pub fn levels_with_custom_palette(&self) -> Vec<u16> {
        self.matching(|r| r.scanned && r.custom_palette)
    }

    /// Music track values used by at least one level.
    pub fn used_music_tracks(&self) -> BTreeSet<u8> {
        self.per_level.iter().filter(|r| r.scanned).map(|r| r.music).collect()
    }

    /// Sprite IDs placed in at least one level.
    pub fn used_sprites(&self) -> BTreeSet<u8> {
        self.per_level.iter().filter(|r| r.scanned).flat_map(|r| r.sprites.iter().copied()).collect()
    }

    /// Map16 tiles present in at least one level's Layer 1 block map.
    pub fn used_map16_tiles(&self) -> BTreeSet<u16> {
        self.per_level.iter().filter(|r| r.scanned).flat_map(|r| r.map16.iter().copied()).collect()
    }

    /// GFX/ExGFX file numbers referenced by at least one level's slots.
    pub fn used_gfx_files(&self) -> BTreeSet<u16> {
        self.per_level.iter().filter(|r| r.scanned).flat_map(|r| r.all_slot_files()).collect()
    }

    /// Installed ExGFX files no level references (dead weight a hacker
    /// could reclaim).
    pub fn unused_exgfx_files(&self) -> Vec<u16> {
        let used = self.used_gfx_files();
        self.exgfx_installed.iter().copied().filter(|f| !used.contains(f)).collect()
    }

    fn matching(&self, pred: impl Fn(&LevelResources) -> bool) -> Vec<u16> {
        self.per_level.iter().filter(|r| pred(r)).map(|r| r.level).collect()
    }
}

// -------------------------------------------------------------------------------------------------

/// Collect the distinct Map16 tile IDs in a level's Layer 1 block map
/// (tile `$0000` — empty space — excluded). Same geometry helpers as the
/// undefined-exit scan so the two can never drift apart.
fn map16_tiles_of_level(stripped: &[u8], level: u16) -> Result<BTreeSet<u16>> {
    let mut cpu = load_level_cpu(stripped, level)?;
    let g = level_geom_of(&mut cpu);
    let (tw, th) = (g.width / 16, g.height / 16);
    let mut tiles = BTreeSet::new();
    for ty in 0..th {
        for tx in 0..tw {
            let id = block_at(&mut cpu, &g, tx, ty, BLOCK_MAP_BASE);
            if id != 0 {
                tiles.insert(id);
            }
        }
    }
    Ok(tiles)
}

/// Scan every level for resource usage.
///
/// See the module docs for the `rom_bytes` contract and the `progress`
/// cancellation protocol.
pub fn scan_resources_with_progress(
    rom_bytes: &[u8], options: ScanOptions, progress: &mut dyn FnMut(u32) -> bool,
) -> Result<ResourceReport> {
    let has_smc = rom_bytes.len() % 0x400 == 0x200;
    let stripped: &[u8] = if has_smc { &rom_bytes[0x200..] } else { rom_bytes };
    let header_offset = if has_smc { 0x200 } else { 0 };
    let rom = Rom::new(stripped.to_vec()).context("parsing ROM image")?;

    let obj_gfx = ObjectGfxList::parse(&rom).ok();
    let spr_gfx = SpriteGfxList::parse(&rom).ok();
    let bypass = BypassData::parse(rom_bytes).unwrap_or_default();
    let palettes = CustomPaletteData::parse(rom_bytes).unwrap_or_default();
    let submap_music =
        if options.music { SubmapMusic::parse(rom_bytes, header_offset).ok().map(|m| m.tracks) } else { None };
    let exgfx_installed: Vec<u16> = ExGfxData::parse(rom_bytes).files.keys().copied().collect();

    let mut per_level = Vec::with_capacity(LEVEL_COUNT);
    let mut levels_scanned = 0u32;
    let mut levels_skipped = 0u32;

    for level in 0..LEVEL_COUNT as u16 {
        let parsed = match Level::parse(&rom, u32::from(level)) {
            Ok(l) => l,
            Err(_) => {
                levels_skipped += 1;
                per_level.push(LevelResources { level, ..Default::default() });
                if !progress(u32::from(level) + 1) {
                    anyhow::bail!("scan cancelled");
                }
                continue;
            }
        };
        let mut lr = LevelResources { level, scanned: true, ..Default::default() };

        if options.music {
            lr.music = parsed.primary_header.music();
        }
        if options.sprites {
            for sprite in &parsed.sprite_layer.sprites {
                lr.sprites.insert(sprite.sprite_id());
            }
        }
        if options.gfx {
            // Same clamping as the Level GFX Slots browser (26-row tables).
            lr.fg_tileset = parsed.primary_header.fg_bg_gfx().min(25);
            lr.sp_tileset = parsed.primary_header.sprite_gfx().min(25);
            if let Some(obj_gfx) = &obj_gfx {
                let files = obj_gfx.files_for_object_tileset(usize::from(lr.fg_tileset));
                for (i, &file) in files.iter().enumerate() {
                    lr.fg_files[i] = bypass.slot(level, i).filter(|&v| v != BYPASS_DEFAULT).unwrap_or(file as u16);
                }
            }
            if let Some(spr_gfx) = &spr_gfx {
                let files = spr_gfx.files_for_sprite_tileset(usize::from(lr.sp_tileset));
                for (i, &file) in files.iter().enumerate() {
                    lr.sp_files[i] = bypass.slot(level, 4 + i).filter(|&v| v != BYPASS_DEFAULT).unwrap_or(file as u16);
                }
            }
        }
        if options.palettes {
            lr.custom_palette = palettes.get(level).is_some();
        }
        if options.map16 {
            match map16_tiles_of_level(stripped, level) {
                Ok(tiles) => {
                    lr.map16 = tiles;
                    lr.map16_ok = true;
                }
                Err(_) => lr.map16_ok = false,
            }
        }

        levels_scanned += 1;
        per_level.push(lr);
        if !progress(u32::from(level) + 1) {
            anyhow::bail!("scan cancelled");
        }
    }

    Ok(ResourceReport { per_level, levels_scanned, levels_skipped, submap_music, exgfx_installed, options })
}

/// Convenience wrapper for [`scan_resources_with_progress`] with no progress
/// reporting and no cancellation.
pub fn scan_resources(rom_bytes: &[u8], options: ScanOptions) -> Result<ResourceReport> {
    scan_resources_with_progress(rom_bytes, options, &mut |_| true)
}

// -------------------------------------------------------------------------------------------------

/// Short label for a GFX slot index, e.g. `"FG1"`.
fn slot_name(slot: usize) -> &'static str {
    BYPASS_SLOT_NAMES[slot]
}

/// Label a GFX/ExGFX file number the way the Level GFX Slots browser does.
fn gfx_file_label(file: u16) -> String {
    if file >= 0x80 {
        format!("ExGFX file ${file:03X}")
    } else {
        format!("GFX file ${file:02X}")
    }
}

fn level_list(levels: &[u16]) -> String {
    levels.iter().map(|l| format!("${l:03X}")).collect::<Vec<_>>().join(", ")
}

/// Render the LM-style text report: which Map16 tiles, GFX/ExGFX files,
/// sprites, and music tracks are used in which levels.
pub fn format_text_report(report: &ResourceReport) -> String {
    let mut out = String::new();
    out.push_str("SMW Resource Analysis Report\n");
    out.push_str("Generated by smw-editor — Tools > Analyze Resources in Levels...\n");
    out.push_str("(Lunar Magic v3.03 parity; music-track reporting added in LM v3.20)\n");
    out.push_str(&format!(
        "Levels scanned: {} ({} skipped: unparseable data)\n\n",
        report.levels_scanned, report.levels_skipped
    ));

    if report.options.music {
        out.push_str("== Music tracks used in levels ==\n");
        for track in report.used_music_tracks() {
            let levels = report.levels_using_music(track);
            out.push_str(&format!(
                "Track {} — used by {} level{}: {}\n",
                format_music_track(track),
                levels.len(),
                if levels.len() == 1 { "" } else { "s" },
                level_list(&levels)
            ));
        }
        if let Some(submaps) = &report.submap_music {
            out.push_str("\n-- Overworld submap music --\n");
            for (i, track) in submaps.iter().enumerate() {
                out.push_str(&format!("Submap {i}: SPC track ${track:02X}\n"));
            }
        }
        out.push('\n');
    }

    if report.options.sprites {
        let sprites = report.used_sprites();
        out.push_str(&format!("== Sprites used in levels ({} distinct) ==\n", sprites.len()));
        for sprite in sprites {
            let levels = report.levels_using_sprite(sprite);
            out.push_str(&format!(
                "Sprite ${sprite:02X} — used by {} level{}: {}\n",
                levels.len(),
                if levels.len() == 1 { "" } else { "s" },
                level_list(&levels)
            ));
        }
        out.push('\n');
    }

    if report.options.map16 {
        let tiles = report.used_map16_tiles();
        out.push_str(&format!(
            "== Map16 tiles used in levels ({} distinct; Layer 1 block maps, tile $0000 excluded) ==\n",
            tiles.len()
        ));
        for tile in tiles {
            let levels = report.levels_using_map16(tile);
            out.push_str(&format!(
                "Tile ${tile:04X} — used by {} level{}: {}\n",
                levels.len(),
                if levels.len() == 1 { "" } else { "s" },
                level_list(&levels)
            ));
        }
        out.push('\n');
    }

    if report.options.gfx {
        out.push_str("== GFX/ExGFX files used in levels ==\n");
        for file in report.used_gfx_files() {
            let levels = report.levels_using_gfx_file(file);
            // Which slots does this file occupy anywhere?
            let mut slots: BTreeSet<usize> = BTreeSet::new();
            for r in &report.per_level {
                if !r.scanned {
                    continue;
                }
                for (i, &f) in r.all_slot_files().iter().enumerate() {
                    if f == file {
                        slots.insert(i);
                    }
                }
            }
            let slot_list = slots.iter().map(|&i| slot_name(i)).collect::<Vec<_>>().join("/");
            out.push_str(&format!(
                "{} — used by {} level{} (slots {}): {}\n",
                gfx_file_label(file),
                levels.len(),
                if levels.len() == 1 { "" } else { "s" },
                slot_list,
                level_list(&levels)
            ));
        }
        let unused = report.unused_exgfx_files();
        if !unused.is_empty() {
            out.push_str(&format!(
                "\nInstalled ExGFX files used by NO level: {}\n",
                unused.iter().map(|f| format!("${f:03X}")).collect::<Vec<_>>().join(", ")
            ));
        } else if !report.exgfx_installed.is_empty() {
            out.push_str("\nEvery installed ExGFX file is used by at least one level.\n");
        }
        out.push('\n');
    }

    if report.options.palettes {
        let levels = report.levels_with_custom_palette();
        out.push_str(&format!(
            "== Custom palettes ({} level{} with a custom palette enabled) ==\n{}\n\n",
            levels.len(),
            if levels.len() == 1 { "" } else { "s" },
            if levels.is_empty() { "(none)".to_string() } else { level_list(&levels) }
        ));
    }

    out.push_str("== Per-level summary ==\n");
    for r in &report.per_level {
        if !r.scanned {
            out.push_str(&format!("Level ${:03X}: could not parse\n", r.level));
            continue;
        }
        let mut parts = Vec::new();
        if report.options.music {
            parts.push(format!("music {}", format_music_track(r.music)));
        }
        if report.options.gfx {
            parts.push(format!(
                "FG1-3/BG1={:02X}/{:02X}/{:02X}/{:02X} SP1-4={:02X}/{:02X}/{:02X}/{:02X}",
                r.fg_files[0],
                r.fg_files[1],
                r.fg_files[2],
                r.fg_files[3],
                r.sp_files[0],
                r.sp_files[1],
                r.sp_files[2],
                r.sp_files[3]
            ));
        }
        if report.options.sprites {
            parts.push(format!("{} sprites", r.sprites.len()));
        }
        if report.options.map16 {
            parts.push(if r.map16_ok {
                format!("{} Map16 tiles", r.map16.len())
            } else {
                "Map16 walk failed".to_string()
            });
        }
        if report.options.palettes && r.custom_palette {
            parts.push("custom palette".to_string());
        }
        out.push_str(&format!("Level ${:03X}: {}\n", r.level, parts.join(" · ")));
    }

    out
}

// -------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gfx_file_labels() {
        assert_eq!(gfx_file_label(0x0E), "GFX file $0E");
        assert_eq!(gfx_file_label(0x80), "ExGFX file $080");
        assert_eq!(gfx_file_label(0xFFF), "ExGFX file $FFF");
    }

    #[test]
    fn slot_names_cover_all_eight() {
        for i in 0..8 {
            assert!(!slot_name(i).is_empty());
        }
    }

    /// Real-ROM test: the scan must run clean over the whole vanilla ROM and
    /// the report must reflect the vanilla game's known resource usage.
    #[test]
    #[ignore]
    fn real_rom_scan_reports_vanilla_usage() {
        let rom_path = std::env::var("ROM_PATH").expect("ROM_PATH must point at a real SMW ROM");
        let rom_bytes = std::fs::read(&rom_path).expect("cannot read ROM");
        let report = scan_resources(&rom_bytes, ScanOptions::all()).expect("scan failed");
        assert_eq!(report.levels_scanned, LEVEL_COUNT as u32, "every level should scan");
        assert_eq!(report.levels_skipped, 0, "no level should fail to parse");

        // Vanilla facts the report must reproduce:
        // - all 8 music tracks are used somewhere in the 512 levels;
        assert_eq!(report.used_music_tracks().len(), 8, "vanilla uses all 8 music tracks");
        // - the overworld submap music table is the known vanilla one;
        assert_eq!(report.submap_music, Some(SubmapMusic::VANILLA));
        // - no ExGFX installed on a vanilla ROM;
        assert!(report.exgfx_installed.is_empty(), "vanilla ROM has no ExGFX files");
        // - no editor custom palettes on a vanilla ROM;
        assert!(report.levels_with_custom_palette().is_empty());
        // - every scanned level's block map walked cleanly;
        for r in &report.per_level {
            assert!(r.map16_ok, "Map16 walk failed for level {:03X}", r.level);
        }

        // Document the headline numbers for the PR description.
        eprintln!("distinct sprites: {}", report.used_sprites().len());
        eprintln!("distinct Map16 tiles: {}", report.used_map16_tiles().len());
        eprintln!("distinct GFX files: {}", report.used_gfx_files().len());
        for track in 0..8u8 {
            eprintln!("music {}: {} levels", format_music_track(track), report.levels_using_music(track).len());
        }
    }
}
