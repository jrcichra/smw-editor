# Lunar Magic Feature Parity Tracker

Tracks what Lunar Magic (LM) can do vs. what smw-editor currently supports, so
work can be prioritized toward feature completeness. Update this file as
features land — flip status and add the PR/commit that did it.

Status legend: ✅ Done · 🟡 Partial · ⛔ Missing

**Confidence note:** this was built from one codebase-survey pass plus
targeted greps, not an exhaustive audit. Two follow-up checks already
corrected the first draft (sprite extra-bits were wrongly marked missing;
freespace-finding was undersold as overworld-only). Treat ⛔ rows as "not
found by grep," not proven absent — re-verify with a grep/read before relying
on a row for planning if it's been a while since the file was touched. Missing
areas not yet cross-checked in depth: overworld animated tiles/indicator
sprites, layer 3 "tide"/water settings across levels, direct Map16
import/export file format, ROM search/analysis tools (e.g. "find levels using
X"), and player (Mario/Yoshi) graphics customization.

## Level Editing

| Feature | Status | Notes |
|---|---|---|
| Object placement/move/resize (layer 1) | ✅ | `level_editor/object_layer.rs`, `editing.rs` |
| Sprite placement | ✅ | `sprite_layer.rs`, `sprite_catalog.rs` (names only, no behavior editing) |
| Primary/secondary header editing | ✅ | `properties.rs`, `left_panel.rs` — raw byte fields |
| Screen exits / secondary entrances | ✅ | `secondary_entrance_editor.rs` |
| Map16 tile picker & editor | ✅ | `tile_picker.rs`, `map16_editor.rs` |
| Palette editor (BG/FG/sprite) | ✅ | `palette_editor.rs` |
| Layer 2/3 background editing | 🟡 | `background_layer.rs` minimal (~510 bytes); L2 header copied verbatim, not user-editable (`level_editor/mod.rs:342`) |
| Music selection | 🟡 | Raw nibble slider only, no track-name mapping |
| Custom level names (overworld name table) | ⛔ | Not found |
| Message box / dialog text editor | ⛔ | Sprite catalog has a "Message Box" sprite entry but no text editor |
| Import/export level as `.mwl` | ⛔ | Not found |
| Move/resize via drag handles (LM-style) | ⛔ | Object editing exists but unclear if drag-resize UX matches LM |

## Overworld Editing

| Feature | Status | Notes |
|---|---|---|
| Submap viewing/navigation | ✅ | `world_editor/mod.rs::load_submap` |
| Layer 1 tile paint/erase | ✅ | `editing.rs` |
| Layer 2 tile paint/erase + repoint on save | ✅ | `write_overworld_l2_stream`, `patch_snes_pointer` |
| Save/repoint to ROM | ✅ | `find_free_space`, `patch_snes_pointer` |
| Path editing (Mario's walk path dots) | ⛔ | No code found |
| Event tile editing | ⛔ | Code only force-activates all events for viewing; no edit UI |
| Level-number/entrance tile assignment | ⛔ | Not found |
| Layer 2 scroll properties (not raw tiles) | ⛔ | Not found |
| Animated overworld tiles editing | ⛔ | Not found |
| Overworld undo/redo | ✅ | Added 2026 (commit `77dfc73`) |

## Graphics / Map16 / Palette Tools

| Feature | Status | Notes |
|---|---|---|
| VRAM/GFX viewer widget | ✅ | `crates/smwe-widgets/src/vram_view.rs` |
| Palette viewer widget | ✅ | `crates/smwe-widgets/palette_view.rs` |
| Map16 editor | ✅ | `map16_editor.rs` |
| Vanilla GFX file reading (0x00-0x33) | ✅ | `crates/smwe-rom/src/graphics/gfx_file/` — read-only, decompresses into `rom.gfx.files` for VRAM composition |
| ExGFX import/export, per-level GFX slot assignment | ⛔ | GFX files are read-only; no write/export/import path found anywhere |
| 8x8 tile bitmap import/export | ⛔ | Not found |

## Sprites

| Feature | Status | Notes |
|---|---|---|
| Sprite placement in levels | ✅ | See above |
| Sprite extra bits (2-bit position field per sprite) | ✅ | `sprite_layer.rs`, `left_panel.rs:103` — confirmed editable, corrected from an earlier pass that missed it |
| Sprite Map / OAM tile editor | ✅ | `src/ui/editor_prototypes/sprite_map_editor/` |
| Sprite header/behavior tables (SDT, tweak bytes) | ⛔ | Only the 2-bit extra-bits field is exposed; no full tweak-byte/SDT editing |
| Sprite category distinction (cluster/extended/generator vs. normal) | ⛔ | No `SpriteCategory`/`SpriteKind` type found — editor treats all sprites uniformly |
| Custom sprite insertion (SA-1/UberASM-style dropins) | ⛔ | Not found |

## Music / Sound

| Feature | Status | Notes |
|---|---|---|
| Music track selection (header nibble) | 🟡 | Editable but no track-name mapping |
| Music/SPC data import or editing | ⛔ | Not found |

## Data / ASM / Patches

| Feature | Status | Notes |
|---|---|---|
| BPS/IPS patch libraries | ✅ (library only) | `smwe-bps`, `smwe-ips` crates exist |
| ASM insertion tool / hijack manager | ⛔ | No user-facing ASM editor |
| 65816 disassembler | ✅ (library only) | `crates/smwe-rom/src/disassembler`, `crates/wdc65816` — not exposed as a user-facing ASM editor |
| ROM expansion (expand to 3/4MB, freespace tracking) | 🟡 | `find_free_space`/`find_free_space_in` exist and are used for level layer1/layer2/sprite data (`level_editor/mod.rs`) *and* overworld L2 (`world_editor/mod.rs`) — corrected from an earlier pass that said overworld-only. Still per-feature bank-scoped search, not a general expand-ROM/global-freespace-map tool like LM's |
| Title screen editor | ⛔ | Not found |
| Credits editor | ⛔ | Not found |

## Save / Export

| Feature | Status | Notes |
|---|---|---|
| Save to ROM | ✅ | `src/ui/mod.rs::save_rom`/`save_rom_as`/`write_rom_to_path` |
| SMC header detection on save | ✅ | `write_rom_to_path` |
| Repointing / freespace allocation | 🟡 | Implemented per-feature (level L1/L2/sprite data, overworld L2), each with its own bank-scoped search — not a general/global freespace manager |

## Misc Tools

| Feature | Status | Notes |
|---|---|---|
| Address converter (PC/SNES) | ✅ | `src/ui/dev_utils/address_converter.rs` |
| Project creator / welcome screen | ✅ | `src/ui/project_creator.rs`, `welcome.rs` |
| Mapper auto-detection (LoROM/HiROM) | 🟡 | Detected from header; SA-1/ExLoROM/ExHiROM unsupported ([[mapper-autodetection]] memory) |
| Block editor (custom blocks/ASM per block) | ⛔ | README lists as "Planned" |
| Graphics editor | ⛔ | README lists as "Planned" |
| ASM code editor | ⛔ | README lists as "Planned" |
| Music editor | ⛔ | README lists as "Planned" |

## Known correctness gaps affecting parity work

- Custom Layer-2 backgrounds in hacked levels can render scrambled — root cause
  understood but not fixed (see memory `custom-layer2-background`).
- SA-1 and ExLoROM/ExHiROM ROMs are not supported by the mapper or ROM header
  parser (see memory `mapper-autodetection`).

## Biggest gaps to close for parity (suggested priority)

1. **Overworld path & event editing** — core LM workflow, currently entirely missing.
2. **ExGFX support** — no way to bring in custom graphics at all right now.
3. **Sprite header/SDT editing** — sprite placement exists but custom sprite behavior can't be configured.
4. **Message box / dialog text editor** — sprite exists in the catalog but is inert.
5. **General freespace/repoint manager** — currently one-off for overworld L2 only; levels/sprites will need it too as more editors gain write support.
6. **Block editor** — already tracked in README as next planned work.
