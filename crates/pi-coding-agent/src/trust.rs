//! Trust model slice for project root verification and persistence.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustDecision {
    Trusted,
    Untrusted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalProjectRoot {
    path: PathBuf,
}

impl CanonicalProjectRoot {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let canonical = path.as_ref().canonicalize()?;
        if !canonical.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "project root is not a directory",
            ));
        }
        Ok(Self { path: canonical })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrustStore {
    pub version: u32,
    pub trusted_roots: Vec<PathBuf>,
}

impl TrustStore {
    pub fn store_path(config_dir: &Path) -> PathBuf {
        config_dir.join("trusted_projects.json")
    }

    pub fn load(config_dir: &Path) -> Self {
        let path = Self::store_path(config_dir);
        if let Ok(meta) = fs::symlink_metadata(&path) {
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Self::default();
            }
        } else {
            return Self::default();
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };

        let store: TrustStore = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };

        if store.version != 1 {
            return Self::default();
        }

        store
    }

    pub fn is_trusted(&self, root: &CanonicalProjectRoot) -> bool {
        self.trusted_roots.iter().any(|r| r == root.path())
    }

    pub fn add_trusted(&mut self, root: &CanonicalProjectRoot) -> bool {
        if !self.is_trusted(root) {
            self.trusted_roots.push(root.path().to_path_buf());
            true
        } else {
            false
        }
    }

    pub fn save(&self, config_dir: &Path) -> io::Result<()> {
        fs::create_dir_all(config_dir)?;

        let store_path = Self::store_path(config_dir);
        if let Ok(meta) = fs::symlink_metadata(&store_path) {
            if meta.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "trust store path is a symlink",
                ));
            }
        }

        let mut data = Self {
            version: 1,
            trusted_roots: self.trusted_roots.clone(),
        };
        data.trusted_roots.sort();
        data.trusted_roots.dedup();

        let json = serde_json::to_string_pretty(&data)?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp_path = config_dir.join(format!(".tmp_trust_{}_{}.json", std::process::id(), nanos));

        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        use std::io::Write;
        let write_res = (|| -> io::Result<()> {
            let mut file = options.open(&tmp_path)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
            Ok(())
        })();

        if let Err(e) = write_res {
            let _ = fs::remove_file(&tmp_path);
            return Err(e);
        }

        if let Err(e) = fs::rename(&tmp_path, &store_path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(e);
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&store_path, fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }
}

/// Evaluates project root trust for execution contexts, considering persisted trust and explicit opt-in.
pub fn resolve_trust(
    root_path: Option<&Path>,
    config_dir: &Path,
    explicit_opt_in: bool,
) -> Result<(TrustDecision, Option<CanonicalProjectRoot>), io::Error> {
    let Some(path) = root_path else {
        return Ok((TrustDecision::Untrusted, None));
    };

    let canonical_root = match CanonicalProjectRoot::new(path) {
        Ok(r) => r,
        Err(_) => return Ok((TrustDecision::Untrusted, None)),
    };

    let mut store = TrustStore::load(config_dir);

    if explicit_opt_in {
        store.add_trusted(&canonical_root);
        store.save(config_dir)?;
        return Ok((TrustDecision::Trusted, Some(canonical_root)));
    }

    if store.is_trusted(&canonical_root) {
        Ok((TrustDecision::Trusted, Some(canonical_root)))
    } else {
        Ok((TrustDecision::Unknown, Some(canonical_root)))
    }
}

/// Evaluates project root trust for execution contexts.
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
    fn test_trust_store_persistence_and_resolution() {
        let config_dir = temp_dir("cfg");
        let project_dir = temp_dir("proj");

        let root = CanonicalProjectRoot::new(&project_dir).unwrap();

        // 1. Initial resolution without opt-in returns Unknown
        let (decision, resolved_root) =
            resolve_trust(Some(&project_dir), &config_dir, false).unwrap();
        assert_eq!(decision, TrustDecision::Unknown);
        assert_eq!(resolved_root.unwrap(), root);

        // 2. Explicit opt-in persists trust and returns Trusted
        let (decision_optin, _) = resolve_trust(Some(&project_dir), &config_dir, true).unwrap();
        assert_eq!(decision_optin, TrustDecision::Trusted);

        // 3. Subsequent resolution without opt-in reads persisted trust and returns Trusted
        let (decision_persisted, _) =
            resolve_trust(Some(&project_dir), &config_dir, false).unwrap();
        assert_eq!(decision_persisted, TrustDecision::Trusted);

        let _ = fs::remove_dir_all(&config_dir);
        let _ = fs::remove_dir_all(&project_dir);
    }

    #[test]
    fn test_trust_store_rejects_symlink_store() {
        let config_dir = temp_dir("symlink-cfg");
        let project_dir = temp_dir("proj-symlink");
        let target_file = config_dir.join("real_store.json");
        fs::write(&target_file, "{}").unwrap();

        let symlink_store = TrustStore::store_path(&config_dir);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_file, &symlink_store).unwrap();

        let root = CanonicalProjectRoot::new(&project_dir).unwrap();
        let mut store = TrustStore::default();
        store.add_trusted(&root);

        #[cfg(unix)]
        assert!(store.save(&config_dir).is_err());

        let _ = fs::remove_dir_all(&config_dir);
        let _ = fs::remove_dir_all(&project_dir);
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
