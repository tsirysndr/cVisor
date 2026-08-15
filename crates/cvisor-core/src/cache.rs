//! Cache: back up and restore a sandbox directory as a compressed archive,
//! keyed by name. The default backend is the host disk; S3 is available with
//! the `s3` feature. Save packs the sandbox's view of the directory; restore
//! unpacks it into the overlay so a later run sees the contents.

use std::path::PathBuf;

use crate::archive::{self, Format};
use crate::error::{Errno, SysError, SysResult};
use crate::fileio;

/// Where cache archives are stored.
pub enum Backend {
    /// A directory on the host (default: `$CVISOR_CACHE_DIR` or
    /// `/tmp/.cvisor/cache`).
    Disk { root: PathBuf },
    /// An S3 bucket + key prefix (requires the `s3` feature).
    S3 {
        bucket: String,
        prefix: String,
        region: Option<String>,
        endpoint: Option<String>,
    },
}

impl Backend {
    /// The default disk backend.
    pub fn default_disk() -> Backend {
        let root = std::env::var("CVISOR_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp/.cvisor/cache"));
        Backend::Disk { root }
    }

    /// Parse a backend spec:
    ///   ""|"disk"                 -> default disk
    ///   "disk:/path" | "/path"    -> disk at /path
    ///   "s3://bucket/prefix?region=..&endpoint=.." -> S3
    pub fn parse(spec: &str) -> SysResult<Backend> {
        let spec = spec.trim();
        if spec.is_empty() || spec == "disk" {
            return Ok(Backend::default_disk());
        }
        if let Some(rest) = spec.strip_prefix("s3://") {
            let (host_path, query) = match rest.split_once('?') {
                Some((a, b)) => (a, Some(b)),
                None => (rest, None),
            };
            let (bucket, prefix) = match host_path.split_once('/') {
                Some((b, p)) => (b.to_string(), p.trim_end_matches('/').to_string()),
                None => (host_path.to_string(), String::new()),
            };
            let mut region = None;
            let mut endpoint = None;
            if let Some(q) = query {
                for kv in q.split('&') {
                    if let Some(v) = kv.strip_prefix("region=") {
                        region = Some(v.to_string());
                    } else if let Some(v) = kv.strip_prefix("endpoint=") {
                        endpoint = Some(v.to_string());
                    }
                }
            }
            if bucket.is_empty() {
                return Err(SysError(Errno::INVAL));
            }
            return Ok(Backend::S3 {
                bucket,
                prefix,
                region,
                endpoint,
            });
        }
        let path = spec.strip_prefix("disk:").unwrap_or(spec);
        Ok(Backend::Disk {
            root: PathBuf::from(path),
        })
    }
}

/// Sanitize a cache key into a safe relative object path (no traversal).
fn safe_key(key: &str) -> SysResult<String> {
    if key.is_empty() || key.contains("..") || key.starts_with('/') {
        return Err(SysError(Errno::INVAL));
    }
    Ok(key.to_string())
}

fn object_name(key: &str, format: Format) -> SysResult<String> {
    Ok(format!("{}.{}", safe_key(key)?, format.ext()))
}

fn io_err(e: std::io::Error) -> SysError {
    SysError(
        e.raw_os_error()
            .and_then(Errno::from_raw)
            .unwrap_or(Errno::IO),
    )
}

/// Pack the sandbox directory at `sandbox_path` and store it under `key`.
pub fn save(
    uid: [u8; 16],
    sandbox_path: &str,
    key: &str,
    backend: &Backend,
    format: Format,
) -> SysResult<()> {
    let src = fileio::read_real_path(uid, sandbox_path)?;
    if !src.is_dir() {
        return Err(SysError(Errno::NOTDIR));
    }
    match backend {
        Backend::Disk { root } => {
            let dest = root.join(object_name(key, format)?);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(io_err)?;
            }
            let file = std::fs::File::create(&dest).map_err(io_err)?;
            // On pack failure (e.g. an unsupported format), don't leave a
            // truncated archive behind.
            archive::pack(&src, format, file).inspect_err(|_| {
                let _ = std::fs::remove_file(&dest);
            })
        }
        Backend::S3 { .. } => {
            let mut buf = Vec::new();
            archive::pack(&src, format, &mut buf)?;
            s3_put(backend, &object_name(key, format)?, &buf)
        }
    }
}

/// Fetch the archive for `key` and unpack it into the overlay at `sandbox_path`
/// (visible to later runs of the same sandbox). Cache-key semantics: an exact
/// `key` match wins; failing that, the newest archive whose key *starts with*
/// `key` is used (a partial hit, like a restore-key prefix). Errors with NOENT
/// if nothing matches.
pub fn restore(
    uid: [u8; 16],
    sandbox_path: &str,
    key: &str,
    backend: &Backend,
    format: Format,
) -> SysResult<()> {
    let name = resolve(key, backend, format)?.ok_or(SysError(Errno::NOENT))?;
    let dst = fileio::write_real_dir(uid, sandbox_path)?;
    match backend {
        Backend::Disk { root } => {
            let file = std::fs::File::open(root.join(&name)).map_err(io_err)?;
            archive::unpack(file, format, &dst)
        }
        Backend::S3 { .. } => {
            let bytes = s3_get(backend, &name)?;
            archive::unpack(&bytes[..], format, &dst)
        }
    }
}

/// Resolve `key` to a stored object name: the exact `key.ext` if it exists,
/// else the newest object named `key*…ext` (prefix match), else None.
fn resolve(key: &str, backend: &Backend, format: Format) -> SysResult<Option<String>> {
    let exact = object_name(key, format)?;
    let prefix = safe_key(key)?;
    let ext = format.ext();
    match backend {
        Backend::Disk { root } => {
            if root.join(&exact).exists() {
                return Ok(Some(exact));
            }
            let Ok(entries) = std::fs::read_dir(root) else {
                return Ok(None);
            };
            let mut best: Option<(std::time::SystemTime, String)> = None;
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with(&prefix) && name.ends_with(&format!(".{ext}")) {
                    let mtime = e
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH);
                    if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
                        best = Some((mtime, name));
                    }
                }
            }
            Ok(best.map(|(_, n)| n))
        }
        Backend::S3 { .. } => s3_resolve(backend, &exact, &prefix, ext),
    }
}

/// Whether an archive matches `key` (exact or prefix).
pub fn exists(key: &str, backend: &Backend, format: Format) -> SysResult<bool> {
    Ok(resolve(key, backend, format)?.is_some())
}

/// Is `name` a cache archive (one of the known archive extensions)?
fn is_archive(name: &str) -> bool {
    name.ends_with(".tar.gz") || name.ends_with(".tar.zst") || name.ends_with(".tar")
}

/// A cached archive: its object name (e.g. `deps-v1.tar.gz`) and byte size.
pub struct Entry {
    pub name: String,
    pub size: u64,
}

/// List the cached archives in `backend`.
pub fn list(backend: &Backend) -> SysResult<Vec<Entry>> {
    match backend {
        Backend::Disk { root } => {
            let mut out = Vec::new();
            let Ok(entries) = std::fs::read_dir(root) else {
                return Ok(out); // no cache dir yet -> empty
            };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if is_archive(&name) {
                    let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                    out.push(Entry { name, size });
                }
            }
            out.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(out)
        }
        Backend::S3 { .. } => s3_list(backend),
    }
}

/// Remove the archive `key.<format ext>`. Returns true if it existed.
pub fn remove(key: &str, backend: &Backend, format: Format) -> SysResult<bool> {
    let name = object_name(key, format)?;
    match backend {
        Backend::Disk { root } => {
            let p = root.join(&name);
            if p.exists() {
                std::fs::remove_file(&p).map_err(io_err)?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        Backend::S3 { .. } => s3_delete(backend, &name).map(|_| true),
    }
}

/// Remove every cached archive. Returns how many were removed.
pub fn clear(backend: &Backend) -> SysResult<usize> {
    let entries = list(backend)?;
    let mut n = 0;
    match backend {
        Backend::Disk { root } => {
            for e in entries {
                if std::fs::remove_file(root.join(&e.name)).is_ok() {
                    n += 1;
                }
            }
        }
        Backend::S3 { .. } => {
            for e in entries {
                if s3_delete(backend, &e.name).is_ok() {
                    n += 1;
                }
            }
        }
    }
    Ok(n)
}

#[cfg(feature = "s3")]
fn s3_bucket(backend: &Backend) -> SysResult<(s3::Bucket, String)> {
    let Backend::S3 {
        bucket,
        prefix,
        region,
        endpoint,
    } = backend
    else {
        return Err(SysError(Errno::INVAL));
    };
    let region = match (region, endpoint) {
        (_, Some(ep)) => s3::Region::Custom {
            region: region.clone().unwrap_or_default(),
            endpoint: ep.clone(),
        },
        (Some(r), None) => r.parse().map_err(|_| SysError(Errno::INVAL))?,
        (None, None) => "us-east-1".parse().map_err(|_| SysError(Errno::INVAL))?,
    };
    let creds = s3::creds::Credentials::default().map_err(|_| SysError(Errno::ACCES))?;
    let b = s3::Bucket::new(bucket, region, creds)
        .map_err(|_| SysError(Errno::INVAL))?
        .with_path_style();
    let key_prefix = if prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", prefix.trim_end_matches('/'))
    };
    Ok((*b, key_prefix))
}

#[cfg(feature = "s3")]
fn s3_put(backend: &Backend, name: &str, data: &[u8]) -> SysResult<()> {
    let (bucket, prefix) = s3_bucket(backend)?;
    bucket
        .put_object_blocking(format!("{prefix}{name}"), data)
        .map_err(|_| SysError(Errno::IO))?;
    Ok(())
}

#[cfg(feature = "s3")]
fn s3_get(backend: &Backend, name: &str) -> SysResult<Vec<u8>> {
    let (bucket, prefix) = s3_bucket(backend)?;
    let resp = bucket
        .get_object_blocking(format!("{prefix}{name}"))
        .map_err(|_| SysError(Errno::IO))?;
    if resp.status_code() == 404 {
        return Err(SysError(Errno::NOENT));
    }
    Ok(resp.bytes().to_vec())
}

#[cfg(feature = "s3")]
fn s3_resolve(
    backend: &Backend,
    exact: &str,
    prefix: &str,
    ext: &str,
) -> SysResult<Option<String>> {
    let (bucket, key_prefix) = s3_bucket(backend)?;
    // Exact hit first. head_object returns Ok even on 404 (with the status
    // code), so only a 200 counts as present.
    if let Ok((_, 200)) = bucket.head_object_blocking(format!("{key_prefix}{exact}")) {
        return Ok(Some(exact.to_string()));
    }
    // Else newest object whose key starts with the prefix and ends with .ext.
    let results = bucket
        .list_blocking(format!("{key_prefix}{prefix}"), None)
        .map_err(|_| SysError(Errno::IO))?;
    let suffix = format!(".{ext}");
    let mut best: Option<(String, String)> = None; // (last_modified, name)
    for page in results {
        for obj in page.contents {
            let name = obj
                .key
                .strip_prefix(&key_prefix)
                .unwrap_or(&obj.key)
                .to_string();
            if name.ends_with(&suffix)
                && best
                    .as_ref()
                    .map(|(t, _)| obj.last_modified > *t)
                    .unwrap_or(true)
            {
                best = Some((obj.last_modified.clone(), name));
            }
        }
    }
    Ok(best.map(|(_, n)| n))
}

#[cfg(feature = "s3")]
fn s3_list(backend: &Backend) -> SysResult<Vec<Entry>> {
    let (bucket, key_prefix) = s3_bucket(backend)?;
    let pages = bucket
        .list_blocking(key_prefix.clone(), None)
        .map_err(|_| SysError(Errno::IO))?;
    let mut out = Vec::new();
    for page in pages {
        for obj in page.contents {
            let name = obj
                .key
                .strip_prefix(&key_prefix)
                .unwrap_or(&obj.key)
                .to_string();
            if is_archive(&name) {
                out.push(Entry {
                    name,
                    size: obj.size,
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[cfg(feature = "s3")]
fn s3_delete(backend: &Backend, name: &str) -> SysResult<()> {
    let (bucket, prefix) = s3_bucket(backend)?;
    bucket
        .delete_object_blocking(format!("{prefix}{name}"))
        .map_err(|_| SysError(Errno::IO))?;
    Ok(())
}

#[cfg(not(feature = "s3"))]
fn s3_list(_backend: &Backend) -> SysResult<Vec<Entry>> {
    Err(SysError(Errno::NOSYS))
}

#[cfg(not(feature = "s3"))]
fn s3_delete(_backend: &Backend, _name: &str) -> SysResult<()> {
    Err(SysError(Errno::NOSYS))
}

#[cfg(not(feature = "s3"))]
fn s3_resolve(
    _backend: &Backend,
    _exact: &str,
    _prefix: &str,
    _ext: &str,
) -> SysResult<Option<String>> {
    Err(SysError(Errno::NOSYS))
}

#[cfg(not(feature = "s3"))]
fn s3_put(_backend: &Backend, _name: &str, _data: &[u8]) -> SysResult<()> {
    Err(SysError(Errno::NOSYS))
}

#[cfg(not(feature = "s3"))]
fn s3_get(_backend: &Backend, _name: &str) -> SysResult<Vec<u8>> {
    Err(SysError(Errno::NOSYS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_parse() {
        assert!(matches!(Backend::parse("").unwrap(), Backend::Disk { .. }));
        assert!(matches!(
            Backend::parse("disk:/x").unwrap(),
            Backend::Disk { .. }
        ));
        match Backend::parse("s3://my-bucket/pre/fix?region=eu-west-1").unwrap() {
            Backend::S3 {
                bucket,
                prefix,
                region,
                ..
            } => {
                assert_eq!(bucket, "my-bucket");
                assert_eq!(prefix, "pre/fix");
                assert_eq!(region.as_deref(), Some("eu-west-1"));
            }
            _ => panic!("expected S3"),
        }
    }

    #[test]
    fn safe_key_rejects_traversal() {
        assert!(safe_key("../etc").is_err());
        assert!(safe_key("/abs").is_err());
        assert!(safe_key("ok/key-1").is_ok());
    }

    #[test]
    fn resolve_prefers_exact_then_prefix() {
        let root = std::env::temp_dir().join(format!(
            "cvisor-resolve-{}",
            crate::generate_uid()
                .iter()
                .map(|b| *b as char)
                .collect::<String>()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let backend = Backend::Disk { root: root.clone() };

        // Only a prefix match exists -> resolve to it.
        std::fs::write(root.join("deps-aaa.tar.gz"), b"x").unwrap();
        assert_eq!(
            resolve("deps-", &backend, Format::Gzip).unwrap().as_deref(),
            Some("deps-aaa.tar.gz")
        );
        // An exact match wins over prefix matches.
        std::fs::write(root.join("deps-.tar.gz"), b"x").unwrap();
        assert_eq!(
            resolve("deps-", &backend, Format::Gzip).unwrap().as_deref(),
            Some("deps-.tar.gz")
        );
        // No match.
        assert_eq!(resolve("nope", &backend, Format::Gzip).unwrap(), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
