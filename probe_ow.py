import struct

with open('smw.smc','rb') as f:
    data = bytearray(f.read())

if len(data) % 0x8000 == 0x200:
    data = data[0x200:]
    print(f'ROM {len(data):#x} bytes after strip')

def lorom(s):
    if not (s & 0x8000): return None
    return int(((s & 0x7F0000) >> 1) | (s & 0x7FFF))

addrs = [
    (0x0C8000,"L2-sm0"), (0x0C8800,"L2-sm0+800"),
    (0x0CA000,"L2+2000"), (0x0CB000,"L2+3000"),
    (0x0CC000,"L2+4000"), (0x0CD000,"L2+5000"),
    (0x0CE000,"L2+6000"), (0x0CF000,"L2+7000"),
    (0x058000,"05:8000"), (0x05C000,"05:C000"),
    (0x05E000,"05:E000"), (0x05F000,"05:F000"),
]
for snes,lbl in addrs:
    p = lorom(snes)
    if p and p+8 < len(data):
        vals = [struct.unpack_from('<H',data,p+i*2)[0] for i in range(4)]
        tiles = [v & 0x3FF for v in vals]
        pals  = [(v>>10)&7 for v in vals]
        raw   = data[p:p+8].hex()
        print(f'{lbl:15s} {snes:#08x} PC={p:#07x}  raw={raw}  tiles={tiles}  pals={pals}')
    else:
        print(f'{lbl:15s} {snes:#08x} OOB')

p0 = lorom(0x0C8000)
print(f'\nFirst 4 rows of 32-col tilemap at 0x0C8000 (PC={p0:#x}):')
for row in range(4):
    row_t = []
    for col in range(32):
        v = struct.unpack_from('<H', data, p0+(row*32+col)*2)[0]
        row_t.append(f'{v&0x3FF:3d}')
    print(f'  r{row:02d}: {" ".join(row_t)}')

vals_sm0 = [struct.unpack_from('<H',data,p0+i*2)[0] & 0x3FF for i in range(32*27)]
print(f'\nSubmap0: min={min(vals_sm0)} max={max(vals_sm0)} unique={len(set(vals_sm0))}')
