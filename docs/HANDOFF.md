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

### 3. .mwl import/export

Format spec: https://github.com/kaizoman666/SMW-Data/blob/master/Misc/MWL%20File%20Format.md
(fetch it; it documents header, per-section offsets — level header, Layer 1,
Layer 2, sprites, palette, ExAnimation, etc.). Suggested scope for a first
pass: export/import of header + Layer 1 objects + Layer 2 + sprites for
vanilla-format levels, refusing files with LM-specific sections we don't
model yet (better to error clearly than corrupt). Put the codec in
`smwe-rom` (e.g. `src/mwl.rs`) with round-trip tests against a level
serialized from the real ROM; wire File-menu items in the level editor.

### 4. Sprite categories (cluster/extended/generator/shooter)

`SpriteTweakers::has_tweakers()` already marks IDs >= 0xC9. Missing: a
`SpriteCategory` enum in `smwe-rom` (normal / shooter 0xC9-0xCB? /
generator 0xD0+? — verify exact ID ranges in SMWDisX `bank_02.asm` sprite
spawn code, don't trust these guesses), category display in the sprite
catalog/picker, and correct rendering/preview handling for non-normal
categories (they don't use the standard 12-slot tables; `exec_sprite_id`
previews will be wrong for them — at minimum label them and skip preview).
Full "custom sprite insertion" (PIXI-style) is a separate large project; do
not start it as a side effect.

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
- Repeated saves with OW level-number overrides orphan 0x800-byte tables
  (documented in `world_editor/mod.rs::save_to_rom`).
- `docs/LUNAR_MAGIC_PARITY.md` still lists smaller ⛔ rows (animated OW
  tiles, Layer-2 event tiles at `$04DD8D`, L2 scroll settings, 8x8 tile
  import/export, custom OW level names — item 1 above); keep flipping rows
  as things land.

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
