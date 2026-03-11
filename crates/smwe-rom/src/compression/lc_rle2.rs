/// LC_RLE2 decompressor.
///
/// Format: each chunk has a 1-byte header `CLLLLLLL` where C=command, L=length.
///   C=0: "Direct Copy" — followed by (L+1) bytes copied verbatim
///   C=1: "RLE"         — followed by 1 byte repeated (L+1) times
///
/// The key property of RLE2: it's **two-dimensional**. The decompressor is run
/// twice over the same two input streams, writing to alternating bytes of the
/// output buffer:
///   - First pass  → bytes 0, 2, 4, … (tile-number stream)
///   - Second pass → bytes 1, 3, 5, … (YXPCCCTT stream)
///
/// Callers must supply `output_len` (the total decompressed byte count, i.e.
/// number of 16-bit tile entries × 2) because there is no end-of-data marker.

pub fn decompress_rle2(tile_data: &[u8], attr_data: &[u8], output_len: usize) -> Vec<u16> {
    let n_tiles = output_len / 2;
    let mut tile_nums = vec![0u8; n_tiles];
    let mut tile_attrs = vec![0u8; n_tiles];

    decompress_pass(tile_data, &mut tile_nums);
    decompress_pass(attr_data, &mut tile_attrs);

    tile_nums.iter().zip(tile_attrs.iter()).map(|(&t, &a)| u16::from_le_bytes([t, a])).collect()
}

fn decompress_pass(input: &[u8], out: &mut [u8]) {
    let mut inp = 0usize;
    let mut outp = 0usize;
    while outp < out.len() && inp < input.len() {
        let header = input[inp];
        inp += 1;
        let command = (header >> 7) & 1;
        let length = (header & 0x7F) as usize + 1;
        if command == 0 {
            // Direct copy
            for _ in 0..length {
                if outp < out.len() && inp < input.len() {
                    out[outp] = input[inp];
                    outp += 1;
                    inp += 1;
                }
            }
        } else {
            // RLE
            if inp < input.len() {
                let byte = input[inp];
                inp += 1;
                for _ in 0..length {
                    if outp < out.len() {
                        out[outp] = byte;
                        outp += 1;
                    }
                }
            }
        }
    }
}
