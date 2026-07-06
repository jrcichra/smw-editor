//! Fixed-location title-screen and ending enemy-credit data.
//!
//! This covers the small, directly editable pieces of SMW's title/credits
//! subsystem:
//! - title screen submap immediate operand in `GM03LoadTitleScreen`
//! - title demo controller playback bytes in `TitleScreenInputSeq`
//! - ending enemy-name stripe images (`EnemyNameStripe00..0C`)
//!
//! The staff roll and credits scene scripts are separate systems in bank 0C and
//! are intentionally not modeled here.

use crate::snes_utils::{
    addr::{AddrPc, AddrSnes},
    rom::Rom,
};

pub const TITLE_SUBMAP_OPERAND_SNES: AddrSnes = AddrSnes(0x0096CE);
pub const TITLE_INPUT_SEQ_SNES: AddrSnes = AddrSnes(0x009C1F);
pub const TITLE_INPUT_SEQ_MAX_SIZE: usize = 0x009C64 - 0x009C1F;
pub const TITLE_SCREEN_STRIPE_SNES: AddrSnes = AddrSnes(0x05B375);
pub const TITLE_SCREEN_STRIPE_END_SNES: AddrSnes = AddrSnes(0x05B7C9);
pub const TITLE_SCREEN_STRIPE_MAX_SIZE: usize = 0x05B7C9 - 0x05B375;

pub const ENEMY_NAME_COUNT: usize = 13;
pub const ENEMY_NAME_STRIPE_STARTS: [AddrSnes; ENEMY_NAME_COUNT] = [
    AddrSnes(0x0DF300),
    AddrSnes(0x0DF42D),
    AddrSnes(0x0DF572),
    AddrSnes(0x0DF66B),
    AddrSnes(0x0DF742),
    AddrSnes(0x0DF837),
    AddrSnes(0x0DF8FA),
    AddrSnes(0x0DF9CD),
    AddrSnes(0x0DFA98),
    AddrSnes(0x0DFB73),
    AddrSnes(0x0DFC58),
    AddrSnes(0x0DFCD5),
    AddrSnes(0x0DFD5C),
];
pub const ENEMY_NAME_STRIPE_END_SNES: AddrSnes = AddrSnes(0x0DFE5A);

pub const ENEMY_NAME_LABELS: [&str; ENEMY_NAME_COUNT] = [
    "Lakitu / Para-bombs",
    "Amazin' Flyin' Hammer Brother",
    "Sumo Brother",
    "Rex / Mega Mole / Banzai Bill",
    "Dino-Rhino / Dino-Torch / Koopas",
    "Spike Top / Swoopers / Buzzy Beetle / Blargg",
    "Blurps / Urchin / Porcu-Puffer / Torpedo Ted / Rip Van Fish",
    "Boo Buddies / Fishin' Boo / Big Boo / Eeries",
    "Lil Sparky / Bony Beetle / Dry Bones / Thwomps",
    "Grinder / Ball 'n' Chain / Fishbone",
    "Reznor",
    "Mechakoopas",
    "Koopalings / Bowser",
];

#[derive(Debug, Clone)]
pub struct TitleDemoInput {
    pub buttons: u8,
    pub duration: u8,
}

#[derive(Debug, Clone)]
pub struct TitleCreditsData {
    pub title_submap: u8,
    pub title_demo_inputs: Vec<TitleDemoInput>,
    pub title_screen_stripe: Vec<u8>,
    pub enemy_name_stripes: Vec<Vec<u8>>,
}

impl TitleCreditsData {
    pub fn empty() -> Self {
        Self {
            title_submap: 0,
            title_demo_inputs: Vec::new(),
            title_screen_stripe: vec![0xFF],
            enemy_name_stripes: vec![vec![0xFF]; ENEMY_NAME_COUNT],
        }
    }

    pub fn parse(rom: &Rom) -> anyhow::Result<Self> {
        let title_submap_pc = AddrPc::try_from_lorom(TITLE_SUBMAP_OPERAND_SNES)?.as_index();
        let title_submap =
            *rom.0.get(title_submap_pc).ok_or_else(|| anyhow::anyhow!("title submap operand out of range"))?;

        let input_pc = AddrPc::try_from_lorom(TITLE_INPUT_SEQ_SNES)?.as_index();
        let input_bytes = rom
            .0
            .get(input_pc..input_pc + TITLE_INPUT_SEQ_MAX_SIZE)
            .ok_or_else(|| anyhow::anyhow!("title input sequence out of range"))?;
        let mut title_demo_inputs = Vec::new();
        let mut i = 0;
        while i < input_bytes.len() {
            if input_bytes[i] == 0xFF {
                break;
            }
            if i + 1 >= input_bytes.len() {
                anyhow::bail!("title input sequence missing duration before end of fixed slot");
            }
            title_demo_inputs.push(TitleDemoInput { buttons: input_bytes[i], duration: input_bytes[i + 1] });
            i += 2;
        }

        let title_screen_stripe = read_terminated_slot(
            rom,
            TITLE_SCREEN_STRIPE_SNES,
            TITLE_SCREEN_STRIPE_END_SNES,
            "title screen stripe image",
        )?;

        let mut enemy_name_stripes = Vec::with_capacity(ENEMY_NAME_COUNT);
        for i in 0..ENEMY_NAME_COUNT {
            let end_snes =
                if i + 1 < ENEMY_NAME_COUNT { ENEMY_NAME_STRIPE_STARTS[i + 1] } else { ENEMY_NAME_STRIPE_END_SNES };
            enemy_name_stripes.push(read_terminated_slot(
                rom,
                ENEMY_NAME_STRIPE_STARTS[i],
                end_snes,
                &format!("enemy name stripe {i}"),
            )?);
        }

        Ok(Self { title_submap, title_demo_inputs, title_screen_stripe, enemy_name_stripes })
    }

    pub fn title_input_bytes(&self) -> anyhow::Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(self.title_demo_inputs.len() * 2 + 1);
        for input in &self.title_demo_inputs {
            bytes.push(input.buttons);
            bytes.push(input.duration);
        }
        bytes.push(0xFF);
        if bytes.len() > TITLE_INPUT_SEQ_MAX_SIZE {
            anyhow::bail!(
                "Title demo input is {} bytes, but the vanilla fixed slot is only {TITLE_INPUT_SEQ_MAX_SIZE} bytes",
                bytes.len()
            );
        }
        Ok(bytes)
    }

    pub fn enemy_name_slot_size(index: usize) -> usize {
        let start = ENEMY_NAME_STRIPE_STARTS[index].0 as usize;
        let end = if index + 1 < ENEMY_NAME_COUNT {
            ENEMY_NAME_STRIPE_STARTS[index + 1].0 as usize
        } else {
            ENEMY_NAME_STRIPE_END_SNES.0 as usize
        };
        end - start
    }

    pub fn validate_enemy_name_stripe(index: usize, bytes: &[u8]) -> anyhow::Result<()> {
        let slot_size = Self::enemy_name_slot_size(index);
        if bytes.len() > slot_size {
            anyhow::bail!(
                "Credits enemy stripe {index:02X} is {} bytes, but its fixed vanilla slot is only {slot_size} bytes",
                bytes.len()
            );
        }
        if !bytes.ends_with(&[0xFF]) {
            anyhow::bail!("Credits enemy stripe {index:02X} must end with FF");
        }
        Ok(())
    }

    pub fn validate_title_screen_stripe(&self) -> anyhow::Result<()> {
        if self.title_screen_stripe.len() > TITLE_SCREEN_STRIPE_MAX_SIZE {
            anyhow::bail!(
                "Title screen stripe is {} bytes, but the vanilla fixed slot is only {TITLE_SCREEN_STRIPE_MAX_SIZE} bytes",
                self.title_screen_stripe.len()
            );
        }
        if !self.title_screen_stripe.ends_with(&[0xFF]) {
            anyhow::bail!("Title screen stripe must end with FF");
        }
        Ok(())
    }
}

fn read_terminated_slot(rom: &Rom, start: AddrSnes, end: AddrSnes, label: &str) -> anyhow::Result<Vec<u8>> {
    let start_pc = AddrPc::try_from_lorom(start)?.as_index();
    let end_pc = AddrPc::try_from_lorom(end)?.as_index();
    let slot = rom.0.get(start_pc..end_pc).ok_or_else(|| anyhow::anyhow!("{label} out of range"))?;
    let end = slot.iter().position(|&b| b == 0xFF).map(|p| p + 1).unwrap_or(slot.len());
    Ok(slot[..end].to_vec())
}

pub fn decode_credit_tile(byte: u8) -> char {
    match byte {
        0x0A..=0x23 => (b'A' + (byte - 0x0A)) as char,
        0x24 => '-',
        0x27 => '.',
        0x85 | 0x86 => '\'',
        0xFC => ' ',
        _ => '?',
    }
}

pub fn summarize_enemy_name_stripe(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i] == 0xFF {
            break;
        }
        let len = bytes[i + 3] as usize + 1;
        let data_start = i + 4;
        let data_end = data_start + len;
        if data_end > bytes.len() {
            break;
        }

        let text_like = bytes[data_start..data_end]
            .chunks_exact(2)
            .filter(|pair| matches!(pair[0], 0x0A..=0x27 | 0x85 | 0x86 | 0xFC) && matches!(pair[1], 0x00 | 0x38 | 0x78))
            .count();
        if text_like >= 2 {
            if !out.is_empty() {
                out.push_str(" / ");
            }
            for pair in bytes[data_start..data_end].chunks_exact(2) {
                out.push(decode_credit_tile(pair[0]));
            }
        }
        i = data_end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_input_bytes_include_terminator() {
        let data = TitleCreditsData {
            title_submap: 0,
            title_demo_inputs: vec![TitleDemoInput { buttons: 0x41, duration: 0x0F }],
            title_screen_stripe: vec![0xFF],
            enemy_name_stripes: vec![Vec::new(); ENEMY_NAME_COUNT],
        };
        assert_eq!(data.title_input_bytes().unwrap(), vec![0x41, 0x0F, 0xFF]);
    }

    #[test]
    fn enemy_name_slots_are_positive() {
        for i in 0..ENEMY_NAME_COUNT {
            assert!(TitleCreditsData::enemy_name_slot_size(i) > 0);
        }
    }

    #[test]
    fn empty_data_has_saveable_enemy_stripes() {
        let data = TitleCreditsData::empty();
        assert_eq!(data.title_screen_stripe, &[0xFF]);
        data.validate_title_screen_stripe().unwrap();
        assert_eq!(data.enemy_name_stripes.len(), ENEMY_NAME_COUNT);
        for (i, stripe) in data.enemy_name_stripes.iter().enumerate() {
            assert_eq!(stripe, &[0xFF]);
            TitleCreditsData::validate_enemy_name_stripe(i, stripe).unwrap();
        }
    }

    #[test]
    fn enemy_name_validation_rejects_missing_terminator() {
        let err = TitleCreditsData::validate_enemy_name_stripe(0, &[0x20, 0x00]).unwrap_err();
        assert!(err.to_string().contains("must end with FF"));
    }
}
