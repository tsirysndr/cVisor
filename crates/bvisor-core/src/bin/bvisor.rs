//! Standalone sandbox runner. Forks a guest, runs a command inside the sandbox,
//! and prints the captured stdout/stderr. With no argument it runs `/smoke`
//! (the in-sandbox scorecard binary), so `cargo xtask run` exercises the whole
//! stack end to end.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("bvisor runs on Linux only");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    use bvisor_core::{execute, generate_uid, LogBuffer, LogLevel};
    use std::sync::Arc;

    let cmd = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/smoke".to_string());

    let stdout = Arc::new(LogBuffer::new());
    let stderr = Arc::new(LogBuffer::new());
    let code = match execute(
        generate_uid(),
        LogLevel::Off,
        &cmd,
        Arc::clone(&stdout),
        Arc::clone(&stderr),
    ) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bvisor: execute failed: {e}");
            1
        }
    };

    use std::io::Write;
    let _ = std::io::stdout().write_all(&stdout.read());
    let _ = std::io::stderr().write_all(&stderr.read());
    std::process::exit(code);
}
