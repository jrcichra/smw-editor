#!/usr/bin/env python3
"""Verify L2 row count by finding the decompressor size parameter."""
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

# LC_RLE2 decompressor at $04DABA:
# The loop at +48: CPX $0E -> compares X (dest counter) with $0E
# $0E was set to #$4000 -> so it decompresses until X >= 0x4000
# That means: 0x4000 / 2 = 0x2000 = 8192 entries? No wait...
# The decompressor writes every OTHER byte (interleaved).
# X increments by 2 for each tile written.
# Loop exits when X >= $0E = $4000.
# So total bytes in output buffer = $4000 = 16384 bytes
# Number of tiles = 16384 / 2 = 8192? That can't be right for 40x61=2440.

# Actually looking again at the decompressor:
# The RLE2 writes to $7F4000+X (long,X indexed).
# X starts at 0 and increments by 2.
# Loop runs until X >= $0E.
# But we said $0E = $4000... hmm. Let me re-read.
# 
# Wait - the size passed is $4000 but that's the DESTINATION OFFSET ($7F4000).
# The COUNTER is in $03 (the length field from the compression).
# $0E might be used differently.
#
# Re-read the decompressor bytes:
# e2 20    SEP #$20  -> 8-bit A
# c2 10    REP #$10  -> 16-bit X,Y
# b7 00    LDA ($00),Y  <- read compressed data byte
# 85 03    STA $03      <- store to $03
# 29 80    AND #$80     <- check command bit
# d0 10    BNE ...      <- branch if RLE
# c8       INY           <- advance source
# b7 00    LDA ($00),Y  <- read data byte
# 9f 00 40 7f STA $7F4000,X  <- write to output
# e8       INX
# e8       INX          <- X += 2 (skip odd byte)
# c6 03    DEC $03      <- decrement length counter
# 10 f3    BPL ...      <- loop until negative
# 4c e9 da JMP $DAE9   <- next chunk
# [RLE branch:]
# a5 03    LDA $03
# 29 7f    AND #$7F     <- get length
# 85 03    STA $03
# c8       INY
# b7 00    LDA ($00),Y  <- get run value
# 9f 00 40 7f STA $7F4000,X
# e8,e8    INX,INX
# c6 03    DEC $03
# 10 f6    BPL ...
# c8       INY
# e4 0e    CPX $0E      <- HERE: compare X with $0E
# 90 cc    BCC ...      <- if X < $0E, continue outer loop
# 60       RTS

# So $0E is the OUTER loop exit condition for X (total output bytes written/2).
# $0E was set to $4000 in the caller! 
# So the total output is $4000 bytes, meaning $4000/2 = 0x2000 tiles?!
# But 40 cols * 0x2000/40 = 204 rows?! That's way too many.

# Wait, I misread. STY $0E where Y = #$4000.
# But Y is the DESTINATION, not the size. Let me re-read CODE_04DC6F:
code_pc = lorom_to_pc(0x04DC6F)
data = rom[code_pc:code_pc+64]
print("CODE_04DC6F bytes:")
for i, b in enumerate(data):
    print(f"  +{i:2d}: {b:02x}", end='')
    if i % 8 == 7:
        print()
print()

# c2 20    REP #$20    (16-bit A)
# a9 33 a5 LDA #$A533  (L2 tile data low word)
# 85 00    STA $00
# e2 30    SEP #$30    (8-bit A,X)
# a9 04    LDA #$04    (bank $04)
# 85 02    STA $02
# c2 10    REP #$10    (16-bit X)
# a0 00 40 LDY #$4000  <-- destination offset $4000 (= $7F4000)
# 84 0e    STY $0E     <-- $0E = $4000 (this IS the size limit? or just dest?)

# Hmm, the decompressor loop at +44: CPX $0E
# If $0E = $4000 and X starts at 0, then the first pass writes 0x4000/2 = 0x2000 tiles.
# But our decompress showed only 2455 tiles before the data ran out.
# That means the COMPRESSED DATA runs out before X reaches $4000.
# The decompressor just stops when it reaches end of compressed data naturally.
# So 2455 tiles is the actual content; the $4000 is a safety max.

# Therefore: OW_L2_ROWS = 2455 / 40 = 61 rows (our calculation was right!)
# But the second pass gave 2560 tiles. Which is authoritative?
# The game only decompresses as much as needed. The actual map data determines
# what gets written. Let me use min(pass1, pass2) / 40 = 61.

# BUT WAIT: let me check if the L2 is actually 27 rows for the main map,
# not 61. The reference PPM is 320x216 = 40x27.
# Perhaps the L2 wraps the main map + submaps differently than L1.
# Let me look at what actual L2 content looks like around row 27-35:

def decompress_lc_rle2_full(data, max_out):
    out = bytearray(max_out)
    pos = 0; out_pos = 0
    while out_pos < max_out and pos < len(data):
        hdr = data[pos]; pos += 1
        cmd = (hdr >> 7) & 1
        length = (hdr & 0x7F) + 1
        if cmd == 0:
            for _ in range(length):
                if out_pos >= max_out or pos >= len(data): break
                out[out_pos] = data[pos]; pos += 1
                out_pos += 2
        else:
            if pos >= len(data): break
            val = data[pos]; pos += 1
            for _ in range(length):
                if out_pos >= max_out: break
                out[out_pos] = val; out_pos += 2
    return out, out_pos // 2

l2_tile_pc = lorom_to_pc(0x04A533)
l2_attr_pc = lorom_to_pc(0x04C02B)
max_buf = 40 * 70 * 2

tile_out, n1 = decompress_lc_rle2_full(rom[l2_tile_pc:l2_tile_pc+0x2000], max_buf)
attr_out = bytearray(max_buf)
pos2 = 0; out_pos2 = 1
data2 = rom[l2_attr_pc:l2_attr_pc+0x2000]
n2_tiles = 0
while out_pos2 < max_buf and pos2 < len(data2):
    hdr = data2[pos2]; pos2 += 1
    cmd = (hdr >> 7) & 1
    length = (hdr & 0x7F) + 1
    if cmd == 0:
        for _ in range(length):
            if out_pos2 >= max_buf or pos2 >= len(data2): break
            attr_out[out_pos2] = data2[pos2]; pos2 += 1
            out_pos2 += 2
    else:
        if pos2 >= len(data2): break
        val = data2[pos2]; pos2 += 1
        for _ in range(length):
            if out_pos2 >= max_buf: break
            attr_out[out_pos2] = val; out_pos2 += 2
n2_tiles = (out_pos2 - 1) // 2

print(f"Pass1 tiles: {n1}, pass2 tiles: {n2_tiles}")
actual = min(n1, n2_tiles)
rows = actual // 40
print(f"Total tiles: {actual}, rows: {rows}")

# Sample rows 25-35 to see where the map changes
print("\nRows 25-35, col 0-3 (tile_num, attr):")
for row in range(25, min(38, rows)):
    tiles = []
    for col in range(4):
        off = ((row * 40) + col) * 2
        t = tile_out[off]
        a = attr_out[off+1] if off+1 < max_buf else 0
        tiles.append(f"({t:#04x},{a:#04x})")
    print(f"  row {row:3d}: {' '.join(tiles)}")

# Now let's answer the key question: does the L2 cover both main map AND submaps?
# The main map L1 is 32x32 map16 blocks = 32*16 = 512 game pixels.
# L2 at 8px/tile: 512/8 = 64 rows would be needed to cover all of L1.
# But we only have 61 rows. Close.
# 
# Actually: the SNES OW L2 is a scrolling BG. The game sets BG2VOFS and BG2HOFS
# to scroll L2 independently. The "parallax" effect.
# For the EDITOR we should show the L2 as a static background aligned to L1.
# 
# The correct approach: render L2 at 0,0 (40*8=320 wide, rows*8 tall),
# render L1 on top at 0,0 (32*16=512 wide, 32*16=512 tall).
# They share the same pixel coordinate system.

print(f"\nConclusion: OW_L2_ROWS should be {rows}")
print(f"L2 covers {rows*8} game pixels height, L1 covers {32*16} game pixels height")
print(f"Canvas should be max of both: {max(rows*8, 32*16)} x {max(40*8, 32*16)} game pixels")
