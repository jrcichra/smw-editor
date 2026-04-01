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

pub fn compressed_size_for_output(input: &[u8], output_len: usize) -> usize {
    let mut inp = 0usize;
    let mut outp = 0usize;
    while outp < output_len && inp < input.len() {
        let header = input[inp];
        inp += 1;
        let length = (header & 0x7F) as usize + 1;
        if (header >> 7) & 1 == 0 {
            inp += length.min(output_len.saturating_sub(outp));
        } else if inp < input.len() {
            inp += 1;
        }
        outp += length.min(output_len.saturating_sub(outp));
    }
    inp
}

pub fn compress_pass(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut i = 0usize;
    while i < input.len() {
        let run_len = count_run(input, i);
        if run_len >= 2 {
            let len = run_len.min(128);
            output.push(0x80 | ((len - 1) as u8));
            output.push(input[i]);
            i += len;
        } else {
            let start = i;
            i += 1;
            while i < input.len() {
                let run = count_run(input, i);
                if run >= 2 || i - start >= 128 {
                    break;
                }
                i += 1;
            }
            let len = i - start;
            output.push((len - 1) as u8);
            output.extend_from_slice(&input[start..start + len]);
        }
    }
    output
}

fn count_run(input: &[u8], start: usize) -> usize {
    let byte = input[start];
    let mut len = 1usize;
    while start + len < input.len() && input[start + len] == byte && len < 128 {
        len += 1;
    }
    len
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
