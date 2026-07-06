//! Shared free-space scanning for repointing ROM data (GFX files, level
//! layer/sprite data, message boxes, overworld layer 2, ...). Previously
//! duplicated near-verbatim in `level_editor` and `world_editor`; consolidated
//! here so every write path that needs to repoint something behaves the same.

/// Find `needed` contiguous bytes of unused ROM space (`0xFF` fill), starting
/// the search at PC address `pc_start`, never spanning a LoROM bank boundary.
/// `header_offset` is `0x200` for SMC-headered ROMs, `0` otherwise.
pub fn find_free_space(rom_bytes: &[u8], needed: usize, pc_start: usize, header_offset: usize) -> Option<usize> {
    let pc_end = rom_bytes.len().saturating_sub(header_offset);
    find_free_space_in(rom_bytes, needed, pc_start, pc_end, header_offset)
}

/// Like `find_free_space`, but restricted to the PC range `[pc_start, pc_end)`.
pub fn find_free_space_in(
    rom_bytes: &[u8], needed: usize, pc_start: usize, pc_end: usize, header_offset: usize,
) -> Option<usize> {
    const BANK: usize = 0x8000;
    // LoROM banks $70-$7D shadow SRAM in the lower half of the address space,
    // so data placed at PC 0x380000+ isn't reachable through the plain bank
    // number every existing pointer-write path derives from the PC address.
    // Never hand out space there (matters once a ROM is expanded to 4 MB).
    const LOROM_SRAM_BANKS_PC: usize = 0x38_0000;
    let pc_end = pc_end.min(LOROM_SRAM_BANKS_PC);
    let mut run_start: Option<usize> = None;
    let mut run_len = 0usize;
    for pc in pc_start..pc_end {
        // Never span a LoROM bank boundary.
        if pc % BANK == 0 && pc != pc_start {
            run_start = None;
            run_len = 0;
        }
        let file = pc + header_offset;
        if file >= rom_bytes.len() {
            break;
        }
        if rom_bytes[file] == 0xFF {
            run_start.get_or_insert(pc);
            run_len += 1;
            if run_len >= needed {
                return run_start;
            }
        } else {
            run_start = None;
            run_len = 0;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_run_of_free_space() {
        let mut rom = vec![0x00u8; 0x10000];
        rom[100..110].fill(0xFF);
        assert_eq!(find_free_space(&rom, 10, 0, 0), Some(100));
    }

    #[test]
    fn does_not_span_a_bank_boundary() {
        let mut rom = vec![0x00u8; 0x20000];
        // Free space straddling the 0x8000 boundary, each side too short alone.
        rom[0x7FF8..0x8008].fill(0xFF);
        assert_eq!(find_free_space(&rom, 10, 0, 0), None);
    }

    #[test]
    fn respects_header_offset() {
        let mut rom = vec![0x00u8; 0x10100];
        // Free space at file offset 0x200..0x210, i.e. PC 0x0..0x10 with a 0x200 header.
        rom[0x200..0x210].fill(0xFF);
        assert_eq!(find_free_space(&rom, 16, 0, 0x200), Some(0));
    }

    #[test]
    fn returns_none_when_not_enough_space() {
        let rom = vec![0x00u8; 0x1000];
        assert_eq!(find_free_space(&rom, 10, 0, 0), None);
    }

    #[test]
    fn find_free_space_in_respects_pc_end() {
        let mut rom = vec![0x00u8; 0x1000];
        rom[500..520].fill(0xFF);
        // Free space exists, but outside the searched range.
        assert_eq!(find_free_space_in(&rom, 10, 0, 400, 0), None);
        assert_eq!(find_free_space_in(&rom, 10, 0, 600, 0), Some(500));
    }
}
