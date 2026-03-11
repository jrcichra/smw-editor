import os

def lorom(snes):
    # From smwe-rom Mapper::LoRom: Some((addr & 0x7F0000) >> 1 | (addr & 0x7FFF))
    # But this only works if addr & 0x8000 != 0
    if snes & 0x8000 == 0:
        return None
    return ((snes & 0x7F0000) >> 1) | (snes & 0x7FFF)

snes_addr = 0x0C8000
pc = lorom(snes_addr)
print(f"SNES {snes_addr:#08x} -> PC {pc:#08x} = {pc}")

size = os.path.getsize("/home/justin/git/smw-editor/smw.smc")
# SMC has 0x200 header, smwe-rom strips it
print(f"ROM file size: {size:#x}, minus 0x200 header = {size-0x200:#x}")
pc_no_header = pc
print(f"PC {pc_no_header:#x} in range? {pc_no_header < size - 0x200}")

# Read some bytes at that offset
with open("/home/justin/git/smw-editor/smw.smc", "rb") as f:
    f.seek(0x200 + pc_no_header)  # skip SMC header
    data = f.read(16)
print(f"Bytes at PC {pc_no_header:#x}: {data.hex()}")
