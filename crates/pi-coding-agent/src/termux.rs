use std::path::PathBuf;

/// Detects if running inside Termux on Android.
pub fn is_termux() -> bool {
    std::env::var("PREFIX")
        .map(|p| p.starts_with("/data/data/com.termux/files/usr"))
        .unwrap_or(false)
}

/// Returns standard tmp directory path for Termux or default temp_dir.
pub fn termux_tmpdir() -> PathBuf {
    if is_termux() {
        let prefix =
            std::env::var("PREFIX").unwrap_or_else(|_| "/data/data/com.termux/files/usr".into());
        PathBuf::from(prefix).join("tmp")
    } else {
        std::env::temp_dir()
    }
}

/// Returns shell path for Termux or system default.
pub fn termux_shell() -> PathBuf {
    if is_termux() {
        let prefix =
            std::env::var("PREFIX").unwrap_or_else(|_| "/data/data/com.termux/files/usr".into());
        PathBuf::from(prefix).join("bin").join("sh")
    } else if let Ok(shell) = std::env::var("SHELL") {
        PathBuf::from(shell)
    } else {
        PathBuf::from("sh")
    }
}
