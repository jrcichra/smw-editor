# AGENTS.md

This file is for coding agents working in this repository. It is not a style
guide in the abstract; it is the set of practices that actually helped move
this codebase forward without breaking the renderer.

## General approach

- Start from the real game path when possible. This repo includes an emulator,
  disassembly symbols, and enough ROM plumbing that "make the editor match SMW"
  is usually easier than re-implementing behavior from scratch.
- Treat the emulator, WRAM, VRAM, and CGRAM as separate sources of truth. A
  visual bug is usually one stage in that pipeline, not "graphics" in the
  abstract.
- Prefer proving a rendering bug with generated images over arguing by code
  inspection alone.
- Make one narrow change at a time. Broad VRAM shuffles and global remaps tend
  to damage unrelated graphics.
- When something is visual and user-facing, compare against a known reference
  image before claiming it is fixed.

## Primary sources — check these FIRST

For any emulation or rendering bug, reach for these before reading Rust code:

- `../SMWDisX/` — the vanilla SMW disassembly. For SMW-specific behavior this
  settles questions in seconds. Grep it immediately when a routine name is known.
  ```
  grep -n "ROUTINE_NAME\|keyword" ../SMWDisX/bank_00.asm
  grep -rn "keyword" ../SMWDisX/
  ```
- `symbols/SMW_U.sym` — address-to-name mappings; grep to find which bank a
  routine lives in before opening that bank's ASM file.
- emulator state in `crates/smwe-emu`
- renderer code in `crates/smwe-render`
- editor entry points in `src/ui/world_editor.rs` and
  `src/ui/editor_prototypes/level_editor`

**If you have read the same Rust file more than twice looking for a bug — stop.
Check the disassembly or generate an image. Ruminating on code in memory is the
most expensive path in this repo.**

## Core workflow

1. Reproduce the bug locally.
2. **If the bug involves any named emulator routine, grep SMWDisX for it before
   reading Rust.** The ASM shows exactly what the real game does; the Rust is an
   approximation.
3. Find the exact render path used by the editor.
4. Confirm whether the bug is in:
   - decompressed WRAM data
   - composed VRAM tilemaps
   - palette upload
   - tile decode
   - UI transform math
   - camera / viewport selection
5. Generate a local image if the bug is visual.
6. Compare against ASM and only then patch code.
7. Rebuild with `cargo check --lib`.

If the bug is ambiguous, isolate it by turning layers off before editing code.
The fastest useful split in this repo is usually: `--no-sprites`, `--layer=1`,
and `--layer=2`.

## Useful local render workflows

### Overworld

- `cargo run --bin render_ow_submap -- --submap=3 --out=/tmp/forest.png`
- `cargo run --bin render_ow_submap -- --submap=0 --out=/tmp/main.png`
- Use `view_image` on the output.
- For overworld submaps, trust the composed VRAM/tilemap path over ad hoc atlas
  reconstruction when they disagree.

Key lesson:

- Non-main overworld maps are easiest to match by rendering the composed BG
  tilemaps in VRAM and cropping to the actual submap viewport.
- If a submap looks like "the main map with different colors", the usual causes
  are wrong viewport selection, wrong composed tilemap source, or missing
  submap setup in the emulation path.

### Levels

- `cargo run --bin render_level -- --level=105 --out=/tmp/level105.png`
- `cargo run --bin render_level -- --level=105 --no-sprites --out=/tmp/level105_bg.png`
- `cargo run --bin render_level -- --level=105 --layer=1 --out=/tmp/level105_l1.png`
- `cargo run --bin render_level -- --level=105 --layer=2 --out=/tmp/level105_l2.png`
- `cargo run --bin render_level -- --level=105 --inspect=272,208 --out=/tmp/ignore.png`

Use these to determine whether a bad pixel comes from layer 1, layer 2, or the
sprite path before touching editor code.

If a bad object still appears in `--no-sprites`, stop looking at OAM. That
means the problem is in level tile assembly or the BG graphics/palette path.

## Known emulation pitfalls

### Animated tiles (coins, ? blocks, turn blocks, etc.)

The full animated-tile initialization is `CODE_00A5F9` (`bank_00.asm`). It
loops 8 times through `CODE_05BB39` + `CODE_00A390` to populate every animated
VRAM slot regardless of its update interval. Calling the pair only once leaves
slots that update on longer intervals still wrong.

`fetch_anim_frame()` in `crates/smwe-emu/src/emu.rs` must call `CODE_00A5F9`,
not the raw pair directly.

If animated tiles look wrong (wrong color, look like ground tiles, etc.):
1. Check whether `fetch_anim_frame` is called after `decompress_sublevel` in
   the level load path.
2. Confirm `fetch_anim_frame` calls `CODE_00A5F9` (the 8-frame loop), not just
   one pass of `CODE_05BB39`+`CODE_00A390`.
3. grep `../SMWDisX/bank_00.asm` for `A5F9` to see what the real game does
   during level init.

### decompress_sublevel and load_overworld

These are approximations of real game flows. If a visual bug persists:
- Compare them against the actual game-mode ASM (`bank_00.asm` around
  `GM??` labels) instead of stacking more guesses on top.
- The hook at `$05D8B7` that sets `$000E` to the level ID is load-order
  sensitive; missing it causes wrong level context downstream.

### 65816 wrap semantics

Plain integer addition where the CPU expects wrapping addresses can produce
fake rendering bugs. Use wrapping arithmetic where the 65816 would wrap.

## Overworld-specific notes

- The main-map Bowser face in the glacier is event-driven. If it is missing,
  check overworld event activation first.
- The overworld init path in the emulator matters. `PrepareGraphicsFile`,
  `CODE_00AD25`, `CODE_00922F`, and `CODE_04D6E9` are the first things to check
  when non-main maps look structurally wrong.
- For submaps, "different palettes of the main map" usually means the wrong
  viewport or wrong composed tilemap source, not just a palette bug.
- Render submaps from the composed BG tilemap in VRAM. Reconstructing them from
  raw packed atlases is error-prone and was the source of several false fixes.
- Overworld overlays must use the same coordinate basis as the GL render path.
  If the hover/selection box drifts while zooming, fix the transform math
  before touching tile data.

## Level-specific notes

- The level editor currently mixes emulator-loaded data with custom rendering.
  Be explicit about which source you are using:
  - WRAM block IDs
  - VRAM tile graphics
  - CGRAM palette
- For suspicious single-tile artifacts, compare the inspected tile against the
  same screen position in `--layer=1`, `--layer=2`, and `--no-sprites` renders
  before changing palette or Map16 code.
- If a tile is "right shape, wrong graphic," check:
  - Map16 pointer lookup
  - block high-byte masking
  - BG tile-number interpretation
  - whether the problem is actually coming from layer 1 instead of sprites
- If a box/overlay drifts while panning or zooming, compare the egui origin
  math with the GL shader offset math. They must use the same local basis.

## Renderer pitfalls

- Do not assume UI coordinates and GL coordinates are aligned just because the
  map looks roughly correct at zoom 1.
- In this repo, many "selection box is floating" bugs are transform mismatches,
  not hit-test bugs.
- If a box changes relative size or position as you zoom, check whether the GL
  path and the egui overlay path are both using the same local origin and the
  same crop offset.
- Tile quadrant order is dangerous. Change it only with image proof.

## How to avoid wasting time

- **If you have read the same file more than twice without a fix — check the
  disassembly or generate a render. Do not keep re-reading Rust.**
- Do not patch globally when one tile or one layer is wrong.
- Do not assume a palette bug if the map shape is also wrong.
- Do not assume a graphics bug if the overlay is drifting.
- Do not trust "looks close enough" for submaps; compare against a reference.
- Do not stop at `cargo check` for visual bugs. Render an image.
- For any named SMW routine (`CODE_XXYYYY`), grep `../SMWDisX/` before
  guessing what it does. The answer is one grep away.

## Verification expectations

- Minimum: `cargo check --lib`
- For visual changes: generate at least one local image and inspect it
- If comparing against a user-provided reference, save the local render and line
  it up before concluding

Good enough for a visual fix means:

- the generated image matches the expected region or reference closely
- overlay boxes stay locked to the rendered content while panning and zooming
- the fix does not corrupt unrelated maps, layers, or palettes

## Repo hygiene

- Keep temporary debug binaries if they are actively useful and small.
- Remove one-off hacks, especially broad VRAM injections, once the real cause is
  understood.
- Prefer fixing shared logic over adding per-level or per-submap special cases.
