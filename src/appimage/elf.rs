//! ELF/AppImage layout checks — source-of-truth for what we accept.
//!
//! Type 2 (the only supported format): ELF64 little-endian, magic bytes
//! `41 49 02` at offset 8, and an appended squashfs image. The offset is NOT
//! `e_shoff` itself: AppImageKit computes the end of the ELF as
//! `max(e_shoff + e_shnum * e_shentsize, last_section.sh_offset + sh_size)`
//! (see libappimage `read_elf64`). We reproduce that formula exactly and
//! verify the `hsqs` superblock before any external tool sees the file.

use std::io::{Read, Seek, SeekFrom};

/// The byte range of the appended squashfs filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppImageLayout {
    pub fs_offset: u64,
}

const ELF_HEADER_LEN: usize = 64;
const SHDR_LEN: u64 = 64;
const SHDR_OFFSET_FIELD: u64 = 0x18;
const SHDR_SIZE_FIELD: u64 = 0x20;

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn read_u64(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

/// Validate the on-disk layout without executing anything. `file_len` must be
/// the real length of `reader`; the caller has already refused non-regular
/// files, so the length cannot change underneath us in a way that matters.
pub fn inspect<R: Read + Seek>(
    reader: &mut R,
    file_len: u64,
) -> Result<AppImageLayout, &'static str> {
    if file_len < ELF_HEADER_LEN as u64 {
        return Err("truncated ELF header");
    }
    let mut header = [0u8; ELF_HEADER_LEN];
    reader
        .read_exact(&mut header)
        .map_err(|_| "cannot read ELF header")?;
    if header[0..4] != [0x7f, b'E', b'L', b'F'] {
        return Err("not an ELF file");
    }
    if header[4] != 2 {
        return Err("only 64-bit AppImages are supported");
    }
    if header[5] != 1 {
        return Err("only little-endian AppImages are supported");
    }
    match header.get(8..11) {
        Some([0x41, 0x49, 0x02]) => {}
        Some([0x41, 0x49, 0x01]) => return Err("type 1 AppImages are not supported"),
        _ => return Err("not a type 2 AppImage"),
    }

    // ELF64 e_shoff (0x28), e_shentsize (0x3a), e_shnum (0x3c).
    let shoff = read_u64(&header, 0x28).ok_or("truncated ELF header")?;
    let shentsize = read_u16(&header, 0x3a).ok_or("truncated ELF header")?;
    let shnum = read_u16(&header, 0x3c).ok_or("truncated ELF header")?;
    if shoff == 0 || shnum == 0 {
        return Err("ELF has no section header table");
    }
    if u64::from(shentsize) != SHDR_LEN {
        // AppImageKit reads `sizeof(Elf64_Shdr)` regardless; anything else
        // would desynchronise our offset from the runtime's.
        return Err("unsupported ELF section header entry size");
    }
    let shnum64 = u64::from(shnum);
    let sht_end = shoff
        .checked_add(
            u64::from(shentsize)
                .checked_mul(shnum64)
                .ok_or("ELF section header table overflow")?,
        )
        .ok_or("ELF section header table overflow")?;
    if sht_end > file_len {
        return Err("ELF section header table extends past end of file");
    }

    // The runtime also considers the last section's payload: a file can end
    // with a section rather than with the table of section headers.
    let last_header = shoff
        .checked_add(
            u64::from(shentsize)
                .checked_mul(shnum64 - 1)
                .ok_or("ELF section header offset overflow")?,
        )
        .ok_or("ELF section header offset overflow")?;
    let last_end = last_header
        .checked_add(SHDR_LEN)
        .ok_or("ELF section header offset overflow")?;
    if last_end > file_len {
        return Err("ELF section header extends past end of file");
    }
    reader
        .seek(SeekFrom::Start(last_header))
        .map_err(|_| "cannot seek to the last ELF section header")?;
    let mut shdr = [0u8; SHDR_LEN as usize];
    reader
        .read_exact(&mut shdr)
        .map_err(|_| "cannot read the last ELF section header")?;
    let section_offset =
        read_u64(&shdr, SHDR_OFFSET_FIELD as usize).ok_or("truncated ELF section header")?;
    let section_size =
        read_u64(&shdr, SHDR_SIZE_FIELD as usize).ok_or("truncated ELF section header")?;
    let last_section_end = section_offset
        .checked_add(section_size)
        .ok_or("ELF section end overflow")?;

    let fs_offset = sht_end.max(last_section_end);
    let magic_end = fs_offset.checked_add(4).ok_or("squashfs offset overflow")?;
    if magic_end > file_len {
        return Err("squashfs offset lies past the end of the file");
    }
    reader
        .seek(SeekFrom::Start(fs_offset))
        .map_err(|_| "cannot seek to the squashfs offset")?;
    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|_| "cannot read the squashfs superblock")?;
    if &magic != b"hsqs" {
        return Err("no squashfs superblock at the declared offset");
    }
    Ok(AppImageLayout { fs_offset })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::inspect;
    use std::io::Cursor;

    /// Build a realistic ELF64 type-2 header: the section header table lives
    /// at `shoff`, the last entry carries `last_section`, and `hsqs` sits at
    /// `fs_offset`. Nothing is crammed into the first 128 bytes.
    fn image(
        shoff: u64,
        shnum: u16,
        shentsize: u16,
        last_section: (u64, u64),
        fs_offset: u64,
    ) -> Vec<u8> {
        let table_end = shoff + u64::from(shentsize) * u64::from(shnum);
        let end = (fs_offset + 4).max(table_end) as usize;
        let mut bytes = vec![0u8; end];
        bytes[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        bytes[4] = 2; // ELF64
        bytes[5] = 1; // little endian
        bytes[8..11].copy_from_slice(&[0x41, 0x49, 0x02]); // AppImage type 2
        bytes[0x28..0x30].copy_from_slice(&shoff.to_le_bytes());
        bytes[0x3a..0x3c].copy_from_slice(&shentsize.to_le_bytes());
        bytes[0x3c..0x3e].copy_from_slice(&shnum.to_le_bytes());
        if shnum > 0 && shentsize == 64 {
            let last = shoff + 64 * u64::from(shnum - 1);
            if last + 64 <= bytes.len() as u64 {
                let at = last as usize;
                bytes[at + 0x18..at + 0x20].copy_from_slice(&last_section.0.to_le_bytes());
                bytes[at + 0x20..at + 0x28].copy_from_slice(&last_section.1.to_le_bytes());
            }
        }
        bytes[fs_offset as usize..fs_offset as usize + 4].copy_from_slice(b"hsqs");
        bytes
    }

    fn check(bytes: &[u8]) -> Result<u64, &'static str> {
        let mut cursor = Cursor::new(bytes);
        inspect(&mut cursor, bytes.len() as u64).map(|l| l.fs_offset)
    }

    #[test]
    fn accepts_the_real_appimage_layout() {
        // Section table at 4096, one 64-byte entry → squashfs at 4160.
        let bytes = image(4096, 1, 64, (0, 0), 4160);
        assert_eq!(check(&bytes), Ok(4160));
    }

    #[test]
    fn accepts_a_last_section_that_ends_after_the_table() {
        let bytes = image(4096, 2, 64, (4160, 100), 4260);
        assert_eq!(check(&bytes), Ok(4260));
    }

    #[test]
    fn accepts_a_megabyte_offset() {
        let bytes = image(1024 * 1024, 3, 64, (0, 0), 1024 * 1024 + 192);
        assert_eq!(check(&bytes), Ok(1024 * 1024 + 192));
    }

    #[test]
    fn rejects_empty_and_non_elf_input() {
        assert!(check(&[]).is_err());
        let mut bytes = image(4096, 2, 64, (0, 0), 4160);
        bytes[1] = b'X';
        assert_eq!(check(&bytes), Err("not an ELF file"));
    }

    #[test]
    fn rejects_type1_and_unknown_magic() {
        let mut t1 = image(4096, 2, 64, (0, 0), 4160);
        t1[8..11].copy_from_slice(&[0x41, 0x49, 0x01]);
        assert_eq!(check(&t1), Err("type 1 AppImages are not supported"));
        let mut other = image(4096, 2, 64, (0, 0), 4160);
        other[8..11].copy_from_slice(&[0x41, 0x49, 0x03]);
        assert_eq!(check(&other), Err("not a type 2 AppImage"));
    }

    #[test]
    fn rejects_32bit_and_big_endian() {
        let mut bit32 = image(4096, 2, 64, (0, 0), 4160);
        bit32[4] = 1;
        assert_eq!(check(&bit32), Err("only 64-bit AppImages are supported"));
        let mut be = image(4096, 2, 64, (0, 0), 4160);
        be[5] = 2;
        assert_eq!(
            check(&be),
            Err("only little-endian AppImages are supported")
        );
    }

    #[test]
    fn rejects_missing_or_unsupported_section_tables() {
        let no_table = image(0, 0, 64, (0, 0), 64);
        assert_eq!(check(&no_table), Err("ELF has no section header table"));
        let zero_count = image(4096, 0, 64, (0, 0), 4160);
        assert_eq!(check(&zero_count), Err("ELF has no section header table"));
        let odd_size = image(4096, 2, 32, (0, 0), 4160);
        assert_eq!(
            check(&odd_size),
            Err("unsupported ELF section header entry size")
        );
    }

    #[test]
    fn rejects_tables_that_escape_the_file() {
        let past_eof = image(4096, 2, 64, (0, 0), 4160);
        let mut bytes = past_eof.clone();
        bytes[0x3c..0x3e].copy_from_slice(&9999u16.to_le_bytes());
        assert_eq!(
            check(&bytes),
            Err("ELF section header table extends past end of file")
        );

        let mut overflow = past_eof.clone();
        overflow[0x28..0x30].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(check(&overflow).is_err());

        // Keep the last header inside the file, but make its payload wrap.
        let mut section_overflow = image(4096, 2, 64, (0, 0), 8192);
        let last = 4096 + 64;
        section_overflow[last + 0x18..last + 0x20].copy_from_slice(&u64::MAX.to_le_bytes());
        section_overflow[last + 0x20..last + 0x28].copy_from_slice(&1u64.to_le_bytes());
        assert_eq!(check(&section_overflow), Err("ELF section end overflow"));
    }

    #[test]
    fn rejects_offsets_that_escape_the_file_or_have_no_superblock() {
        let mut beyond = image(4096, 2, 64, (0, 0), 4160);
        // Keep the table inside the file, but move the magic past EOF.
        beyond.truncate(4224);
        assert_eq!(
            check(&beyond),
            Err("squashfs offset lies past the end of the file")
        );

        let mut no_magic = image(4096, 2, 64, (0, 0), 4224);
        no_magic[4224..4228].copy_from_slice(b"nope");
        assert_eq!(
            check(&no_magic),
            Err("no squashfs superblock at the declared offset")
        );
    }
}
