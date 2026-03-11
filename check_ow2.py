import struct

with open("/home/justin/git/smw-editor/smw.smc", "rb") as f:
    rom = f.read()

# Strip 512-byte SMC header
rom = rom[0x200:]

def lorom_to_pc(snes):
    if snes & 0x8000 == 0:
        return None
    return ((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)

# The OW layer2 base is 0x0C8000 = PC 0x060000
ow_layer2_pc_start = lorom_to_pc(0x0C8000)
ow_layer2_pc_end = lorom_to_pc(0x0C8000) + 6 * 0x800  # 6 submaps
ow_layer1_pc_start = lorom_to_pc(0x0CAC00)
ow_layer1_pc_end = lorom_to_pc(0x0CAC00) + 6 * 0x800

print(f"OW layer2: PC 0x{ow_layer2_pc_start:06x} - 0x{ow_layer2_pc_end:06x}")
print(f"OW layer1: PC 0x{ow_layer1_pc_start:06x} - 0x{ow_layer1_pc_end:06x}")

# SMW level layer1 data pointer table is at bank05 $8000
# Level pointer table: $058000, 512 levels * 3 bytes
ptr_base_snes = 0x058000
ptr_base_pc = lorom_to_pc(ptr_base_snes)
print(f"\nLevel L1 ptr table at PC 0x{ptr_base_pc:06x}")

# Check a handful of level pointers
for lvl in range(0, 20):
    off = ptr_base_pc + lvl * 3
    b0, b1, b2 = rom[off], rom[off+1], rom[off+2]
    snes = b0 | (b1 << 8) | (b2 << 16)
    pc = lorom_to_pc(snes)
    if pc is not None:
        # Is this in the OW range?
        if ow_layer2_pc_start <= pc < ow_layer2_pc_end or ow_layer1_pc_start <= pc < ow_layer1_pc_end:
            print(f"  Level {lvl:#05x}: L1 ptr SNES={snes:#08x} PC={pc:#08x} *** IN OW RANGE ***")
