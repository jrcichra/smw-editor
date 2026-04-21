//! IPS (International Patching System) format implementation
//! Allows creation of IPS patches for ROM distribution

use std::io::Write;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IpsError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("File too large for IPS format (max 16MB)")]
    FileTooLarge,
}

/// Creates an IPS patch that transforms source into target
///
/// IPS format is simpler than BPS but limited to 16MB files.
/// This uses a linear algorithm to encode changed regions.
pub fn create_patch(source: &[u8], target: &[u8]) -> Result<Vec<u8>, IpsError> {
    // IPS format is limited to 16MB (24-bit addressing)
    if source.len() > 0xFF_FF_FF || target.len() > 0xFF_FF_FF {
        return Err(IpsError::FileTooLarge);
    }

    let mut patch = Vec::new();

    // Write header
    patch.write_all(b"PATCH")?;

    // Find all changed regions
    let max_len = source.len().max(target.len());
    let mut offset = 0;

    while offset < max_len {
        // Check if bytes differ at this offset
        let source_byte = source.get(offset);
        let target_byte = target.get(offset);

        if source_byte != target_byte {
            // Found a change, collect the changed region
            let region_start = offset;
            let mut region_data = Vec::new();

            // Collect consecutive changed bytes
            while offset < max_len {
                let src = source.get(offset);
                let tgt = target.get(offset);

                if src != tgt {
                    region_data.push(tgt.copied().unwrap_or(0));
                    offset += 1;
                } else {
                    break;
                }
            }

            // Encode this region
            // Check if it's a good candidate for RLE
            if region_data.len() >= 4 && is_rle_candidate(&region_data) {
                encode_rle_chunk(&mut patch, region_start, &region_data)?;
            } else {
                encode_raw_chunk(&mut patch, region_start, &region_data)?;
            }
        } else {
            offset += 1;
        }
    }

    // Write EOF marker
    patch.write_all(&[0x45, 0x4F, 0x46])?;

    // Write truncate size (final output size)
    write_24bit(&mut patch, target.len() as u32)?;

    Ok(patch)
}

/// Check if data would benefit from RLE encoding
fn is_rle_candidate(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    // RLE is good if we have many repeated bytes
    let first = data[0];
    data.iter().filter(|&&b| b == first).count() >= data.len() / 2
}

/// Encode a region as RLE if beneficial
fn encode_rle_chunk(patch: &mut Vec<u8>, offset: usize, data: &[u8]) -> Result<(), IpsError> {
    // For RLE: encode as a run of the most common byte
    let most_common = find_most_common_byte(data);

    // Write offset (24-bit)
    write_24bit(patch, offset as u32)?;

    // Write size as 0 (indicates RLE)
    patch.write_all(&[0, 0])?;

    // Write RLE count (16-bit)
    patch.write_all(&[(data.len() >> 8) as u8, (data.len() & 0xFF) as u8])?;

    // Write the repeated byte
    patch.write_all(&[most_common])?;

    Ok(())
}

/// Encode a region as raw data
fn encode_raw_chunk(patch: &mut Vec<u8>, offset: usize, data: &[u8]) -> Result<(), IpsError> {
    // Write offset (24-bit)
    write_24bit(patch, offset as u32)?;

    // Write size (16-bit, non-zero for raw data)
    let size = data.len();
    patch.write_all(&[(size >> 8) as u8, (size & 0xFF) as u8])?;

    // Write raw data
    patch.write_all(data)?;

    Ok(())
}

/// Find the most common byte in a slice
fn find_most_common_byte(data: &[u8]) -> u8 {
    let mut counts = [0usize; 256];
    for &byte in data {
        counts[byte as usize] += 1;
    }
    let (byte, _) = counts
        .iter()
        .enumerate()
        .max_by_key(|(_, &count)| count)
        .unwrap();
    byte as u8
}

/// Write a 24-bit value in big-endian format
fn write_24bit(writer: &mut impl Write, value: u32) -> std::io::Result<()> {
    writer.write_all(&[(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_patch() {
        let source = b"Hello World";
        let target = b"Hello Rust!";

        let patch = create_patch(source, target).expect("patch creation failed");

        // Should have PATCH header, some chunks, EOF marker, and truncate size
        assert!(patch.starts_with(b"PATCH"));
        assert!(patch.windows(3).any(|w| w == [0x45, 0x4F, 0x46])); // EOF
        assert!(patch.len() > 8);
    }

    #[test]
    fn test_identical_files() {
        let data = b"Same content";
        let patch = create_patch(data, data).expect("patch creation failed");

        // Should still have header, EOF, and truncate
        assert!(patch.starts_with(b"PATCH"));
        assert!(patch.windows(3).any(|w| w == [0x45, 0x4F, 0x46]));
    }

    #[test]
    fn test_single_byte_change() {
        let source = b"test";
        let target = b"best";

        let patch = create_patch(source, target).expect("patch creation failed");
        assert!(patch.starts_with(b"PATCH"));
    }

    #[test]
    fn test_file_too_large() {
        let large_data = vec![0u8; 0x100_0000]; // 16MB + 1
        let result = create_patch(&large_data, &large_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_max_size_allowed() {
        let max_data = vec![0u8; 0xFF_FF_FF]; // Exactly 16MB - 1
        let patch = create_patch(&max_data, &max_data).expect("should allow 16MB");
        assert!(patch.starts_with(b"PATCH"));
    }

    #[test]
    fn test_rle_optimization() {
        // Create data with a lot of repeated bytes
        let source = vec![0u8; 100];
        let mut target = vec![0u8; 100];
        target[50..].fill(0xFF); // Second half changed to 0xFF

        let patch = create_patch(&source, &target).expect("patch creation failed");
        assert!(patch.starts_with(b"PATCH"));
        // Should be relatively small due to RLE
        assert!(patch.len() < 100);
    }
}
