//! Trust model slice for project root verification.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustDecision {
    Trusted,
    Untrusted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalProjectRoot {
    path: PathBuf,
}

impl CanonicalProjectRoot {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let canonical = path.as_ref().canonicalize()?;
        Ok(Self { path: canonical })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Evaluates project root trust for execution contexts.
///
/// Returns `TrustDecision::Untrusted` if the project root is missing or cannot be canonicalized.
/// Returns `TrustDecision::Unknown` for default/noninteractive use.
pub fn evaluate_trust(root_path: Option<&Path>, interactive: bool) -> TrustDecision {
    let Some(path) = root_path else {
        return TrustDecision::Untrusted;
    };

    if CanonicalProjectRoot::new(path).is_err() {
        return TrustDecision::Untrusted;
    }

    if !interactive {
        return TrustDecision::Unknown;
    }

    // Default interactive posture before explicit policy override
    TrustDecision::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pi-rs-trust-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_canonical_project_root_valid() {
        let dir = temp_dir("valid");
        let root = CanonicalProjectRoot::new(&dir).expect("should canonicalize existing dir");
        assert!(root.path().is_absolute());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_canonical_project_root_nonexistent() {
        let nonexistent = std::env::temp_dir().join("pi-rs-nonexistent-root-12345");
        assert!(CanonicalProjectRoot::new(&nonexistent).is_err());
    }

    #[test]
    fn test_evaluate_trust_noninteractive_returns_unknown() {
        let dir = temp_dir("noninteractive");
        let decision = evaluate_trust(Some(&dir), false);
        assert_eq!(decision, TrustDecision::Unknown);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_evaluate_trust_unresolvable_returns_untrusted() {
        let nonexistent = std::env::temp_dir().join("pi-rs-unresolvable-root-12345");
        let decision = evaluate_trust(Some(&nonexistent), false);
        assert_eq!(decision, TrustDecision::Untrusted);

        let decision_interactive = evaluate_trust(Some(&nonexistent), true);
        assert_eq!(decision_interactive, TrustDecision::Untrusted);
    }

    #[test]
    fn test_evaluate_trust_none_returns_untrusted() {
        assert_eq!(evaluate_trust(None, false), TrustDecision::Untrusted);
        assert_eq!(evaluate_trust(None, true), TrustDecision::Untrusted);
    }
}
