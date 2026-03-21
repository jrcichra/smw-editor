# SMW Editor

> [!NOTE]
> This is an AI-generated community fork of the [original SMW Editor](https://github.com/SMW-Editor/smw-editor).

![Overworld Editor](assets/overworld.png)

SMW Editor is an open-source, multi-platform, modern alternative to Lunar Magic,
providing all the necessary tools for SMW romhacking. It uses a built-in emulator
to decompress and render graphics directly from the ROM, ensuring accurate
visualization of vanilla SMW content.

## Features

### Currently Functional

- **Overworld Editor** — Browse and edit all 7 submaps rendered from composed VRAM
  tilemaps. Paint tiles with a visual picker, erase, switch between L1/L2, and
  inspect individual tiles with rendered previews.
- **Level Editor** — View, navigate, and edit levels rendered via the emulator's
  decompression routines. Paint blocks with a Map16 tile picker, erase objects,
  switch between L1/L2, inspect tile contents, and undo/redo changes.
- **Sprite Tile Editor** — Place, move, delete, flip, and copy/paste tiles on a
  32x32 canvas with VRAM browser, palette selection, and full undo/redo.
- **Address Converter** — Convert between PC and SNES address spaces with
  LoROM/HiROM and header options.
- **ROM Loading** — Parses standard SMW ROMs with internal header detection.
  Persists recent files between sessions.

### Editor Controls (Level & Overworld)

Both editors share the same controls:

| Key | Action |
|-----|--------|
| `1` | Select mode — click to inspect tiles |
| `2` | Draw mode — pick a tile from the picker, click to paint |
| `3` | Erase mode — click to delete/blank tiles |
| `Scroll wheel` | Zoom |
| `Middle-mouse drag` | Pan |
| `Shift` | Show grid overlay |
| `Alt+click` | Inspect block ID at tile |

Level editor additionally supports:

| Key | Action |
|-----|--------|
| `4` | Probe mode — click to inspect objects |
| `Ctrl+Z` / `Ctrl+Y` | Undo / Redo |
| `Delete` | Delete selected object |

![Level Editor](assets/level_editor.png)

### In Development

- Block editor UI (viewing and editing not yet implemented)
- Level save/export to ROM
- Overworld undo/redo

### Planned

- Block editor (custom block creation)
- Graphics editor
- ASM code editor
- Music editor
- Custom plugins and extensions
- Multiple language support

## Getting Started

Make sure you have [rustup](https://rustup.rs/) installed, then build and launch the editor:

```bash
cargo run --release
```

The editor opens with an empty workspace. Use **File > Open ROM** (or drag and
drop an `.smc`/`.sfc` file onto the window) to load a Super Mario World ROM.
Once loaded, open an editor tab from the **Editors** menu.

To open a ROM directly from the command line:

```bash
ROM_PATH=/path/to/smw.smc cargo run --release
```

### Render Binaries

The repository also includes CLI tools for rendering levels and overworld maps
to PNG files (useful for debugging and comparison):

```bash
# Render a specific level
cargo run --bin render_level -- --level=105 --out=level.png

# Render an overworld submap
cargo run --bin render_ow_submap -- --submap=3 --out=forest.png
```

## Technical Overview

The editor is structured around a workspace of crates:

- **smwe-emu** — 65816 CPU emulator with accurate WRAM, VRAM, CGRAM, and DMA
  emulation
- **smwe-rom** — ROM parsing for levels, graphics, Map16, and overworld data
- **smwe-render** — OpenGL tile and palette rendering with geometry shaders for
  efficient batching
- **smwe-widgets** — Reusable UI components (VRAM viewer, palette grid)
- **smwe-math** — Coordinate type wrappers for consistent math across renderers

Rendering is backed by the emulator where possible — levels are decompressed
using the actual game code rather than ad hoc reconstruction, which keeps
visuals synchronized with vanilla SMW behavior.

## Contribution

This is a community fork of the [original SMW Editor](https://github.com/SMW-Editor/smw-editor).
Contributions are welcome — open an issue or pull request to discuss changes.
AI-assisted contributions are accepted, but must include screenshots demonstrating they work.

If you're looking to contribute, experience in any of the following is helpful:
- [Rust](https://www.rust-lang.org/)
- ASM programming for the 65816/SNES
- SMW romhacking and disassembly
- UI design with egui

## License

This project is licensed under the MIT License.
