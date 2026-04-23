//! BPS (Binary Patch) format implementation
//! Allows creation of BPS patches for ROM distribution

use std::io::Write;
use thiserror::Error;

mod encoding;
pub use encoding::*;

#[derive(Debug, Error)]
pub enum BpsError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid patch data")]
    InvalidPatch,
    #[error("Metadata is too large")]
    MetadataTooLarge,
}

/// Configuration for BPS patch creation
#[derive(Debug, Clone, Default)]
pub struct BpsConfig {
    /// Optional metadata (e.g., XML with description, author, etc.)
    pub metadata: Vec<u8>,
}

/// Creates a BPS patch that transforms source into target
///
/// This uses the linear algorithm which is simpler but may create larger patches
/// compared to the delta algorithm used by Flips.
pub fn create_patch(source: &[u8], target: &[u8], config: BpsConfig) -> Result<Vec<u8>, BpsError> {
    let mut patch = Vec::new();

    // Write header
    patch.write_all(b"BPS1")?;

    // Write sizes
    encode_number(source.len() as u64, &mut patch)?;
    encode_number(target.len() as u64, &mut patch)?;

    // Write metadata
    encode_number(config.metadata.len() as u64, &mut patch)?;
    patch.write_all(&config.metadata)?;

    // Calculate checksums
    let source_crc = crc32_sum(source);
    let target_crc = crc32_sum(target);

    // Encode patch commands
    let mut source_offset = 0;
    let mut target_offset = 0;

    while target_offset < target.len() {
        // Try to use SourceRead for matching bytes
        if source_offset < source.len() && source[source_offset] == target[target_offset] {
            let mut length = 0;
            while source_offset + length < source.len()
                && target_offset + length < target.len()
                && source[source_offset + length] == target[target_offset + length]
                && length < 0x1000000 // Reasonable limit
            {
                length += 1;
            }

            // Encode SourceRead command
            let cmd = (length - 1) << 2;
            encode_number(cmd as u64, &mut patch)?;
            source_offset += length;
            target_offset += length;
        } else {
            // Use TargetRead for differing bytes
            let mut length = 0;
            while target_offset + length < target.len()
                && (source_offset + length >= source.len() || source[source_offset + length] != target[target_offset + length])
                && length < 0x1000000
            {
                // Skip if we'd get a better match with SourceRead
                if source_offset + length < source.len()
                    && source[source_offset + length] == target[target_offset + length]
                {
                    break;
                }
                length += 1;
            }

            if length == 0 {
                length = 1;
            }

            // Encode TargetRead command
            let cmd = 1 | ((length - 1) << 2);
            encode_number(cmd as u64, &mut patch)?;
            patch.write_all(&target[target_offset..target_offset + length])?;
            target_offset += length;
            source_offset += length;
        }
    }

    // Calculate patch checksum before writing footer
    let patch_crc = crc32_sum(&patch);

    // Write footer
    patch.write_all(&source_crc.to_le_bytes())?;
    patch.write_all(&target_crc.to_le_bytes())?;
    patch.write_all(&patch_crc.to_le_bytes())?;

    Ok(patch)
}

/// Calculate CRC32 checksum
fn crc32_sum(data: &[u8]) -> u32 {
    let crc = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
    let mut digest = crc.digest();
    digest.update(data);
    digest.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_patch() {
        let source = b"Hello World";
        let target = b"Hello Rust!";

        let config = BpsConfig::default();
        let patch = create_patch(source, target, config).expect("patch creation failed");

        // Should have BPS1 header, sizes, metadata, commands, and footer
        assert!(patch.starts_with(b"BPS1"));
        assert!(patch.len() > 16); // At least header + footer
    }

    #[test]
    fn test_identical_files() {
        let data = b"Same content";
        let config = BpsConfig::default();
        let patch = create_patch(data, data, config).expect("patch creation failed");

        assert!(patch.starts_with(b"BPS1"));
    }

    #[test]
    fn test_with_metadata() {
        let source = b"test";
        let target = b"best";
        let metadata = b"<?xml version=\"1.0\"?><patch><author>Test</author></patch>";

        let config = BpsConfig {
            metadata: metadata.to_vec(),
        };
        let patch = create_patch(source, target, config).expect("patch creation failed");

        assert!(patch.starts_with(b"BPS1"));
        // Metadata should be in the patch
        assert!(patch.windows(metadata.len()).any(|w| w == *metadata));
    }
}
