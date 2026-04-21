//! Variable-length integer encoding for BPS format

use std::io::Write;

/// Encode a variable-length number into the BPS format
///
/// The encoding uses 7 bits per byte with the high bit as a continuation flag.
/// This allows support for any file size, not limited to 32-bit or 64-bit bounds.
pub fn encode_number(mut data: u64, writer: &mut impl Write) -> std::io::Result<()> {
    loop {
        let x = (data & 0x7f) as u8;
        data >>= 7;
        if data == 0 {
            writer.write_all(&[0x80 | x])?;
            break;
        }
        writer.write_all(&[x])?;
        data -= 1;
    }
    Ok(())
}

/// Decode a variable-length number from BPS format
pub fn decode_number(reader: &mut impl std::io::Read) -> std::io::Result<u64> {
    let mut data = 0u64;
    let mut shift = 1u64;

    loop {
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte)?;
        let x = byte[0];

        data += ((x & 0x7f) as u64) * shift;
        if x & 0x80 != 0 {
            break;
        }
        shift <<= 7;
        data += shift;
    }

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_encode_decode_small() {
        let mut buf = Vec::new();
        encode_number(42, &mut buf).unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded = decode_number(&mut cursor).unwrap();
        assert_eq!(decoded, 42);
    }

    #[test]
    fn test_encode_decode_zero() {
        let mut buf = Vec::new();
        encode_number(0, &mut buf).unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded = decode_number(&mut cursor).unwrap();
        assert_eq!(decoded, 0);
    }

    #[test]
    fn test_encode_decode_large() {
        let mut buf = Vec::new();
        encode_number(1_000_000, &mut buf).unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded = decode_number(&mut cursor).unwrap();
        assert_eq!(decoded, 1_000_000);
    }

    #[test]
    fn test_encode_decode_max() {
        let mut buf = Vec::new();
        encode_number(u64::MAX, &mut buf).unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded = decode_number(&mut cursor).unwrap();
        assert_eq!(decoded, u64::MAX);
    }

    #[test]
    fn test_encode_multiple() {
        let mut buf = Vec::new();
        encode_number(100, &mut buf).unwrap();
        encode_number(200, &mut buf).unwrap();
        encode_number(300, &mut buf).unwrap();

        let mut cursor = Cursor::new(buf);
        assert_eq!(decode_number(&mut cursor).unwrap(), 100);
        assert_eq!(decode_number(&mut cursor).unwrap(), 200);
        assert_eq!(decode_number(&mut cursor).unwrap(), 300);
    }
}
