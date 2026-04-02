use crate::undo::Undo;

#[derive(Clone, Debug, Default)]
pub(super) struct EditableBackgroundLayer {
    pub tile_ids: Vec<u8>,
}

impl EditableBackgroundLayer {
    pub fn new(tile_ids: Vec<u8>) -> Self {
        Self { tile_ids }
    }
}

impl Undo for EditableBackgroundLayer {
    fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { tile_ids: bytes }
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.tile_ids.clone()
    }

    fn size_bytes(&self) -> usize {
        self.tile_ids.len()
    }
}
