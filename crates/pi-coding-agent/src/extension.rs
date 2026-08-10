//! Extension support policy, fail-closed diagnostics, and metadata checks for `pi-rs`.

use std::fmt;
use std::path::{Path, PathBuf};

/// Error types related to extension load attempts.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ExtensionError {
    /// Extensions requesting runtime dynamic loading (TS/JS/Node) are explicitly unsupported.
    UnsupportedExtension { path: PathBuf, reason: String },
}

impl fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedExtension { path, reason } => {
                write!(
                    f,
                    "Unsupported extension at '{}': {}. Dynamic extension execution is disabled.",
                    path.display(),
                    reason
                )
            }
        }
    }
}

impl std::error::Error for ExtensionError {}

/// Diagnostic inspector for extension entrypoints.
/// Fail-closed: Returns `UnsupportedExtension` error when dynamic extension paths/manifests are encountered.
#[derive(Debug, Default)]
pub struct ExtensionDiagnostic;

impl ExtensionDiagnostic {
    pub fn new() -> Self {
        Self
    }

    /// Check if an explicit caller-provided extension path/manifest is unsupported, failing closed.
    /// Sanitizes control/non-printable characters from path display in diagnostics.
    pub fn check_extension_path(&self, path: &Path) -> Result<(), ExtensionError> {
        let path_str = path.to_string_lossy();

        let is_extension_candidate = path_str.ends_with(".ts")
            || path_str.ends_with(".js")
            || path_str.contains(".pi/extensions")
            || path.file_name().and_then(|n| n.to_str()) == Some("extension.json");

        if is_extension_candidate {
            let sanitized_path = PathBuf::from(
                path.to_string_lossy()
                    .chars()
                    .map(|c| if c.is_control() { '?' } else { c })
                    .collect::<String>(),
            );
            return Err(ExtensionError::UnsupportedExtension {
                path: sanitized_path,
                reason: "TypeScript/JavaScript runtime extension execution is deferred in pi-rs"
                    .to_string(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_diagnostic_fail_closed() {
        let inspector = ExtensionDiagnostic::new();

        let ts_ext = Path::new("/project/.pi/extensions/tool.ts");
        let res = inspector.check_extension_path(ts_ext);
        assert!(matches!(
            res,
            Err(ExtensionError::UnsupportedExtension { .. })
        ));
        if let Err(err) = res {
            assert!(err.to_string().contains("TypeScript/JavaScript runtime"));
        }

        let js_ext = Path::new("/project/extension.js");
        assert!(matches!(
            inspector.check_extension_path(js_ext),
            Err(ExtensionError::UnsupportedExtension { .. })
        ));

        let manifest = Path::new("/project/.pi/extensions/my-ext/extension.json");
        assert!(matches!(
            inspector.check_extension_path(manifest),
            Err(ExtensionError::UnsupportedExtension { .. })
        ));

        let safe_file = Path::new("/project/src/main.rs");
        assert!(inspector.check_extension_path(safe_file).is_ok());
    }

    #[test]
    fn test_extension_diagnostic_sanitizes_control_chars() {
        let inspector = ExtensionDiagnostic::new();
        let path_with_control = Path::new("/project/\n/tool.ts");
        let err = inspector
            .check_extension_path(path_with_control)
            .unwrap_err();
        assert!(!err.to_string().contains('\n'));
        assert!(err
            .to_string()
            .contains("Unsupported extension at '/project/?/tool.ts'"));
    }

    #[test]
    fn test_extension_diagnostic_boundary_paths() {
        let inspector = ExtensionDiagnostic::new();
        assert!(inspector.check_extension_path(Path::new("")).is_ok());
        assert!(inspector
            .check_extension_path(Path::new("not_an_extension.rs"))
            .is_ok());
        assert!(inspector
            .check_extension_path(Path::new(".pi/extensions"))
            .is_err());
    }
}
