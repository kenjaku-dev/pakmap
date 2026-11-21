//! ELF/AppImage layout checks — source-of-truth for what we accept.
//!
//! Type 2 (the only supported format): ELF64 little-endian, magic bytes
//! `41 49 02` at offset 8, and `e_shoff` (offset 0x28) repurposed to hold the
//! byte offset of the appended squashfs image. We verify the `hsqs` superblock
//! at that offset before any external tool sees the file.

/// The byte range of the appended squashfs filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppImageLayout {
    pub fs_offset: u64,
}

/// Validate the on-disk layout without executing anything.
pub fn inspect(bytes: &[u8]) -> Result<AppImageLayout, &'static str> {
    if bytes.len() < 64 {
        return Err("truncated ELF header");
    }
    if bytes[0..4] != [0x7f, b'E', b'L', b'F'] {
        return Err("not an ELF file");
    }
    if bytes[4] != 2 {
        return Err("only 64-bit AppImages are supported");
    }
    if bytes[5] != 1 {
        return Err("only little-endian AppImages are supported");
    }
    match bytes.get(8..11) {
        Some([0x41, 0x49, 0x02]) => {}
        Some([0x41, 0x49, 0x01]) => return Err("type 1 AppImages are not supported"),
        _ => return Err("not a type 2 AppImage"),
    }
    // AppImageKit repurposes e_shoff (ELF64 offset 0x28) for the byte offset
    // of the appended squashfs image.
    let slice: [u8; 8] = bytes[0x28..0x30]
        .try_into()
        .map_err(|_| "truncated ELF header")?;
    let offset = u64::from_le_bytes(slice);
    if offset < 64 {
        return Err("squashfs offset lies inside the ELF header");
    }
    let start = usize::try_from(offset).map_err(|_| "squashfs offset is too large")?;
    let end = start.checked_add(4).ok_or("squashfs offset overflow")?;
    if bytes.get(start..end) != Some(b"hsqs".as_slice()) {
        return Err("no squashfs superblock at the declared offset");
    }
    Ok(AppImageLayout { fs_offset: offset })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::inspect;

    /// Build a minimal, valid type-2 header (+ squashfs marker) for tests.
    fn elf64(magic: [u8; 3], class: u8, data: u8, offset: u64) -> Vec<u8> {
        let mut bytes = vec![0u8; 128];
        bytes[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        bytes[4] = class;
        bytes[5] = data;
        bytes[8..11].copy_from_slice(&magic);
        bytes[0x28..0x30].copy_from_slice(&offset.to_le_bytes());
        // "hsqs" is the little-endian squashfs superblock magic.
        bytes[offset as usize..offset as usize + 4].copy_from_slice(b"hsqs");
        bytes
    }

    #[test]
    fn accepts_a_type2_elf64_with_squashfs_at_the_declared_offset() {
        let bytes = elf64([0x41, 0x49, 0x02], 2, 1, 64);
        let layout = inspect(&bytes).expect("valid type-2 layout");
        assert_eq!(layout.fs_offset, 64);
    }

    #[test]
    fn rejects_empty_and_non_elf_input() {
        assert!(inspect(&[]).is_err());
        let mut bytes = elf64([0x41, 0x49, 0x02], 2, 1, 64);
        bytes[1] = b'X';
        assert!(inspect(&bytes).is_err(), "not an ELF file");
    }

    #[test]
    fn rejects_type1_and_unknown_magic() {
        let t1 = elf64([0x41, 0x49, 0x01], 2, 1, 64);
        assert!(inspect(&t1).is_err(), "type 1 must be refused, not guessed");
        let other = elf64([0x41, 0x49, 0x03], 2, 1, 64);
        assert!(inspect(&other).is_err());
    }

    #[test]
    fn rejects_32bit_and_big_endian() {
        let bit32 = elf64([0x41, 0x49, 0x02], 1, 1, 64);
        assert!(inspect(&bit32).is_err());
        let be = elf64([0x41, 0x49, 0x02], 2, 2, 64);
        assert!(inspect(&be).is_err());
    }

    #[test]
    fn rejects_offsets_that_escape_the_file_or_have_no_squashfs_superblock() {
        let mut beyond = elf64([0x41, 0x49, 0x02], 2, 1, 64);
        beyond[0x28..0x30].copy_from_slice(&999u64.to_le_bytes());
        assert!(inspect(&beyond).is_err(), "offset past EOF");

        let mut no_magic = elf64([0x41, 0x49, 0x02], 2, 1, 64);
        no_magic[64..68].copy_from_slice(b"nope");
        assert!(inspect(&no_magic).is_err(), "no hsqs superblock");

        let mut tiny = elf64([0x41, 0x49, 0x02], 2, 1, 64);
        tiny[0x28..0x30].copy_from_slice(&8u64.to_le_bytes());
        assert!(inspect(&tiny).is_err(), "offset inside the ELF header");
    }
}
