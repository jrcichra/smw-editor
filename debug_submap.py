#!/usr/bin/env python3
"""Trace CODE_00AD25 palette loading to see if it reads $0DD6."""
import struct

ROM_PATH = '/home/justin/git/smw-editor/smw.smc'

def lorom_to_pc(snes_addr):
    bank = (snes_addr >> 16) & 0x7F
    offset = snes_addr & 0xFFFF
    if offset < 0x8000:
        return None
    return (bank << 15) | (offset & 0x7FFF)

with open(ROM_PATH, 'rb') as f:
    rom = bytearray(f.read())
if len(rom) % 0x400 == 0x200:
    rom = rom[0x200:]

# From CODE_00AD25 bytes:
# c2 30        REP #$30     (16-bit A, X, Y)
# a0 d8 b3     LDY #$B3D8   (Y = some address)
# ad ea 1e     LDA $1EEA
# 10 03        BPL +3
# a0 32 b7     LDY #$B732   (alternate address)
# b7 84        LDA ($84),Y  <- indirect load via $84/$85/$86 (24-bit pointer)
# 00
# ...
# ad 31 19     LDA $1931    <- reads the GFX file number set by CODE_04DC09!
# 29 0f 00     AND #$000F
# ...

# CODE_00AD25 DOES read $1931 which was set based on $0DD6.
# So our fix propagates correctly.

# Now let's check: what does $1EEA contain and why is CODE_00AD25 checking it?
# $1EEA is likely a "boss defeated" or "event" flag that switches palettes.
# This shouldn't affect submap differentiation.

# CRITICAL: What about the palette base address?
# The routine uses LDY #$B3D8 then LDA ($84),Y where $84 is set to 0 (bank 0?).
# It's loading from $00B3D8 which is RAM bank $7E/$00.
# So the palette data comes from somewhere in WRAM that gets set up.

# Let's look at what CODE_00AD25 actually does step by step:
# From bytes (full 100 bytes):
ad25_pc = lorom_to_pc(0x00AD25)
data = rom[ad25_pc:ad25_pc+160]
print(f"CODE_00AD25 full bytes (160):")
for i in range(0, 160, 16):
    print(f"  +{i:3d}: {' '.join(f'{b:02x}' for b in data[i:i+16])}")

# Let's also check what OW palette table is at $00ABxx:
# From bytes: b9 1e ad = LDA $AD1E,Y - loads from ROM $00AD1E
pal_table_pc = lorom_to_pc(0x00AD1E)
print(f"\nPalette table at $00AD1E (PC {pal_table_pc:#08x}):")
print(f"  First 48 bytes: {rom[pal_table_pc:pal_table_pc+48].hex()}")

# The key question for the bug:
# When we switch from submap 0 to submap 1, does JUST calling load_overworld again
# properly update all data? The CPU state persists, so if CODE_04DC09 runs with
# $0DD6=4 (submap 1), it should set $1931=$12 (GFX file 18 for submap 1).
# Then UploadSpriteGFX uses $192B and $1931 to load the right GFX.

# Let me verify CODE_04DC09 stores to $1931:
# From disassembly:
# +8:  LDA $1F11,X -> gets submap value
# +11: TAX
# +12: LDA $04DC02,X -> gfx_file = $11+submap
# +16: STA $1931  -> stores gfx file number

# YES. $1931 gets updated based on $0DD6.

# Now: does UploadSpriteGFX use $1931 or $192B?
# From bytes: a9 80 8d 15 21 a2 03 ad 2b 19 0a 0a a8...
# LDA $192B, ASL, ASL, TAY -> uses $192B (not $1931)
# So we need to check if CODE_04DC09 also updates $192B.

# From disassembly of CODE_04DC09:
# +19: LDA #$11 ($11 = decimal 17)  
# +21: STA $192B   <- hardcoded $11? Or does it change?
# Wait, let me re-read: a9 11 8d 2b 19 = LDA #$11, STA $192B
# That's HARDCODED to $11! Not dependent on submap!
# So UploadSpriteGFX always uses GFX file 17 (the main OW GFX), regardless of submap.
# That makes sense for the base OW tileset.

# And STA $1931: what's there?
# +16: 8d 31 19 = STA $1931 -- stores the per-submap GFX file ($11-$17).
# That's the SUBMAP-specific GFX (different graphics per OW area).
# But is $1931 actually USED by UploadSpriteGFX? 
# UploadSpriteGFX reads $192B (=17, always), not $1931.

# So $1931 is maybe used by a DIFFERENT loader we're not calling.
# The GFX loading for OW submaps might happen via a different code path.

# Let me check what DOES read $1931:
print()
print("Searching for $1931 reads in ROM:")
for i in range(len(rom) - 3):
    if rom[i+1:i+3] == bytes([0x31, 0x19]):
        op = rom[i]
        if op in [0xAD, 0xBD, 0xAE]:  # LDA/LDX abs
            print(f"  PC {i:#08x}: op={op:#04x} -> reads $1931, context: {rom[i:i+6].hex()}")

# Summary of what I know:
print()
print("=== Summary ===")
print("1. OWL1TileData is the SAME for all submaps (same tile layout).")
print("   Main map: rows 0-31 (indices 0x000-0x3FF)")
print("   All submaps share: rows 32-63 (indices 0x400-0x7FF)")
print("   -> Different submaps show the SAME tile positions, just different GFX/palette.")
print()
print("2. CODE_04DC09 reads $0DD6 to determine submap -> sets $1931 (GFX file).")
print("   Our fix: store submap*4 at $0DD6 -> should work.")
print()
print("3. UploadSpriteGFX reads $192B (hardcoded $11), not $1931.")
print("   The per-submap GFX file ($1931) must be used elsewhere.")
print()
print("4. CODE_00AD25 reads $1931 for palette selection.")
print("   This IS submap-specific.")
print()
print("5. The CPU/WRAM state is REUSED between submap loads.")
print("   This could cause issues if routines check flags set by previous runs.")
