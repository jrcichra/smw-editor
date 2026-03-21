#![allow(clippy::identity_op)]
#![allow(dead_code)]

use smwe_rom::{level::Level, objects::Object};

use crate::undo::Undo;

const SCREEN_WIDTH: u32 = 16;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct EditableObject {
    pub x: u32,
    pub y: u32,
    pub id: u8,
    pub settings: u8,
    pub is_extended: bool,
    pub extended_id: u8,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct EditableExit {
    pub screen: u8,
    pub midway: bool,
    pub secondary: bool,
    pub id: u16,
}

#[derive(Clone, Debug, Default)]
pub(super) struct EditableObjectLayer {
    pub objects: Vec<EditableObject>,
    pub exits: Vec<EditableExit>,
}

impl EditableObject {
    pub fn from_raw(object: Object, current_screen: u32, vertical_level: bool) -> Option<Self> {
        if object.is_exit() || object.is_screen_jump() {
            return None;
        }

        let (local_x, local_y) = if vertical_level {
            (object.y() as u32, object.x() as u32)
        } else {
            (object.x() as u32, object.y() as u32)
        };

        let abs_x = local_x + if vertical_level { 0 } else { current_screen * SCREEN_WIDTH };
        let abs_y = local_y + if vertical_level { current_screen * SCREEN_WIDTH } else { 0 };

        Some(EditableObject {
            x: abs_x,
            y: abs_y,
            id: object.standard_object_number(),
            settings: object.settings(),
            is_extended: object.is_extended(),
            extended_id: object.settings(),
        })
    }

    pub fn to_raw(self, new_screen: bool) -> Object {
        if self.is_extended {
            Object(
                ((new_screen as u32) << 31)
                    | ((self.y & 0x1F) << 24)
                    | ((self.x & 0x0F) << 16)
                    | (self.extended_id as u32),
            )
        } else {
            Object(
                ((new_screen as u32) << 31)
                    | ((self.id as u32 & 0x30) << 30)
                    | ((self.id as u32 & 0x0F) << 20)
                    | ((self.y & 0x1F) << 24)
                    | ((self.x & 0x0F) << 16)
                    | (self.settings as u32),
            )
        }
    }
}

impl EditableExit {
    pub fn from_raw(object: Object) -> Option<Self> {
        object.is_exit().then(|| EditableExit {
            screen: object.screen_number(),
            midway: object.is_midway(),
            secondary: object.is_secondary_exit(),
            id: object.exit_id(),
        })
    }

    pub fn to_raw(self) -> Object {
        Object(
            ((self.screen as u32 & 0x1F) << 24)
                | ((self.midway as u32) << 19)
                | ((self.secondary as u32) << 17)
                | (self.id as u32 & 0x01FF),
        )
    }
}

impl EditableObjectLayer {
    pub fn from_level(level: &Level) -> Self {
        let is_vertical = level.secondary_header.vertical_level();
        let raw_bytes = level.layer1.as_bytes();
        let raw_objects = match Object::parse_from_layer(raw_bytes) {
            Some(objs) => objs,
            None => return Self::default(),
        };
        let mut layer = Self::default();
        let mut current_screen: u8 = 0;
        for raw_object in raw_objects {
            if raw_object.is_exit() {
                if let Some(exit) = EditableExit::from_raw(raw_object) {
                    layer.exits.push(exit);
                }
            } else if raw_object.is_screen_jump() {
                current_screen = raw_object.screen_number();
            } else {
                if raw_object.is_new_screen() {
                    current_screen = current_screen.saturating_add(1);
                }
                if let Some(obj) = EditableObject::from_raw(raw_object, current_screen as u32, is_vertical) {
                    layer.objects.push(obj);
                }
            }
        }
        layer
    }
}

// ── Undo support ────────────────────────────────────────────────────────────

impl Undo for EditableObjectLayer {
    fn from_bytes(bytes: Vec<u8>) -> Self {
        if bytes.len() < 4 {
            return Self::default();
        }
        let num_objects = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
        let num_exits = u16::from_le_bytes([bytes[2], bytes[3]]) as usize;

        let mut objects = Vec::with_capacity(num_objects);
        let mut offset = 4;
        for _ in 0..num_objects {
            if offset + 12 > bytes.len() {
                break;
            }
            let x = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            let y = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
            let id = bytes[offset + 8];
            let settings = bytes[offset + 9];
            let is_extended = bytes[offset + 10] != 0;
            let extended_id = bytes[offset + 11];
            objects.push(EditableObject { x, y, id, settings, is_extended, extended_id });
            offset += 12;
        }

        let mut exits = Vec::with_capacity(num_exits);
        for _ in 0..num_exits {
            if offset + 5 > bytes.len() {
                break;
            }
            let screen = bytes[offset];
            let midway = bytes[offset + 1] != 0;
            let secondary = bytes[offset + 2] != 0;
            let id = u16::from_le_bytes([bytes[offset + 3], bytes[offset + 4]]);
            exits.push(EditableExit { screen, midway, secondary, id });
            offset += 5;
        }

        Self { objects, exits }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4 + self.objects.len() * 12 + self.exits.len() * 5);
        bytes.extend_from_slice(&(self.objects.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&(self.exits.len() as u16).to_le_bytes());
        for obj in &self.objects {
            bytes.extend_from_slice(&obj.x.to_le_bytes());
            bytes.extend_from_slice(&obj.y.to_le_bytes());
            bytes.push(obj.id);
            bytes.push(obj.settings);
            bytes.push(obj.is_extended as u8);
            bytes.push(obj.extended_id);
        }
        for exit in &self.exits {
            bytes.push(exit.screen);
            bytes.push(exit.midway as u8);
            bytes.push(exit.secondary as u8);
            bytes.extend_from_slice(&exit.id.to_le_bytes());
        }
        bytes
    }

    fn size_bytes(&self) -> usize {
        4 + self.objects.len() * 12 + self.exits.len() * 5
    }
}
