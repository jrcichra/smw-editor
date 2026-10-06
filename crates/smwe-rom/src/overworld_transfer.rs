//! "Copy Overworld to Another ROM" (Lunar Magic v3.40 parity).
//!
//! Lunar Magic v3.40 (2023-09-24) added a "Copy Overworld to Another ROM"
//! menu item to the overworld editor's File menu, described as a shortcut to
//! the command-line function of the same name (`-TransferOverworld <dest>
//! <src>`). The transfer copies the source ROM's overworld into the
//! destination ROM: overworld tilemaps, event data, sprites, palettes, music
//! settings, level names, message/boss text, and start positions. Observed LM
//! behavior (third-party oracle fixture of LM 3.63's `-TransferOverworld`):
//! the destination is expanded when too small (0x80200 → 0x100200 file bytes
//! observed), relocated data goes into fresh RATS allocations, and the source
//! ROM is left untouched.
//!
//! [`transfer_overworld`] implements that contract over smw-editor's own
//! overworld domain model (grounded in SMWDisX): every section is parsed from
//! the source image and re-serialized into the destination image with the
//! same writers the editor's save path uses, so a transfer is byte-equivalent
//! to opening the source and saving each overworld editor over the
//! destination. The source image is never modified; the destination checksum
//! is repaired; the destination is expanded 512 KiB → 1 MiB first when it is
//! smaller (mirroring LM's observed expansion).
//!
//! Honest scope notes (documented, not silent):
//! - Only the overworld domain travels: tilemaps, event data, OW sprites,
//!   OW palettes, submap music, level names, message-box and boss-sequence
//!   text, start positions, reveal list, event ownership, secret exits,
//!   star/pipe warp links, OW ExAnimation, and the custom level-number table.
//!   Level data, Map16, GFX/ExGFX, title/credits, and level palettes are
//!   never touched.
//! - The ExAnimation and secondary-exit-extension RATS blocks are shared with
//!   the level editor, so only the overworld-owned parts are replaced there
//!   (the OW animation list; the star/pipe teleport table). The destination's
//!   level/global ExAnimation lists and per-entrance options are preserved.
//! - OW Layer 2 streams are written with the same writer the world editor's
//!   save uses: they are replaced in place when the new payload fits the
//!   destination's current stream space, and the transfer refuses (rather
//!   than half-applying) when they don't — the same limitation the world
//!   editor's save has on real ROMs.
//! - The source's Layer 2 streams are located through the game's own
//!   `CODE_04DC6A` immediates, so a source whose streams were relocated by
//!   another tool still transfers its live data.

use crate::{
    boss_text::BossText,
    compression::lc_rle2,
    exanimation::{ExAnimError, ExAnimationData},
    freespace::find_free_space,
    internal_header::RomInternalHeader,
    level::secondary_entrance::{SecExitExtError, SecondaryExitExtData},
    message_boxes::{
        MessageBoxes,
        MESSAGE_BOXES_MAX_SIZE,
        MESSAGE_BOXES_SNES,
        MESSAGE_POINTER_COUNT,
        MESSAGE_POINTER_TABLE_SNES,
    },
    overworld::{
        self,
        event_ownership::EventOwnership,
        level_names,
        reveal_list::RevealTileList,
        secret_exits,
        sprites as ow_sprites,
        start_positions::OverworldStartPositions,
        submap_music::SubmapMusic,
        write_overworld_l2_stream,
        OverworldData,
        OverworldEvents,
        LEVEL_NUMBER_PATCH_OPERAND_SNES,
        OWL1_TILE_DATA_SIZE,
        OWL1_TILE_DATA_SNES,
        OW_EVENT_COUNT,
        OW_EVENT_REVEAL_COUNT,
        OW_EVENT_TILEMAP_DECODED_LEN,
        OW_EVENT_TILEMAP_PROP_SNES,
        OW_EVENT_TILE_OFFSET_SNES,
        OW_L2_EVENT_BOUNDARIES_SNES,
        OW_L2_EVENT_BOUNDARY_COUNT,
        OW_L2_EVENT_ENTRY_COUNT,
        OW_L2_EVENT_ENTRY_SIZE,
        OW_L2_EVENT_TABLE_SNES,
        OW_SILENT_EVENT_COUNT,
        OW_SILENT_EVENT_DATA_SNES,
        OW_SILENT_EVENT_DEST_SNES,
        OW_SILENT_EVENT_FLAGS_SNES,
        OW_SILENT_EVENT_LIST_SNES,
    },
    rom_expansion::{expand_rom, rewrite_checksum, EXPANSION_TARGETS},
    snes_utils::{
        addr::{AddrPc, AddrSnes},
        rom::Rom,
    },
};

// ── Public types ─────────────────────────────────────────────────────────────

/// One transferred domain, for the result report.
pub struct TransferSection {
    /// Short domain name, e.g. `"Overworld tilemaps"`.
    pub name:   &'static str,
    /// Human-readable detail, e.g. `"L1 0x800 B + L2 RLE2 streams"`.
    pub detail: String,
}

/// Outcome of [`transfer_overworld`].
pub struct TransferOutcome {
    /// The destination image with the source's overworld installed and a
    /// repaired checksum. Same SMC-header presence as the input destination.
    pub dest_bytes: Vec<u8>,
    /// What was transferred.
    pub report:     TransferReport,
}

/// Human-readable summary of a transfer.
pub struct TransferReport {
    pub sections:      Vec<TransferSection>,
    /// True when the destination was expanded 512 KiB → 1 MiB first.
    pub dest_expanded: bool,
}

impl TransferReport {
    /// One-line-per-section summary for the result dialog.
    pub fn summary(&self) -> String {
        let mut out = String::new();
        if self.dest_expanded {
            out.push_str("• Destination expanded 512 KiB → 1 MiB (ROM too small)\n");
        }
        for s in &self.sections {
            out.push_str(&format!("• {}: {}\n", s.name, s.detail));
        }
        out
    }
}

// ── Entry point ─────────────────────────────────────────────────────────────

/// Copy the overworld domain from `source_image` into `dest_image`
/// (Lunar Magic v3.40 "Copy Overworld to Another ROM" parity).
///
/// Both images may carry an SMC header. The source is only read. The
/// destination must parse as a Super Mario World ROM; it is expanded
/// 512 KiB → 1 MiB first when smaller (mirroring LM), then every overworld
/// section is installed, and the checksum is repaired. The transfer is
/// all-or-nothing: any failure returns `Err` with the destination untouched.
pub fn transfer_overworld(source_image: &[u8], dest_image: &[u8]) -> anyhow::Result<TransferOutcome> {
    if source_image.is_empty() {
        anyhow::bail!("Source image is empty");
    }
    if dest_image.is_empty() {
        anyhow::bail!("Destination image is empty");
    }
    if source_image.as_ptr() == dest_image.as_ptr() && source_image.len() == dest_image.len() {
        anyhow::bail!("Source and destination are the same image; refusing to copy a ROM onto itself");
    }

    let src_header = header_offset(source_image);
    let mut dst_header = header_offset(dest_image);

    // Both sides must be plausible ROM images; the destination must actually
    // be a Super Mario World ROM (we are about to rewrite its overworld).
    let src_rom = Rom::new(source_image.to_vec()).map_err(|e| anyhow::anyhow!("Source is not a valid ROM: {e}"))?;
    let dst_rom = Rom::new(dest_image.to_vec()).map_err(|e| anyhow::anyhow!("Destination is not a valid ROM: {e}"))?;
    RomInternalHeader::parse(&src_rom).map_err(|e| anyhow::anyhow!("Source has no SMW internal header: {e}"))?;
    RomInternalHeader::parse(&dst_rom).map_err(|e| anyhow::anyhow!("Destination has no SMW internal header: {e}"))?;

    // Mirror LM's observed expansion: a 512 KiB destination grows to 1 MiB
    // before the transfer writes anything.
    let mut dest_bytes: Vec<u8> = dest_image.to_vec();
    let mut dest_expanded = false;
    let dst_body_len = dest_bytes.len() - dst_header;
    if dst_body_len < 0x10_0000 {
        if !EXPANSION_TARGETS.contains(&0x10_0000) {
            anyhow::bail!("Internal error: 1 MiB is not an expansion target");
        }
        let (smc, body) = dest_bytes.split_at(dst_header);
        let smc = smc.to_vec();
        let expanded = expand_rom(
            &Rom::new(body.to_vec()).map_err(|e| anyhow::anyhow!("Destination ROM invalid: {e}"))?,
            0x10_0000,
        )
        .map_err(|e| anyhow::anyhow!("Could not expand destination ROM to 1 MiB: {e}"))?;
        dest_bytes = smc.into_iter().chain(expanded.bytes().iter().copied()).collect();
        dst_header = header_offset(&dest_bytes);
        dest_expanded = true;
    }

    let mut sections = Vec::new();
    let mut push = |name: &'static str, detail: String| sections.push(TransferSection { name, detail });

    // Each section is fallible; any error aborts the whole transfer with the
    // destination untouched (we only return `dest_bytes` on success).
    let r = transfer_tilemaps(source_image, src_header, &mut dest_bytes, dst_header, &mut push);
    let r = r
        .and_then(|_| transfer_event_data(source_image, src_header, &mut dest_bytes, dst_header, &mut push))
        .and_then(|_| transfer_event_ownership(source_image, src_header, &mut dest_bytes, dst_header, &mut push))
        .and_then(|_| transfer_level_names(source_image, src_header, &mut dest_bytes, dst_header, &mut push))
        .and_then(|_| transfer_text(source_image, &mut dest_bytes, dst_header, &mut push))
        .and_then(|_| transfer_sprites(source_image, src_header, &mut dest_bytes, dst_header, &mut push))
        .and_then(|_| transfer_settings(source_image, src_header, &mut dest_bytes, dst_header, &mut push))
        .and_then(|_| transfer_editor_blocks(source_image, src_header, &mut dest_bytes, dst_header, &mut push))
        .and_then(|_| transfer_palettes(source_image, src_header, &mut dest_bytes, dst_header, &mut push));
    r.map_err(|e| anyhow::anyhow!("Overworld transfer failed: {e:#}"))?;

    // Repair the destination checksum over the headerless body.
    let (smc, body) = dest_bytes.split_at(dst_header);
    let mut new_bytes = Vec::with_capacity(dest_bytes.len());
    new_bytes.extend_from_slice(smc);
    let mut body = body.to_vec();
    rewrite_checksum(&mut body);
    new_bytes.extend_from_slice(&body);

    Ok(TransferOutcome { dest_bytes: new_bytes, report: TransferReport { sections, dest_expanded } })
}

fn header_offset(image: &[u8]) -> usize {
    usize::from(image.len() % 0x400 == 0x200) * 0x200
}

/// PC file range for `[snes, snes + len)` plus the SMC header offset,
/// bounds-checked against `rom_bytes`.
fn pc_range(
    rom_bytes: &[u8], snes: AddrSnes, len: usize, header_offset: usize, what: &str,
) -> anyhow::Result<std::ops::Range<usize>> {
    let pc = AddrPc::try_from_lorom(snes).map_err(|e| anyhow::anyhow!("{what} address conversion: {e}"))?.as_index()
        + header_offset;
    let end = pc.checked_add(len).ok_or_else(|| anyhow::anyhow!("{what} range overflows"))?;
    if end > rom_bytes.len() {
        anyhow::bail!("{what} range ${snes:06X}+{len:#X} extends past end of ROM");
    }
    Ok(pc..end)
}

// ── Sections ─────────────────────────────────────────────────────────────────

/// OW Layer 1 tilemap (fixed) + Layer 2 RLE2 streams (relocated via the
/// editor's own stream writer).
fn transfer_tilemaps(
    src: &[u8], src_header: usize, dst: &mut [u8], dst_header: usize, push: &mut impl FnMut(&'static str, String),
) -> anyhow::Result<()> {
    // Layer 1: fixed 0x800-byte tilemap.
    let src_ow = OverworldData::parse(&Rom::new(src.to_vec()).map_err(|e| anyhow::anyhow!("source ROM: {e}"))?)
        .map_err(|e| anyhow::anyhow!("source overworld L1: {e}"))?;
    let range = pc_range(dst, OWL1_TILE_DATA_SNES, OWL1_TILE_DATA_SIZE, dst_header, "OW layer 1 tilemap")?;
    dst[range].copy_from_slice(&src_ow.layer1_tiles);

    // Layer 2: two RLE2 streams. Locate the source's live streams through the
    // game's own CODE_04DC6A immediates (so a source relocated by another
    // tool still transfers its live data), decompress, re-compress, and write
    // into the destination with the editor's stream writer.
    let (tile_snes, attr_snes) = ow_l2_stream_addrs(src, src_header)?;
    let tile_pc = AddrPc::try_from_lorom(AddrSnes(tile_snes))
        .map_err(|e| anyhow::anyhow!("OW L2 tile stream address: {e}"))?
        .as_index()
        + src_header;
    let attr_pc = AddrPc::try_from_lorom(AddrSnes(attr_snes))
        .map_err(|e| anyhow::anyhow!("OW L2 attr stream address: {e}"))?
        .as_index()
        + src_header;
    // 0x1000 words: matches the editor's WRAM read (`read_overworld_l2_words`
    // reads 0x2000 bytes at $7F4000).
    let words = lc_rle2::decompress_rle2(
        src.get(tile_pc..).ok_or_else(|| anyhow::anyhow!("OW L2 tile stream out of bounds"))?,
        src.get(attr_pc..).ok_or_else(|| anyhow::anyhow!("OW L2 attr stream out of bounds"))?,
        0x2000,
    );
    if words.len() != 0x1000 {
        anyhow::bail!("OW L2 streams decoded to {} words, expected 0x1000", words.len());
    }
    let tile_stream: Vec<u8> = words.iter().map(|w| (w & 0xFF) as u8).collect();
    let attr_stream: Vec<u8> = words.iter().map(|w| (w >> 8) as u8).collect();
    let tile_compressed = lc_rle2::compress_pass(&tile_stream);
    let attr_compressed = lc_rle2::compress_pass(&attr_stream);

    let dst_has_header = dst_header != 0;
    let tile_vanilla_pc = AddrPc::try_from_lorom(AddrSnes(0x04A533))
        .map_err(|e| anyhow::anyhow!("OWTileNumbers address: {e}"))?
        .as_index();
    let attr_vanilla_pc =
        AddrPc::try_from_lorom(AddrSnes(0x04C02B)).map_err(|e| anyhow::anyhow!("OWTilemap address: {e}"))?.as_index();
    write_overworld_l2_stream(dst, dst_has_header, tile_vanilla_pc, words.len(), &tile_compressed, "OWTileNumbers")
        .map_err(|e| {
            anyhow::anyhow!(
                "OW layer 2 tile stream did not fit the destination's stream space ({e:#}); the transfer refuses \
                 rather than half-applying — same limitation as the world editor's save"
            )
        })?;
    write_overworld_l2_stream(dst, dst_has_header, attr_vanilla_pc, words.len(), &attr_compressed, "OWTilemap")
        .map_err(|e| {
            anyhow::anyhow!(
                "OW layer 2 attr stream did not fit the destination's stream space ({e:#}); the transfer refuses \
                 rather than half-applying — same limitation as the world editor's save"
            )
        })?;

    push("Overworld tilemaps", format!("L1 0x800 B + L2 RLE2 streams ({} words)", words.len()));
    Ok(())
}

/// Read the live OW Layer 2 stream SNES addresses from the game's own
/// `CODE_04DC6A` immediates (`LDA.W #OWTileNumbers` / `LDA.W #OWTilemap` /
/// `LDA.B #bank`). Verified byte-for-byte against a real ROM: `A9 33 A5`
/// at PC $25C71 (bank imm `A9 04` at $25C79) and `A9 2B C0` at PC $25C8C;
/// the attr stream reuses the tile stream's bank (the game never reloads
/// `_2` between the two `CODE_04DABA` calls).
fn ow_l2_stream_addrs(rom_bytes: &[u8], header_offset: usize) -> anyhow::Result<(u32, u32)> {
    const TILE_IMM_PC: usize = 0x25C71; // `A9 33 A5` — LDA.W #OWTileNumbers
    const TILE_BANK_PC: usize = 0x25C78; // `A9 04` — LDA.B #OWTileNumbers>>16
    const ATTR_IMM_PC: usize = 0x25C8C; // `A9 2B C0` — LDA.W #OWTilemap

    let read_imm16 = |pc: usize, what: &str| -> anyhow::Result<u16> {
        let off = pc + header_offset;
        let bytes = rom_bytes.get(off..off + 3).ok_or_else(|| anyhow::anyhow!("{what} immediate out of bounds"))?;
        if bytes[0] != 0xA9 {
            anyhow::bail!("{what}: expected LDA immediate opcode $A9 at PC ${pc:06X}, found ${:02X}", bytes[0]);
        }
        Ok(u16::from_le_bytes([bytes[1], bytes[2]]))
    };
    let read_imm8 = |pc: usize, what: &str| -> anyhow::Result<u8> {
        let off = pc + header_offset;
        let bytes = rom_bytes.get(off..off + 2).ok_or_else(|| anyhow::anyhow!("{what} immediate out of bounds"))?;
        if bytes[0] != 0xA9 {
            anyhow::bail!("{what}: expected LDA immediate opcode $A9 at PC ${pc:06X}, found ${:02X}", bytes[0]);
        }
        Ok(bytes[1])
    };

    let tile_lo = read_imm16(TILE_IMM_PC, "OWTileNumbers address")?;
    let bank = read_imm8(TILE_BANK_PC, "OWTileNumbers bank")?;
    let attr_lo = read_imm16(ATTR_IMM_PC, "OWTilemap address")?;
    Ok((u32::from(bank) << 16 | u32::from(tile_lo), u32::from(bank) << 16 | u32::from(attr_lo)))
}

/// Event data: per-event tile offsets, reveal pairs, Layer 2 event tables,
/// silent events, and the per-event RLE tilemap blob.
fn transfer_event_data(
    src: &[u8], src_header: usize, dst: &mut [u8], dst_header: usize, push: &mut impl FnMut(&'static str, String),
) -> anyhow::Result<()> {
    let src_rom = Rom::new(src.to_vec()).map_err(|e| anyhow::anyhow!("source ROM: {e}"))?;

    // Per-event tile offsets ($04D85D, 0x6F words).
    let events = OverworldEvents::parse(&src_rom).map_err(|e| anyhow::anyhow!("source overworld events: {e}"))?;
    let range = pc_range(dst, OW_EVENT_TILE_OFFSET_SNES, OW_EVENT_COUNT * 2, dst_header, "OW event tile offsets")?;
    for (i, &off) in events.tile_offsets.iter().enumerate() {
        dst[range.start + i * 2..range.start + i * 2 + 2].copy_from_slice(&off.to_le_bytes());
    }

    // Reveal pairs ($04DA1D/$04DA33, 22 pairs) via the editor's own codec.
    let reveals = RevealTileList::parse(src, src_header).map_err(|e| anyhow::anyhow!("source reveal list: {e}"))?;
    reveals.apply_to_rom(dst, dst_header).map_err(|e| anyhow::anyhow!("destination reveal list: {e}"))?;

    // Layer 2 event tables + silent events: fixed ranges, byte copy.
    let fixed: &[(AddrSnes, usize, &str)] = &[
        (OW_L2_EVENT_TABLE_SNES, OW_L2_EVENT_ENTRY_COUNT * OW_L2_EVENT_ENTRY_SIZE, "OW L2 event table"),
        (OW_L2_EVENT_BOUNDARIES_SNES, OW_L2_EVENT_BOUNDARY_COUNT * 2, "OW L2 event boundaries"),
        (OW_SILENT_EVENT_LIST_SNES, OW_SILENT_EVENT_COUNT, "OW silent event list"),
        (OW_SILENT_EVENT_FLAGS_SNES, OW_SILENT_EVENT_COUNT, "OW silent event flags"),
        (OW_SILENT_EVENT_DEST_SNES, OW_SILENT_EVENT_COUNT * 2, "OW silent event dest"),
        (OW_SILENT_EVENT_DATA_SNES, OW_SILENT_EVENT_COUNT * 2, "OW silent event data"),
    ];
    for &(snes, len, what) in fixed {
        let src_range = pc_range(src, snes, len, src_header, what)?;
        let dst_range = pc_range(dst, snes, len, dst_header, what)?;
        dst[dst_range].copy_from_slice(&src[src_range]);
    }

    // Per-event RLE tilemap blob ($0C8D00, $FFFF-terminated): walk the
    // packets exactly like the game's CODE_04DD57 and copy the encoded
    // bytes, verifying the decoded size matches the WRAM buffer.
    let blob_len = event_tilemap_blob_len(src, src_header)?;
    let src_range = pc_range(src, OW_EVENT_TILEMAP_PROP_SNES, blob_len, src_header, "OW event tilemaps")?;
    let dst_range = pc_range(dst, OW_EVENT_TILEMAP_PROP_SNES, blob_len, dst_header, "OW event tilemaps")?;
    dst[dst_range].copy_from_slice(&src[src_range]);

    push(
        "Event data",
        format!(
            "{} tile offsets, {} reveal pairs, L2/silent event tables, event tilemaps",
            OW_EVENT_COUNT, OW_EVENT_REVEAL_COUNT
        ),
    );
    Ok(())
}

/// Encoded length of the [`OW_EVENT_TILEMAP_PROP_SNES`] blob: walk the RLE
/// packets exactly like the game's `CODE_04DD57` until the `$FFFF` terminator
/// word, verifying the decoded output fills the WRAM buffer exactly.
fn event_tilemap_blob_len(rom_bytes: &[u8], header_offset: usize) -> anyhow::Result<usize> {
    let start = AddrPc::try_from_lorom(OW_EVENT_TILEMAP_PROP_SNES)
        .map_err(|e| anyhow::anyhow!("event tilemap address: {e}"))?
        .as_index()
        + header_offset;
    let mut pos = start;
    let mut decoded = 0usize;
    loop {
        let b = *rom_bytes.get(pos).ok_or_else(|| anyhow::anyhow!("event tilemap blob overruns ROM"))?;
        pos += 1;
        let count = (b & 0x7F) as usize + 1;
        if b & 0x80 != 0 {
            // RLE run: one value byte follows.
            pos += 1;
        } else {
            pos += count;
        }
        decoded += count;
        if pos + 2 > rom_bytes.len() {
            anyhow::bail!("event tilemap blob overruns ROM");
        }
        // The game checks for the terminator with a 16-bit load after each
        // packet (`LDA.B [_2],Y; CMP.W #$FFFF`).
        if rom_bytes[pos] == 0xFF && rom_bytes[pos + 1] == 0xFF {
            pos += 2;
            break;
        }
        if decoded > OW_EVENT_TILEMAP_DECODED_LEN {
            anyhow::bail!("event tilemap blob decodes past the WRAM buffer ({decoded:#X} > 0xD00)");
        }
    }
    if decoded != OW_EVENT_TILEMAP_DECODED_LEN {
        anyhow::bail!("event tilemap blob decoded {decoded:#X} bytes, expected 0xD00");
    }
    Ok(pos - start)
}

/// Which event each level triggers ($05D608, 93 bytes).
fn transfer_event_ownership(
    src: &[u8], src_header: usize, dst: &mut [u8], dst_header: usize, push: &mut impl FnMut(&'static str, String),
) -> anyhow::Result<()> {
    let ownership =
        EventOwnership::parse(src, src_header).map_err(|e| anyhow::anyhow!("source event ownership: {e}"))?;
    ownership.apply_to_rom(dst, dst_header).map_err(|e| anyhow::anyhow!("destination event ownership: {e}"))?;
    push("Event ownership", "93 level→event assignments".to_string());
    Ok(())
}

/// The 93 level names, re-encoded and relocated into the destination with
/// the editor's own name writer.
fn transfer_level_names(
    src: &[u8], src_header: usize, dst: &mut [u8], dst_header: usize, push: &mut impl FnMut(&'static str, String),
) -> anyhow::Result<()> {
    let patched = level_names::is_patch_applied(src, src_header);
    let names = level_names::decode_all(src, src_header, patched)
        .ok_or_else(|| anyhow::anyhow!("could not decode source level names"))?;
    let encoded =
        level_names::encode_names(&names).map_err(|e| anyhow::anyhow!("could not encode level names: {e}"))?;
    level_names::apply_to_rom(dst, dst_header, &encoded)
        .map_err(|e| anyhow::anyhow!("could not write level names to destination: {e}"))?;
    push("Level names", format!("{} names", names.len()));
    Ok(())
}

/// Message-box text (blob + pointer table) and boss-sequence text (53 fixed
/// stripe blobs).
fn transfer_text(
    src: &[u8], dst: &mut [u8], dst_header: usize, push: &mut impl FnMut(&'static str, String),
) -> anyhow::Result<()> {
    let src_rom = Rom::new(src.to_vec()).map_err(|e| anyhow::anyhow!("source ROM: {e}"))?;

    let messages = MessageBoxes::parse(&src_rom).map_err(|e| anyhow::anyhow!("source message boxes: {e}"))?;
    let (blob, pointers) = messages.to_blob_and_pointers().map_err(|e| anyhow::anyhow!("message encode: {e}"))?;
    let blob_pc = AddrPc::try_from_lorom(MESSAGE_BOXES_SNES)
        .map_err(|e| anyhow::anyhow!("message blob address: {e}"))?
        .as_index()
        + dst_header;
    let blob_end = blob_pc + blob.len();
    let max_end = blob_pc + MESSAGE_BOXES_MAX_SIZE;
    if max_end > dst.len() {
        anyhow::bail!("message blob range extends past end of destination ROM");
    }
    dst[blob_pc..blob_end].copy_from_slice(&blob);
    // Zero-fill any shrunk tail so it doesn't look like stray data (same as
    // the level editor's save).
    if blob_end < max_end {
        dst[blob_end..max_end].fill(0xFF);
    }
    let ptr_pc = AddrPc::try_from_lorom(MESSAGE_POINTER_TABLE_SNES)
        .map_err(|e| anyhow::anyhow!("message pointer table address: {e}"))?
        .as_index()
        + dst_header;
    if ptr_pc + MESSAGE_POINTER_COUNT * 2 > dst.len() {
        anyhow::bail!("message pointer table extends past end of destination ROM");
    }
    for (i, &offset) in pointers.iter().enumerate() {
        dst[ptr_pc + i * 2..ptr_pc + i * 2 + 2].copy_from_slice(&offset.to_le_bytes());
    }

    let boss = BossText::parse(&src_rom).map_err(|e| anyhow::anyhow!("source boss text: {e}"))?;
    let mut boss_count = 0;
    for boss_msgs in &boss.messages {
        for msg in boss_msgs {
            let pc =
                AddrPc::try_from_lorom(msg.snes).map_err(|e| anyhow::anyhow!("boss text address: {e}"))?.as_index()
                    + dst_header;
            let bytes = msg.to_bytes();
            dst.get_mut(pc..pc + bytes.len())
                .ok_or_else(|| anyhow::anyhow!("boss text blob ${:06X} out of range", msg.snes.0))?
                .copy_from_slice(&bytes);
            boss_count += 1;
        }
    }

    push("Message + boss text", format!("{} message boxes, {} boss stripes", messages.messages.len(), boss_count));
    Ok(())
}

/// Overworld sprites: vanilla table + visibility (fixed), custom table and
/// record-size table (RATS).
fn transfer_sprites(
    src: &[u8], src_header: usize, dst: &mut [u8], dst_header: usize, push: &mut impl FnMut(&'static str, String),
) -> anyhow::Result<()> {
    let vanilla =
        ow_sprites::VanillaOwSprites::parse(src, src_header).map_err(|e| anyhow::anyhow!("source OW sprites: {e}"))?;
    vanilla.write(dst, dst_header).map_err(|e| anyhow::anyhow!("destination OW sprites: {e}"))?;

    let mut detail = format!("{} vanilla", ow_sprites::VANILLA_SPRITE_COUNT);
    match ow_sprites::parse_custom_table(src, src_header).map_err(|e| anyhow::anyhow!("source custom sprites: {e}"))? {
        Some(table) => {
            ow_sprites::write_custom_table(&table, dst, dst_header)
                .map_err(|e| anyhow::anyhow!("destination custom sprites: {e}"))?;
            detail.push_str(&format!(", {} custom", table.total_count()));
        }
        None => {}
    }
    match ow_sprites::parse_size_table(src, src_header).map_err(|e| anyhow::anyhow!("source sprite sizes: {e}"))? {
        Some(table) => {
            match ow_sprites::write_size_table(&table, dst, dst_header) {
                Ok(()) => {}
                Err(ow_sprites::SpriteError::NoSizeTable) => {
                    ow_sprites::create_size_table(&table, dst, dst_header)
                        .map_err(|e| anyhow::anyhow!("destination sprite size table: {e}"))?;
                }
                Err(e) => return Err(anyhow::anyhow!("destination sprite size table: {e}")),
            }
            detail.push_str(", record-size table");
        }
        None => {}
    }

    push("Overworld sprites", detail);
    Ok(())
}

/// Start positions, submap music, and the reveal list is handled with event
/// data; this covers the remaining fixed settings tables.
fn transfer_settings(
    src: &[u8], src_header: usize, dst: &mut [u8], dst_header: usize, push: &mut impl FnMut(&'static str, String),
) -> anyhow::Result<()> {
    let pos =
        OverworldStartPositions::parse(src, src_header).map_err(|e| anyhow::anyhow!("source start positions: {e}"))?;
    pos.apply_to_rom(dst, dst_header).map_err(|e| anyhow::anyhow!("destination start positions: {e}"))?;

    let music = SubmapMusic::parse(src, src_header).map_err(|e| anyhow::anyhow!("source submap music: {e}"))?;
    music.apply_to_rom(dst, dst_header).map_err(|e| anyhow::anyhow!("destination submap music: {e}"))?;

    push("Start positions + submap music", "Mario/Luigi starts, 7 submap tracks".to_string());
    Ok(())
}

/// Editor-native overworld blocks: secret exits, the overworld ExAnimation
/// list (merged into the destination's shared block so its level/global
/// lists survive), the star/pipe teleport table (merged likewise), and the
/// custom level-number table + patch.
fn transfer_editor_blocks(
    src: &[u8], src_header: usize, dst: &mut [u8], dst_header: usize, push: &mut impl FnMut(&'static str, String),
) -> anyhow::Result<()> {
    let mut names = Vec::new();

    // Secret exits: only when the source actually has settings; an empty
    // write would erase the destination's block.
    let secret = secret_exits::parse_secret_exits(src, header_offset(src));
    if !secret.entries.is_empty() {
        secret_exits::write_secret_exits(&secret, dst, dst_header)
            .map_err(|e| anyhow::anyhow!("destination secret exits: {e}"))?;
        names.push(format!("secret exits ({})", secret.entries.len()));
    }

    // OW ExAnimation: merge the source's overworld list into the
    // destination's block, preserving its level/global lists.
    match ExAnimationData::parse(src) {
        Ok(src_data) => {
            let mut merged = match ExAnimationData::parse(dst) {
                Ok(d) => d,
                Err(ExAnimError::NotFound) => ExAnimationData::default(),
                Err(e) => return Err(anyhow::anyhow!("destination ExAnimation: {e}")),
            };
            merged.overworld = src_data.overworld;
            merged.write_to_rom(dst, dst_header).map_err(|e| anyhow::anyhow!("destination ExAnimation: {e}"))?;
            names.push("overworld ExAnimation".to_string());
        }
        Err(ExAnimError::NotFound) => {}
        Err(e) => return Err(anyhow::anyhow!("source ExAnimation: {e}")),
    }

    // Star/pipe teleport table: same merge pattern (per-entrance options in
    // the destination's block are preserved).
    match SecondaryExitExtData::parse(src) {
        Ok(src_data) => {
            let mut merged = match SecondaryExitExtData::parse(dst) {
                Ok(d) => d,
                Err(SecExitExtError::NotFound) => SecondaryExitExtData::default(),
                Err(e) => return Err(anyhow::anyhow!("destination secondary-exit data: {e}")),
            };
            merged.teleport_table = src_data.teleport_table;
            merged
                .write_to_rom(dst, dst_header)
                .map_err(|e| anyhow::anyhow!("destination secondary-exit data: {e}"))?;
            names.push("star/pipe warp links".to_string());
        }
        Err(SecExitExtError::NotFound) => {}
        Err(e) => return Err(anyhow::anyhow!("source secondary-exit data: {e}")),
    }

    // Custom level-number table: present when the source's patch operand no
    // longer points at WRAM $7ED000 (vanilla bytes `00 D0 7E`).
    let patch_pc = AddrPc::try_from_lorom(LEVEL_NUMBER_PATCH_OPERAND_SNES)
        .map_err(|e| anyhow::anyhow!("level-number patch address: {e}"))?
        .as_index();
    let operand = src
        .get(src_header + patch_pc..src_header + patch_pc + 3)
        .ok_or_else(|| anyhow::anyhow!("level-number patch operand out of bounds"))?;
    if operand != [0x00, 0xD0, 0x7E] {
        let table_snes = u32::from_le_bytes([operand[0], operand[1], operand[2], 0]);
        let table_pc = AddrPc::try_from_lorom(AddrSnes(table_snes))
            .map_err(|e| anyhow::anyhow!("custom level-number table address: {e}"))?
            .as_index();
        let table = src
            .get(src_header + table_pc..src_header + table_pc + overworld::OWL1_TILE_DATA_SIZE)
            .ok_or_else(|| anyhow::anyhow!("custom level-number table out of bounds"))?
            .to_vec();
        let dest_pc = find_free_space(dst, table.len(), 0x008000, dst_header)
            .ok_or_else(|| anyhow::anyhow!("no free space for the custom level-number table"))?;
        dst[dst_header + dest_pc..dst_header + dest_pc + table.len()].copy_from_slice(&table);
        let dest_snes = AddrSnes::try_from_lorom(AddrPc(dest_pc as u32))
            .map_err(|e| anyhow::anyhow!("custom level-number table address: {e}"))?;
        let bytes = dest_snes.0.to_le_bytes();
        dst[dst_header + patch_pc..dst_header + patch_pc + 3].copy_from_slice(&bytes[..3]);
        names.push("custom level numbers".to_string());
    }

    push(
        "Editor overworld blocks",
        if names.is_empty() { "none present in source".to_string() } else { names.join(", ") },
    );
    Ok(())
}

/// Overworld palettes: the fixed layer 1 / layer 2 (normal + special-world) /
/// layer 3 tables plus the two indirection tables the game uses to select
/// the submap palette.
fn transfer_palettes(
    src: &[u8], src_header: usize, dst: &mut [u8], dst_header: usize, push: &mut impl FnMut(&'static str, String),
) -> anyhow::Result<()> {
    use smwe_render::color::ABGR1555_SIZE;

    let tables: &[(u32, usize, &str)] = &[
        (0x00B528, ABGR1555_SIZE * 7 * 6, "OW layer 1 palettes"),
        (0x00B5EC, ABGR1555_SIZE * 8 * 2, "OW layer 3 palettes"),
        (0x00B3D8, ABGR1555_SIZE * 7 * 4, "OW layer 2 palettes"),
        (0x00B732, ABGR1555_SIZE * 7 * 4, "OW layer 2 palettes (special world)"),
        (0x00AD1E, 7, "OW palette indices"),
        (0x00ABDF, 14, "OW palette offsets"),
    ];
    for &(snes, len, what) in tables {
        let src_range = pc_range(src, AddrSnes(snes), len, src_header, what)?;
        let dst_range = pc_range(dst, AddrSnes(snes), len, dst_header, what)?;
        dst[dst_range].copy_from_slice(&src[src_range]);
    }
    push("Overworld palettes", "layer 1/2/3 + special-world sets".to_string());
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal synthetic ROM: 512 KiB of $FF with a valid LoROM internal
    /// header so `Rom::new` + `RomInternalHeader::parse` accept it. Only the
    /// fixed tables the transfer touches are populated; everything else stays
    /// $FF (free space for RATS allocations).
    fn synthetic_rom() -> Vec<u8> {
        let mut rom = vec![0xFFu8; 0x80000];
        // Internal header at PC $7FC0: name + LoROM map mode $20.
        let hdr = 0x7FC0;
        rom[hdr..hdr + 21].copy_from_slice(b"SYNTHETIC TRANSFER TS");
        rom[hdr + 21] = 0x20; // map mode: LoROM
        rom[hdr + 22] = 0x00; // rom type
        rom[hdr + 23] = 0x09; // rom size: 512 KiB
        rom[hdr + 24] = 0x00; // sram size
        rom[hdr + 25] = 0x01; // region: US
                              // Checksum over the image (not strictly needed for the transfer, but
                              // keeps the fixture honest).
        rewrite_checksum(&mut rom);
        rom
    }

    fn pc(snes: u32) -> usize {
        AddrPc::try_from_lorom(AddrSnes(snes)).unwrap().as_index()
    }

    /// Plant the fixtures a real ROM has but the $FF-filled synthetic image
    /// lacks: the `CODE_04DC6A` L2-stream immediates, minimal valid RLE2
    /// streams at the vanilla locations, and a valid $FFFF-terminated event
    /// tilemap blob.
    fn plant_ow_fixtures(rom: &mut [u8]) {
        // `LDA.W #OWTileNumbers` / `LDA.B #bank` / `LDA.W #OWTilemap`
        // (verified against a real ROM).
        rom[0x25C71..0x25C74].copy_from_slice(&[0xA9, 0x33, 0xA5]);
        rom[0x25C78..0x25C7A].copy_from_slice(&[0xA9, 0x04]);
        rom[0x25C8C..0x25C8F].copy_from_slice(&[0xA9, 0x2B, 0xC0]);
        // RLE2 streams decoding to 0x1000 zero words each: 32 runs of 128.
        let stream = vec![0xFF, 0x00].repeat(32);
        for snes in [0x04A533, 0x04C02B] {
            let start = pc(snes);
            rom[start..start + stream.len()].copy_from_slice(&stream);
        }
        // Event tilemap blob: 26 RLE runs of 128 zero bytes = 0xD00 decoded,
        // then the $FFFF terminator.
        let mut blob = vec![0xFF, 0x00].repeat(26);
        blob.extend_from_slice(&[0xFF, 0xFF]);
        let start = pc(0x0C8D00);
        rom[start..start + blob.len()].copy_from_slice(&blob);
        // Vanilla level-number patch operand (`LDA.L $7ED000,X`): the
        // transfer treats any other value as an installed custom table.
        let patch = pc(0x05D89C);
        rom[patch..patch + 3].copy_from_slice(&[0x00, 0xD0, 0x7E]);
    }

    #[test]
    fn rejects_empty_images() {
        assert!(transfer_overworld(&[], &synthetic_rom()).is_err());
        assert!(transfer_overworld(&synthetic_rom(), &[]).is_err());
    }

    #[test]
    fn rejects_non_rom_destination() {
        let garbage = vec![0x00u8; 0x80000];
        assert!(transfer_overworld(&synthetic_rom(), &garbage).is_err());
    }

    #[test]
    fn copies_fixed_tables_and_repairs_checksum() {
        let mut src = synthetic_rom();
        plant_ow_fixtures(&mut src);
        // Distinctive fixed-table edits in the source.
        src[pc(0x0CF7DF)] = 0x42; // OW L1 first tile
        src[pc(0x009EF0)] = 0x11; // start positions first byte
        src[pc(0x048D8A)] = 0x07; // submap music first byte
        src[pc(0x05D608)] = 0x2A; // event ownership first byte
        src[pc(0x04D85D)] = 0x34; // event tile offset low byte
        src[pc(0x04DA1D)] = 0x99; // reveal-before first byte
        src[pc(0x00B528)] = 0x77; // OW palette first byte

        let dest = synthetic_rom();
        let mut dest = dest;
        plant_ow_fixtures(&mut dest);
        let outcome = transfer_overworld(&src, &dest).expect("transfer");
        let out = &outcome.dest_bytes;

        assert_eq!(out[pc(0x0CF7DF)], 0x42);
        assert_eq!(out[pc(0x009EF0)], 0x11);
        assert_eq!(out[pc(0x048D8A)], 0x07);
        assert_eq!(out[pc(0x05D608)], 0x2A);
        assert_eq!(out[pc(0x04D85D)], 0x34);
        assert_eq!(out[pc(0x04DA1D)], 0x99);
        assert_eq!(out[pc(0x00B528)], 0x77);

        // Source untouched.
        assert_eq!(src[pc(0x0CF7DF)], 0x42);

        // Destination grew 512 KiB -> 1 MiB and the checksum is valid.
        assert_eq!(out.len(), 0x10_0000);
        assert!(outcome.report.dest_expanded);
        let mut check = out.clone();
        rewrite_checksum(&mut check);
        assert_eq!(check, *out, "checksum not repaired");

        // The report lists every section.
        assert!(outcome.report.sections.len() >= 8);
        assert!(outcome.report.summary().contains("Overworld tilemaps"));
    }

    #[test]
    fn leaves_large_destination_unexpanded() {
        let mut src = synthetic_rom();
        plant_ow_fixtures(&mut src);
        let mut dest = vec![0xFFu8; 0x10_0000];
        let synth = synthetic_rom();
        dest[..0x80000].copy_from_slice(&synth);
        plant_ow_fixtures(&mut dest);
        let outcome = transfer_overworld(&src, &dest).expect("transfer");
        assert_eq!(outcome.dest_bytes.len(), 0x10_0000);
        assert!(!outcome.report.dest_expanded);
    }

    #[test]
    fn event_tilemap_blob_walker_matches_game_decoder() {
        // Synthetic blob: literal run of 3, RLE run of 5, terminator.
        // Control byte b: (b & 0x7F) + 1 output bytes; bit 7 set = RLE.
        let mut blob = vec![
            0x02, 0xAA, 0xBB, 0xCC, // literal: 3 bytes
            0x84, 0xDD, // RLE: 5 x 0xDD
        ];
        // Pad with literal runs to reach exactly 0xD00 decoded bytes.
        let mut decoded = 3 + 5;
        while decoded < OW_EVENT_TILEMAP_DECODED_LEN {
            let n = (OW_EVENT_TILEMAP_DECODED_LEN - decoded).min(128);
            blob.push((n - 1) as u8);
            blob.extend(std::iter::repeat(0x11).take(n));
            decoded += n;
        }
        blob.extend_from_slice(&[0xFF, 0xFF]);

        let mut rom = synthetic_rom();
        let start = pc(0x0C8D00);
        rom[start..start + blob.len()].copy_from_slice(&blob);
        // Corrupt the byte right after the terminator to prove the walker
        // stops at the terminator instead of running on.
        rom[start + blob.len()] = 0x00;

        assert_eq!(event_tilemap_blob_len(&rom, 0).unwrap(), blob.len());
    }

    #[test]
    fn event_tilemap_blob_walker_rejects_truncated_blob() {
        let rom = synthetic_rom(); // $FF fill: first control byte $FF = RLE x128, then $FFFF terminator
                                   // $FF control -> RLE run of 128, consumes 1 value byte ($FF), decoded=128;
                                   // next word is $FFFF -> terminator, but decoded (128) != 0xD00 -> error.
        assert!(event_tilemap_blob_len(&rom, 0).is_err());
    }

    #[test]
    fn ow_l2_immediate_reader_rejects_moved_code() {
        let mut rom = synthetic_rom();
        // Clobber the opcode byte: the reader must fail closed, not return a
        // bogus stream address.
        rom[0x25C71] = 0x00;
        assert!(ow_l2_stream_addrs(&rom, 0).is_err());
    }

    /// Real-ROM end-to-end test: edit the overworld of a scratch copy through
    /// the real writer paths, transfer into another scratch copy, and verify
    /// the destination's overworld now matches the source's while everything
    /// else (levels, source image) is untouched. The real ROM file itself is
    /// never modified.
    #[test]
    #[ignore]
    fn real_rom_transfer_end_to_end() {
        use crate::{
            exanimation::{ExAnimFrame, ExAnimFrameKind, ExAnimTrigger},
            overworld::level_names::LEVEL_NAMES_COUNT,
        };

        let path = std::env::var("ROM_PATH").expect("ROM_PATH must point at a real SMW ROM for ignored tests");
        let raw = std::fs::read(path).expect("cannot read ROM");
        assert_eq!(raw.len(), 0x80000, "expected a 512 KiB headerless SMW ROM");
        let header_offset = 0;

        // ── Build the edited source scratch image via the real writers ──
        let mut src = raw.clone();

        // L1 tiles: stamp a distinctive block.
        {
            let l1_pc = pc(0x0CF7DF);
            for i in 0..32 {
                src[l1_pc + 100 + i] = 0x56; // level tile range
            }
        }
        // Reveal pairs.
        {
            let mut reveals = RevealTileList::parse(&src, header_offset).unwrap();
            reveals.before[0] = 0x51;
            reveals.after[0] = 0x52;
            reveals.apply_to_rom(&mut src, header_offset).unwrap();
        }
        // Submap music.
        {
            let mut music = SubmapMusic::parse(&src, header_offset).unwrap();
            assert!(music.set(0, 5));
            music.apply_to_rom(&mut src, header_offset).unwrap();
        }
        // Level names.
        {
            let mut names =
                level_names::decode_all(&src, header_offset, level_names::is_patch_applied(&src, header_offset))
                    .expect("decode names");
            assert_eq!(names.len(), LEVEL_NAMES_COUNT);
            names[0] = "TRANSFER TEST".to_string();
            let enc = level_names::encode_names(&names).unwrap();
            level_names::apply_to_rom(&mut src, header_offset, &enc).unwrap();
        }
        // Start positions.
        {
            let mut pos = OverworldStartPositions::parse(&src, header_offset).unwrap();
            pos.mario.tile_x = 0x1234;
            pos.apply_to_rom(&mut src, header_offset).unwrap();
        }
        // OW ExAnimation list.
        {
            let mut data = ExAnimationData::parse(&src).unwrap_or_default();
            data.overworld = crate::exanimation::ExAnimation {
                frames:           vec![ExAnimFrame {
                    kind:            ExAnimFrameKind::Line8x8,
                    dest:            0x6000,
                    speed:           4,
                    trigger:         ExAnimTrigger::Always,
                    trigger_num:     0,
                    frames:          2,
                    units_per_frame: 1,
                    payload:         vec![0x0100, 0x0200],
                }],
                disable_original: false,
            };
            data.write_to_rom(&mut src, header_offset).unwrap();
        }
        // Secret exits.
        {
            let mut secret = secret_exits::parse_secret_exits(&src, header_offset);
            secret.set(secret_exits::SecretExitEntry { level: 0x105, exit2: 1, exit3: 0 });
            secret_exits::write_secret_exits(&secret, &mut src, header_offset).unwrap();
        }
        // A message-box edit (via the real parse/encode path; messages keep
        // their per-message byte budgets, so edit in place).
        {
            let mut messages = MessageBoxes::parse(&Rom::new(src.clone()).unwrap()).unwrap();
            messages.messages[0][0] = 0x00;
            messages.messages[0][1] = 0x01;
            messages.messages[0][2] = 0x02;
            let (blob, pointers) = messages.to_blob_and_pointers().unwrap();
            let blob_pc = pc(0x05A5D9);
            src[blob_pc..blob_pc + blob.len()].copy_from_slice(&blob);
            let ptr_pc = pc(0x05A5A7);
            for (i, &o) in pointers.iter().enumerate() {
                src[ptr_pc + i * 2..ptr_pc + i * 2 + 2].copy_from_slice(&o.to_le_bytes());
            }
        }

        // ── Transfer into a pristine scratch destination ──
        let dest = raw.clone();
        let src_before = src.clone();
        let outcome = transfer_overworld(&src, &dest).expect("transfer");
        let out = &outcome.dest_bytes;

        // Source preserved byte-for-byte.
        assert_eq!(src, src_before, "transfer modified the source image");
        // Destination expanded 512 KiB -> 1 MiB, like LM's observed 0x80200 -> 0x100200.
        assert_eq!(out.len(), 0x10_0000);
        assert!(outcome.report.dest_expanded);

        // ── Destination's overworld now matches the source's ──
        let l1 = pc(0x0CF7DF);
        assert_eq!(out[l1 + 100..l1 + 132], src[l1 + 100..l1 + 132]);
        let reveals = RevealTileList::parse(out, header_offset).unwrap();
        assert_eq!((reveals.before[0], reveals.after[0]), (0x51, 0x52));
        let music = SubmapMusic::parse(out, header_offset).unwrap();
        assert_eq!(music.tracks[0], 5);
        let names =
            level_names::decode_all(out, header_offset, level_names::is_patch_applied(out, header_offset)).unwrap();
        assert_eq!(names[0].trim_end(), "TRANSFER TEST");
        let pos = OverworldStartPositions::parse(out, header_offset).unwrap();
        assert_eq!(pos.mario.tile_x, 0x1234);
        let exanim = ExAnimationData::parse(out).unwrap();
        assert_eq!(exanim.overworld.frames.len(), 1);
        assert_eq!(exanim.overworld.frames[0].dest, 0x6000);
        let secret = secret_exits::parse_secret_exits(out, header_offset);
        assert!(secret.get(0x105).is_some());
        let out_messages = MessageBoxes::parse(&Rom::new(out.clone()).unwrap()).unwrap();
        assert_eq!(out_messages.messages[0][..3], [0x00, 0x01, 0x02]);
        // Event tile offsets traveled too.
        let off_pc = pc(0x04D85D);
        assert_eq!(out[off_pc..off_pc + 4], src[off_pc..off_pc + 4]);

        // ── Non-overworld data untouched ──
        // Level layer-1 pointer table ($05E000): level data must be identical.
        let lvl_ptr_pc = pc(0x05E000);
        assert_eq!(out[lvl_ptr_pc..lvl_ptr_pc + 16], raw[lvl_ptr_pc..lvl_ptr_pc + 16]);
        // Title stripe untouched (title/credits are not overworld data).
        let title_pc = pc(0x05AF2C);
        assert_eq!(out[title_pc..title_pc + 16], raw[title_pc..title_pc + 16]);
        // Internal header untouched.
        assert_eq!(out[0x7FC0..0x7FD5], raw[0x7FC0..0x7FD5]);

        // Checksum repaired: recomputing over the body is a fixed point.
        let mut recheck = out.clone();
        rewrite_checksum(&mut recheck);
        assert_eq!(recheck, *out, "destination checksum not repaired");

        // Report covers every section.
        assert!(outcome.report.sections.len() >= 8, "report: {:?}", outcome.report.summary());
    }
}
