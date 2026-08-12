//! Per-process filesystem info (analogue of Linux `fs_struct`): the current
//! working directory. Shared on CLONE_FS, cloned otherwise. Port of `FsInfo.zig`
//! (umask/root are not yet virtualized).

#[derive(Clone)]
pub struct FsInfo {
    pub cwd: String,
}

impl Default for FsInfo {
    fn default() -> Self {
        FsInfo::new()
    }
}

impl FsInfo {
    pub fn new() -> FsInfo {
        FsInfo {
            cwd: "/".to_string(),
        }
    }

    pub fn set_cwd(&mut self, cwd: &str) {
        self.cwd = cwd.to_string();
    }
}
