# Handoff: remaining Lunar Magic parity work

Status snapshot written 2026-07-06, after the session that landed commits
`b1af452..a8ccfb5`. This is the working list for whoever (human or agent)
picks the parity effort back up. Read `docs/LUNAR_MAGIC_PARITY.md` first for
the full feature matrix; read `AGENTS.md` for the debugging methodology that
actually works in this repo (short version: grep `../SMWDisX/` first, render
images to prove visual claims, and prefer *running the ROM's own code in the
emulator* over re-implementing it — that technique cracked every hard bug
this session).

## Done this session (context for what follows)

- **TOP2020 scrambled BG root cause** (`b1af452`): `UploadSpriteGFX`'s GFX
  decompression overruns the `$7EAD00` buffer into the `$7EB900` BG tilemap
  on LM ROMs. `decompress_sublevel`/`decompress_extram` now snapshot/restore
  that region (snapshot taken when the trampoline reaches `$2010`, i.e. right
  after `CODE_05801E`).
- **ROM expansion + checksum on save** (`641369f`): `src/rom_expand.rs`.
- **WYSIWYG message editor** (`a5d9d42`): font chart in
  `smwe-rom/src/message_boxes.rs` (`byte_to_char` etc.), validated against
  all vanilla messages.
- **OW event ownership** (`d61bbfe`): `TranslevelEvents` (`DATA_05D608`),
  editable in world editor tile-inspect panel.
- **Music track names** (`48787b2`): `LevelMusicTable` mapping.
- **LM Map16 resolution rewrite** (`a8ccfb5`): extended FG blocks and BG
  Map16 base now resolved by *executing* LM's own routines via
  `smwe_emu::emu::lm_ext_map16_data_addr` / `lm_bg_map16_base`
  (trampoline helper `run_trampoline` at fake-mem `$2F00`). Key LM facts
  discovered, all verified against TOP2020:
  - LM's GetMap16 routine lives at `$06F540` (input: 16-bit A = block_id*2;
    output: A = data addr low word, `$0C` = bank).
  - LM hijacks `$05:8DA8` (vanilla `STA $0A / LDA $1928`) with
    `JSL <bg resolver>`; the resolver reads a rack of 24-bit BG Map16 base
    pointers (TOP2020: at `$0E:FD50` → `$158000`, `$168000`) selected by the
    per-level BG flag byte at `$7FC00B` (written by LM's BG-load hijack at
    `$05:803B` → `JML $0EF510`; per-level flag table at `$0E:F310`).
  - LM BG tilemaps still decode through the vanilla RLE decoder
    (`CODE_058126`: cmd bit7 set = repeat next byte (cmd&0x7F)+1 times,
    clear = literal cmd+1 bytes).

## Remaining items, in suggested order

### 1. Overworld level names editor — DONE (2026-07-06)

Landed in `crates/smwe-rom/src/overworld/level_names.rs` + world-editor
tile-inspect panel. Key facts (all verified against smw.smc by decoding all
0x5D names): `LevelNames` $04A0FC (2B/translevel: byte1&0x7F → piece-1 table
`DATA_049C91`, byte0 hi-nibble → piece-2 `DATA_049CCF`, lo-nibble → piece-3
`DATA_049CED`; each table entry is a u16 offset into `LevelNameStrings`
$049AC5, budget 0x1CC bytes, strings end on a bit-7 byte). Display rules from
`CODE_049D07`: piece 1 skipped if first byte has bit 7, piece 2 skipped if
first byte == $9F, field is 19 tiles (truncation is per-*tile*: the squished
glyph runs $32-$37 = " ILLUSI", $38-$3C = "YELLOW" pack ~7 chars into 5-6
tiles). OW font ≠ message font: digits '1'-'7' at $64-$6A ('0','8','9'
unverified, unmapped). Rebuild uses dedupe + substring sharing (vanilla needs
441/460 bytes). Real-ROM tests: `vanilla_level_names_decode_correctly`,
`vanilla_level_names_round_trip` (run with `ROM_PATH=... --ignored`).

### 2. ExGFX colored preview + per-level GFX slot cross-link — DONE (2026-07-06)

GFX editor shows a CGRAM-row-colorized preview (reflects pending imports);
header editor FG/BG-GFX and Sprite-GFX rows show the slot files from
`OBJECTGFXLIST`/`SPRITEGFXLIST` ($00A92B/$00A8C3, helpers in
`smwe_rom::graphics`) as jump-buttons into the GFX editor.

### 3. .mwl import/export — DONE (2026-07-06)

Codec `crates/smwe-rom/src/mwl.rs`, editor glue `level_editor/mwl.rs`.
Spec facts worth keeping: 8 sections of (offset u32, size u32) pointers at
0x40; Layer-2 section header byte 6 (source-address bank) == $FF marks a
BG-tilemap level (data = interleaved u16 Map16, vs the ROM's separate
low/high blocks); sprite extension sizes aren't stored (assume 3
bytes/sprite for vanilla); LM's 5th secondary-header byte + midway +
extended bytes are preserved verbatim in `lm_level_info_extra`. Import
goes through editor state (never straight to ROM), refuses L2-kind
mismatches, and lists skipped LM-only payloads. Untested against real
Lunar Magic — if a user has LM handy, verifying an exported .mwl imports
into LM cleanly (and vice versa) is the remaining validation step.
Also fixed while in there: saving now uses the *edited* vertical flag
(toggling "Vertical Level" used to serialize coordinates in the old
orientation until a second save).

### 4. Sprite categories — DONE (2026-07-06)

`smwe_rom::sprite_categories::SpriteCategory`, ranges verified in
`bank_02.asm` `CODE_02A88C`: Normal 00-C8, Shooter C9-CA (`LoadShooter`),
Generator CB-D9 (`CurrentGenerator = id-$CA`; jump table at
`CallGenerator`), Special DA-E0 (DA-DD/DF = stationary sprite `id-$DA+4`
with status 9, DE = 5 Eeries, E0 = 3 chain platforms), Cluster E1-E6
(`CODE_02AAC0`), Undefined E7+. Catalog names all non-slot IDs; non-Normal
IDs get a color-coded placeholder instead of the old garbage OAM preview.
PIXI-style custom sprite insertion remains a separate project.

### 5. Custom block behavior via ASM hook (highest risk — attempt last)

Goal: let a block do something no vanilla ID range does, via a JSL hook in
the Map16 interaction dispatcher. Approach sketch: find the dispatch point
in SMWDisX (block interaction routines near `CODE_00F160`-ish, grep for the
"acts like" settings usage documented in `block_behavior.rs`), insert a JSL
to freespace (use `rom_freespace::find_free_space` + RATS-style guard), and
start with ONE canned behavior (e.g. "solid custom block") to prove the
hook. Verify by running the emulator against a level containing the block
and asserting Mario/sprite interaction state — never land this on code
inspection alone. If it doesn't verify cleanly, write up findings here
instead of landing.

### 6. Known non-parity followups

- `decompress_sublevel` trample fix protects `$7EB900-$7EC0FF` only; if a
  future hack's GFX overruns even further (past `$7EC100`), the same
  approach extends trivially — the measurement harness is described in
  memory `custom-layer2-background`.
- SA-1 / ExLoROM / ExHiROM mapper support still missing (parity doc, Misc).
- ~~Repeated saves with OW level-number overrides orphan 0x800-byte tables~~
  Fixed 2026-07-06: `save_to_rom` now reuses the table the patch operand
  already points at; only a still-vanilla operand ($7ED000) allocates.
- Layer-2 event tiles at `$04DD8D`: now PARSED and surfaced
  (`smwe_rom::overworld::Layer2EventTiles`; records decoded from
  `CODE_04E496`: [source stream index u16][L2 tilemap byte offset u16],
  `<0x900` = 6×6 block else 2×2). Remaining: an *editor* for the reveal
  records (LM lets you redraw event paths on the map).
- `docs/LUNAR_MAGIC_PARITY.md` still lists smaller ⛔ rows (animated OW
  tiles, L2 scroll settings, 8x8 tile import/export, drag-resize object
  handles); keep flipping rows as things land.
- .mwl support has never been validated against real Lunar Magic — if the
  user has LM available, round-tripping a file both directions is the
  outstanding check.

## Techniques that paid off (reuse these)

- **Run LM's/SMW's own code instead of reverse-implementing it**:
  `run_trampoline` in `crates/smwe-emu/src/emu.rs` executes a tiny
  instruction sequence on the fake-mem trampoline. Calibrate against a case
  with a known answer (e.g. vanilla `$0FBE` pointers) before trusting it.
- **WRAM write-watch**: copy the `decompress_sublevel` dispatch loop into a
  scratch bin, compare a few watched bytes after every `cpu.dispatch()`,
  print `pbr:pc` on change. Found the BG trample in minutes after static
  analysis had failed for days.
- **Search the ROM for known data**: python one-liners diffing/`re.finditer`
  over TOP2020 vs vanilla located relocated tables and LM pointer racks
  (search for the 3-byte little-endian SNES address of a discovered blob to
  find who points at it).
- **`dump_vram_tiles` bin**: renders VRAM as a 2bpp tile sheet; how the
  message font was found. Works for any "which tiles are these" question.
- Test ROMs in repo root: `smw.smc` (vanilla U), `TOP2020.smc` (LM hack,
  2MB). `render_level --rom=X --level=NN --layer=N --no-sprites` for
  isolation; always regression-diff vanilla renders byte-for-byte (`cmp`).
