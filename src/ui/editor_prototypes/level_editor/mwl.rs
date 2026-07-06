//! Lunar Magic `.mwl` single-level import/export for the level editor.
//!
//! Export serializes the *current editor state* (unsaved edits included) so
//! what you see is what you share. Import replaces the editor state with the
//! file's contents and marks the level edited — nothing touches the ROM until
//! the regular save path runs, which also means an import you don't like can
//! be discarded by reloading the level.
//!
//! Vanilla-format scope: level info + Layer 1 + Layer 2 (objects or BG
//! tilemap) + sprites + secondary entrances. LM-specific payloads (custom
//! palette, ExAnimation, ExGFX bypass) are detected and reported as skipped
//! rather than silently dropped or half-applied.

use rfd::{MessageButtons, MessageDialog, MessageLevel};
use smwe_rom::{
    level::{
        background::BackgroundData,
        Layer2Data,
        Level,
        ObjectLayer,
        PrimaryHeader,
        SecondaryHeader,
        SpriteHeader,
        SpriteLayer,
        PRIMARY_HEADER_SIZE,
    },
    mwl::{MwlFile, MwlLayer2},
    snes_utils::addr::{AddrPc, AddrSnes},
};

use super::UiLevelEditor;

impl UiLevelEditor {
    pub(super) fn export_mwl(&mut self) {
        let Some(path) =
            rfd::FileDialog::new().set_file_name(format!("level{:03X}.mwl", self.level_num)).save_file()
        else {
            return;
        };
        match self.build_mwl().map(|mwl| std::fs::write(&path, mwl.serialize())) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => error_dialog(format!("Failed to write .mwl: {e}")),
            Err(e) => error_dialog(format!("Failed to export .mwl: {e}")),
        }
    }

    pub(super) fn import_mwl(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Lunar Magic level", &["mwl"]).pick_file() else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => return error_dialog(format!("Failed to read .mwl: {e}")),
        };
        let mwl = match MwlFile::parse(&bytes) {
            Ok(mwl) => mwl,
            Err(e) => return error_dialog(format!("Not a usable .mwl file: {e}")),
        };
        match self.apply_mwl(&mwl) {
            Ok(()) => {
                let mut msg = format!(
                    "Imported level {:03X} data from {} into level {:03X}.",
                    mwl.level_number,
                    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    self.level_num,
                );
                let skipped = mwl.unsupported_sections();
                if !skipped.is_empty() {
                    msg.push_str(&format!("\n\nSkipped LM-specific data this editor doesn't model yet: {}.", skipped.join(", ")));
                }
                msg.push_str("\n\nNothing is written to the ROM until you save.");
                MessageDialog::new()
                    .set_level(MessageLevel::Info)
                    .set_buttons(MessageButtons::Ok)
                    .set_description(msg)
                    .show();
            }
            Err(e) => error_dialog(format!("Import failed, level unchanged: {e}")),
        }
    }

    /// Serialize the current editor state as an [`MwlFile`].
    fn build_mwl(&self) -> anyhow::Result<MwlFile> {
        let vertical = self.level_properties.is_vertical;

        let mut layer1 = self.primary_header_bytes().to_vec();
        layer1.extend(self.layer1.read(|l| l.serialize_layer1_bytes(vertical))?);

        let rom_bytes = self.rom.disassembly.rom_bytes();
        let layer2 = if let Some(bg) = &self.layer2_background {
            // ROM keeps the low and high tile bytes in two separate blocks;
            // the MWL wants interleaved 16-bit Map16 tiles.
            let ids = bg.read(|b| b.tile_ids.clone());
            let half = ids.len() / 2;
            let tiles =
                (0..half).map(|i| u16::from_le_bytes([ids[i], ids.get(half + i).copied().unwrap_or(0)])).collect();
            MwlLayer2::BgTilemap(tiles)
        } else if let Some(objects) = &self.layer2_objects {
            // Raw ROM block = [5-byte L2 header (not edited by this editor,
            // copied from the ROM)][serialized objects].
            let l2_ptr_pc = AddrPc::try_from_lorom(AddrSnes(0x05E600))?.as_index() + self.level_num as usize * 3;
            let l2_snes = rom_bytes
                .get(l2_ptr_pc..l2_ptr_pc + 3)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], 0]))
                .ok_or_else(|| anyhow::anyhow!("L2 pointer table out of range"))?;
            let l2_pc = AddrPc::try_from_lorom(AddrSnes(l2_snes))?.as_index();
            let mut data = rom_bytes
                .get(l2_pc..l2_pc + PRIMARY_HEADER_SIZE)
                .ok_or_else(|| anyhow::anyhow!("L2 header out of range"))?
                .to_vec();
            data.extend(objects.read(|l| l.serialize_layer1_bytes(vertical))?);
            MwlLayer2::Objects(data)
        } else {
            anyhow::bail!("level has no layer 2 state loaded");
        };

        // Sprite block = [sprite header byte (not edited, from ROM)][data…FF].
        let spr_ptr_pc = AddrPc::try_from_lorom(AddrSnes(0x05EC00))?.as_index() + self.level_num as usize * 2;
        let spr_off = rom_bytes
            .get(spr_ptr_pc..spr_ptr_pc + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .ok_or_else(|| anyhow::anyhow!("sprite pointer table out of range"))?;
        let spr_pc = AddrPc::try_from_lorom(AddrSnes(spr_off as u32 | 0x070000))?.as_index();
        let sprite_header = rom_bytes.get(spr_pc).copied().unwrap_or(0);
        let mut sprites = vec![sprite_header];
        sprites.extend(self.sprites.read(|s| s.serialize_bytes(vertical))?);

        // Secondary entrances whose destination is this level.
        let secondary_entrances = self
            .secondary_entrance_data
            .iter()
            .enumerate()
            .filter(|(_, bytes)| {
                let dest = ((bytes[3] as u16 & 0b1000) << 5) | bytes[0] as u16;
                dest == self.level_num
            })
            .map(|(id, bytes)| (id as u16, [bytes[1], bytes[2], bytes[3]]))
            .collect();

        Ok(MwlFile {
            level_number: self.level_num,
            secondary_header: self.secondary_header_bytes(),
            lm_level_info_extra: [0; 8],
            layer1,
            custom_palette: false,
            layer2,
            layer2_flag: 0,
            sprites,
            secondary_entrances,
            raw_sections: Default::default(),
        })
    }

    /// Replace the editor state with the .mwl's level data.
    fn apply_mwl(&mut self, mwl: &MwlFile) -> anyhow::Result<()> {
        if mwl.layer1.len() < PRIMARY_HEADER_SIZE + 1 {
            anyhow::bail!("layer 1 data too short");
        }
        let primary_header = PrimaryHeader::new(&mwl.layer1[..PRIMARY_HEADER_SIZE]);
        let (_, (layer1, _)) = ObjectLayer::parse(&mwl.layer1[PRIMARY_HEADER_SIZE..])
            .map_err(|e| anyhow::anyhow!("bad layer 1 object data: {e}"))?;

        let layer2 = match &mwl.layer2 {
            MwlLayer2::Objects(data) => {
                if data.len() < PRIMARY_HEADER_SIZE + 1 {
                    anyhow::bail!("layer 2 object data too short");
                }
                let (_, (objects, _)) = ObjectLayer::parse(&data[PRIMARY_HEADER_SIZE..])
                    .map_err(|e| anyhow::anyhow!("bad layer 2 object data: {e}"))?;
                Layer2Data::Objects(objects)
            }
            MwlLayer2::BgTilemap(tiles) => {
                // Back to the ROM's two-block layout: low bytes then high.
                let mut ids = Vec::with_capacity(tiles.len() * 2);
                ids.extend(tiles.iter().map(|t| (*t & 0xFF) as u8));
                ids.extend(tiles.iter().map(|t| (*t >> 8) as u8));
                Layer2Data::Background(BackgroundData::from_tile_ids(ids))
            }
        };

        // The save path can only write layer 2 in the same representation the
        // ROM currently uses for this level (the $05E600 pointer decides).
        let rom_level = self
            .rom
            .levels
            .get(self.level_num as usize)
            .ok_or_else(|| anyhow::anyhow!("level {:03X} out of range", self.level_num))?;
        match (&rom_level.layer2, &layer2) {
            (Layer2Data::Objects(_), Layer2Data::Objects(_)) | (Layer2Data::Background(_), Layer2Data::Background(_)) => {}
            (Layer2Data::Objects(_), Layer2Data::Background(_)) => {
                anyhow::bail!(
                    "this .mwl uses a layer 2 background tilemap, but level {:03X} in the ROM uses layer 2 \
                     objects; import it over a background-tilemap level instead",
                    self.level_num
                )
            }
            (Layer2Data::Background(_), Layer2Data::Objects(_)) => {
                anyhow::bail!(
                    "this .mwl uses layer 2 objects, but level {:03X} in the ROM uses a background tilemap; \
                     import it over a layer-2-objects level instead",
                    self.level_num
                )
            }
        }

        if mwl.sprites.is_empty() {
            anyhow::bail!("sprite data missing");
        }
        let sprite_header = SpriteHeader(mwl.sprites[0]);
        let (_, (sprite_layer, _)) =
            SpriteLayer::parse(&mwl.sprites[1..]).map_err(|e| anyhow::anyhow!("bad sprite data: {e}"))?;

        let level = Level {
            primary_header,
            secondary_header: SecondaryHeader(mwl.secondary_header),
            sprite_header,
            layer1,
            layer2,
            sprite_layer,
        };
        self.apply_level_to_editor(&level);

        // Secondary entrances: retarget each imported entrance at *this*
        // level (the .mwl may have been exported from a different number).
        for &(id, bytes) in &mwl.secondary_entrances {
            if let Some(slot) = self.secondary_entrance_data.get_mut(id as usize) {
                slot[0] = (self.level_num & 0xFF) as u8;
                slot[1] = bytes[0];
                slot[2] = bytes[1];
                slot[3] = (bytes[2] & !0b1000) | (((self.level_num >> 8) as u8 & 1) << 3);
            }
        }

        self.mark_edited();
        Ok(())
    }
}

fn error_dialog(msg: String) {
    MessageDialog::new().set_level(MessageLevel::Error).set_buttons(MessageButtons::Ok).set_description(msg).show();
}
