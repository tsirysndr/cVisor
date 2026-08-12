//! End-to-end passthrough tests: run a real command in the sandbox and confirm
//! captured output. Linux-only (needs seccomp + fork); runs in the Alpine
//! container via the cargo docker runner.

#![cfg(target_os = "linux")]

use std::sync::{Arc, Mutex};

use bvisor_core::{execute, generate_uid, LogBuffer, LogLevel};

// Each sandbox forks; forking concurrently from many harness threads is fragile
// (fork + threads). Serialize the fork+supervise section so the suite is safe
// under the default parallel test runner. Recover from a poisoned lock so one
// failing test doesn't cascade.
static SERIAL: Mutex<()> = Mutex::new(());

fn run(cmd: &str) -> (String, String) {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let stdout = Arc::new(LogBuffer::new());
    let stderr = Arc::new(LogBuffer::new());
    execute(
        generate_uid(),
        LogLevel::Off,
        cmd,
        Arc::clone(&stdout),
        Arc::clone(&stderr),
    )
    .expect("execute failed");
    (
        String::from_utf8_lossy(&stdout.read()).into_owned(),
        String::from_utf8_lossy(&stderr.read()).into_owned(),
    )
}

#[test]
fn echo_hello_captured_on_stdout() {
    let (out, _err) = run("echo hello world");
    assert_eq!(out, "hello world\n");
}

#[test]
fn multiple_writes_accumulate() {
    let (out, _err) = run("echo a; echo b; echo c");
    assert_eq!(out, "a\nb\nc\n");
}

#[test]
fn buffered_stdio_writev_is_captured() {
    // `printf` flushes buffered stdout via writev(1, ...); M3's writev handler
    // gathers and captures it.
    let (out, _err) = run("printf 'a'; printf 'b'; printf 'c'");
    assert_eq!(out, "abc");
}

#[test]
fn real_stderr_write_is_captured() {
    let (out, err) = run("ls /definitely-missing-xyzzy");
    assert_eq!(out, "");
    assert!(!err.is_empty(), "expected an error on stderr, got empty");
}

#[test]
fn many_subprocesses_fork_and_exit() {
    // A sequence of subshells each fork a child that exits (exit_group),
    // exercising lazy /proc child discovery and per-group exit pruning under
    // real seccomp. The parent shell's final write to real stdout must survive
    // all the child churn.
    let (out, _err) = run("(true); (false); (exit 3); echo survived");
    assert_eq!(out, "survived\n");
}

#[test]
fn pipeline_filters_correctly() {
    // With pipe2 + dup2 tracking, a shell pipeline connects child stdout to the
    // next child's stdin instead of leaking into the captured buffer, so grep
    // actually filters.
    let (out, _err) = run("printf 'alpha\\nbeta\\ngamma\\n' | grep p");
    assert_eq!(out, "alpha\n");
}

#[test]
fn redirect_to_tmp_file_then_read_back() {
    // `echo > /tmp/f` dup2's the opened file onto fd 1 in the child only; the
    // bytes go to the tmp overlay file, not the captured stdout. Reading it back
    // in the same sandbox returns the content, proving the redirect landed in
    // the file rather than leaking to the captured stdout.
    let (out, _err) = run("echo redirected > /tmp/redir.txt; grep redirected /tmp/redir.txt");
    assert_eq!(out, "redirected\n");
}

#[test]
fn subshell_exit_is_handled() {
    // A subshell exits (exit_group) while the parent shell continues; the
    // supervisor must prune the child's virtual thread and keep serving the
    // parent's later writes.
    let (out, _err) = run("(exit 0); echo after-subshell");
    assert_eq!(out, "after-subshell\n");
}

#[test]
fn blocked_syscall_fails_in_sandbox() {
    // `chroot` is blocked (ENOSYS); busybox chroot reports the failure on
    // stderr and the shell reports non-empty output rather than succeeding.
    let (out, err) = run("chroot /tmp /bin/true 2>&1; echo done");
    assert!(
        out.contains("done"),
        "command should have run: {out:?} {err:?}"
    );
    // chroot must not have succeeded silently — some error text is present.
    assert!(out.len() > "done\n".len() || !err.is_empty());
}

#[test]
fn uname_reports_virtual_nodename() {
    // uname's nodename is virtualized to "bvisor".
    let (out, _err) = run("uname -n");
    assert_eq!(out, "bvisor\n");
}

#[test]
fn mkdir_then_list_in_tmp() {
    // mkdirat creates a dir in the tmp overlay; getdents64 lists it; the created
    // file shows up via the merged listing.
    let (out, _err) = run("mkdir /tmp/d && echo hi > /tmp/d/f && ls /tmp/d");
    assert_eq!(out, "f\n");
}

#[test]
fn stat_and_test_existence_in_tmp() {
    // `test -f` uses newfstatat/faccessat; a created file must be seen, a
    // missing one must not.
    let (out, _err) = run("echo x > /tmp/exists.txt; \
         if [ -f /tmp/exists.txt ]; then echo yes; else echo no; fi; \
         if [ -e /tmp/missing.txt ]; then echo yes2; else echo no2; fi");
    assert_eq!(out, "yes\nno2\n");
}

#[test]
fn rm_tombstones_file_in_tmp() {
    // unlinkat removes a tmp file; a subsequent existence check must fail.
    let (out, _err) = run("echo x > /tmp/gone.txt; rm /tmp/gone.txt; \
         if [ -e /tmp/gone.txt ]; then echo present; else echo removed; fi");
    assert_eq!(out, "removed\n");
}

#[test]
fn ls_root_shows_real_entries() {
    // getdents64 merged listing of a cow (real) directory: /bin should contain
    // sh among its entries.
    let (out, _err) = run("ls /bin | grep -x sh");
    assert_eq!(out, "sh\n");
}

#[test]
fn symlink_in_tmp_readlink_returns_target() {
    // symlinkat creates a link in the tmp overlay; readlinkat returns the stored
    // target verbatim. (Following an overlay symlink whose target is a guest
    // path is a separate, deferred concern — see the overlay-symlink note.)
    let (out, _err) = run("ln -s /tmp/target.txt /tmp/link.txt; readlink /tmp/link.txt");
    assert_eq!(out, "/tmp/target.txt\n");
}

#[test]
fn cd_changes_working_directory() {
    // `cd` (chdir) followed by pwd (getcwd) reflects the new directory; the
    // relative path resolves against it.
    // grep (read-based) rather than cat (which uses zero-copy sendfile that
    // bypasses stdout capture — a separate deferred concern).
    let (out, _err) = run("cd /tmp && mkdir sub && echo hi > sub/f && cd sub && grep hi f && pwd");
    assert_eq!(out, "hi\n/tmp/sub\n");
}

#[test]
fn chmod_and_touch_in_tmp() {
    // touch creates a file (utimensat/openat), chmod changes its mode
    // (fchmodat), and the executable bit is then observable via `test -x`.
    let (out, _err) = run("touch /tmp/script.sh; chmod +x /tmp/script.sh; \
         if [ -x /tmp/script.sh ]; then echo executable; else echo not; fi");
    assert_eq!(out, "executable\n");
}

#[test]
fn kill_signals_a_child() {
    // Background a sleeper, then kill it; the shell reports the job was killed.
    // Exercises kill with namespaced-pid translation and the process model.
    let (out, _err) = run("sleep 30 & pid=$!; kill $pid; wait $pid 2>/dev/null; echo killed");
    assert_eq!(out, "killed\n");
}

#[test]
fn proc_self_status_is_virtualized() {
    // The synthetic /proc/self/status reports the fixed guest name; reading it
    // exercises the proc backend (openat → read of generated content).
    let (out, _err) = run("grep Name /proc/self/status");
    assert_eq!(out, "Name:\tbvisor-guest\n");
}

#[test]
fn proc_lists_self_entry() {
    // getdents64 on /proc synthesizes a `self` entry among the pids.
    let (out, _err) = run("ls /proc | grep -x self");
    assert_eq!(out, "self\n");
}

#[test]
fn cow_read_through_reads_real_file() {
    // `grep` opens a real file (routed to the cow backend, read-through since no
    // copy exists) and reads it via read() — exercising the cow read path and
    // the read handler (as opposed to `cat`, which uses zero-copy sendfile that
    // bypasses interception; see the doc note on unhandled copy syscalls).
    std::fs::write("/etc/bvisor_cow_probe", b"cow-content\n").expect("seed file");
    let (out, _err) = run("grep cow /etc/bvisor_cow_probe");
    let _ = std::fs::remove_file("/etc/bvisor_cow_probe");
    assert_eq!(out, "cow-content\n");
}
