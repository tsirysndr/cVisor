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
            archive::pack(&src, format, file)
        }
        Backend::S3 { .. } => {
            let mut buf = Vec::new();
            archive::pack(&src, format, &mut buf)?;
            s3_put(backend, &object_name(key, format)?, &buf)
        }
    }
}

/// Fetch the archive stored under `key` and unpack it into the overlay at
/// `sandbox_path` (visible to later runs of the same sandbox).
pub fn restore(
    uid: [u8; 16],
    sandbox_path: &str,
    key: &str,
    backend: &Backend,
    format: Format,
) -> SysResult<()> {
    let dst = fileio::write_real_dir(uid, sandbox_path)?;
    match backend {
        Backend::Disk { root } => {
            let src = root.join(object_name(key, format)?);
            let file = std::fs::File::open(&src).map_err(io_err)?;
            archive::unpack(file, format, &dst)
        }
        Backend::S3 { .. } => {
            let bytes = s3_get(backend, &object_name(key, format)?)?;
            archive::unpack(&bytes[..], format, &dst)
        }
    }
}

/// Whether an archive exists under `key`.
pub fn exists(key: &str, backend: &Backend, format: Format) -> SysResult<bool> {
    let name = object_name(key, format)?;
    match backend {
        Backend::Disk { root } => Ok(root.join(&name).exists()),
        Backend::S3 { .. } => Ok(s3_get(backend, &name).is_ok()),
    }
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
}
