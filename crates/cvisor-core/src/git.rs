//! Git conveniences: seed the host's git identity into a sandbox and clone a
//! repository inside it (used by `create_sandbox(repo_url)` and the SDK/CLI).

use crate::setup::{shell_argv, spawn_session, ExecOpts, PtyMode};
use crate::types::LogLevel;

/// Copy the host's global git identity (`~/.gitconfig`) and ssh material
/// (`~/.ssh`: keys, config, known_hosts) into the sandbox at `/`, where guests
/// (HOME=/) look for them. Best-effort: anything missing is skipped.
pub fn seed_git_identity(uid: [u8; 16]) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let home = std::path::PathBuf::from(home);
    for (src, dst) in [(".gitconfig", "/.gitconfig"), (".ssh", "/.ssh")] {
        let p = home.join(src);
        if p.exists() {
            let _ = crate::fileio::copy_into(uid, &p, dst);
        }
    }
}

/// Clone `url` into the sandbox at `/tmp/<repo>`, running git inside the
/// sandbox itself so the checkout lands in its overlay. (/tmp is the overlay's
/// tmp backend, where git's lockfile/realpath dance is fully supported; the
/// cow root currently trips git's realpath walk.) Returns git's stderr on
/// failure. The URL is restricted to a conservative character set (notably no
/// quotes or whitespace) so it embeds safely in the `sh -c` command line.
pub fn clone_repo(uid: [u8; 16], url: &str) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Ok(());
    }
    if !url
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "@:/._+~%#?=&-".contains(c))
    {
        return Err("repo url contains unsupported characters".into());
    }
    let opts = ExecOpts {
        allow_network: true,
        timeout: Some(std::time::Duration::from_secs(600)),
        ..ExecOpts::default()
    };
    // No prompts (fail fast on missing credentials); auto-accept unseen ssh
    // host keys so ssh clones work without a seeded known_hosts. Run on a
    // buffered PTY (like an interactive session): -q keeps output to errors.
    let cmd = format!(
        "cd /tmp && GIT_TERMINAL_PROMPT=0 \
         GIT_SSH_COMMAND='ssh -o StrictHostKeyChecking=accept-new' \
         exec git clone -q -- '{url}'"
    );
    let session = spawn_session(
        uid,
        LogLevel::Off,
        &shell_argv(&cmd),
        opts,
        PtyMode::Buffered,
    )
    .map_err(|e| format!("clone failed to start: {e:?}"))?;
    let code = session.wait();
    if code != 0 {
        let err = String::from_utf8_lossy(&session.read_stdout())
            .trim()
            .to_string();
        return Err(if err.is_empty() {
            format!("git clone exited with status {code}")
        } else {
            err
        });
    }
    Ok(())
}
