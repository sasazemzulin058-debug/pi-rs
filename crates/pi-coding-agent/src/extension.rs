//! Passive extension discovery, diagnostics, and trust containment for `pi-rs`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::trust::{CanonicalProjectRoot, TrustDecision};

/// Scope of extension origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtensionScope {
    Project,
    Global,
    Explicit,
}

/// Validated candidate for extension discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionCandidate {
    pub entry_path: PathBuf,
    pub canonical_entry_path: PathBuf,
    pub scope: ExtensionScope,
}

/// Stable diagnostic code for fail-closed logging or reporting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionDiagnostic {
    pub code: &'static str,
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for ExtensionDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}: {}",
            self.code,
            self.path.display(),
            self.message
        )
    }
}

/// Discovery result holding ordered candidates and fail-closed diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionDiscoveryResult {
    pub candidates: Vec<ExtensionCandidate>,
    pub diagnostics: Vec<ExtensionDiagnostic>,
}

/// Error type for explicit extension validation checks.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ExtensionError {
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

/// Legacy diagnostic inspector retained for diagnostic boundary checks.
#[derive(Debug, Default)]
pub struct ExtensionDiagnosticInspector;

impl ExtensionDiagnosticInspector {
    pub fn new() -> Self {
        Self
    }

    /// Check if explicit path is unsupported extension entry point.
    /// Sanitizes control characters for display.
    pub fn check_extension_path(&self, path: &Path) -> Result<(), ExtensionError> {
        let path_str = path.to_string_lossy();
        let is_candidate = path_str.ends_with(".ts")
            || path_str.ends_with(".js")
            || path.file_name().and_then(|n| n.to_str()) == Some("package.json");

        if is_candidate {
            let sanitized = PathBuf::from(
                path.to_string_lossy()
                    .chars()
                    .map(|c| if c.is_control() { '?' } else { c })
                    .collect::<String>(),
            );
            return Err(ExtensionError::UnsupportedExtension {
                path: sanitized,
                reason: "TypeScript/JavaScript dynamic extension runtime is deferred".to_string(),
            });
        }
        Ok(())
    }
}

/// Helper function to sanitize non-printable path display string for diagnostics.
fn sanitize_path(path: &Path) -> PathBuf {
    PathBuf::from(
        path.to_string_lossy()
            .chars()
            .map(|c| if c.is_control() { '?' } else { c })
            .collect::<String>(),
    )
}

/// Check if filename has valid extension suffix (.ts or .js)
fn is_supported_extension_file(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        ext == "ts" || ext == "js"
    } else {
        false
    }
}

/// Resolve manifest entries while preserving fail-closed diagnostics.
fn manifest_entries(
    path: &Path,
    content: &str,
    result: &mut ExtensionDiscoveryResult,
) -> Option<Option<Vec<String>>> {
    let value: Value = match serde_json::from_str(content) {
        Ok(value) => value,
        Err(err) => {
            result.diagnostics.push(ExtensionDiagnostic {
                code: "MALFORMED_MANIFEST",
                path: sanitize_path(path),
                message: format!("Failed to parse package.json: {}", err),
            });
            return None;
        }
    };
    let pi = match value.get("pi") {
        None => return Some(None),
        Some(value) if value.is_object() => value,
        Some(_) => {
            result.diagnostics.push(ExtensionDiagnostic {
                code: "INVALID_MANIFEST_FIELD",
                path: sanitize_path(path),
                message: "package.json pi must be an object".to_string(),
            });
            return None;
        }
    };
    let extensions = match pi.get("extensions") {
        None => return Some(None),
        Some(value) if value.is_array() => value,
        Some(value) if value.is_string() => value,
        Some(_) => {
            result.diagnostics.push(ExtensionDiagnostic {
                code: "INVALID_MANIFEST_FIELD",
                path: sanitize_path(path),
                message: "package.json pi.extensions must be string or array".to_string(),
            });
            return None;
        }
    };
    let values = if let Some(value) = extensions.as_str() {
        vec![value.to_string()]
    } else {
        let array = extensions.as_array().unwrap();
        if array.iter().any(|value| !value.is_string()) {
            result.diagnostics.push(ExtensionDiagnostic {
                code: "INVALID_MANIFEST_FIELD",
                path: sanitize_path(path),
                message: "package.json pi.extensions entries must be strings".to_string(),
            });
            return None;
        }
        array
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect()
    };
    Some(Some(values))
}

/// Resolve one trusted explicit file or package directory.
fn resolve_explicit_path(path: &Path, allowed_root: &Path, result: &mut ExtensionDiscoveryResult) {
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(err) => {
            result.diagnostics.push(ExtensionDiagnostic {
                code: "METADATA_FAILED",
                path: sanitize_path(path),
                message: format!("Failed to inspect explicit extension path: {}", err),
            });
            return;
        }
    };
    if metadata.file_type().is_symlink() {
        result.diagnostics.push(ExtensionDiagnostic {
            code: "SYMLINK_ESCAPE",
            path: sanitize_path(path),
            message: "Symlink explicit extension paths are rejected fail-closed".to_string(),
        });
        return;
    }
    let canonical = match path.canonicalize() {
        Ok(path) => path,
        Err(err) => {
            result.diagnostics.push(ExtensionDiagnostic {
                code: "CANONICALIZATION_FAILED",
                path: sanitize_path(path),
                message: format!("Failed to canonicalize explicit extension path: {}", err),
            });
            return;
        }
    };
    if !canonical.starts_with(allowed_root) {
        result.diagnostics.push(ExtensionDiagnostic {
            code: "SYMLINK_ESCAPE",
            path: sanitize_path(path),
            message: "Explicit extension path escapes allowed root".to_string(),
        });
        return;
    }
    if metadata.is_file() {
        add_candidate_if_within_root(path, ExtensionScope::Explicit, allowed_root, result);
    } else if metadata.is_dir() {
        discover_package_or_index_dir(&canonical, ExtensionScope::Explicit, allowed_root, result);
    } else {
        result.diagnostics.push(ExtensionDiagnostic {
            code: "NOT_A_FILE",
            path: sanitize_path(path),
            message: "Explicit extension path is not a regular file or directory".to_string(),
        });
    }
}

/// Discover extensions inside a root directory (e.g. `.pi/extensions` or `agent_dir/extensions`).
fn discover_directory_extensions(
    dir_path: &Path,
    scope: ExtensionScope,
    allowed_root: &Path,
    result: &mut ExtensionDiscoveryResult,
) {
    let read_dir = match fs::read_dir(dir_path) {
        Ok(read_dir) => read_dir,
        Err(err) => {
            result.diagnostics.push(ExtensionDiagnostic {
                code: "ROOT_READ_FAILED",
                path: sanitize_path(dir_path),
                message: format!("Failed to read extension directory: {}", err),
            });
            return;
        }
    };
    let mut entries = Vec::new();
    for entry in read_dir {
        match entry {
            Ok(entry) => entries.push(entry.path()),
            Err(err) => result.diagnostics.push(ExtensionDiagnostic {
                code: "ENTRY_READ_FAILED",
                path: sanitize_path(dir_path),
                message: format!("Failed to read extension directory entry: {}", err),
            }),
        }
    }
    entries.sort();

    for entry_path in entries {
        let file_type = match entry_path.symlink_metadata() {
            Ok(meta) => meta.file_type(),
            Err(err) => {
                result.diagnostics.push(ExtensionDiagnostic {
                    code: "METADATA_FAILED",
                    path: sanitize_path(&entry_path),
                    message: format!("Failed to inspect extension entry: {}", err),
                });
                continue;
            }
        };

        if file_type.is_symlink() {
            result.diagnostics.push(ExtensionDiagnostic {
                code: "SYMLINK_ESCAPE",
                path: sanitize_path(&entry_path),
                message: "Symlink extension entries are rejected fail-closed".to_string(),
            });
        } else if file_type.is_file() {
            if is_supported_extension_file(&entry_path) {
                add_candidate_if_within_root(&entry_path, scope, allowed_root, result);
            } else if entry_path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ts") || ext.eq_ignore_ascii_case("js"))
            {
                result.diagnostics.push(ExtensionDiagnostic {
                    code: "UNSUPPORTED_SUFFIX",
                    path: sanitize_path(&entry_path),
                    message: "Extension entry point must end with .ts or .js".to_string(),
                });
            }
        } else if file_type.is_dir() {
            discover_package_or_index_dir(&entry_path, scope, allowed_root, result);
        }
    }
}

/// Process a candidate subdirectory (one level depth maximum).
fn discover_package_or_index_dir(
    dir_path: &Path,
    scope: ExtensionScope,
    allowed_root: &Path,
    result: &mut ExtensionDiscoveryResult,
) {
    let pkg_json_path = dir_path.join("package.json");
    if pkg_json_path.is_file() {
        match fs::read_to_string(&pkg_json_path) {
            Ok(content) => match manifest_entries(&pkg_json_path, &content, result) {
                None => return,
                Some(Some(ext_paths)) => {
                    for rel_str in ext_paths {
                        let target_path = dir_path.join(&rel_str);
                        if !target_path.exists() {
                            result.diagnostics.push(ExtensionDiagnostic {
                                code: "MISSING_ENTRY",
                                path: sanitize_path(&target_path),
                                message: "Manifest-declared extension entry does not exist"
                                    .to_string(),
                            });
                            continue;
                        }
                        add_candidate_if_within_root(&target_path, scope, allowed_root, result);
                    }
                    return;
                }
                Some(None) => {}
            },
            Err(err) => {
                result.diagnostics.push(ExtensionDiagnostic {
                    code: "UNREADABLE_MANIFEST",
                    path: sanitize_path(&pkg_json_path),
                    message: format!("Failed to read package.json: {}", err),
                });
                return;
            }
        }
    }

    // Fallback: check index.ts then index.js
    let index_ts = dir_path.join("index.ts");
    if index_ts.is_file() {
        add_candidate_if_within_root(&index_ts, scope, allowed_root, result);
        return;
    }

    let index_js = dir_path.join("index.js");
    if index_js.is_file() {
        add_candidate_if_within_root(&index_js, scope, allowed_root, result);
    }
}

/// Validates entry path, canonicalizes it, checks containment in `allowed_root`, and adds candidate/diagnostic.
fn add_candidate_if_within_root(
    entry_path: &Path,
    scope: ExtensionScope,
    allowed_root: &Path,
    result: &mut ExtensionDiscoveryResult,
) {
    if !is_supported_extension_file(entry_path) {
        result.diagnostics.push(ExtensionDiagnostic {
            code: "UNSUPPORTED_SUFFIX",
            path: sanitize_path(entry_path),
            message: "Extension entry point must end with .ts or .js".to_string(),
        });
        return;
    }

    let canonical_entry = match entry_path.canonicalize() {
        Ok(c) => c,
        Err(err) => {
            result.diagnostics.push(ExtensionDiagnostic {
                code: "CANONICALIZATION_FAILED",
                path: sanitize_path(entry_path),
                message: format!("Failed to canonicalize entry path: {}", err),
            });
            return;
        }
    };

    if !canonical_entry.is_file() {
        result.diagnostics.push(ExtensionDiagnostic {
            code: "NOT_A_FILE",
            path: sanitize_path(entry_path),
            message: "Extension candidate is not a regular file".to_string(),
        });
        return;
    }

    if !canonical_entry.starts_with(allowed_root) {
        result.diagnostics.push(ExtensionDiagnostic {
            code: "SYMLINK_ESCAPE",
            path: sanitize_path(entry_path),
            message: format!(
                "Candidate target '{}' escapes allowed root '{}'",
                canonical_entry.display(),
                allowed_root.display()
            ),
        });
        return;
    }

    result.candidates.push(ExtensionCandidate {
        entry_path: entry_path.to_path_buf(),
        canonical_entry_path: canonical_entry,
        scope,
    });
}

/// Discover project extensions if `trust_context` is `(TrustDecision::Trusted, Some(root))`.
pub fn discover_project_extensions(
    project_dir: &Path,
    trust_context: &(TrustDecision, Option<CanonicalProjectRoot>),
) -> ExtensionDiscoveryResult {
    let mut result = ExtensionDiscoveryResult::default();

    let (decision, canonical_root_opt) = trust_context;
    if *decision != TrustDecision::Trusted {
        return result;
    }

    let canonical_root = match canonical_root_opt {
        Some(root) => root.path(),
        None => return result,
    };

    let canonical_project_dir = match project_dir.canonicalize() {
        Ok(path) => path,
        Err(err) => {
            result.diagnostics.push(ExtensionDiagnostic {
                code: "PROJECT_ROOT_CANONICALIZATION_FAILED",
                path: sanitize_path(project_dir),
                message: format!("Failed to canonicalize project directory: {}", err),
            });
            return result;
        }
    };
    if canonical_project_dir != canonical_root {
        result.diagnostics.push(ExtensionDiagnostic {
            code: "TRUST_ROOT_MISMATCH",
            path: sanitize_path(project_dir),
            message: "Project directory does not equal trusted project root".to_string(),
        });
        return result;
    }

    let project_ext_dir = canonical_project_dir.join(".pi").join("extensions");
    discover_directory_extensions(
        &project_ext_dir,
        ExtensionScope::Project,
        canonical_root,
        &mut result,
    );

    result
}

/// Discover global extensions inside agent configuration directory.
pub fn discover_global_extensions(agent_dir: &Path) -> ExtensionDiscoveryResult {
    let mut result = ExtensionDiscoveryResult::default();

    let canonical_agent_dir = match agent_dir.canonicalize() {
        Ok(c) => c,
        Err(err) => {
            result.diagnostics.push(ExtensionDiagnostic {
                code: "CANONICALIZATION_FAILED",
                path: sanitize_path(agent_dir),
                message: format!("Failed to canonicalize agent dir: {}", err),
            });
            return result;
        }
    };

    let global_ext_dir = canonical_agent_dir.join("extensions");
    discover_directory_extensions(
        &global_ext_dir,
        ExtensionScope::Global,
        &canonical_agent_dir,
        &mut result,
    );

    result
}

/// Primary entry point for passive extension discovery.
/// Evaluates project extensions (if trusted), global extensions, and explicit paths.
/// Deduplicates candidates by canonical entry path (first occurrence wins).
pub fn discover_extensions(
    project_dir: Option<&Path>,
    trust_context: &(TrustDecision, Option<CanonicalProjectRoot>),
    agent_dir: Option<&Path>,
    explicit_paths: &[PathBuf],
) -> ExtensionDiscoveryResult {
    let mut combined = ExtensionDiscoveryResult::default();

    if let Some(pdir) = project_dir {
        let proj_res = discover_project_extensions(pdir, trust_context);
        combined.candidates.extend(proj_res.candidates);
        combined.diagnostics.extend(proj_res.diagnostics);
    }

    if let Some(adir) = agent_dir {
        let glob_res = discover_global_extensions(adir);
        combined.candidates.extend(glob_res.candidates);
        combined.diagnostics.extend(glob_res.diagnostics);
    }

    // Explicit paths require trusted project root boundary check
    if !explicit_paths.is_empty() {
        let (decision, canonical_root_opt) = trust_context;
        if *decision == TrustDecision::Trusted {
            if let Some(root) = canonical_root_opt {
                for exp_path in explicit_paths {
                    resolve_explicit_path(exp_path, root.path(), &mut combined);
                }
            } else {
                for exp_path in explicit_paths {
                    combined.diagnostics.push(ExtensionDiagnostic {
                        code: "MISSING_TRUST_ROOT",
                        path: sanitize_path(exp_path),
                        message: "Explicit extension path rejected because trusted project root is missing".to_string(),
                    });
                }
            }
        } else {
            for exp_path in explicit_paths {
                combined.diagnostics.push(ExtensionDiagnostic {
                    code: "UNTRUSTED_EXPLICIT_PATH",
                    path: sanitize_path(exp_path),
                    message: "Explicit extension path rejected because project is untrusted"
                        .to_string(),
                });
            }
        }
    }

    // Deduplicate candidates by canonical_entry_path (first wins)
    let mut unique_candidates = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();
    for candidate in combined.candidates {
        if seen_paths.insert(candidate.canonical_entry_path.clone()) {
            unique_candidates.push(candidate);
        }
    }
    combined.candidates = unique_candidates;

    combined
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct AutoTempDir {
        path: PathBuf,
    }

    impl AutoTempDir {
        fn new() -> Self {
            let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
            let path =
                std::env::temp_dir().join(format!("pi_ext_test_{}_{}", std::process::id(), id));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for AutoTempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_extension_diagnostic_inspector_fail_closed() {
        let inspector = ExtensionDiagnosticInspector::new();

        let ts_ext = Path::new("/project/.pi/extensions/tool.ts");
        let res = inspector.check_extension_path(ts_ext);
        assert!(matches!(
            res,
            Err(ExtensionError::UnsupportedExtension { .. })
        ));

        let pkg = Path::new("/project/.pi/extensions/my-ext/package.json");
        assert!(matches!(
            inspector.check_extension_path(pkg),
            Err(ExtensionError::UnsupportedExtension { .. })
        ));

        let safe_file = Path::new("/project/src/main.rs");
        assert!(inspector.check_extension_path(safe_file).is_ok());
    }

    #[test]
    fn test_untrusted_project_returns_zero_candidates() {
        let temp = AutoTempDir::new();
        let proj_dir = temp.path();
        let ext_dir = proj_dir.join(".pi").join("extensions");
        fs::create_dir_all(&ext_dir).unwrap();
        fs::write(ext_dir.join("helper.ts"), "// code").unwrap();

        let canonical_root = CanonicalProjectRoot::new(proj_dir).unwrap();

        // Untrusted
        let res = discover_project_extensions(
            proj_dir,
            &(TrustDecision::Untrusted, Some(canonical_root.clone())),
        );
        assert!(res.candidates.is_empty());

        // Unknown
        let res_unknown = discover_project_extensions(
            proj_dir,
            &(TrustDecision::Unknown, Some(canonical_root.clone())),
        );
        assert!(res_unknown.candidates.is_empty());

        // Trusted
        let res_trusted =
            discover_project_extensions(proj_dir, &(TrustDecision::Trusted, Some(canonical_root)));
        assert_eq!(res_trusted.candidates.len(), 1);
        assert_eq!(
            res_trusted.candidates[0].entry_path,
            ext_dir.join("helper.ts")
        );
    }

    #[test]
    fn test_manifest_discovery_and_index_fallback() {
        let temp = AutoTempDir::new();
        let proj_dir = temp.path();
        let ext_dir = proj_dir.join(".pi").join("extensions");
        fs::create_dir_all(&ext_dir).unwrap();

        // Ext 1: package.json with pi.extensions
        let ext1_dir = ext_dir.join("ext-one");
        fs::create_dir_all(&ext1_dir).unwrap();
        fs::write(
            ext1_dir.join("package.json"),
            r#"{"pi": {"extensions": ["src/custom.ts"]}}"#,
        )
        .unwrap();
        let ext1_src = ext1_dir.join("src");
        fs::create_dir_all(&ext1_src).unwrap();
        fs::write(ext1_src.join("custom.ts"), "// code").unwrap();

        // Ext 2: directory with index.ts (and index.js - index.ts wins)
        let ext2_dir = ext_dir.join("ext-two");
        fs::create_dir_all(&ext2_dir).unwrap();
        fs::write(ext2_dir.join("index.ts"), "// ts").unwrap();
        fs::write(ext2_dir.join("index.js"), "// js").unwrap();

        // Direct file
        fs::write(ext_dir.join("alpha.js"), "// alpha").unwrap();

        let canonical_root = CanonicalProjectRoot::new(proj_dir).unwrap();
        let res =
            discover_project_extensions(proj_dir, &(TrustDecision::Trusted, Some(canonical_root)));

        assert_eq!(res.candidates.len(), 3);
        // Sorted lexically: alpha.js, ext-one/src/custom.ts, ext-two/index.ts
        assert_eq!(res.candidates[0].entry_path, ext_dir.join("alpha.js"));
        assert_eq!(
            res.candidates[1].entry_path,
            ext1_dir.join("src").join("custom.ts")
        );
        assert_eq!(res.candidates[2].entry_path, ext2_dir.join("index.ts"));
    }

    #[test]
    fn test_symlink_escape_fails_closed() {
        let temp = AutoTempDir::new();
        let proj_dir = temp.path().join("proj");
        let outside_dir = temp.path().join("outside");
        fs::create_dir_all(&proj_dir).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();

        let ext_dir = proj_dir.join(".pi").join("extensions");
        fs::create_dir_all(&ext_dir).unwrap();

        let outside_file = outside_dir.join("evil.ts");
        fs::write(&outside_file, "// evil").unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside_file, ext_dir.join("sym.ts")).unwrap();

            let canonical_root = CanonicalProjectRoot::new(&proj_dir).unwrap();
            let res = discover_project_extensions(
                &proj_dir,
                &(TrustDecision::Trusted, Some(canonical_root)),
            );

            assert!(res.candidates.is_empty());
            assert_eq!(res.diagnostics.len(), 1);
            assert_eq!(res.diagnostics[0].code, "SYMLINK_ESCAPE");
        }
    }

    #[test]
    fn test_unsupported_suffix_and_non_deep_recursion() {
        let temp = AutoTempDir::new();
        let proj_dir = temp.path();
        let ext_dir = proj_dir.join(".pi").join("extensions");
        fs::create_dir_all(&ext_dir).unwrap();

        // Unsupported suffix foo.ts.txt
        fs::write(ext_dir.join("foo.ts.txt"), "// text").unwrap();

        // Subdirectory with deep file without manifest/index
        let sub = ext_dir.join("sub");
        let deep = sub.join("nested");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("deep.ts"), "// deep").unwrap();

        let canonical_root = CanonicalProjectRoot::new(proj_dir).unwrap();
        let res =
            discover_project_extensions(proj_dir, &(TrustDecision::Trusted, Some(canonical_root)));

        assert!(res.candidates.is_empty());
    }

    #[test]
    fn test_trusted_root_mismatch_rejected_with_stable_diagnostic() {
        let temp = AutoTempDir::new();
        let trusted = temp.path().join("trusted");
        let supplied = temp.path().join("supplied");
        fs::create_dir_all(supplied.join(".pi/extensions")).unwrap();
        fs::create_dir_all(&trusted).unwrap();
        fs::write(supplied.join(".pi/extensions/tool.ts"), "// tool").unwrap();
        let root = CanonicalProjectRoot::new(&trusted).unwrap();
        let result = discover_project_extensions(&supplied, &(TrustDecision::Trusted, Some(root)));
        assert!(result.candidates.is_empty());
        assert_eq!(result.diagnostics[0].code, "TRUST_ROOT_MISMATCH");
    }

    #[test]
    fn test_missing_roots_and_directory_symlink_report_diagnostics() {
        let temp = AutoTempDir::new();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let root = CanonicalProjectRoot::new(&project).unwrap();
        let missing = discover_project_extensions(&project, &(TrustDecision::Trusted, Some(root)));
        assert_eq!(missing.diagnostics[0].code, "ROOT_READ_FAILED");

        #[cfg(unix)]
        {
            let extensions = project.join(".pi/extensions");
            fs::create_dir_all(&extensions).unwrap();
            let outside = temp.path().join("outside");
            fs::create_dir_all(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, extensions.join("linked")).unwrap();
            let root = CanonicalProjectRoot::new(&project).unwrap();
            let result =
                discover_project_extensions(&project, &(TrustDecision::Trusted, Some(root)));
            assert!(result
                .diagnostics
                .iter()
                .any(|d| d.code == "SYMLINK_ESCAPE"));
        }
    }

    #[test]
    fn test_extension_json_is_not_legacy_heuristic() {
        let inspector = ExtensionDiagnosticInspector::new();
        assert!(inspector
            .check_extension_path(Path::new("/project/.pi/extensions/extension.json"))
            .is_ok());
    }

    #[test]
    fn test_manifest_rejection_diagnostics() {
        let temp = AutoTempDir::new();
        let root = temp.path();
        let ext = root.join(".pi/extensions");
        fs::create_dir_all(&ext).unwrap();
        for (name, content, _code) in [
            ("bad", "{", "MALFORMED_MANIFEST"),
            (
                "shape",
                r#"{"pi":{"extensions":42}}"#,
                "INVALID_MANIFEST_FIELD",
            ),
            (
                "missing",
                r#"{"pi":{"extensions":["nope.ts"]}}"#,
                "MISSING_ENTRY",
            ),
        ] {
            let dir = ext.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("package.json"), content).unwrap();
        }
        let trust = (
            TrustDecision::Trusted,
            Some(CanonicalProjectRoot::new(root).unwrap()),
        );
        let result = discover_project_extensions(root, &trust);
        assert!(result.candidates.is_empty());
        for code in [
            "MALFORMED_MANIFEST",
            "INVALID_MANIFEST_FIELD",
            "MISSING_ENTRY",
        ] {
            assert!(result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code));
        }
    }

    #[test]
    fn test_uppercase_suffix_rejected() {
        let temp = AutoTempDir::new();
        let ext = temp.path().join(".pi/extensions");
        fs::create_dir_all(&ext).unwrap();
        fs::write(ext.join("UPPER.TS"), "// code").unwrap();
        fs::write(ext.join("UPPER.JS"), "// code").unwrap();
        let trust = (
            TrustDecision::Trusted,
            Some(CanonicalProjectRoot::new(temp.path()).unwrap()),
        );
        let result = discover_project_extensions(temp.path(), &trust);
        assert!(result.candidates.is_empty());
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|d| d.code == "UNSUPPORTED_SUFFIX")
                .count(),
            2
        );
    }

    #[test]
    fn test_explicit_file_package_and_missing_root() {
        let temp = AutoTempDir::new();
        let root = temp.path();
        let file = root.join("tool.ts");
        fs::write(&file, "// code").unwrap();
        let package = root.join("pkg");
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("package.json"),
            r#"{"pi":{"extensions":"main.js"}}"#,
        )
        .unwrap();
        fs::write(package.join("main.js"), "// code").unwrap();
        let trust = (
            TrustDecision::Trusted,
            Some(CanonicalProjectRoot::new(root).unwrap()),
        );
        let result = discover_extensions(None, &trust, None, &[file.clone(), package]);
        assert_eq!(result.candidates.len(), 2);
        assert!(result
            .candidates
            .iter()
            .all(|candidate| candidate.scope == ExtensionScope::Explicit));
        let missing = discover_extensions(None, &(TrustDecision::Trusted, None), None, &[file]);
        assert!(missing.candidates.is_empty());
        assert_eq!(missing.diagnostics[0].code, "MISSING_TRUST_ROOT");
    }

    #[test]
    fn test_global_ordering_and_symlink_containment() {
        let temp = AutoTempDir::new();
        let agent_dir = temp.path().join("agent");
        let extensions = agent_dir.join("extensions");
        fs::create_dir_all(&extensions).unwrap();
        fs::write(extensions.join("z-last.ts"), "// z").unwrap();
        fs::write(extensions.join("a-first.js"), "// a").unwrap();

        #[cfg(unix)]
        {
            let package = extensions.join("escape-package");
            fs::create_dir_all(&package).unwrap();
            fs::write(
                package.join("package.json"),
                r#"{"pi":{"extensions":["escape.ts"]}}"#,
            )
            .unwrap();
            let outside = temp.path().join("outside.ts");
            fs::write(&outside, "// outside").unwrap();
            std::os::unix::fs::symlink(&outside, package.join("escape.ts")).unwrap();
        }

        let result = discover_global_extensions(&agent_dir);
        assert_eq!(
            result
                .candidates
                .iter()
                .map(|candidate| candidate.entry_path.file_name().unwrap())
                .collect::<Vec<_>>(),
            vec![
                std::ffi::OsStr::new("a-first.js"),
                std::ffi::OsStr::new("z-last.ts")
            ]
        );
        #[cfg(unix)]
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "SYMLINK_ESCAPE"));
    }

    #[test]
    fn test_deduplication_first_wins() {
        let temp = AutoTempDir::new();
        let proj_dir = temp.path();
        let ext_dir = proj_dir.join(".pi").join("extensions");
        fs::create_dir_all(&ext_dir).unwrap();

        let main_ts = ext_dir.join("main.ts");
        fs::write(&main_ts, "// main").unwrap();

        let canonical_root = CanonicalProjectRoot::new(proj_dir).unwrap();
        let trust_ctx = (TrustDecision::Trusted, Some(canonical_root));

        let res = discover_extensions(
            Some(proj_dir),
            &trust_ctx,
            None,
            std::slice::from_ref(&main_ts), // explicit duplicate of project extension
        );

        assert_eq!(res.candidates.len(), 1);
        assert_eq!(res.candidates[0].scope, ExtensionScope::Project);
    }
}
