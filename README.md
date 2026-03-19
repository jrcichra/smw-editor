# SMW Editor

> [!NOTE]
> This is an AI-generated community fork of the [original SMW Editor](https://github.com/SMW-Editor/smw-editor).

SMW Editor is an open-source, multi-platform, modern alternative to Lunar Magic,
providing all the necessary tools for SMW romhacking. It uses a built-in emulator
to decompress and render graphics directly from the ROM, ensuring accurate
visualization of vanilla SMW content.

## Features

### Currently Functional

- **Level Editor** — View and navigate levels rendered via the emulator's
  decompression routines. Supports zoom, grid overlay, layer toggles, and
  object layer visualization with exit markers.
- **Overworld Editor** — Browse all 7 submaps rendered from composed VRAM
  tilemaps. Toggle layer 1/2, pan and zoom, and inspect individual tiles.
- **Address Converter** — Convert between PC and SNES address spaces with
  LoROM/HiROM and header options.
- **ROM Loading** — Parses standard SMW ROMs with internal header detection.
  Persists recent files between sessions.

### In Development

- Sprite tile editor with VRAM browser and palette viewer
- Block editor UI (viewing and editing not yet implemented)

### Planned

- Level editing (object placement, tile modification)
- Overworld editing (tile placement, event editing)
- Block editor (custom block creation)
- Graphics editor
- ASM code editor
- Music editor
- Custom plugins and extensions
- Multiple language support

## Building

Make sure you have [rustup](https://rustup.rs/) installed.

```bash
cargo run --release
```

Set the `ROM_PATH` environment variable to load a ROM on startup:

```bash
ROM_PATH=/path/to/smw.smc cargo run --release
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


