//! Reading the UF2 container format, enough to tell real firmware from a
//! wrong file before it is copied to a keyboard.

const BLOCK: usize = 512;
const MAGIC_START_0: u32 = 0x0A32_4655;
const MAGIC_START_1: u32 = 0x9E5D_5157;
const MAGIC_END: u32 = 0x0AB1_6F30;
/// Set when the block's size field holds a family ID.
const FLAG_FAMILY: u32 = 0x0000_2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uf2Info {
    pub blocks: usize,
    /// The distinct family IDs in the file, in order of first appearance.
    /// A combined left-and-right file has two.
    pub families: Vec<u32>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Uf2Error {
    #[error("the file is empty")]
    Empty,
    #[error("this is not a UF2 firmware file")]
    NotUf2,
}

fn word(block: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        block[offset],
        block[offset + 1],
        block[offset + 2],
        block[offset + 3],
    ])
}

pub fn inspect(firmware: &[u8]) -> Result<Uf2Info, Uf2Error> {
    if firmware.is_empty() {
        return Err(Uf2Error::Empty);
    }
    if !firmware.len().is_multiple_of(BLOCK) {
        return Err(Uf2Error::NotUf2);
    }
    let mut families = Vec::new();
    for block in firmware.chunks(BLOCK) {
        let magic = (word(block, 0), word(block, 4), word(block, BLOCK - 4));
        if magic != (MAGIC_START_0, MAGIC_START_1, MAGIC_END) {
            return Err(Uf2Error::NotUf2);
        }
        let family = word(block, 28);
        if word(block, 8) & FLAG_FAMILY != 0 && !families.contains(&family) {
            families.push(family);
        }
    }
    Ok(Uf2Info {
        blocks: firmware.len() / BLOCK,
        families,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal UF2 image with two blocks for each family.
    pub(crate) fn image(families: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        for family in families {
            for _ in 0..2 {
                let mut block = vec![0u8; BLOCK];
                block[0..4].copy_from_slice(&MAGIC_START_0.to_le_bytes());
                block[4..8].copy_from_slice(&MAGIC_START_1.to_le_bytes());
                block[8..12].copy_from_slice(&FLAG_FAMILY.to_le_bytes());
                block[28..32].copy_from_slice(&family.to_le_bytes());
                block[BLOCK - 4..].copy_from_slice(&MAGIC_END.to_le_bytes());
                out.extend(block);
            }
        }
        out
    }

    #[test]
    fn families_and_block_counts_are_read() {
        let info = inspect(&image(&[0x9809_B007, 0x980A_B007])).unwrap();
        assert_eq!(info.blocks, 4);
        assert_eq!(info.families, [0x9809_B007, 0x980A_B007]);
    }

    #[test]
    fn other_files_are_rejected() {
        assert_eq!(inspect(&[]), Err(Uf2Error::Empty));
        assert_eq!(inspect(b"hello"), Err(Uf2Error::NotUf2));
        assert_eq!(inspect(&[0u8; 512]), Err(Uf2Error::NotUf2));
        let mut damaged = image(&[1]);
        damaged[1020] = 0;
        assert_eq!(inspect(&damaged), Err(Uf2Error::NotUf2));
    }
}
