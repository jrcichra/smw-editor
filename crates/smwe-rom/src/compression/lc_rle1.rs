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
    assert!(!input.len() >= 2);
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
    let bytes_consumed = input.len() - in_it.len();
    Ok((output, bytes_consumed))
}

pub fn compress(input: &[u8]) -> Vec<u8> {
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
