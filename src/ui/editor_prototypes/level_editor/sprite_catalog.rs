pub(super) fn sprite_name(id: u8) -> &'static str {
    match id {
        0x00 => "Green Koopa, no shell",
        0x01 => "Red Koopa, no shell",
        0x02 => "Blue Koopa, no shell",
        0x03 => "Yellow Koopa, no shell",
        0x04 => "Green Koopa",
        0x05 => "Red Koopa",
        0x06 => "Blue Koopa",
        0x07 => "Yellow Koopa",
        0x08 => "Green Koopa, flying left",
        0x09 => "Green bouncing Koopa",
        0x0A => "Red vertical flying Koopa",
        0x0B => "Red horizontal flying Koopa",
        0x0C => "Yellow Koopa with wings",
        0x0D => "Bob-omb",
        0x0E => "Keyhole",
        0x0F => "Goomba",
        0x10 => "Bouncing Goomba with wings",
        0x11 => "Buzzy Beetle",
        0x12 => "Unused",
        0x13 => "Spiny",
        0x14 => "Spiny falling",
        0x15 => "Fish, horizontal",
        0x16 => "Fish, vertical",
        0x17 => "Fish, created from generator",
        0x18 => "Surface jumping fish",
        0x19 => "Message Box #1 text",
        0x1A => "Classic Piranha Plant",
        0x1B => "Bouncing football in place",
        0x1C => "Bullet Bill",
        0x1D => "Hopping flame",
        0x1E => "Lakitu",
        0x1F => "Magikoopa",
        0x20 => "Magikoopa's magic",
        0x21 => "Moving coin",
        0x22 => "Green vertical net Koopa",
        0x23 => "Red vertical net Koopa",
        0x24 => "Green horizontal net Koopa",
        0x25 => "Red horizontal net Koopa",
        0x26 => "Thwomp",
        0x27 => "Thwimp",
        0x28 => "Big Boo",
        0x29 => "Koopa Kid",
        0x2A => "Upside down Piranha Plant",
        0x2B => "Sumo Brother's fire lightning",
        0x2C => "Yoshi egg",
        0x2D => "Baby green Yoshi",
        0x2E => "Spike Top",
        0x2F => "Portable spring board",
        0x30 => "Dry Bones, throws bones",
        0x31 => "Bony Beetle",
        0x32 => "Dry Bones, stay on ledge",
        0x33 => "Fireball",
        0x34 => "Boss fireball",
        0x35 => "Green Yoshi",
        0x36 => "Unused",
        0x37 => "Boo",
        0x38 => "Eerie",
        0x39 => "Eerie, wave motion",
        0x3A => "Urchin, fixed",
        0x3B => "Urchin, wall detect",
        0x3C => "Urchin, wall follow",
        0x3D => "Rip Van Fish",
        0x3E => "POW",
        0x3F => "Para-Goomba",
        0x40 => "Para-Bomb",
        0x41 => "Dolphin, horizontal",
        0x42 => "Dolphin2, horizontal",
        0x43 => "Dolphin, vertical",
        0x44 => "Torpedo Ted",
        0x45 => "Directional coins",
        0x46 => "Diggin' Chuck",
        0x47 => "Swimming/Jumping fish",
        0x48 => "Diggin' Chuck's rock",
        0x49 => "Growing/shrinking pipe end",
        0x4A => "Goal Point Question Sphere",
        0x4B => "Pipe dwelling Lakitu",
        0x4C => "Exploding Block",
        0x4D => "Ground dwelling Monty Mole",
        0x4E => "Ledge dwelling Monty Mole",
        0x4F => "Jumping Piranha Plant",
        0x50 => "Jumping Piranha Plant, spit fire",
        0x51 => "Ninji",
        0x52 => "Moving ledge hole in ghost house",
        0x53 => "Throw block sprite",
        0x54 => "Climbing net door",
        0x55 => "Checkerboard platform, horizontal",
        0x56 => "Flying rock platform, horizontal",
        0x57 => "Checkerboard platform, vertical",
        0x58 => "Flying rock platform, vertical",
        0x59 => "Turn block bridge, horizontal and vertical",
        0x5A => "Turn block bridge, horizontal",
        0x5B => "Brown platform floating in water",
        0x5C => "Checkerboard platform that falls",
        0x5D => "Orange platform floating in water",
        0x5E => "Orange platform, goes on forever",
        0x5F => "Brown platform on a chain",
        0x60 => "Flat green switch palace switch",
        0x61 => "Floating skulls",
        0x62 => "Brown platform, line-guided",
        0x63 => "Checker/brown platform, line-guided",
        0x64 => "Rope mechanism, line-guided",
        0x65 => "Chainsaw, line-guided",
        0x66 => "Upside down chainsaw, line-guided",
        0x67 => "Grinder, line-guided",
        0x68 => "Fuzz ball, line-guided",
        0x69 => "Unused",
        0x6A => "Coin game cloud",
        0x6B => "Spring board, left wall",
        0x6C => "Spring board, right wall",
        0x6D => "Invisible solid block",
        0x6E => "Dino Rhino",
        0x6F => "Dino Torch",
        0x70 => "Pokey",
        0x71 => "Super Koopa, red cape",
        0x72 => "Super Koopa, yellow cape",
        0x73 => "Super Koopa, feather",
        0x74 => "Mushroom",
        0x75 => "Flower",
        0x76 => "Star",
        0x77 => "Feather",
        0x78 => "1-Up",
        0x79 => "Growing Vine",
        0x7A => "Firework",
        0x7B => "Goal Point",
        0x7C => "Princess Peach",
        0x7D => "Balloon",
        0x7E => "Flying Red coin",
        0x7F => "Flying yellow 1-Up",
        0x80 => "Key",
        0x81 => "Changing item from translucent block",
        0x82 => "Bonus game sprite",
        0x83 => "Left flying question block",
        0x84 => "Flying question block",
        0x85 => "Unused",
        0x86 => "Wiggler",
        0x87 => "Lakitu's cloud",
        0x88 => "Unused (Winged cage sprite)",
        0x89 => "Layer 3 smash",
        0x8A => "Bird from Yoshi's house",
        0x8B => "Puff of smoke from Yoshi's house",
        0x8C => "Fireplace smoke/side exit",
        0x8D => "Ghost house exit sign and door",
        0x8E => "Invisible Warp Hole blocks",
        0x8F => "Scale platforms",
        0x90 => "Large green gas bubble",
        0x91 => "Chargin' Chuck",
        0x92 => "Splittin' Chuck",
        0x93 => "Bouncin' Chuck",
        0x94 => "Whistlin' Chuck",
        0x95 => "Clapin' Chuck",
        0x96 => "Unused (Chargin' Chuck clone)",
        0x97 => "Puntin' Chuck",
        0x98 => "Pitchin' Chuck",
        0x99 => "Volcano Lotus",
        0x9A => "Sumo Brother",
        0x9B => "Hammer Brother",
        0x9C => "Flying blocks for Hammer Brother",
        0x9D => "Bubble with sprite",
        0x9E => "Ball and Chain",
        0x9F => "Banzai Bill",
        0xA0 => "Activates Bowser scene",
        0xA1 => "Bowser's bowling ball",
        0xA2 => "MechaKoopa",
        0xA3 => "Grey platform on chain",
        0xA4 => "Floating Spike ball",
        0xA5 => "Fuzzball/Sparky, ground-guided",
        0xA6 => "HotHead, ground-guided",
        0xA7 => "Iggy's ball",
        0xA8 => "Blargg",
        0xA9 => "Reznor",
        0xAA => "Fishbone",
        0xAB => "Rex",
        0xAC => "Wooden Spike, moving down and up",
        0xAD => "Wooden Spike, moving up/down first",
        0xAE => "Fishin' Boo",
        0xAF => "Boo Block",
        0xB0 => "Reflecting stream of Boo Buddies",
        0xB1 => "Creating/Eating block",
        0xB2 => "Falling Spike",
        0xB3 => "Bowser statue fireball",
        0xB4 => "Grinder, non-line-guided",
        0xB5 => "Sinking fireball used in boss battles",
        0xB6 => "Reflecting fireball",
        0xB7 => "Carrot Top lift, upper right",
        0xB8 => "Carrot Top lift, upper left",
        0xB9 => "Info Box",
        0xBA => "Timed lift",
        0xBB => "Grey moving castle block",
        0xBC => "Bowser statue",
        0xBD => "Sliding Koopa without a shell",
        0xBE => "Swooper bat",
        0xBF => "Mega Mole",
        0xC0 => "Grey platform on lava",
        0xC1 => "Flying grey turnblocks",
        0xC2 => "Blurp fish",
        0xC3 => "Porcu-Puffer fish",
        0xC4 => "Grey platform that falls",
        0xC5 => "Big Boo Boss",
        0xC6 => "Dark room with spot light",
        0xC7 => "Invisible mushroom",
        0xC8 => "Light switch block for dark room",
        0xFE => "Mario (spawn point)",
        _ => "Unknown / unsupported",
    }
}

pub(super) fn sprite_matches_search(id: u8, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }

    let hex = format!("{id:02X}").to_ascii_lowercase();
    let name = sprite_name(id);
    let q = query.to_ascii_lowercase();
    hex.contains(&q) || name.to_ascii_lowercase().contains(&q)
}

pub(super) fn preview_sprite_tileset(id: u8) -> Option<u8> {
    match id {
        // Forest sprite tileset ($00,$01,$13,$02): SP3=13 / SP4=02 family.
        0x0D | 0x13 | 0x14 | 0x1D | 0x1E | 0x2C | 0x3F | 0x40 | 0x49 | 0x4B => Some(0),
        // Castle sprite tileset ($00,$01,$12,$03): SP3=12 / SP4=03 family.
        0x1F | 0x20 | 0x22 | 0x23 | 0x24 | 0x25 | 0x26 | 0x27 | 0x30 | 0x31 | 0x32 | 0x54 | 0xAC | 0xAD | 0xB2
        | 0xB3 | 0xB4 | 0xB6 | 0xBB | 0xBC => Some(1),
        // Mushroom sprite tileset ($00,$01,$13,$05): SP4=05 family.
        0x46 | 0x48 | 0x4A | 0x55 | 0x56 | 0x57 | 0x58 | 0x5C | 0x5D | 0x5E | 0x5F | 0x64 | 0xC0 => Some(2),
        // Underground sprite tileset ($00,$01,$13,$04): SP4=04 family.
        0x11 | 0x1B | 0x2A | 0x2E | 0x4F | 0x50 | 0x61 | 0xBE => Some(3),
        // Water sprite tileset ($00,$01,$13,$06): SP4=06 family.
        0x3A | 0x3B | 0x3C | 0x3D | 0x41 | 0x42 | 0x43 | 0x44 | 0xC2 | 0xC3 => Some(4),
        // Pokey sprite tileset ($00,$01,$13,$09): SP4=09 family.
        0x2B | 0x4D | 0x4E | 0x91 | 0x92 | 0x93 | 0x94 | 0x95 | 0x96 | 0x97 | 0x98 | 0x99 | 0x9A => Some(5),
        // Ghost House sprite tileset ($00,$01,$06,$11): SP4=11 family.
        0x28 | 0x37 | 0x38 | 0x39 | 0x52 | 0x8D | 0xAE | 0xAF | 0xB0 | 0xC5 => Some(7),
        // Banzai Bill sprite tileset ($00,$01,$13,$20): SP4=20 family.
        0x9F | 0xAB | 0xB7 | 0xB8 | 0xBA | 0xBF => Some(8),
        // Switch Palace sprite tileset ($00,$01,$0D,$14): SP3=0D family.
        0x60 => Some(11),
        // Wendy/Lemmy sprite tileset ($00,$01,$0A,$22): SP3=0A family.
        0x29 => Some(13),
        // Ninji sprite tileset ($00,$01,$13,$0E): SP4=0E family.
        0x51 | 0xC6 => Some(14),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{preview_sprite_tileset, sprite_matches_search, sprite_name};

    #[test]
    fn names_cover_common_sprites() {
        assert_eq!(sprite_name(0x0F), "Goomba");
        assert_eq!(sprite_name(0xAB), "Rex");
        assert_eq!(sprite_name(0xA2), "MechaKoopa");
    }

    #[test]
    fn search_matches_name_and_hex() {
        assert!(sprite_matches_search(0xAB, "rex"));
        assert!(sprite_matches_search(0xAB, "ab"));
        assert!(!sprite_matches_search(0xAB, "goomba"));
    }

    #[test]
    fn preview_tileset_covers_problem_ids() {
        assert_eq!(preview_sprite_tileset(0x0D), Some(0));
        assert_eq!(preview_sprite_tileset(0x20), Some(1));
        assert_eq!(preview_sprite_tileset(0x22), Some(1));
        assert_eq!(preview_sprite_tileset(0x26), Some(1));
        assert_eq!(preview_sprite_tileset(0x3F), Some(0));
        assert_eq!(preview_sprite_tileset(0x37), Some(7));
        assert_eq!(preview_sprite_tileset(0x41), Some(4));
        assert_eq!(preview_sprite_tileset(0x46), Some(2));
        assert_eq!(preview_sprite_tileset(0xBE), Some(3));
        assert_eq!(preview_sprite_tileset(0xC2), Some(4));
        assert_eq!(preview_sprite_tileset(0xC5), Some(7));
        assert_eq!(preview_sprite_tileset(0x60), Some(11));
        assert_eq!(preview_sprite_tileset(0x29), Some(13));
        assert_eq!(preview_sprite_tileset(0x51), Some(14));
        assert_eq!(preview_sprite_tileset(0x0F), None);
    }
}
