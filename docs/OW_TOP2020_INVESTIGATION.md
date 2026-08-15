# TOP2020 overworld "a few tiles out of place" — investigation notes

Status: **open / unresolved**. Root cause not yet isolated. This file captures
everything found so a later session can resume without re-deriving it.

## The report

User: on `TOP2020.smc`, the **main overworld map renders fine**, but the other
submaps have **"a few tiles out of place"** — described as *garbled/scrambled
tiles* and *wrong tiles / wrong area shown*. Subtle, systematic, affects all
non-main submaps. Vanilla (`smw.smc`) is fine.

Not caused by the recent level-Map16 resolver work (commit `ffcc5fe`); that
never touched the overworld path. This is pre-existing.

## Tools

- `cargo run --bin render_ow_submap -- --rom=TOP2020.smc --submap=N --out=/tmp/sN.png`
  (submaps 0..6). Renders the composed OW VRAM at the submap's scroll pos.
  - `--full` renders 1024x512 (both L1/L2 across the whole VRAM).
  - `--dump-l1=col0,row0,col1,row1` prints L1 VRAM 8x8 tile indices for a region.
- Submap → world: 0 Main, 1 Yoshi's Island, 2 Vanilla Dome, 3 Forest of
  Illusion, 4 Valley of Bowser, 5 Special World, 6 Star World
  (`SUBMAP_NAMES` in `crates/smwe-rom/src/overworld/mod.rs`).
- NOTE: the bin renders the whole 512x512 composed VRAM, which contains 4
  quadrant-sized sub-areas; only one quadrant is the actual submap. The GUI
  (`src/ui/world_editor/mod.rs::build_bg_tiles`) crops to `SUBMAP_VIEW_X=16,
  Y=40, W=224, H=168` at the submap scroll pos. **The bin and the GUI crop
  differently** — reproduce the GUI crop when chasing a GUI-only symptom.

## Key code

- Emulator OW load: `crates/smwe-emu/src/emu.rs::load_overworld(cpu, submap)`.
  Hard-codes vanilla submap viewports:
  ```
  OW_VIEW_X = [0x0000, 0xFFEF, 0xFFEF, 0xFFEF, 0x00F0, 0x00F0, 0x00F0]
  OW_VIEW_Y = [0x0000, 0xFFD8, 0x0080, 0x0128, 0xFFD8, 0x0080, 0x0128]
  ```
  Runs routines: `CODE_04DC09`, `DecompressOverworldL2`, `UploadSpriteGFX`,
  `[LDY #$14]`, `PrepareGraphicsFile`, `CODE_00AD25`, `CODE_00922F`,
  `CODE_04D6E9`. **Does NOT** run any OW animated-tile routine (see below).
- GUI render: `src/ui/world_editor/mod.rs` — `load_submap`, `build_bg_tiles`,
  `tilemap_vram_addr` (64x64 tilemap laid out as 2x2 quadrants of 32x32).

## What was checked and RULED OUT

1. **Animated OW tiles (water/waterfall) not initialized.** There IS an OW
   animated-tile system the emulator never runs:
   - `OW_Tile_Animation` ($0480E0, `bank_04.asm`) + init `CODE_048086` ($048086)
     fill the WRAM buffer `GfxDecompOWAni` ($0AF6, 0x160 bytes) from decompressed
     GFX at $7EB300; `CODE_048086` is called from `CODE_048EE1`.
   - NMI `CODE_00A4E3` ($00A4E3, `bank_00.asm`) DMAs `GfxDecompOWAni` → VRAM word
     $0750 (byte $0EA0), size $0160 → VRAM tiles ~0x75-0x7F.
   - Note the near-`RTS` vs `RTL` issue: `CODE_048086`/`OW_Tile_Animation` are
     JSR-called (end in RTS), so they can't be JSL'd directly by
     `run_routines`; you'd need a bank-$04 WRAM shim (`JSR x / RTL`) driven by
     `run_trampoline`.
   - **RULED OUT as the cause:** the VRAM anim slot [$0EA0..+0x160] is already
     populated (133/352 nonzero) identically for TOP2020 and vanilla (loaded by
     `PrepareGraphicsFile`), so the static frame-0 anim tiles are present.
     `GfxDecompOWAni` ($0AF6) is empty (0/352) but that buffer only matters for
     live animation, not the static editor frame. (Still worth wiring up
     eventually for correct animation, but it is not this bug.)

2. **The tan/"+" water pattern.** Submap water shows a tan cross pattern vs
   vanilla's blue waves. Traced it:
   - The Layer-1 water tile is `0x122`, which is **all-zeros (blank) in both**
     vanilla and TOP2020 → the water comes from **Layer 2**, not L1.
   - The per-submap **palette IS loaded and differs** between submaps: submap 0
     (main) CGRAM row0 cols 13-15 = `01CA 01A9 0168` (blue); submap 1 = `3E75
     3212 25AF` (tan). So the tan water is submap 1's own loaded palette.
   - **Likely intended hack content, not a bug** — could not prove otherwise
     without a ground-truth reference. Also "all water" ≠ "a few tiles."

## Where it stands / next steps

Could not isolate a definitive "few tiles out of place" bug from vanilla
comparison alone: tilemap indices match, per-submap palettes load, GFX present.
**Blocked on a concrete reference.** To resume, get from the user:
- which submap, and roughly where (a landmark: castle, level dot, map edge,
  path junction);
- ideally a **screenshot of the editor** for one submap vs how it should look;
- whether the wrongness is *wrong graphics* (grass where water should be) vs
  *wrong position* (right tiles nudged), and whether it's stable or shifts.

Then trace one specific wrong tile through: GUI crop/scroll (`build_bg_tiles`,
`SUBMAP_VIEW_*`, `OW_VIEW_*`) → L1/L2 VRAM tilemap index (`--dump-l1`) → Map16 →
GFX slot → palette.

### Untested hypotheses worth trying first next time
- **GUI crop/viewport offset** (`SUBMAP_VIEW_X/Y` + `OW_VIEW_X/Y`): a few-tile
  offset would read as "wrong area / tiles out of place." The bin doesn't apply
  the GUI crop, so a GUI-only offset bug would be invisible to `render_ow_submap`
  — reproduce the exact GUI crop (224x168 @ 16,40) when checking.
- **LM extended/relocated overworld Map16 or ExGFX** (LM 2.x OW editing),
  analogous to the level Map16 relocation fixed in `ffcc5fe`. Check whether LM
  hijacks any OW tile/GFX routine that `load_overworld` doesn't follow.
- Wire up the OW animated-tile init anyway (`CODE_048086` + `OW_Tile_Animation`
  via a shim, then DMA `$0AF6`→VRAM `$0EA0`) for animation correctness.
