//! Lunar Magic `.mwl` single-level file codec.
//!
//! Format per the community spec (kaizoman666/SMW-Data, accurate as of LM
//! 2.53): 0x40-byte header (`"LM"` + version, pointer-list offset/size,
//! flags, 48-byte branding string), then 8 `(offset u32, size u32)` section
//! pointers: level info, Layer 1, Layer 2, sprites, palette, secondary
//! entrances, ExAnimation, ExGFX/bypass.
//!
//! Scope: this codec models the sections a vanilla-format level needs (level
//! info, Layer 1, Layer 2 as objects or BG tilemap, sprites, secondary
//! entrances) and preserves the rest as raw bytes so LM-specific data is
//! never silently mangled — importers can inspect `unsupported_sections()`
//! and tell the user exactly what was ignored.

use crate::level::{PRIMARY_HEADER_SIZE, SECONDARY_HEADER_SIZE};

pub const MWL_HEADER_SIZE: usize = 0x40;
pub const MWL_SECTION_COUNT: usize = 8;

/// Section indices in the pointer list.
pub const SEC_LEVEL_INFO: usize = 0;
pub const SEC_LAYER1: usize = 1;
pub const SEC_LAYER2: usize = 2;
pub const SEC_SPRITES: usize = 3;
pub const SEC_PALETTE: usize = 4;
pub const SEC_SECONDARY_ENTRANCES: usize = 5;
pub const SEC_EXANIMATION: usize = 6;
pub const SEC_EXGFX: usize = 7;

pub const SECTION_NAMES: [&str; MWL_SECTION_COUNT] =
    ["level info", "layer 1", "layer 2", "sprites", "palette", "secondary entrances", "ExAnimation", "ExGFX/bypass"];

/// The 48-byte branding string LM writes; kept byte-compatible so other
/// tools that eyeball it aren't confused. 3 lines × 16 chars.
const BRANDING: &[u8; 48] = b"Lunar Magic 2.53  @2015 FuSoYa  Defender of Relm";

#[derive(Debug, Clone)]
pub enum MwlLayer2 {
    /// Raw ROM-format object data: `[5-byte header][objects…0xFF]`.
    Objects(Vec<u8>),
    /// BG-tilemap layer 2 as 16-bit Map16 tiles (left tilemap half first,
    /// right second, matching both the MWL and the ROM order).
    BgTilemap(Vec<u16>),
}

#[derive(Debug, Clone)]
pub struct MwlFile {
    /// Source level number.
    pub level_number: u16,
    /// The 4 vanilla secondary-header bytes ($05F000/$05F200/$05F400/$05F600).
    pub secondary_header: [u8; SECONDARY_HEADER_SIZE],
    /// LM's 5th secondary-header byte + midway bytes + 3 extended bytes,
    /// preserved verbatim (all zero for exports from this editor).
    pub lm_level_info_extra: [u8; 8],
    /// Raw ROM-format Layer 1 block: `[5-byte primary header][objects…0xFF]`.
    pub layer1: Vec<u8>,
    /// Set when the MWL declares a custom palette (Layer-1 header byte 0 bit 0).
    pub custom_palette: bool,
    pub layer2: MwlLayer2,
    /// Layer-2 section header byte 0 (LM's per-level $0EF310 BG flag).
    pub layer2_flag: u8,
    /// Raw ROM-format sprite block: `[1-byte sprite header][3n bytes…0xFF]`.
    pub sprites: Vec<u8>,
    /// Secondary entrances: `(id, [$05FA00, $05FC00, $05FE00] bytes)`.
    pub secondary_entrances: Vec<(u16, [u8; 3])>,
    /// Raw bytes of sections this codec doesn't model (palette, ExAnimation,
    /// ExGFX), preserved for inspection; `None` when absent/empty.
    pub raw_sections: [Option<Vec<u8>>; MWL_SECTION_COUNT],
}

fn rd_u32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
}

impl MwlFile {
    pub fn parse(bytes: &[u8]) -> anyhow::Result<Self> {
        if bytes.len() < MWL_HEADER_SIZE || &bytes[0..2] != b"LM" {
            anyhow::bail!("not an .mwl file (missing LM magic)");
        }
        let ptr_list = rd_u32(bytes, 4).unwrap() as usize;
        let ptr_bytes = rd_u32(bytes, 8).unwrap() as usize;
        let n_sections = (ptr_bytes / 8).min(MWL_SECTION_COUNT);

        let mut sections: [Option<&[u8]>; MWL_SECTION_COUNT] = [None; MWL_SECTION_COUNT];
        for (i, section) in sections.iter_mut().take(n_sections).enumerate() {
            let off = rd_u32(bytes, ptr_list + i * 8)
                .ok_or_else(|| anyhow::anyhow!("truncated .mwl: pointer list out of range"))? as usize;
            let size = rd_u32(bytes, ptr_list + i * 8 + 4).unwrap_or(0) as usize;
            if size == 0 {
                continue;
            }
            *section = Some(
                bytes
                    .get(off..off + size)
                    .ok_or_else(|| anyhow::anyhow!("truncated .mwl: {} section out of range", SECTION_NAMES[i]))?,
            );
        }

        // ── Level info ───────────────────────────────────────────────────
        let info = sections[SEC_LEVEL_INFO].ok_or_else(|| anyhow::anyhow!(".mwl has no level info section"))?;
        if info.len() < 9 {
            anyhow::bail!("level info section too short ({} bytes)", info.len());
        }
        let level_number = u16::from_le_bytes([info[0], info[1]]);
        let mut secondary_header = [0u8; SECONDARY_HEADER_SIZE];
        secondary_header.copy_from_slice(&info[2..2 + SECONDARY_HEADER_SIZE]);
        let mut lm_level_info_extra = [0u8; 8];
        // 5th secondary byte, then (after 2 unused) midway 4 + 3 extended.
        lm_level_info_extra[0] = info[6];
        for (i, slot) in lm_level_info_extra[1..].iter_mut().enumerate() {
            *slot = info.get(9 + i).copied().unwrap_or(0);
        }

        // ── Layer 1 ──────────────────────────────────────────────────────
        let l1 = sections[SEC_LAYER1].ok_or_else(|| anyhow::anyhow!(".mwl has no layer 1 section"))?;
        if l1.len() < 8 + PRIMARY_HEADER_SIZE {
            anyhow::bail!("layer 1 section too short ({} bytes)", l1.len());
        }
        let custom_palette = l1[0] & 1 != 0;
        let layer1 = l1[8..].to_vec();

        // ── Layer 2 ──────────────────────────────────────────────────────
        let l2 = sections[SEC_LAYER2].ok_or_else(|| anyhow::anyhow!(".mwl has no layer 2 section"))?;
        if l2.len() < 8 {
            anyhow::bail!("layer 2 section too short ({} bytes)", l2.len());
        }
        let layer2_flag = l2[0];
        // The section header's source-address bank byte is $FF for BG-tilemap
        // layer 2 (mirroring the ROM's $05E600 pointer-table convention).
        let layer2 = if l2[6] == 0xFF {
            let data = &l2[8..];
            let tiles = data.chunks_exact(2).map(|w| u16::from_le_bytes([w[0], w[1]])).collect();
            MwlLayer2::BgTilemap(tiles)
        } else {
            MwlLayer2::Objects(l2[8..].to_vec())
        };

        // ── Sprites ──────────────────────────────────────────────────────
        let spr = sections[SEC_SPRITES].ok_or_else(|| anyhow::anyhow!(".mwl has no sprite section"))?;
        if spr.len() < 9 {
            anyhow::bail!("sprite section too short ({} bytes)", spr.len());
        }
        let sprites = spr[8..].to_vec();

        // ── Secondary entrances ──────────────────────────────────────────
        let mut secondary_entrances = Vec::new();
        if let Some(sec) = sections[SEC_SECONDARY_ENTRANCES] {
            for entry in sec.get(8..).unwrap_or(&[]).chunks_exact(8) {
                let id = u16::from_le_bytes([entry[0], entry[1]]);
                secondary_entrances.push((id, [entry[2], entry[3], entry[4]]));
            }
        }

        let mut raw_sections: [Option<Vec<u8>>; MWL_SECTION_COUNT] = Default::default();
        for i in [SEC_PALETTE, SEC_EXANIMATION, SEC_EXGFX] {
            raw_sections[i] = sections[i].map(|s| s.to_vec());
        }

        Ok(Self {
            level_number,
            secondary_header,
            lm_level_info_extra,
            layer1,
            custom_palette,
            layer2,
            layer2_flag,
            sprites,
            secondary_entrances,
            raw_sections,
        })
    }

    /// Names of LM-specific sections present in this file that the importer
    /// won't apply (plus the custom-palette flag), for user-facing warnings.
    /// ExAnimation/palette headers with no payload beyond the 8 header bytes
    /// don't count.
    pub fn unsupported_sections(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.custom_palette {
            out.push("custom palette");
        }
        for i in [SEC_PALETTE, SEC_EXANIMATION, SEC_EXGFX] {
            if let Some(raw) = &self.raw_sections[i] {
                let payload = raw.get(8..).unwrap_or(&[]);
                // The palette section is always written by LM; it only
                // matters when the custom-palette flag is set (handled above).
                if i != SEC_PALETTE && payload.iter().any(|&b| b != 0) {
                    out.push(SECTION_NAMES[i]);
                }
            }
        }
        out
    }

    pub fn serialize(&self) -> Vec<u8> {
        // Section payloads in order.
        let mut info = vec![0u8; 0x20];
        info[0..2].copy_from_slice(&self.level_number.to_le_bytes());
        info[2..2 + SECONDARY_HEADER_SIZE].copy_from_slice(&self.secondary_header);
        info[6] = self.lm_level_info_extra[0];
        info[9..9 + 7].copy_from_slice(&self.lm_level_info_extra[1..]);

        let mut l1 = vec![0u8; 8];
        l1[0] = self.custom_palette as u8;
        l1.extend_from_slice(&self.layer1);

        let mut l2 = vec![0u8; 8];
        l2[0] = self.layer2_flag;
        match &self.layer2 {
            MwlLayer2::Objects(data) => l2.extend_from_slice(data),
            MwlLayer2::BgTilemap(tiles) => {
                l2[6] = 0xFF;
                for t in tiles {
                    l2.extend_from_slice(&t.to_le_bytes());
                }
            }
        }

        let mut spr = vec![0u8; 8];
        spr.extend_from_slice(&self.sprites);

        let mut entrances = vec![0u8; 8];
        for (id, bytes) in &self.secondary_entrances {
            entrances.extend_from_slice(&id.to_le_bytes());
            entrances.extend_from_slice(bytes);
            entrances.extend_from_slice(&[0, 0, 0]);
        }

        let sections: [Option<Vec<u8>>; MWL_SECTION_COUNT] = [
            Some(info),
            Some(l1),
            Some(l2),
            Some(spr),
            self.raw_sections[SEC_PALETTE].clone(),
            Some(entrances),
            self.raw_sections[SEC_EXANIMATION].clone(),
            self.raw_sections[SEC_EXGFX].clone(),
        ];

        // Header + pointer list + data.
        let mut out = Vec::new();
        out.extend_from_slice(b"LM");
        out.extend_from_slice(&[0x53, 0x02]); // "created by" LM version 2.53
        out.extend_from_slice(&(MWL_HEADER_SIZE as u32).to_le_bytes()); // pointer list offset
        out.extend_from_slice(&((MWL_SECTION_COUNT * 8) as u32).to_le_bytes()); // pointer bytes
        out.extend_from_slice(&[0u8; 4]); // flags
        out.extend_from_slice(BRANDING);
        debug_assert_eq!(out.len(), MWL_HEADER_SIZE);

        let mut data_off = MWL_HEADER_SIZE + MWL_SECTION_COUNT * 8;
        let mut data = Vec::new();
        for section in &sections {
            match section {
                Some(payload) if !payload.is_empty() => {
                    out.extend_from_slice(&(data_off as u32).to_le_bytes());
                    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                    data.extend_from_slice(payload);
                    data_off += payload.len();
                }
                _ => out.extend_from_slice(&[0u8; 8]),
            }
        }
        out.extend_from_slice(&data);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MwlFile {
        MwlFile {
            level_number: 0x105,
            secondary_header: [0x12, 0x34, 0x56, 0x78],
            lm_level_info_extra: [0; 8],
            layer1: vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x62, 0x00, 0x10, 0xFF],
            custom_palette: false,
            layer2: MwlLayer2::Objects(vec![0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x63, 0x11, 0x22, 0xFF]),
            layer2_flag: 0,
            sprites: vec![0x08, 0x00, 0x10, 0x0F, 0xFF],
            secondary_entrances: vec![(0x0042, [0xAA, 0xBB, 0xCC])],
            raw_sections: Default::default(),
        }
    }

    #[test]
    fn round_trips_objects_level() {
        let mwl = sample();
        let bytes = mwl.serialize();
        let parsed = MwlFile::parse(&bytes).unwrap();
        assert_eq!(parsed.level_number, 0x105);
        assert_eq!(parsed.secondary_header, mwl.secondary_header);
        assert_eq!(parsed.layer1, mwl.layer1);
        assert!(matches!(&parsed.layer2, MwlLayer2::Objects(d) if *d == vec![0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x63, 0x11, 0x22, 0xFF]));
        assert_eq!(parsed.sprites, mwl.sprites);
        assert_eq!(parsed.secondary_entrances, mwl.secondary_entrances);
        assert!(parsed.unsupported_sections().is_empty());
    }

    #[test]
    fn round_trips_bg_tilemap_level() {
        let mut mwl = sample();
        mwl.layer2 = MwlLayer2::BgTilemap(vec![0x0123, 0x0456, 0x0789]);
        let parsed = MwlFile::parse(&mwl.serialize()).unwrap();
        assert!(matches!(&parsed.layer2, MwlLayer2::BgTilemap(t) if *t == vec![0x0123, 0x0456, 0x0789]));
    }

    #[test]
    fn rejects_non_mwl() {
        assert!(MwlFile::parse(b"not an mwl file at all........................................").is_err());
        assert!(MwlFile::parse(&[]).is_err());
    }

    /// Full-pipeline check against a real ROM: pull level 0x105's raw
    /// Layer-1/sprite blocks out of smw.smc, wrap them in an .mwl,
    /// serialize+reparse, and confirm the payloads survive byte-for-byte and
    /// still parse with the level-data parsers the importer uses. Run with
    /// `ROM_PATH=/path/to/smw.smc cargo test -p smwe-rom --lib -- --ignored
    /// real_level_survives_mwl_round_trip`.
    #[test]
    #[ignore]
    fn real_level_survives_mwl_round_trip() {
        use crate::{
            level::{ObjectLayer, SpriteLayer},
            snes_utils::addr::{AddrPc, AddrSnes},
        };
        let rom_path = std::env::var("ROM_PATH").expect("set ROM_PATH");
        let raw = std::fs::read(rom_path).expect("read ROM");
        let rom = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
        let pc = |snes: u32| AddrPc::try_from_lorom(AddrSnes(snes)).unwrap().as_index();

        let level = 0x105usize;
        let l1_ptr = pc(0x05E000) + level * 3;
        let l1_snes = u32::from_le_bytes([rom[l1_ptr], rom[l1_ptr + 1], rom[l1_ptr + 2], 0]);
        let l1_at = pc(l1_snes);
        let (_, (_, l1_len)) = ObjectLayer::parse(&rom[l1_at + PRIMARY_HEADER_SIZE..]).expect("parse L1");
        let layer1 = rom[l1_at..l1_at + PRIMARY_HEADER_SIZE + l1_len].to_vec();

        let spr_ptr = pc(0x05EC00) + level * 2;
        let spr_snes = u16::from_le_bytes([rom[spr_ptr], rom[spr_ptr + 1]]) as u32 | 0x070000;
        let spr_at = pc(spr_snes);
        let (_, (_, spr_len)) = SpriteLayer::parse(&rom[spr_at + 1..]).expect("parse sprites");
        let sprites = rom[spr_at..spr_at + 1 + spr_len].to_vec();

        let mwl = MwlFile {
            level_number: level as u16,
            secondary_header: [
                rom[pc(0x05F000) + level],
                rom[pc(0x05F200) + level],
                rom[pc(0x05F400) + level],
                rom[pc(0x05F600) + level],
            ],
            lm_level_info_extra: [0; 8],
            layer1: layer1.clone(),
            custom_palette: false,
            layer2: MwlLayer2::Objects(vec![0, 0, 0, 0, 0, 0xFF]),
            layer2_flag: 0,
            sprites: sprites.clone(),
            secondary_entrances: Vec::new(),
            raw_sections: Default::default(),
        };
        let parsed = MwlFile::parse(&mwl.serialize()).expect("reparse");
        assert_eq!(parsed.layer1, layer1);
        assert_eq!(parsed.sprites, sprites);
        // The importer's parsers accept the round-tripped payloads.
        ObjectLayer::parse(&parsed.layer1[PRIMARY_HEADER_SIZE..]).expect("round-tripped L1 parses");
        SpriteLayer::parse(&parsed.sprites[1..]).expect("round-tripped sprites parse");
    }

    #[test]
    fn flags_unsupported_lm_sections() {
        let mut mwl = sample();
        mwl.custom_palette = true;
        mwl.raw_sections[SEC_EXANIMATION] = Some({
            let mut v = vec![0u8; 8];
            v.extend_from_slice(&[1, 2, 3]);
            v
        });
        let parsed = MwlFile::parse(&mwl.serialize()).unwrap();
        let unsupported = parsed.unsupported_sections();
        assert!(unsupported.contains(&"custom palette"));
        assert!(unsupported.contains(&"ExAnimation"));
    }
}
