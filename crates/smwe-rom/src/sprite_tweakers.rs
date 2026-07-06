//! Sprite "tweaker" behavior byte tables — one byte per vanilla sprite ID,
//! shared globally across every placement of that sprite (this is Lunar
//! Magic's "Sprite Header Editor").
//!
//! Ported from SMWDisX `bank_07.asm` (`LoadTweakerBytes`) and `symbols/SMW_U.sym`.
//! Runtime copies live at WRAM $1656/$1662/$166E/$167A/$1686/$190F; the ROM
//! source tables below are copied into those on sprite init.
//!
//! Bit layouts confirmed via community documentation (smwcentral.net) since
//! SMWDisX doesn't label them:
//! - Tweaker A ($1656): `sSjJcccc` — s=smoke on death, S=hop in/kick shells,
//!   j=dies when jumped on, J=can be jumped on, cccc=object clipping
//! - Tweaker B ($1662): `dscccccc` — d=falls straight down when killed,
//!   s=use shell as death frame, cccccc=sprite clipping
//! - Tweaker C ($166E): `lwcfpppg` — l=don't interact with layer 2/3 tides,
//!   w=disable water splash, c=disable cape killing, f=disable fireball
//!   killing, ppp=palette, g=use second graphics page
//! - Tweaker D ($167A): `dpmksPiS` — d=don't use default player interaction,
//!   p=gives powerup when eaten by Yoshi, m=process interaction every frame,
//!   k=can't be kicked like a shell, s=don't turn into shell when stunned,
//!   P=process while off screen, i=invincible to star/cape/fire/bouncing
//!   bricks, S=don't disable clipping when killed with star
//! - Tweaker E ($1686): `dnctswye` — d=don't interact with objects,
//!   n=spawns a new sprite, c=don't turn into coin when goal passed,
//!   t=don't change direction if touched, s=don't interact with other
//!   sprites, w=weird ground behavior, y=stay in Yoshi's mouth, e=inedible
//! - Tweaker F ($190F): single-bit "is two tiles high" flag per sprite.

use crate::snes_utils::{
    addr::{AddrPc, AddrSnes},
    rom::Rom,
};

/// Number of sprite IDs with tweaker bytes (`$00`-`$C8`). IDs above this are
/// cluster/extended/generator sprites that don't use the standard tweaker
/// system.
pub const SPRITE_TWEAKER_COUNT: usize = 0xC9;

pub const SPRITE_TWEAKER_A_SNES: AddrSnes = AddrSnes(0x07F26C);
pub const SPRITE_TWEAKER_B_SNES: AddrSnes = AddrSnes(0x07F335);
pub const SPRITE_TWEAKER_C_SNES: AddrSnes = AddrSnes(0x07F3FE);
pub const SPRITE_TWEAKER_D_SNES: AddrSnes = AddrSnes(0x07F4C7);
pub const SPRITE_TWEAKER_E_SNES: AddrSnes = AddrSnes(0x07F590);
pub const SPRITE_TWEAKER_F_SNES: AddrSnes = AddrSnes(0x07F659);

#[derive(Debug, Clone)]
pub struct SpriteTweakers {
    pub tweaker_a: Vec<u8>,
    pub tweaker_b: Vec<u8>,
    pub tweaker_c: Vec<u8>,
    pub tweaker_d: Vec<u8>,
    pub tweaker_e: Vec<u8>,
    pub tweaker_f: Vec<u8>,
}

impl SpriteTweakers {
    pub fn parse(rom: &Rom) -> anyhow::Result<Self> {
        let read_table = |addr: AddrSnes| -> anyhow::Result<Vec<u8>> {
            let pc = AddrPc::try_from_lorom(addr).map_err(|e| anyhow::anyhow!("sprite tweaker addr conversion: {e}"))?.0
                as usize;
            let end = pc + SPRITE_TWEAKER_COUNT;
            if end > rom.0.len() {
                anyhow::bail!("sprite tweaker table extends past end of ROM");
            }
            Ok(rom.0[pc..end].to_vec())
        };
        Ok(Self {
            tweaker_a: read_table(SPRITE_TWEAKER_A_SNES)?,
            tweaker_b: read_table(SPRITE_TWEAKER_B_SNES)?,
            tweaker_c: read_table(SPRITE_TWEAKER_C_SNES)?,
            tweaker_d: read_table(SPRITE_TWEAKER_D_SNES)?,
            tweaker_e: read_table(SPRITE_TWEAKER_E_SNES)?,
            tweaker_f: read_table(SPRITE_TWEAKER_F_SNES)?,
        })
    }

    /// Whether `sprite_id` has tweaker bytes at all (IDs `>= SPRITE_TWEAKER_COUNT`
    /// are cluster/extended/generator sprites).
    pub fn has_tweakers(sprite_id: u8) -> bool {
        (sprite_id as usize) < SPRITE_TWEAKER_COUNT
    }
}

macro_rules! bit_accessor {
    ($get:ident, $set:ident, $table:ident, $bit:expr) => {
        pub fn $get(&self, sprite_id: u8) -> bool {
            self.$table.get(sprite_id as usize).is_some_and(|b| b & (1 << $bit) != 0)
        }

        pub fn $set(&mut self, sprite_id: u8, value: bool) {
            if let Some(b) = self.$table.get_mut(sprite_id as usize) {
                if value {
                    *b |= 1 << $bit;
                } else {
                    *b &= !(1 << $bit);
                }
            }
        }
    };
}

impl SpriteTweakers {
    // Tweaker A ($1656): sSjJcccc
    bit_accessor!(smoke_on_death, set_smoke_on_death, tweaker_a, 7);

    bit_accessor!(hop_in_kick_shells, set_hop_in_kick_shells, tweaker_a, 6);

    bit_accessor!(dies_when_jumped_on, set_dies_when_jumped_on, tweaker_a, 5);

    bit_accessor!(can_be_jumped_on, set_can_be_jumped_on, tweaker_a, 4);

    // Tweaker B ($1662): dscccccc
    bit_accessor!(falls_when_killed, set_falls_when_killed, tweaker_b, 7);

    bit_accessor!(use_shell_death_frame, set_use_shell_death_frame, tweaker_b, 6);

    // Tweaker C ($166E): lwcfpppg
    bit_accessor!(no_layer2_interaction, set_no_layer2_interaction, tweaker_c, 7);

    bit_accessor!(disable_water_splash, set_disable_water_splash, tweaker_c, 6);

    bit_accessor!(disable_cape_killing, set_disable_cape_killing, tweaker_c, 5);

    bit_accessor!(disable_fireball_killing, set_disable_fireball_killing, tweaker_c, 4);

    bit_accessor!(use_second_gfx_page, set_use_second_gfx_page, tweaker_c, 0);

    // Tweaker D ($167A): dpmksPiS
    bit_accessor!(no_default_interaction, set_no_default_interaction, tweaker_d, 7);

    bit_accessor!(powerup_from_yoshi, set_powerup_from_yoshi, tweaker_d, 6);

    bit_accessor!(process_interaction_every_frame, set_process_interaction_every_frame, tweaker_d, 5);

    bit_accessor!(cant_be_kicked, set_cant_be_kicked, tweaker_d, 4);

    bit_accessor!(no_shell_when_stunned, set_no_shell_when_stunned, tweaker_d, 3);

    bit_accessor!(process_offscreen, set_process_offscreen, tweaker_d, 2);

    bit_accessor!(invincible_to_star, set_invincible_to_star, tweaker_d, 1);

    bit_accessor!(no_clipping_change_on_star_kill, set_no_clipping_change_on_star_kill, tweaker_d, 0);

    // Tweaker E ($1686): dnctswye
    bit_accessor!(no_object_interaction, set_no_object_interaction, tweaker_e, 7);

    bit_accessor!(spawns_new_sprite, set_spawns_new_sprite, tweaker_e, 6);

    bit_accessor!(no_coin_on_goal, set_no_coin_on_goal, tweaker_e, 5);

    bit_accessor!(no_direction_change_on_touch, set_no_direction_change_on_touch, tweaker_e, 4);

    bit_accessor!(no_sprite_interaction, set_no_sprite_interaction, tweaker_e, 3);

    bit_accessor!(weird_ground_behavior, set_weird_ground_behavior, tweaker_e, 2);

    bit_accessor!(stay_in_yoshi_mouth, set_stay_in_yoshi_mouth, tweaker_e, 1);

    bit_accessor!(inedible, set_inedible, tweaker_e, 0);

    // Tweaker F ($190F): two-tiles-high flag (bit 0 per community docs/usage).
    bit_accessor!(two_tiles_high, set_two_tiles_high, tweaker_f, 0);

    pub fn object_clipping(&self, sprite_id: u8) -> u8 {
        self.tweaker_a.get(sprite_id as usize).copied().unwrap_or(0) & 0x0F
    }

    pub fn set_object_clipping(&mut self, sprite_id: u8, value: u8) {
        if let Some(b) = self.tweaker_a.get_mut(sprite_id as usize) {
            *b = (*b & 0xF0) | (value & 0x0F);
        }
    }

    pub fn sprite_clipping(&self, sprite_id: u8) -> u8 {
        self.tweaker_b.get(sprite_id as usize).copied().unwrap_or(0) & 0x3F
    }

    pub fn set_sprite_clipping(&mut self, sprite_id: u8, value: u8) {
        if let Some(b) = self.tweaker_b.get_mut(sprite_id as usize) {
            *b = (*b & 0xC0) | (value & 0x3F);
        }
    }

    pub fn palette(&self, sprite_id: u8) -> u8 {
        (self.tweaker_c.get(sprite_id as usize).copied().unwrap_or(0) >> 1) & 0x07
    }

    pub fn set_palette(&mut self, sprite_id: u8, value: u8) {
        if let Some(b) = self.tweaker_c.get_mut(sprite_id as usize) {
            *b = (*b & !0x0E) | ((value & 0x07) << 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_tweakers() -> SpriteTweakers {
        SpriteTweakers {
            tweaker_a: vec![0u8; SPRITE_TWEAKER_COUNT],
            tweaker_b: vec![0u8; SPRITE_TWEAKER_COUNT],
            tweaker_c: vec![0u8; SPRITE_TWEAKER_COUNT],
            tweaker_d: vec![0u8; SPRITE_TWEAKER_COUNT],
            tweaker_e: vec![0u8; SPRITE_TWEAKER_COUNT],
            tweaker_f: vec![0u8; SPRITE_TWEAKER_COUNT],
        }
    }

    #[test]
    fn bit_flag_round_trips() {
        let mut t = empty_tweakers();
        assert!(!t.can_be_jumped_on(5));
        t.set_can_be_jumped_on(5, true);
        assert!(t.can_be_jumped_on(5));
        // Doesn't affect neighboring sprite IDs or other bits in the same byte.
        assert!(!t.can_be_jumped_on(4));
        assert!(!t.dies_when_jumped_on(5));
        t.set_can_be_jumped_on(5, false);
        assert!(!t.can_be_jumped_on(5));
    }

    #[test]
    fn multi_bit_field_round_trips() {
        let mut t = empty_tweakers();
        t.set_object_clipping(10, 0x0F);
        assert_eq!(t.object_clipping(10), 0x0F);
        // Doesn't bleed into the adjacent flag bits in the same byte.
        assert!(!t.can_be_jumped_on(10));
        t.set_object_clipping(10, 0x03);
        assert_eq!(t.object_clipping(10), 0x03);
    }

    #[test]
    fn out_of_range_sprite_id_is_noop_not_panic() {
        let mut t = SpriteTweakers {
            tweaker_a: vec![0u8; 5],
            tweaker_b: vec![0u8; 5],
            tweaker_c: vec![0u8; 5],
            tweaker_d: vec![0u8; 5],
            tweaker_e: vec![0u8; 5],
            tweaker_f: vec![0u8; 5],
        };
        assert!(!t.can_be_jumped_on(200));
        t.set_can_be_jumped_on(200, true); // must not panic
        assert!(!SpriteTweakers::has_tweakers(250));
        assert!(SpriteTweakers::has_tweakers(5));
    }
}
