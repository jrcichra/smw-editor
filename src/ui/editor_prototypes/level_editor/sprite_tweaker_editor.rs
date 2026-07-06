use egui::{Context, Slider};

use super::UiLevelEditor;

/// Editor for sprite tweaker bytes: global, per-sprite-ID behavior flags
/// (Lunar Magic's "Sprite Header Editor"). Editing here affects every
/// placement of the chosen sprite ID across the whole ROM, not just this level.
impl UiLevelEditor {
    pub(super) fn sprite_tweaker_editor_window(&mut self, ctx: &Context) {
        if !self.show_sprite_tweaker_editor {
            return;
        }
        let mut open = self.show_sprite_tweaker_editor;
        egui::Window::new("Sprite Behavior (global)").open(&mut open).resizable(true).default_size([360.0, 480.0]).show(
            ctx,
            |ui| {
                ui.label("Edits here are global: they affect every placement of this sprite ID across the ROM.");
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Sprite ID:");
                    let mut id = self.tweaker_editor_sprite_id as i32;
                    if ui.add(Slider::new(&mut id, 0..=0xFF).hexadecimal(2, false, false)).changed() {
                        self.tweaker_editor_sprite_id = id as u8;
                    }
                    ui.label(super::sprite_catalog::sprite_name(self.tweaker_editor_sprite_id));
                });

                let id = self.tweaker_editor_sprite_id;
                if !smwe_rom::sprite_tweakers::SpriteTweakers::has_tweakers(id) {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 160, 60),
                        "This sprite ID is a cluster/extended/generator sprite; it doesn't use the standard tweaker bytes.",
                    );
                    return;
                }

                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let t = &mut self.sprite_tweakers;
                    let mut changed = false;

                    ui.label("Tweaker A ($1656):");
                    changed |= checkbox_field(ui, "Smoke on death", t.smoke_on_death(id), |v| t.set_smoke_on_death(id, v));
                    changed |=
                        checkbox_field(ui, "Hop in/kick shells", t.hop_in_kick_shells(id), |v| t.set_hop_in_kick_shells(id, v));
                    changed |=
                        checkbox_field(ui, "Dies when jumped on", t.dies_when_jumped_on(id), |v| t.set_dies_when_jumped_on(id, v));
                    changed |= checkbox_field(ui, "Can be jumped on", t.can_be_jumped_on(id), |v| t.set_can_be_jumped_on(id, v));
                    changed |= slider_field(ui, "Object clipping", t.object_clipping(id), 0..=0x0F, |v| {
                        t.set_object_clipping(id, v)
                    });

                    ui.separator();
                    ui.label("Tweaker B ($1662):");
                    changed |=
                        checkbox_field(ui, "Falls straight down when killed", t.falls_when_killed(id), |v| {
                            t.set_falls_when_killed(id, v)
                        });
                    changed |= checkbox_field(ui, "Use shell as death frame", t.use_shell_death_frame(id), |v| {
                        t.set_use_shell_death_frame(id, v)
                    });
                    changed |= slider_field(ui, "Sprite clipping", t.sprite_clipping(id), 0..=0x3F, |v| {
                        t.set_sprite_clipping(id, v)
                    });

                    ui.separator();
                    ui.label("Tweaker C ($166E):");
                    changed |= checkbox_field(ui, "Don't interact with layer 2/3 tides", t.no_layer2_interaction(id), |v| {
                        t.set_no_layer2_interaction(id, v)
                    });
                    changed |= checkbox_field(ui, "Disable water splash", t.disable_water_splash(id), |v| {
                        t.set_disable_water_splash(id, v)
                    });
                    changed |= checkbox_field(ui, "Disable cape killing", t.disable_cape_killing(id), |v| {
                        t.set_disable_cape_killing(id, v)
                    });
                    changed |= checkbox_field(ui, "Disable fireball killing", t.disable_fireball_killing(id), |v| {
                        t.set_disable_fireball_killing(id, v)
                    });
                    changed |= slider_field(ui, "Palette", t.palette(id), 0..=7, |v| t.set_palette(id, v));
                    changed |= checkbox_field(ui, "Use second graphics page", t.use_second_gfx_page(id), |v| {
                        t.set_use_second_gfx_page(id, v)
                    });

                    ui.separator();
                    ui.label("Tweaker D ($167A):");
                    changed |= checkbox_field(ui, "Don't use default player interaction", t.no_default_interaction(id), |v| {
                        t.set_no_default_interaction(id, v)
                    });
                    changed |= checkbox_field(ui, "Gives powerup when eaten by Yoshi", t.powerup_from_yoshi(id), |v| {
                        t.set_powerup_from_yoshi(id, v)
                    });
                    changed |= checkbox_field(
                        ui,
                        "Process interaction every frame",
                        t.process_interaction_every_frame(id),
                        |v| t.set_process_interaction_every_frame(id, v),
                    );
                    changed |= checkbox_field(ui, "Can't be kicked like a shell", t.cant_be_kicked(id), |v| {
                        t.set_cant_be_kicked(id, v)
                    });
                    changed |= checkbox_field(ui, "Don't turn into shell when stunned", t.no_shell_when_stunned(id), |v| {
                        t.set_no_shell_when_stunned(id, v)
                    });
                    changed |= checkbox_field(ui, "Process while off screen", t.process_offscreen(id), |v| {
                        t.set_process_offscreen(id, v)
                    });
                    changed |= checkbox_field(
                        ui,
                        "Invincible to star/cape/fire/bouncing bricks",
                        t.invincible_to_star(id),
                        |v| t.set_invincible_to_star(id, v),
                    );
                    changed |= checkbox_field(
                        ui,
                        "Don't disable clipping when killed with star",
                        t.no_clipping_change_on_star_kill(id),
                        |v| t.set_no_clipping_change_on_star_kill(id, v),
                    );

                    ui.separator();
                    ui.label("Tweaker E ($1686):");
                    changed |= checkbox_field(ui, "Don't interact with objects", t.no_object_interaction(id), |v| {
                        t.set_no_object_interaction(id, v)
                    });
                    changed |= checkbox_field(ui, "Spawns a new sprite", t.spawns_new_sprite(id), |v| {
                        t.set_spawns_new_sprite(id, v)
                    });
                    changed |= checkbox_field(ui, "Don't turn into coin when goal passed", t.no_coin_on_goal(id), |v| {
                        t.set_no_coin_on_goal(id, v)
                    });
                    changed |= checkbox_field(
                        ui,
                        "Don't change direction if touched",
                        t.no_direction_change_on_touch(id),
                        |v| t.set_no_direction_change_on_touch(id, v),
                    );
                    changed |= checkbox_field(ui, "Don't interact with other sprites", t.no_sprite_interaction(id), |v| {
                        t.set_no_sprite_interaction(id, v)
                    });
                    changed |= checkbox_field(ui, "Weird ground behavior", t.weird_ground_behavior(id), |v| {
                        t.set_weird_ground_behavior(id, v)
                    });
                    changed |= checkbox_field(ui, "Stay in Yoshi's mouth", t.stay_in_yoshi_mouth(id), |v| {
                        t.set_stay_in_yoshi_mouth(id, v)
                    });
                    changed |= checkbox_field(ui, "Inedible", t.inedible(id), |v| t.set_inedible(id, v));

                    ui.separator();
                    ui.label("Tweaker F ($190F):");
                    changed |=
                        checkbox_field(ui, "Two tiles high", t.two_tiles_high(id), |v| t.set_two_tiles_high(id, v));

                    if changed {
                        self.sprite_tweakers_dirty = true;
                        self.has_edits = true;
                    }
                });
            },
        );
        self.show_sprite_tweaker_editor = open;
    }
}

fn checkbox_field(ui: &mut egui::Ui, label: &str, mut value: bool, mut apply: impl FnMut(bool)) -> bool {
    if ui.checkbox(&mut value, label).changed() {
        apply(value);
        true
    } else {
        false
    }
}

fn slider_field(
    ui: &mut egui::Ui, label: &str, value: u8, range: std::ops::RangeInclusive<u8>, mut apply: impl FnMut(u8),
) -> bool {
    let mut v = value as i32;
    let changed = ui
        .horizontal(|ui| {
            ui.label(label);
            ui.add(Slider::new(&mut v, *range.start() as i32..=*range.end() as i32).hexadecimal(1, false, false))
                .changed()
        })
        .inner;
    if changed {
        apply(v as u8);
    }
    changed
}
