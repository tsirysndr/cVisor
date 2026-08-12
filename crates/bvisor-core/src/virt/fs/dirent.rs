//! `linux_dirent64` serialization for getdents64, plus the insertion-ordered
//! name→d_type map used to merge overlay and lower-layer directory listings.
//! Port of `dirent.zig`.

use crate::virt::tombstones::Tombstones;
use std::collections::HashMap;

/// Offset of the name field: d_ino(8) + d_off(8) + d_reclen(2) + d_type(1).
pub const NAME_OFFSET: usize = 19;

/// Insertion-ordered map of entry name -> d_type. Order matters because
/// getdents pagination uses positional offsets as cookies.
#[derive(Default)]
pub struct DirEntryMap {
    order: Vec<String>,
    types: HashMap<String, u8>,
}

impl DirEntryMap {
    pub fn new() -> DirEntryMap {
        DirEntryMap::default()
    }

    /// Insert `name` with `d_type`. When `dedup` is true, an existing entry is
    /// left untouched (lower-layer wins); otherwise the d_type is overwritten.
    pub fn insert(&mut self, name: &str, d_type: u8, dedup: bool) {
        if let Some(slot) = self.types.get_mut(name) {
            if !dedup {
                *slot = d_type;
            }
        } else {
            self.order.push(name.to_string());
            self.types.insert(name.to_string(), d_type);
        }
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Iterate entries in insertion order as (name, d_type).
    pub fn iter(&self) -> impl Iterator<Item = (&str, u8)> {
        self.order.iter().map(move |k| (k.as_str(), self.types[k]))
    }
}

/// Aligned record length for a dirent with the given name length.
pub fn rec_len(name_len: usize) -> usize {
    (NAME_OFFSET + name_len + 1).div_ceil(8) * 8
}

/// Serialize one `linux_dirent64` into `buf` (which must be at least
/// `rec_len` bytes). Trailing bytes up to `rec_len` are zeroed.
pub fn write_dirent(buf: &mut [u8], ino: u64, d_off: i64, rec_len: u16, d_type: u8, name: &[u8]) {
    buf[0..8].copy_from_slice(&ino.to_le_bytes());
    buf[8..16].copy_from_slice(&d_off.to_le_bytes());
    buf[16..18].copy_from_slice(&rec_len.to_le_bytes());
    buf[18] = d_type;
    buf[NAME_OFFSET..NAME_OFFSET + name.len()].copy_from_slice(name);
    for b in &mut buf[NAME_OFFSET + name.len()..rec_len as usize] {
        *b = 0;
    }
}

/// Parse dirent64 entries from a raw kernel buffer into `map`.
pub fn collect_dirents(raw: &[u8], map: &mut DirEntryMap, dedup: bool) {
    let mut pos = 0usize;
    while pos + NAME_OFFSET < raw.len() {
        let reclen = u16::from_le_bytes([raw[pos + 16], raw[pos + 17]]) as usize;
        if reclen < NAME_OFFSET || pos + reclen > raw.len() {
            break;
        }
        let d_type = raw[pos + 18];
        let name_bytes = &raw[pos + NAME_OFFSET..pos + reclen];
        let name_len = name_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_bytes.len());
        if let Ok(name) = std::str::from_utf8(&name_bytes[..name_len]) {
            map.insert(name, d_type, dedup);
        }
        pos += reclen;
    }
}

/// Parse just the entry names from a serialized buffer (test helper).
pub fn parse_dirent_names(buf: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + NAME_OFFSET < buf.len() {
        let reclen = u16::from_le_bytes([buf[pos + 16], buf[pos + 17]]) as usize;
        if reclen < NAME_OFFSET || pos + reclen > buf.len() {
            break;
        }
        let name_bytes = &buf[pos + NAME_OFFSET..pos + reclen];
        let name_len = name_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_bytes.len());
        if let Ok(name) = std::str::from_utf8(&name_bytes[..name_len]) {
            out.push(name.to_string());
        }
        pos += reclen;
    }
    out
}

/// A merged directory is empty from the guest's view if it holds only `.`/`..`
/// after filtering tombstoned children.
pub fn is_map_empty(map: &DirEntryMap, dir_path: &str, tombstones: &Tombstones) -> bool {
    for (name, _) in map.iter() {
        if name == "." || name == ".." {
            continue;
        }
        if !tombstones.is_child_tombstoned(dir_path, name) {
            return false;
        }
    }
    true
}

/// Serialize directory entries into `buf`, skipping already-returned entries
/// (before `dirents_offset`) and tombstoned children. `dirents_offset` is
/// advanced past every entry consumed (including skipped ones). Returns bytes
/// written.
pub fn serialize_entries(
    map: &DirEntryMap,
    buf: &mut [u8],
    dir_path: &str,
    dirents_offset: &mut usize,
    tombstones: &Tombstones,
) -> usize {
    let mut buf_pos = 0usize;
    let mut entry_idx = 0usize;

    for (name, d_type) in map.iter() {
        if entry_idx < *dirents_offset {
            entry_idx += 1;
            continue;
        }

        if name != "." && name != ".." && tombstones.is_child_tombstoned(dir_path, name) {
            entry_idx += 1;
            *dirents_offset += 1;
            continue;
        }

        let reclen = rec_len(name.len());
        if buf_pos + reclen > buf.len() {
            break;
        }

        write_dirent(
            &mut buf[buf_pos..],
            (entry_idx + 1) as u64,
            (entry_idx + 1) as i64,
            reclen as u16,
            d_type,
            name.as_bytes(),
        );

        buf_pos += reclen;
        entry_idx += 1;
        *dirents_offset += 1;
    }

    buf_pos
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT_REG: u8 = 8;
    const DT_DIR: u8 = 4;

    #[test]
    fn rec_len_is_8_aligned() {
        assert_eq!(rec_len(1), 24); // 19 + 1 + 1 = 21 -> 24
        assert_eq!(rec_len(5), 32); // 19 + 5 + 1 = 25 -> 32
        assert_eq!(rec_len(4), 24); // 19 + 4 + 1 = 24 -> 24
    }

    #[test]
    fn write_then_parse_roundtrip() {
        let mut buf = vec![0u8; rec_len(3)];
        write_dirent(&mut buf, 7, 7, rec_len(3) as u16, DT_REG, b"foo");
        assert_eq!(parse_dirent_names(&buf), vec!["foo".to_string()]);
    }

    #[test]
    fn collect_roundtrips_through_serialize() {
        let mut map = DirEntryMap::new();
        map.insert("a", DT_REG, false);
        map.insert("b", DT_DIR, false);
        let ts = Tombstones::new();
        let mut buf = vec![0u8; 4096];
        let mut off = 0;
        let n = serialize_entries(&map, &mut buf, "/d", &mut off, &ts);
        assert_eq!(parse_dirent_names(&buf[..n]), vec!["a", "b"]);
        assert_eq!(off, 2);
    }

    #[test]
    fn dedup_keeps_first_type() {
        let mut map = DirEntryMap::new();
        map.insert("x", DT_REG, false);
        map.insert("x", DT_DIR, true); // dedup: keep DT_REG
        assert_eq!(map.iter().collect::<Vec<_>>(), vec![("x", DT_REG)]);
        map.insert("x", DT_DIR, false); // overwrite
        assert_eq!(map.iter().collect::<Vec<_>>(), vec![("x", DT_DIR)]);
    }

    #[test]
    fn tombstoned_children_filtered() {
        let mut map = DirEntryMap::new();
        map.insert("keep", DT_REG, false);
        map.insert("gone", DT_REG, false);
        let mut ts = Tombstones::new();
        ts.add("/d/gone");
        let mut buf = vec![0u8; 4096];
        let mut off = 0;
        let n = serialize_entries(&map, &mut buf, "/d", &mut off, &ts);
        assert_eq!(parse_dirent_names(&buf[..n]), vec!["keep"]);
        assert_eq!(off, 2); // both consumed, one emitted
    }

    #[test]
    fn pagination_via_offset() {
        let mut map = DirEntryMap::new();
        for name in ["a", "b", "c"] {
            map.insert(name, DT_REG, false);
        }
        let ts = Tombstones::new();
        // Buffer big enough for only one entry.
        let mut buf = vec![0u8; rec_len(1)];
        let mut off = 0;
        let n1 = serialize_entries(&map, &mut buf, "/d", &mut off, &ts);
        assert_eq!(parse_dirent_names(&buf[..n1]), vec!["a"]);
        assert_eq!(off, 1);
        let n2 = serialize_entries(&map, &mut buf, "/d", &mut off, &ts);
        assert_eq!(parse_dirent_names(&buf[..n2]), vec!["b"]);
        assert_eq!(off, 2);
    }

    #[test]
    fn is_map_empty_ignores_dot_entries() {
        let mut map = DirEntryMap::new();
        map.insert(".", DT_DIR, false);
        map.insert("..", DT_DIR, false);
        let ts = Tombstones::new();
        assert!(is_map_empty(&map, "/d", &ts));
        map.insert("real", DT_REG, false);
        assert!(!is_map_empty(&map, "/d", &ts));
        let mut ts2 = Tombstones::new();
        ts2.add("/d/real");
        assert!(is_map_empty(&map, "/d", &ts2));
    }
}
