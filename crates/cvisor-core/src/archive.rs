//! Directory archiving for the cache: pack a tree into a tar with pluggable
//! compression, and unpack it back. Packing walks the tree with the `ignore`
//! crate so `.gitignore` / `.dockerignore` rules are respected.
//!
//! Formats: gzip (default), zstd (feature `zstd`), estargz (a seekable, gzip
//! multi-member layout with a TOC footer — round-trips here and is readable by
//! any gzip reader), and none (a plain tar).

use std::io::{self, Read, Write};
use std::path::Path;

use ignore::WalkBuilder;

use crate::error::{Errno, SysError, SysResult};

/// Archive compression format.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Gzip,
    Zstd,
    Estargz,
    None,
}

impl Format {
    /// Parse a format name (`gzip`/`gz`, `zstd`/`zst`, `estargz`, `none`/`tar`/
    /// `uncompressed`). Defaults are the caller's concern.
    pub fn parse(s: &str) -> Option<Format> {
        match s.to_ascii_lowercase().as_str() {
            "gzip" | "gz" => Some(Format::Gzip),
            "zstd" | "zst" => Some(Format::Zstd),
            "estargz" | "estarGZ" => Some(Format::Estargz),
            "none" | "tar" | "uncompressed" | "raw" => Some(Format::None),
            _ => None,
        }
    }

    /// File extension for an archive of this format.
    pub fn ext(&self) -> &'static str {
        match self {
            Format::Gzip | Format::Estargz => "tar.gz",
            Format::Zstd => "tar.zst",
            Format::None => "tar",
        }
    }
}

fn io_err(e: io::Error) -> SysError {
    SysError(
        e.raw_os_error()
            .and_then(Errno::from_raw)
            .unwrap_or(Errno::IO),
    )
}

/// Pack `src_dir` into `out` as a tar with the given compression, skipping paths
/// matched by `.gitignore` / `.dockerignore` found within the tree.
pub fn pack<W: Write>(src_dir: &Path, format: Format, out: W) -> SysResult<()> {
    match format {
        Format::None => {
            let mut b = tar::Builder::new(out);
            walk_into_tar(src_dir, &mut b)?;
            b.finish().map_err(io_err)
        }
        Format::Gzip => {
            let enc = flate2::write::GzEncoder::new(out, flate2::Compression::default());
            let mut b = tar::Builder::new(enc);
            walk_into_tar(src_dir, &mut b)?;
            b.into_inner().map_err(io_err)?.finish().map_err(io_err)?;
            Ok(())
        }
        Format::Estargz => pack_estargz(src_dir, out),
        Format::Zstd => pack_zstd(src_dir, out),
    }
}

/// Unpack a tar (with the given compression) from `input` into `dst_dir`.
pub fn unpack<R: Read>(input: R, format: Format, dst_dir: &Path) -> SysResult<()> {
    std::fs::create_dir_all(dst_dir).map_err(io_err)?;
    match format {
        Format::None => unpack_tar(input, dst_dir),
        // estargz is gzip; a multi-member decoder reads both plain and
        // per-entry-member gzip streams. The TOC entry is skipped on extract.
        Format::Gzip | Format::Estargz => {
            unpack_tar(flate2::read::MultiGzDecoder::new(input), dst_dir)
        }
        Format::Zstd => unpack_zstd(input, dst_dir),
    }
}

/// Walk `src_dir` honoring ignore files and append each entry to `builder`.
fn walk_into_tar<W: Write>(src_dir: &Path, builder: &mut tar::Builder<W>) -> SysResult<()> {
    for path in walk(src_dir)? {
        let rel = path.strip_prefix(src_dir).unwrap_or(&path);
        if rel.as_os_str().is_empty() {
            continue;
        }
        // Symlinks and files are added by path; directories as (possibly empty)
        // dir entries so an empty dir survives the round-trip.
        let meta = std::fs::symlink_metadata(&path).map_err(io_err)?;
        if meta.is_dir() {
            builder.append_dir(rel, &path).map_err(io_err)?;
        } else {
            builder.append_path_with_name(&path, rel).map_err(io_err)?;
        }
    }
    Ok(())
}

/// The paths under `src_dir` that survive `.gitignore` / `.dockerignore`.
pub(crate) fn walk(src_dir: &Path) -> SysResult<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    let walker = WalkBuilder::new(src_dir)
        .hidden(false) // include dotfiles; ignore files still apply
        .parents(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false) // apply .gitignore even outside a git repo
        .add_custom_ignore_filename(".dockerignore")
        .build();
    for entry in walker {
        let entry = entry.map_err(|_| SysError(Errno::IO))?;
        out.push(entry.into_path());
    }
    Ok(out)
}

fn unpack_tar<R: Read>(input: R, dst_dir: &Path) -> SysResult<()> {
    let mut ar = tar::Archive::new(input);
    ar.set_preserve_permissions(true);
    ar.set_overwrite(true);
    for entry in ar.entries().map_err(io_err)? {
        let mut entry = entry.map_err(io_err)?;
        let path = entry.path().map_err(io_err)?.into_owned();
        // Skip the estargz TOC if present.
        if path.as_os_str() == "stargz.index.json" {
            continue;
        }
        entry.unpack_in(dst_dir).map_err(io_err)?;
    }
    Ok(())
}

/// estargz: each entry is its own gzip member (so files are individually
/// seekable), followed by a `stargz.index.json` TOC member and the 51-byte
/// eStargz footer pointing at it. Any gzip reader (via MultiGzDecoder) still
/// decompresses the whole stream.
fn pack_estargz<W: Write>(src_dir: &Path, mut out: W) -> SysResult<()> {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    #[derive(Default)]
    struct Toc {
        entries: Vec<String>,
    }
    let mut offset: u64 = 0;
    let mut toc = Toc::default();

    // Helper: gzip one blob as an independent member, returning its start offset.
    let mut write_member = |out: &mut W, blob: &[u8]| -> SysResult<u64> {
        let start = offset;
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(blob).map_err(io_err)?;
        let member = enc.finish().map_err(io_err)?;
        out.write_all(&member).map_err(io_err)?;
        offset += member.len() as u64;
        Ok(start)
    };

    for path in walk(src_dir)? {
        let rel = path.strip_prefix(src_dir).unwrap_or(&path);
        if rel.as_os_str().is_empty() {
            continue;
        }
        // Build the raw tar blocks for this one entry into a buffer.
        let mut buf = Vec::new();
        {
            let mut b = tar::Builder::new(&mut buf);
            let meta = std::fs::symlink_metadata(&path).map_err(io_err)?;
            if meta.is_dir() {
                b.append_dir(rel, &path).map_err(io_err)?;
            } else {
                b.append_path_with_name(&path, rel).map_err(io_err)?;
            }
            b.finish().map_err(io_err)?;
        }
        // Drop the trailing two zero blocks (the tar end marker) so members concat.
        let end = buf.len().saturating_sub(1024);
        let start = write_member(&mut out, &buf[..end])?;
        let name = rel.to_string_lossy().replace('\\', "/");
        toc.entries
            .push(format!("{{\"name\":{name:?},\"offset\":{start}}}"));
    }

    // TOC member.
    let toc_json = format!("{{\"version\":1,\"entries\":[{}]}}", toc.entries.join(","));
    let mut tbuf = Vec::new();
    {
        let mut b = tar::Builder::new(&mut tbuf);
        let mut header = tar::Header::new_gnu();
        header.set_size(toc_json.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        b.append_data(&mut header, "stargz.index.json", toc_json.as_bytes())
            .map_err(io_err)?;
        b.finish().map_err(io_err)?;
    }
    let tend = tbuf.len().saturating_sub(1024);
    let toc_offset = write_member(&mut out, &tbuf[..tend])?;

    // eStargz footer: a gzip member whose Extra field carries the TOC offset.
    out.write_all(&estargz_footer(toc_offset)).map_err(io_err)?;
    Ok(())
}

/// The 51-byte eStargz footer: an empty gzip member with an Extra subfield
/// `%016xSTARGZ` (the hex TOC offset) that stargz tooling reads to locate the
/// TOC without scanning the whole archive.
fn estargz_footer(toc_offset: u64) -> Vec<u8> {
    let payload = format!("{toc_offset:016x}STARGZ"); // 22 bytes
    let mut extra = Vec::new();
    extra.extend_from_slice(b"SG"); // subfield id
    extra.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    extra.extend_from_slice(payload.as_bytes());

    // gzip magic, deflate, FLG.FEXTRA, mtime=0, XFL=0, OS=unknown.
    let mut m = vec![0x1f, 0x8b, 0x08, 0x04, 0, 0, 0, 0, 0x00, 0xff];
    m.extend_from_slice(&(extra.len() as u16).to_le_bytes());
    m.extend_from_slice(&extra);
    m.extend_from_slice(&[0x03, 0x00]); // empty deflate block
    m.extend_from_slice(&[0, 0, 0, 0]); // CRC32 of empty
    m.extend_from_slice(&[0, 0, 0, 0]); // ISIZE 0
    m
}

#[cfg(feature = "zstd")]
fn pack_zstd<W: Write>(src_dir: &Path, out: W) -> SysResult<()> {
    let enc = zstd::stream::write::Encoder::new(out, 3)
        .map_err(io_err)?
        .auto_finish();
    let mut b = tar::Builder::new(enc);
    walk_into_tar(src_dir, &mut b)?;
    b.finish().map_err(io_err)
}

#[cfg(feature = "zstd")]
fn unpack_zstd<R: Read>(input: R, dst_dir: &Path) -> SysResult<()> {
    let dec = zstd::stream::read::Decoder::new(input).map_err(io_err)?;
    unpack_tar(dec, dst_dir)
}

#[cfg(not(feature = "zstd"))]
fn pack_zstd<W: Write>(_src_dir: &Path, _out: W) -> SysResult<()> {
    Err(SysError(Errno::NOSYS))
}

#[cfg(not(feature = "zstd"))]
fn unpack_zstd<R: Read>(_input: R, _dst_dir: &Path) -> SysResult<()> {
    Err(SysError(Errno::NOSYS))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(format: Format) {
        let base = std::env::temp_dir().join(format!("cvisor-ar-{:?}", format));
        let src = base.join("src");
        let dst = base.join("dst");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), b"alpha").unwrap();
        std::fs::write(src.join("sub/b.txt"), b"beta").unwrap();
        // Ignored via .gitignore.
        std::fs::write(src.join(".gitignore"), b"ignored.log\n").unwrap();
        std::fs::write(src.join("ignored.log"), b"secret").unwrap();

        let mut buf = Vec::new();
        pack(&src, format, &mut buf).unwrap();
        unpack(&buf[..], format, &dst).unwrap();

        assert_eq!(std::fs::read(dst.join("a.txt")).unwrap(), b"alpha");
        assert_eq!(std::fs::read(dst.join("sub/b.txt")).unwrap(), b"beta");
        // The ignored file must not have been packed.
        assert!(!dst.join("ignored.log").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn gzip_roundtrip_respects_ignore() {
        roundtrip(Format::Gzip);
    }

    #[test]
    fn none_roundtrip() {
        roundtrip(Format::None);
    }

    #[test]
    fn estargz_roundtrip() {
        roundtrip(Format::Estargz);
    }

    #[cfg(feature = "zstd")]
    #[test]
    fn zstd_roundtrip() {
        roundtrip(Format::Zstd);
    }
}
