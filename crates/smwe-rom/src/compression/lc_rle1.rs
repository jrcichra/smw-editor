use thiserror::Error;

use crate::compression::DecompressionError;

// -------------------------------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum LcRle1Error {
    #[error("Wrong command: {0:03b}")]
    Command(u8),
    #[error("Direct Copy - Cannot read {0} bytes")]
    DirectCopy(usize),
    #[error("Byte Fill - Cannot read byte")]
    ByteFill,
}

// -------------------------------------------------------------------------------------------------

const COMMAND_DIRECT_COPY: u8 = 0;
const COMMAND_BYTE_FILL: u8 = 1;

// -------------------------------------------------------------------------------------------------

/// Returns decompressed data and the size of compressed data.
pub fn decompress(input: &[u8]) -> Result<(Vec<u8>, usize), DecompressionError> {
    assert!(!input.is_empty());
    assert!(input.len() >= 2);
    let mut output = Vec::with_capacity(input.len() * 2);
    let mut in_it = input;
    while let Some(chunk_header) = in_it.first().copied() {
        if chunk_header == 0xFF && (in_it.len() == 1 || in_it[1] == 0xFF) {
            break;
        }
        in_it = &in_it[1..];
        let command = chunk_header >> 7;
        let length = (chunk_header & 0b01111111) as usize + 1;

        match command {
            COMMAND_DIRECT_COPY => {
                if length <= in_it.len() {
                    let (bytes, rest) = in_it.split_at(length);
                    output.extend_from_slice(bytes);
                    in_it = rest;
                } else {
                    return Err(LcRle1Error::DirectCopy(length).into());
                }
            }
            COMMAND_BYTE_FILL => {
                let byte = *in_it.first().ok_or(LcRle1Error::ByteFill)?;
                output.resize(output.len() + length, byte);
                in_it = &in_it[1..];
            }
            _ => unreachable!(),
        }
    }

    output.shrink_to_fit();
    // Advance past the terminator byte(s) so bytes_consumed reflects the true
    // on-disk size, matching what compress() emits (0xFF 0xFF).
    if in_it.first() == Some(&0xFF) {
        in_it = &in_it[1..];
        if in_it.first() == Some(&0xFF) {
            in_it = &in_it[1..];
        }
    }
    let bytes_consumed = input.len() - in_it.len();
    Ok((output, bytes_consumed))
}

pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut i = 0usize;
    while i < input.len() {
        let run_len = count_run(input, i);
        // Use byte-fill only for runs of 3+. A 2-byte run saves nothing over
        // direct-copy (both cost 2 bytes for the run itself) but forces an
        // extra chunk header for whatever comes before/after, so it's a net
        // loss in the common case.
        if run_len >= 3 {
            let len = run_len.min(128);
            output.push(0x80 | ((len - 1) as u8));
            output.push(input[i]);
            i += len;
        } else {
            let start = i;
            i += 1;
            while i < input.len() {
                let run = count_run(input, i);
                if run >= 3 || i - start >= 128 {
                    break;
                }
                i += 1;
            }
            let len = i - start;
            output.push((len - 1) as u8);
            output.extend_from_slice(&input[start..start + len]);
        }
    }
    output.push(0xFF);
    output.push(0xFF);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(data: &[u8]) {
        let compressed = compress(data);
        let (decompressed, bytes_consumed) = decompress(&compressed).expect("decompress failed");
        assert_eq!(decompressed, data, "round-trip data mismatch");
        assert_eq!(bytes_consumed, compressed.len(), "bytes_consumed should equal compressed len");
    }

    #[test]
    fn round_trip_empty() {
        round_trip(&[]);
    }

    #[test]
    fn round_trip_all_same() {
        round_trip(&[0xABu8; 200]);
    }

    #[test]
    fn round_trip_no_runs() {
        let data: Vec<u8> = (0u8..=127).collect();
        round_trip(&data);
    }

    #[test]
    fn round_trip_mixed() {
        // varied bytes with occasional runs — the pattern that exposed the 2-byte-run bug
        let data = [0x00u8, 0x10, 0x10, 0x20, 0x30, 0x30, 0x30, 0x40, 0x50, 0x50, 0x60];
        round_trip(&data);
    }

    #[test]
    fn two_byte_run_not_inflated() {
        // [B, A, A, C] — a 2-byte run flanked by varied bytes.
        // With the old threshold of >=2 this produced 3 chunks (6 bytes + terminator).
        // With threshold >=3 it folds into one direct-copy (5 bytes + terminator).
        let data = [0x01u8, 0xAA, 0xAA, 0x02];
        let compressed = compress(&data);
        // One direct-copy chunk header + 4 bytes + 2-byte terminator = 7 bytes.
        assert_eq!(compressed.len(), 7, "2-byte run should not break into a separate fill chunk");
        round_trip(&data);
    }

    #[test]
    fn three_byte_run_uses_fill() {
        // A run of 3+ should still use the more compact byte-fill encoding.
        let data = [0x01u8, 0xAA, 0xAA, 0xAA, 0x02];
        let compressed = compress(&data);
        // Expect: [0x00, 0x01] direct-copy B + [0x82, 0xAA] fill 3 + [0x00, 0x02] direct-copy C + terminator
        // = 2 + 2 + 2 + 2 = 8 bytes
        assert_eq!(compressed.len(), 8);
        round_trip(&data);
    }
}
