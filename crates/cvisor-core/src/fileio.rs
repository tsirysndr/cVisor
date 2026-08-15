//! Host-side file transfer in and out of a sandbox's overlay, without a running
//! guest. Files live in the per-uid overlay at `/tmp/.cvisor/sb/{uid}` and
//! persist across runs, so a file written here is visible to a later `execute`
//! of the same sandbox. Routing mirrors the supervisor's `openat`: writes land
//! in the cow/tmp upper layer; reads prefer the upper layer, then fall through
//! to the real (host) file for cow paths.

use crate::error::{Errno, SysError, SysResult};
use crate::virt::overlay_root::OverlayRoot;
use crate::virt::path::{resolve_and_route, BackendType, ResolvedRoute};

fn io_errno(e: &std::io::Error) -> SysError {
    SysError(
        e.raw_os_error()
            .and_then(Errno::from_raw)
            .unwrap_or(Errno::IO),
    )
}

/// Resolve an absolute guest path to its backend + normalized form.
fn route(guest_path: &str) -> SysResult<(BackendType, String)> {
    if !guest_path.starts_with('/') {
        return Err(SysError(Errno::INVAL));
    }
    match resolve_and_route("/", guest_path)? {
        ResolvedRoute::Block => Err(SysError(Errno::PERM)),
        ResolvedRoute::Handle {
            backend,
            normalized,
        } => Ok((backend, normalized)),
    }
}

/// Write `data` to `guest_path` inside sandbox `uid`, creating parent dirs. The
/// file becomes visible to subsequent runs of the same sandbox. Passthrough and
/// synthetic (`/proc`) paths are not writable.
pub fn write_file(uid: [u8; 16], guest_path: &str, data: &[u8]) -> SysResult<()> {
    let overlay = OverlayRoot::new(uid).map_err(|e| io_errno(&e))?;
    let (backend, normalized) = route(guest_path)?;
    let real = match backend {
        BackendType::Cow => {
            overlay
                .create_cow_parent_dirs(&normalized)
                .map_err(|e| io_errno(&e))?;
            overlay.resolve_cow(&normalized)
        }
        BackendType::Tmp => {
            let p = overlay.resolve_tmp(&normalized)?;
            OverlayRoot::create_parent_dirs(&p).map_err(|e| io_errno(&e))?;
            p
        }
        BackendType::Passthrough | BackendType::Proc | BackendType::Event => {
            return Err(SysError(Errno::PERM));
        }
    };
    std::fs::write(&real, data).map_err(|e| io_errno(&e))
}

/// Read `guest_path` from sandbox `uid`, as the guest would see it: the cow/tmp
/// upper-layer copy if present, else the real (host) file for a cow path.
pub fn read_file(uid: [u8; 16], guest_path: &str) -> SysResult<Vec<u8>> {
    let overlay = OverlayRoot::new(uid).map_err(|e| io_errno(&e))?;
    let (backend, normalized) = route(guest_path)?;
    let real = match backend {
        BackendType::Cow => {
            let upper = overlay.resolve_cow(&normalized);
            if OverlayRoot::path_exists_on_real_fs(&upper) {
                upper
            } else {
                normalized // read-through to the real host file
            }
        }
        BackendType::Tmp => overlay.resolve_tmp(&normalized)?,
        BackendType::Passthrough => normalized,
        BackendType::Proc | BackendType::Event => return Err(SysError(Errno::NOSYS)),
    };
    std::fs::read(&real).map_err(|e| io_errno(&e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate_uid;

    #[test]
    fn tmp_write_then_read_roundtrips() {
        let uid = generate_uid();
        write_file(uid, "/tmp/hello.txt", b"world").unwrap();
        assert_eq!(read_file(uid, "/tmp/hello.txt").unwrap(), b"world");
        // Cleanup.
        let _ =
            std::fs::remove_dir_all(format!("/tmp/.cvisor/sb/{}", String::from_utf8_lossy(&uid)));
    }

    #[test]
    fn cow_write_shadows_and_reads_back() {
        let uid = generate_uid();
        write_file(uid, "/etc/cvisor_probe.conf", b"key=val").unwrap();
        assert_eq!(
            read_file(uid, "/etc/cvisor_probe.conf").unwrap(),
            b"key=val"
        );
        let _ =
            std::fs::remove_dir_all(format!("/tmp/.cvisor/sb/{}", String::from_utf8_lossy(&uid)));
    }

    #[test]
    fn non_absolute_and_proc_paths_rejected() {
        let uid = generate_uid();
        assert!(write_file(uid, "relative/path", b"x").is_err());
        assert!(write_file(uid, "/proc/self/status", b"x").is_err());
    }
}
