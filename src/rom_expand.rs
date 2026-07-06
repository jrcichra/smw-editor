//! ROM expansion (Lunar Magic's "Expand ROM" equivalent) and SNES internal
//! header checksum fixing.
//!
//! Expansion pads the ROM with `0xFF` — the same fill `rom_freespace` treats
//! as free — so newly added space is immediately usable by every repointing
//! write path, and updates the internal header's ROM-size byte and checksum.

/// Expansion targets offered to the user (PC sizes, without SMC header).
/// Mirrors Lunar Magic's standard LoROM options. Sizes above 2MB are capped
/// at 4MB; free-space scanning separately refuses to hand out the LoROM
/// SRAM-shadowed banks ($70-$7D → PC 0x380000+), so a 4MB ROM's tail is used
/// only as addressable padding, exactly like LM's plain-LoROM 4MB option.
pub const EXPANSION_SIZES: [(usize, &str); 3] = [(0x10_0000, "1 MB"), (0x20_0000, "2 MB"), (0x40_0000, "4 MB")];

/// Expand `rom_bytes` (a full ROM file, SMC header included if present) to
/// `target_pc_size` bytes of ROM data, padding with `0xFF`. Updates the
/// internal header size byte and checksum. Errors if the ROM is already at
/// least that large.
pub fn expand_rom(rom_bytes: &mut Vec<u8>, target_pc_size: usize) -> Result<(), String> {
    let header_offset = if rom_bytes.len() % 0x400 == 0x200 { 0x200 } else { 0 };
    let current = rom_bytes.len() - header_offset;
    if current >= target_pc_size {
        return Err(format!("ROM is already {} bytes; cannot shrink to {} bytes", current, target_pc_size));
    }
    rom_bytes.resize(header_offset + target_pc_size, 0xFF);

    // Internal-header ROM size byte: 2^n KiB.
    let size_exponent = {
        let kib = target_pc_size / 0x400;
        let mut n = 0u8;
        while (1usize << n) < kib {
            n += 1;
        }
        n
    };
    // The ROM-size byte sits at the header's $xFD7 slot (header base $xFC0 + 0x17).
    let header_base = locate_internal_header(rom_bytes, header_offset);
    let size_byte_pos = header_offset + header_base + 0x17;
    if size_byte_pos < rom_bytes.len() {
        rom_bytes[size_byte_pos] = size_exponent;
    }
    fix_checksum(rom_bytes);
    Ok(())
}

/// Return the PC offset of the internal header block ($xFC0) — `0x7FC0` for
/// LoROM, `0xFFC0` for HiROM — picking whichever currently contains a
/// self-consistent checksum/complement pair, defaulting to LoROM.
fn locate_internal_header(rom_bytes: &[u8], header_offset: usize) -> usize {
    for base in [0x7FC0usize, 0xFFC0] {
        let checksum_pos = header_offset + base + 0x1E;
        let complement_pos = header_offset + base + 0x1C;
        if checksum_pos + 1 >= rom_bytes.len() {
            continue;
        }
        let checksum = u16::from_le_bytes([rom_bytes[checksum_pos], rom_bytes[checksum_pos + 1]]);
        let complement = u16::from_le_bytes([rom_bytes[complement_pos], rom_bytes[complement_pos + 1]]);
        if checksum ^ complement == 0xFFFF {
            return base;
        }
    }
    0x7FC0
}

/// Recompute and write the SNES internal-header checksum ($xFDE) and its
/// complement ($xFDC). The sum is computed with the four checksum bytes
/// normalized to `FF FF 00 00` (the standard convention, making the result
/// independent of the previous checksum). For non-power-of-two sizes where
/// the remainder evenly divides the power-of-two base, the remainder is
/// counted multiple times (standard mirroring); otherwise a plain sum is
/// used (emulators do not verify checksums, so this is cosmetic).
pub fn fix_checksum(rom_bytes: &mut [u8]) {
    let header_offset = if rom_bytes.len() % 0x400 == 0x200 { 0x200 } else { 0 };
    let pc_len = rom_bytes.len() - header_offset;
    if pc_len < 0x8000 {
        return;
    }
    let header_base = locate_internal_header(rom_bytes, header_offset);
    let complement_pos = header_offset + header_base + 0x1C;
    let checksum_pos = header_offset + header_base + 0x1E;

    let byte_at = |i: usize| -> u32 {
        let file = i + header_offset;
        if file == complement_pos || file == complement_pos + 1 {
            0xFF
        } else if file == checksum_pos || file == checksum_pos + 1 {
            0x00
        } else {
            rom_bytes[file] as u32
        }
    };

    let base = {
        let mut b = 1usize;
        while b * 2 <= pc_len {
            b *= 2;
        }
        b
    };
    let mut sum: u32 = (0..base).map(byte_at).sum();
    let rem = pc_len - base;
    if rem > 0 {
        let rem_sum: u32 = (base..pc_len).map(byte_at).sum();
        let multiplier = if base % rem == 0 { (base / rem) as u32 } else { 1 };
        sum = sum.wrapping_add(rem_sum.wrapping_mul(multiplier));
    }
    let checksum = (sum & 0xFFFF) as u16;
    let complement = checksum ^ 0xFFFF;
    rom_bytes[complement_pos..complement_pos + 2].copy_from_slice(&complement.to_le_bytes());
    rom_bytes[checksum_pos..checksum_pos + 2].copy_from_slice(&checksum.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_lorom(pc_size: usize) -> Vec<u8> {
        let mut rom = vec![0u8; pc_size];
        // Plausible LoROM header: mode byte + self-consistent checksum pair.
        rom[0x7FD5] = 0x20;
        rom[0x7FDC] = 0xFF;
        rom[0x7FDD] = 0xFF;
        rom[0x7FDE] = 0x00;
        rom[0x7FDF] = 0x00;
        rom
    }

    #[test]
    fn expands_and_pads_with_ff() {
        let mut rom = make_lorom(0x80000);
        expand_rom(&mut rom, 0x100000).unwrap();
        assert_eq!(rom.len(), 0x100000);
        assert!(rom[0x80000..0xFFFFF].iter().all(|&b| b == 0xFF));
        // 1 MiB = 1024 KiB = 2^10.
        assert_eq!(rom[0x7FC0 + 0x17], 0x0A);
    }

    #[test]
    fn refuses_to_shrink() {
        let mut rom = make_lorom(0x200000);
        assert!(expand_rom(&mut rom, 0x100000).is_err());
    }

    #[test]
    fn expansion_preserves_smc_header() {
        let mut rom = vec![0u8; 0x80200];
        rom[..0x200].fill(0xAB);
        rom[0x200 + 0x7FDC] = 0xFF;
        rom[0x200 + 0x7FDD] = 0xFF;
        expand_rom(&mut rom, 0x100000).unwrap();
        assert_eq!(rom.len(), 0x100200);
        assert!(rom[..0x200].iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn checksum_is_self_consistent_and_verifies() {
        let mut rom = make_lorom(0x80000);
        for (i, b) in rom.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        fix_checksum(&mut rom);
        let checksum = u16::from_le_bytes([rom[0x7FDE], rom[0x7FDF]]);
        let complement = u16::from_le_bytes([rom[0x7FDC], rom[0x7FDD]]);
        assert_eq!(checksum ^ complement, 0xFFFF);
        // Recomputing the sum over the final bytes must reproduce the stored
        // checksum: sum with the 4 checksum bytes as FF FF 00 00 equals sum of
        // the actual bytes because complement + checksum == 0x1FE either way.
        let plain: u32 = rom.iter().map(|&b| b as u32).sum();
        assert_eq!((plain & 0xFFFF) as u16, checksum);
    }

    #[test]
    fn fix_checksum_is_idempotent() {
        let mut rom = make_lorom(0x80000);
        for (i, b) in rom.iter_mut().enumerate() {
            *b = (i % 13) as u8;
        }
        fix_checksum(&mut rom);
        let first: Vec<u8> = rom.clone();
        fix_checksum(&mut rom);
        assert_eq!(first, rom);
    }

    #[test]
    fn non_power_of_two_with_even_remainder_uses_mirroring() {
        // 3 MiB = 2 MiB base + 1 MiB remainder counted twice.
        let mut rom = make_lorom(0x300000);
        rom[0x250000] = 7;
        fix_checksum(&mut rom);
        let checksum = u16::from_le_bytes([rom[0x7FDE], rom[0x7FDF]]);
        let base_sum: u32 = rom[..0x200000].iter().map(|&b| b as u32).sum();
        let rem_sum: u32 = rom[0x200000..].iter().map(|&b| b as u32).sum();
        assert_eq!(checksum, ((base_sum + rem_sum * 2) & 0xFFFF) as u16);
    }
}
