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

## Primary sources

Use these first:

- `../SMWDisX/` (the vanilla SMW disassembly, useful to check baseline mechanics)
- `symbols/SMW_U.sym`
- emulator state in `crates/smwe-emu`
- renderer code in `crates/smwe-render`
- editor entry points in `src/ui/world_editor.rs` and
  `src/ui/editor_prototypes/level_editor`

Do not rely on memory when the ASM is available. For SMW-specific behavior,
the disassembly usually settles the question quickly.

## Core workflow

1. Reproduce the bug locally.
2. Find the exact render path used by the editor.
3. Confirm whether the bug is in:
   - decompressed WRAM data
   - composed VRAM tilemaps
   - palette upload
   - tile decode
   - UI transform math
   - camera / viewport selection
4. Generate a local image if the bug is visual.
5. Compare against ASM and only then patch code.
6. Rebuild with `cargo check --lib`.

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

## Emulator pitfalls

- `decompress_sublevel()` and `load_overworld()` are approximations of real game
  flows. If a visual bug persists, compare them against the actual game-mode ASM
  instead of stacking more guesses on top.
- If you attempt to run a more complete in-game routine sequence and hit an
  emulator panic or illegal instruction, back the experiment out before moving
  on.
- 65816 wrap semantics matter. Plain integer addition where the CPU expects
  wrapping addresses can produce fake rendering bugs.

## How to avoid wasting time

- Do not patch globally when one tile or one layer is wrong.
- Do not assume a palette bug if the map shape is also wrong.
- Do not assume a graphics bug if the overlay is drifting.
- Do not trust "looks close enough" for submaps; compare against a reference.
- Do not stop at `cargo check` for visual bugs. Render an image.

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
