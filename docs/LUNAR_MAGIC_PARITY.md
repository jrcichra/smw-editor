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
| Sprite placement | ✅ | `sprite_layer.rs`, `sprite_catalog.rs`; global per-ID behavior editable via `sprite_tweaker_editor.rs` (see Sprites section) |
| Primary/secondary header editing | ✅ | `properties.rs`, `left_panel.rs` — raw byte fields |
| Screen exits / secondary entrances | ✅ | `secondary_entrance_editor.rs` |
| Map16 tile picker & editor | ✅ | `tile_picker.rs`, `map16_editor.rs` |
| Palette editor (BG/FG/sprite) | ✅ | `palette_editor.rs` |
| Layer 2/3 background editing | 🟡 | `background_layer.rs` minimal (~510 bytes); L2 header copied verbatim, not user-editable (`level_editor/mod.rs:342`) |
| Music selection | ✅ | Header music slider now shows vanilla track names (0-7 → Overworld/Underground/Athletic/Castle/Ghost House/Underwater/Boss Fight/Bonus Game, per `LevelMusicTable` at `$0584DB` in SMWDisX `bank_05.asm`) |
| Custom level names (overworld name table) | ⛔ | Not found |
| Message box / dialog text editor | ✅ | `crates/smwe-rom/src/message_boxes.rs` + `level_editor/message_editor.rs` — WYSIWYG text editing for all 22 vanilla messages. The font was located empirically: message byte = BG3 tile `0x100\|byte` (attr `$39`, per `CODE_05B208`), glyphs at VRAM byte offset 0x9000 after level load; the byte↔char chart (`byte_to_char`/`char_to_byte`, A-Z a-z 0-9 `!.-,?#()'` space) was transcribed from a VRAM tile-sheet render and validated by decoding every vanilla message to readable English (real-ROM test asserts the intro decodes to "Welcome!…"). Text edits pad each 18-char game row with spaces; messages using special non-text tiles fall back to the raw byte grid (now annotated with each byte's glyph). Byte budget handling unchanged (2854-byte non-repointable blob) |
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
| Event *ownership* editing (which level/action triggers which event) | ✅ | `crates/smwe-rom/src/overworld/mod.rs::TranslevelEvents` parses `DATA_05D608` (confirmed: `LDA DATA_05D608,Y`, Y=`TranslevelNo` → `OverworldEvent`; secret exit = value+1 per `CODE_04E5EE`'s `INC OverworldEvent`; `0xFF` = none). Real-ROM test asserts the vanilla table bytes. Editable per level tile in the world editor's tile-inspect panel (checkbox for none + event slider); fixed-size table rewritten in place on save, so untouched ROMs stay byte-identical |
| Layer 2 event tiles (separate from Layer 1 reveal-tile swaps) | ⛔ | `$04DD8D` table identified but not parsed; not covered by the current implementation |
| Level-number display per tile (read-only, vanilla-accurate) | ✅ | `crates/smwe-rom/src/overworld/mod.rs::level_number_at`/`translevel_at` (+ tests), surfaced in `world_editor/mod.rs` tile-inspect panel. Confirmed against real ROM (`render_ow_submap --dump-levels`): matches vanilla's translevel scan-order numbering including the documented 0x25→0x01 wraparound |
| Level-number free reassignment (LM-style, arbitrary) | ✅ | Turned out not to need code injection: a single existing instruction (`LDA.L $7ED000,X` at SNES `$05D89B`, confirmed byte-for-byte against a real ROM: `BF 00 D0 7E`) is repointed to a custom ROM table instead of the vanilla WRAM-computed one, using the same `layer1_tiles` index space. `encode_custom_level_number` inverts the vanilla remap so the existing (unmodified) remap code still produces the right final number; verified with an exhaustive round-trip test over all 220 representable values (0x00-0xDB). UI in `world_editor/mod.rs` tile-inspect panel; only touches the ROM if the user actually overrides a level number, so untouched hacks stay byte-identical to vanilla in this area. Known limitation: repeated saves with active overrides allocate a fresh table each time rather than reusing one in place (documented in code, harmless but wasteful) |
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
| ROM expansion (expand to 3/4MB, freespace tracking) | 🟡 | `src/rom_freespace.rs::find_free_space`/`find_free_space_in` (unified, tested) used by level layer1/layer2/sprite data, GFX files, message boxes, and overworld L2. Still a bank-scoped free-space *scanner*, not a general expand-ROM/global-freespace-map tool like LM's (no way to grow the ROM itself) |
| Title screen editor | 🟡 | `Title/Credits…` window in the level editor exposes fixed-slot title data: opening overworld submap immediate operand, title demo controller playback (`TitleScreenInputSeq`), and raw Layer-3 title stripe image (`TitleScreenStripe`). Still raw-byte editing for stripe data; no WYSIWYG title-logo/menu tilemap editor yet |
| Credits editor | 🟡 | `Title/Credits…` window exposes raw ending enemy-name stripe images (`EnemyNameStripe00..0C`) with decoded text summaries and fixed-slot bounds checks. Staff roll text/scripts, credits scene scripts, HDMA, sprite choreography, and ending special enemy-name overlays are not modeled yet |

## Save / Export

| Feature | Status | Notes |
|---|---|---|
| Save to ROM | ✅ | `src/ui/mod.rs::save_rom`/`save_rom_as`/`write_rom_to_path` |
| SMC header detection on save | ✅ | `write_rom_to_path` |
| Repointing / freespace allocation | 🟡 | Free-space *scanning* is now unified in `src/rom_freespace.rs` (with tests), used by level L1/L2/sprite data, GFX files, message boxes, and overworld L2 — previously duplicated verbatim in two places. Writing the new pointer bytes themselves is still feature-specific (different pointer table layouts per feature: 3-byte SNES pointers, GFX's 3-lookup-table split, message boxes' 25-entry u16 table, etc.), which is inherent to the ROM format rather than something to further unify |

## Misc Tools

| Feature | Status | Notes |
|---|---|---|
| Address converter (PC/SNES) | ✅ | `src/ui/dev_utils/address_converter.rs` |
| Project creator / welcome screen | ✅ | `src/ui/project_creator.rs`, `welcome.rs` |
| Mapper auto-detection (LoROM/HiROM) | 🟡 | Detected from header; SA-1/ExLoROM/ExHiROM unsupported ([[mapper-autodetection]] memory) |
| Block editor: "acts like" reference | ✅ | `crates/smwe-rom/src/block_behavior.rs` — vanilla dispatches block collision/interaction by hardcoded ID range, not a per-block data byte (source: SMW Central Data Repository, "Detailed explanation of interaction of each tile," MarioFanGamer, 17 Oct 2024). A custom block already gets any of these behaviors for free by using a Map16 ID from the matching range with custom graphics (already fully supported by the existing Map16 editor). Surfaced as an "Acts like: ..." label in `map16_editor.rs` when selecting/editing a block. Ranges + specific tile behaviors covered by tests |
| Block editor: novel (non-vanilla) custom behavior via ASM insertion | ⛔ | Giving a block a behavior that doesn't correspond to any existing vanilla ID range would need a real JSL hook into the interaction dispatcher — genuine new code, not a data patch (unlike the overworld level-number case above). Not started; this is a categorically higher-risk piece of work than anything else in this tracker |
| Graphics editor | ⛔ | README lists as "Planned" |
| ASM code editor | ⛔ | README lists as "Planned" |
| Music editor | ⛔ | README lists as "Planned" |

## Known correctness gaps affecting parity work

- ~~Custom Layer-2 backgrounds in hacked levels can render scrambled~~ FIXED:
  root cause was `UploadSpriteGFX`'s decompression overrunning the `$7EAD00`
  buffer into the `$7EB900` BG tilemap on ROMs with Lunar-Magic-sized GFX files
  (harmless on hardware where the BG is converted to VRAM first; fatal for the
  editor which renders from that WRAM). `decompress_sublevel`/`decompress_extram`
  now snapshot the BG tilemap after `CODE_05801E` and restore it at the end.
- SA-1 and ExLoROM/ExHiROM ROMs are not supported by the mapper or ROM header
  parser (see memory `mapper-autodetection`).
- ~~`lm_map16_ptr`'s hard-coded per-page pointer-table addresses read
  mid-instruction bytes of LM's code~~ FIXED (2026-07-06): extended (id >=
  0x200) FG Map16 blocks are now resolved by *running* Lunar Magic's own
  resolver routine at `$06F540` in the emulator
  (`smwe_emu::emu::lm_ext_map16_data_addr`, calibrated: exact match with the
  `$0FBE` table for vanilla ids), and BG Map16 bases by running the resolver
  LM installs via its `JSL` hijack at `$05:8DA8`
  (`lm_bg_map16_base`, rack of 24-bit base pointers — `$158000`/`$168000` in
  TOP2020). Both are guarded (vanilla byte patterns → fall back to
  `Map16BGTiles`/static parse), so vanilla renders are byte-identical, and
  hacks with *edited* BG Map16 or relocated tables now resolve correctly for
  any LM version.
- Title/credits editing is currently U-ROM fixed-address only for the modeled
  slots; non-U variants have different `TitleScreenInputSeq`/stripe addresses
  in the symbols and need region-aware address selection before they are safe.

## Biggest gaps to close for parity (suggested priority)

1. **Block editor: novel custom behavior via ASM insertion** — the "acts like" reference (free, ID-range-based) now works; this remaining piece needs a real JSL hook into the interaction dispatcher, the highest-risk item in this tracker (actual new code, not a data patch).
2. ~~**Message box font/WYSIWYG preview**~~ — DONE: font located in VRAM/BG3, chart validated against all vanilla messages; users now type readable text.
3. ~~**Overworld event *ownership* editing**~~ — DONE: `DATA_05D608` mapped, verified, and editable per level tile.
4. **Custom sprite insertion / cluster-extended-generator sprite category editing** — tweaker byte editing now covers the ~0xC9 normal sprite IDs; the other categories still have no dedicated support.
5. **ExGFX colored preview + per-level slot browser cross-linking** — the core import/export loop works now; this is the remaining UX polish.
6. ~~**ROM expansion**~~ — DONE: `src/rom_expand.rs` + File → Expand ROM (1/2/4 MB), pads with 0xFF so the free-space scanner can use it, fixes the internal-header size byte and checksum (checksum now also fixed on every save); the scanner refuses the LoROM SRAM-shadow banks (PC 0x380000+).
