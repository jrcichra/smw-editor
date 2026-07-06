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
| Path tile drawing (the visual road/dots) | ✅ | Confirmed path tiles are ordinary L1 tiles (SMWDisX has no separate path-data table for the general case) — already fully paintable via the existing L1 draw/erase tools. Only a curated "path piece" picker/auto-tile UX is missing, not the underlying capability |
| Path movement data (LineGuide-style step tables) | N/A | Investigated and ruled out as a separate system for the general case — SMW's ~10 hard-coded special-case connections (`HardCodedOWPaths`/`OWHardCodedTiles`/`OWHardCodedDirs`, `bank_04.asm:~1646`) are the only exception, not worth building UI for |
| Event tile preview toggles (which "destruction" events are shown) | ✅ | `crates/smwe-rom/src/overworld/mod.rs::OverworldEvents` parses the real reveal-tile-swap tables (`$04D85D` tile offsets, `$04DA1D`/`$04DA33` before/after tile IDs, ported from SMWDisX `CODE_04DA49`) + tests; `world_editor` now has a per-event checkbox panel (`events_panel`) replacing the old blanket "activate all events" hack — toggling writes real `OWEventsActivated` WRAM bits so the actual emulated game code applies the swap. Verified against the real ROM (`render_ow_submap --dump-events`: 49 tiles change when all events applied, matching expected castle/fortress/switch-palace reveal graphics) |
| Event *ownership* editing (which level/action triggers which event) | ⛔ | Not investigated — likely the `$05D608` events-by-translevel table found in Level-number research. Editing which event a level triggers is a separate, not-yet-scoped piece |
| Layer 2 event tiles (separate from Layer 1 reveal-tile swaps) | ⛔ | `$04DD8D` table identified but not parsed; not covered by the current implementation |
| Level-number display per tile (read-only, vanilla-accurate) | ✅ | `crates/smwe-rom/src/overworld/mod.rs::level_number_at`/`translevel_at` (+ tests), surfaced in `world_editor/mod.rs` tile-inspect panel. Confirmed against real ROM (`render_ow_submap --dump-levels`): matches vanilla's translevel scan-order numbering including the documented 0x25→0x01 wraparound |
| Level-number free reassignment (LM-style, arbitrary) | ⛔ | Vanilla SMW derives level number from tile-placement scan order, not a free field (confirmed in `bank_04.asm` `CODE_04D832`/`bank_05.asm` `CODE_05D8A2`) — Lunar Magic achieves free assignment via its own ASM hijack replacing this lookup. Doing the same here is a distinct, larger follow-up (ROM code injection + repointing), not yet started |
| Layer 2 scroll properties (not raw tiles) | ⛔ | Not found |
| Animated overworld tiles editing | ⛔ | Not found |
| Overworld undo/redo | ✅ | Added 2026 (commit `77dfc73`) |

## Graphics / Map16 / Palette Tools

| Feature | Status | Notes |
|---|---|---|
| VRAM/GFX viewer widget | ✅ | `crates/smwe-widgets/src/vram_view.rs` |
| Palette viewer widget | ✅ | `crates/smwe-widgets/palette_view.rs` |
| Map16 editor | ✅ | `map16_editor.rs` |
| Vanilla GFX file reading (0x00-0x33) | ✅ | `crates/smwe-rom/src/graphics/gfx_file/` — decompresses into `rom.gfx.files` for VRAM composition |
| GFX write plumbing (compress + tile encode + repoint) | ✅ | `compression::lc_lz2::compress` (direct-copy + byte-fill, verified round-trip against real ROM GFX data) + `GfxFile::to_raw_bytes`/`decode_tiles` (tile encoders, exact inverse of the existing decoders, verified round-trip against real ROM data) + pointer-table read/repoint logic in `level_editor/mod.rs::save_to_rom` |
| ExGFX import/export UI | ✅ | `level_editor/gfx_editor.rs` — export any GFX file slot (0x00-0x33) as a lossless grayscale PNG (pixel intensity = color index, not a colored preview) via `rfd` file dialogs; import a PNG back, staged in `gfx_edits` and written on save (LC_LZ2-compressed, repointed via `find_free_space` if the new data doesn't fit in place). Verified end-to-end against real ROM GFX file 0 (export→import→re-encode reproduces the original bytes exactly, and compresses/decompresses correctly) |
| Colored (palette-aware) GFX preview/import, per-level GFX slot *browser* tied to the editor | ⛔ | Current export/import is grayscale-index-only (deliberate, for exact round-tripping); no palette-colored WYSIWYG view yet. Per-level FG/BG/sprite GFX slot selection already works as a raw nibble via the level header (`fg_bg_gfx`/`sprite_gfx` in `properties.rs`), but isn't cross-linked to the GFX editor UI (e.g. jump from a level's assigned slot straight into editing it) |
| 8x8 tile bitmap import/export | ⛔ | Not found |

## Sprites

| Feature | Status | Notes |
|---|---|---|
| Sprite placement in levels | ✅ | See above |
| Sprite extra bits (2-bit position field per sprite) | ✅ | `sprite_layer.rs`, `left_panel.rs:103` — confirmed editable, corrected from an earlier pass that missed it |
| Sprite Map / OAM tile editor | ✅ | `src/ui/editor_prototypes/sprite_map_editor/` |
| Sprite tweaker/behavior byte editing (6 global tables, "Sprite Header Editor") | ✅ | `crates/smwe-rom/src/sprite_tweakers.rs` parses the 6 ROM tables ($07F26C/$07F335/$07F3FE/$07F4C7/$07F590/$07F659, 0xC9 entries each) with named bit accessors (+ tests); `sprite_tweaker_editor.rs` in the level editor exposes all of them with save-to-ROM support. Verified against real ROM: Goomba (0x0F) shows `can_be_jumped_on=true`, `dies_when_jumped_on=false`, matching known vanilla behavior. Edits are global (affect every placement of that sprite ID), matching how LM's own editor works |
| Sprite category distinction (cluster/extended/generator vs. normal) | 🟡 | `SpriteTweakers::has_tweakers()` now encodes the boundary (IDs `>= 0xC9` don't have tweaker bytes) and the tweaker editor warns when selecting one; no dedicated `SpriteCategory` type or separate editing UI for those categories yet |
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

1. **Message box / dialog text editor** — sprite exists in the catalog but is inert. Now the largest remaining gap with zero progress.
2. **Overworld event *ownership* editing** — which level/action triggers which event (`$05D608`?) is still unmapped; reveal-tile preview toggling itself now works.
3. **Overworld level-number free reassignment (ASM hijack)** — read-only vanilla-accurate display now works; matching LM's free-assignment UX requires ASM code injection, a distinct and larger undertaking.
4. **Custom sprite insertion / cluster-extended-generator sprite category editing** — tweaker byte editing now covers the ~0xC9 normal sprite IDs; the other categories still have no dedicated support.
5. **ExGFX colored preview + per-level slot browser cross-linking** — the core import/export loop works now; this is the remaining UX polish.
6. **General freespace/repoint manager** — GFX/level/sprite/overworld-L2 writes each implement their own repoint logic now; still not unified into one shared utility.
7. **Block editor** — already tracked in README as next planned work.
