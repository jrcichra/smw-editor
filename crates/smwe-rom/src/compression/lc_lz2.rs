use std::ops::Range;

use thiserror::Error;

use crate::compression::DecompressionError;

// -------------------------------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum LcLz2Error {
    #[error("Wrong command: {0:03b}")]
    Command(u8),
    #[error("Long Length - Wrong command: {0:03b}")]
    LongLengthCommand(u8),
    #[error("Long Length - Cannot read second byte of header")]
    LongLength,
    #[error("Direct Copy - Cannot read {0} bytes")]
    DirectCopy(usize),
    #[error("Byte Fill - Cannot read byte")]
    ByteFill,
    #[error("Word Fill - Cannot read word")]
    WordFill,
    #[error("Increasing Fill - Cannot read byte")]
    IncreasingFill,
    #[error("Repeat - Cannot read offset")]
    RepeatIncomplete,
    #[error("Repeat - Range ({}..{}) out of bounds (out buffer size: {1})", .0.start, .0.end)]
    RepeatRangeOutOfBounds(Range<usize>, usize),
    #[error("Double Long Length")]
    DoubleLongLength,
}

// -------------------------------------------------------------------------------------------------

/// Followed by (L+1) bytes of data
const DIRECT_COPY: u8 = 0b000;

/// Followed by one byte to be repeated (L+1) times
const BYTE_FILL: u8 = 0b001;

/// Followed by two bytes. Output first byte, then second, then first,
/// then second, etc. until (L+1) bytes has been outputted
const WORD_FILL: u8 = 0b010;

/// Followed by one byte to be repeated (L+1) times, but the byte is
/// increased by 1 after each write
const INCREASING_FILL: u8 = 0b011;

/// Followed by two bytes (ABCD byte order) containing address (in the
/// output buffer) to copy (L+1) bytes from
const REPEAT: u8 = 0b100;

/// This command has got a two-byte header:
/// ```text
/// 111CCCLL LLLLLLLL
/// CCC:        Real command
/// LLLLLLLLLL: Length
/// ```
const LONG_LENGTH: u8 = 0b111;

// -------------------------------------------------------------------------------------------------

pub fn decompress(input: &[u8], little_endian_in_repeat: bool) -> Result<Vec<u8>, DecompressionError> {
    decompress_with_len(input, little_endian_in_repeat).map(|(output, _)| output)
}

/// Like `decompress`, but also returns the number of input bytes consumed
/// (including the `0xFF` terminator), so a caller can know exactly how much
/// ROM space the existing compressed data occupies (e.g. to free it when
/// repointing to a new location).
pub fn decompress_with_len(
    input: &[u8], little_endian_in_repeat: bool,
) -> Result<(Vec<u8>, usize), DecompressionError> {
    assert!(!input.is_empty());

    let mut output = Vec::with_capacity(input.len() * 2);
    let mut in_it = input;
    while let Some(chunk_header) = in_it.first().copied() {
        if chunk_header == 0xFF {
            in_it = &in_it[1..];
            break;
        }
        in_it = &in_it[1..];

        let mut command = chunk_header >> 5;
        let length = match command {
            LONG_LENGTH => {
                command = (chunk_header >> 2) & 0b111;

                if !matches!(command, DIRECT_COPY..=LONG_LENGTH) {
                    return Err(LcLz2Error::LongLengthCommand(command).into());
                }

                let next_byte = *in_it.first().ok_or(LcLz2Error::LongLength)?;
                in_it = &in_it[1..];

                u16::from_le_bytes([next_byte, chunk_header & 3])
            }
            DIRECT_COPY..=0b110 => u16::from(chunk_header & 0x1F),
            _ => return Err(LcLz2Error::Command(command).into()),
        };

        let length = usize::from(length) + 1;

        match command {
            DIRECT_COPY => {
                if length <= in_it.len() {
                    let (bytes, rest) = in_it.split_at(length);
                    output.extend_from_slice(bytes);
                    in_it = rest;
                } else {
                    return Err(LcLz2Error::DirectCopy(length).into());
                }
            }
            BYTE_FILL => {
                let byte = *in_it.first().ok_or(LcLz2Error::ByteFill)?;
                output.resize(output.len() + length, byte);
                in_it = &in_it[1..];
            }
            WORD_FILL => {
                if in_it.len() >= 2 {
                    let (bytes, rest) = in_it.split_at(2);
                    output.extend(bytes.iter().cycle().take(length));
                    in_it = rest;
                } else {
                    return Err(LcLz2Error::WordFill.into());
                }
            }
            INCREASING_FILL => {
                let mut byte = *in_it.first().ok_or(LcLz2Error::IncreasingFill)?;
                output.extend(
                    std::iter::repeat_with(|| {
                        let temp = byte;
                        byte = byte.wrapping_add(1);
                        temp
                    })
                    .take(length),
                );
                in_it = &in_it[1..];
            }
            REPEAT..=LONG_LENGTH => {
                if in_it.len() >= 2 {
                    let (bytes, rest) = in_it.split_at(2);
                    let from_bytes = if little_endian_in_repeat { u16::from_le_bytes } else { u16::from_be_bytes };
                    let read_start = usize::from(from_bytes([bytes[0], bytes[1]]));
                    if read_start >= output.len() {
                        log::warn!(
                            "LC-LZ2 repeat source {}..{} exceeds current output size {}; zero-filling missing bytes",
                            read_start,
                            read_start + length,
                            output.len()
                        );
                    }
                    output.reserve(length);
                    for i in 0..length {
                        output.push(output.get(read_start + i).copied().unwrap_or(0));
                    }
                    in_it = rest;
                } else {
                    return Err(LcLz2Error::RepeatIncomplete.into());
                }
            }
            _ => unreachable!(),
        }
    }

    output.shrink_to_fit();
    let consumed = input.len() - in_it.len();
    Ok((output, consumed))
}

// -------------------------------------------------------------------------------------------------

/// Compress `input` into a valid LC_LZ2 stream that `decompress` will turn
/// back into exactly `input`.
///
/// This is intentionally simple (direct-copy runs, plus byte-fill runs of 3+
/// identical bytes) rather than a full back-reference optimizer: it is not
/// size-optimal (Lunar Magic's own compressor does better via repeat/back-
/// reference commands), but it is straightforward to verify correct, which
/// matters more for a first working compressor. Round-trip correctness is
/// covered by tests below, including against real ROM GFX file data.
pub fn compress(input: &[u8]) -> Vec<u8> {
    const MAX_CHUNK: usize = 1024;

    fn write_command_header(out: &mut Vec<u8>, command: u8, length: usize) {
        let l = (length - 1) as u16;
        if l < 32 {
            out.push((command << 5) | l as u8);
        } else {
            out.push(0xE0 | (command << 2) | ((l >> 8) as u8));
            out.push((l & 0xFF) as u8);
        }
    }

    fn flush_literal(out: &mut Vec<u8>, input: &[u8], start: usize, end: usize) {
        let mut pos = start;
        while pos < end {
            let chunk_len = (end - pos).min(MAX_CHUNK);
            write_command_header(out, DIRECT_COPY, chunk_len);
            out.extend_from_slice(&input[pos..pos + chunk_len]);
            pos += chunk_len;
        }
    }

    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    let mut literal_start = 0usize;

    while i < input.len() {
        let byte = input[i];
        let mut run = 1;
        while i + run < input.len() && input[i + run] == byte && run < MAX_CHUNK {
            run += 1;
        }
        if run >= 3 {
            flush_literal(&mut out, input, literal_start, i);
            write_command_header(&mut out, BYTE_FILL, run);
            out.push(byte);
            i += run;
            literal_start = i;
        } else {
            i += 1;
        }
    }
    flush_literal(&mut out, input, literal_start, input.len());
    out.push(0xFF);
    out
}

// -------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    fn assert_decompression(compressed: &[u8], decompressed: &[u8]) {
        let res = super::decompress(compressed, false);
        let res = res.unwrap_or_else(|err| panic!("decompression failed unexpectedly ({err})"));
        if res.as_slice() != decompressed {
            panic!("decompression gave wrong results (got: {res:?}, expected: {decompressed:?})")
        }
    }

    #[test]
    fn test_slice_repeat() {
        let compressed = [
            // Insert [1, 2, 3, 4]
            (0b011 << 5) | (4 - 1),
            1,
            // Repeat 7 bytes from address 1
            (0b100 << 5) | (7 - 1),
            0,
            1,
        ];
        assert_decompression(&compressed, &[1, 2, 3, 4, 2, 3, 4, 2, 3, 4, 2]);
    }

    #[test]
    fn test_multiple_repeat_commands_short() {
        const EXPECTED: [u8; 6] = [1, 2, 3, 4, 2, 3];
        for command in [0b100, 0b101, 0b110] {
            let compressed = [
                // Insert [1, 2, 3, 4]
                (0b011 << 5) | (4 - 1),
                1,
                // Repeat 2 bytes from address 1
                (command << 5) | (2 - 1),
                0,
                1,
            ];
            assert_decompression(&compressed, &EXPECTED)
        }
    }

    #[test]
    fn test_multiple_repeat_commands_long() {
        const EXPECTED: [u8; 6] = [1, 2, 3, 4, 2, 3];
        for command in [0b100, 0b101, 0b110, 0b111] {
            let compressed = [
                // Insert [1, 2, 3, 4]
                (0b011 << 5) | (4 - 1),
                1,
                // Repeat 2 bytes from address 1
                (0b111 << 5) | (command << 2),
                2 - 1,
                0,
                1,
            ];
            assert_decompression(&compressed, &EXPECTED)
        }
    }

    fn assert_round_trip(original: &[u8]) {
        let compressed = super::compress(original);
        let decompressed = super::decompress(&compressed, false).expect("recompressed data should decompress");
        assert_eq!(decompressed, original, "round-trip mismatch (compressed len {})", compressed.len());
    }

    #[test]
    fn round_trip_empty() {
        assert_round_trip(&[]);
    }

    #[test]
    fn consumed_len_matches_actual_stream_length_with_trailing_garbage() {
        let mut compressed = super::compress(&[1, 2, 3, 4, 5]);
        let real_len = compressed.len();
        compressed.extend_from_slice(&[0xAA; 10]); // trailing garbage past the terminator
        let (output, consumed) = super::decompress_with_len(&compressed, false).unwrap();
        assert_eq!(output, vec![1, 2, 3, 4, 5]);
        assert_eq!(consumed, real_len);
    }

    #[test]
    fn round_trip_all_literal() {
        assert_round_trip(&(0..=255u8).collect::<Vec<_>>());
    }

    #[test]
    fn round_trip_long_zero_run() {
        let data = vec![0u8; 5000];
        assert_round_trip(&data);
    }

    #[test]
    fn round_trip_mixed_runs_and_literals() {
        let mut data = Vec::new();
        data.extend_from_slice(&[1, 2, 3, 4, 5]);
        data.extend(std::iter::repeat_n(0xAAu8, 40));
        data.extend_from_slice(&[9, 8, 7]);
        data.extend(std::iter::repeat_n(0x00u8, 1500));
        data.extend_from_slice(&[1, 2]);
        assert_round_trip(&data);
    }

    #[test]
    fn compress_uses_byte_fill_for_runs_of_3_or_more() {
        let data = [5u8, 5, 5, 5, 5];
        let compressed = super::compress(&data);
        // header (BYTE_FILL, len-1=4) + fill byte + terminator = 3 bytes,
        // versus 6 bytes for a direct-copy of the same data.
        assert_eq!(compressed.len(), 3);
        assert_round_trip(&data);
    }

    /// Round-trips real vanilla GFX file data (decompress -> recompress ->
    /// decompress) to make sure `compress` handles genuine graphics data, not
    /// just synthetic patterns. Run with `ROM_PATH=/path/to/smw.smc cargo test
    /// -p smwe-rom --lib -- --ignored real_gfx_file_round_trip`.
    #[test]
    #[ignore]
    fn real_gfx_file_round_trip() {
        use crate::snes_utils::addr::{AddrPc, AddrSnes};

        let rom_path = std::env::var("ROM_PATH").expect("set ROM_PATH");
        let raw = std::fs::read(rom_path).expect("read ROM");
        let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };

        // GFX file 0, vanilla address (see graphics/gfx_file/data.rs).
        let pc = AddrPc::try_from_lorom(AddrSnes(0x08D9F9)).unwrap().0 as usize;
        let original_decompressed = super::decompress(&rom_bytes[pc..], false).expect("decompress real GFX file 0");
        assert!(original_decompressed.len() > 1000, "sanity: expect a nontrivial amount of tile data");

        let recompressed = super::compress(&original_decompressed);
        let redecompressed = super::decompress(&recompressed, false).expect("decompress our recompressed output");
        assert_eq!(redecompressed, original_decompressed);
    }
}
