use std::fs;
use std::path::PathBuf;

#[path = "../src/trust.rs"]
mod trust;

#[path = "../src/project.rs"]
mod project;

use trust::{resolve_trust, CanonicalProjectRoot, TrustDecision, TrustStore};

fn tempfile_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("pi-rs-u4-{label}-{}-{}", std::process::id(), nanos));
    fs::create_dir_all(&dir).expect("create temporary fixture directory");
    dir
}

#[test]
fn test_persisted_trust_workflow_and_symlink_aliases() {
    let cfg_dir = tempfile_dir("cfg");
    let proj_dir = tempfile_dir("proj");

    let root_path = &proj_dir;
    let root = CanonicalProjectRoot::new(root_path).expect("canonical root");

    // 1. Initial resolution without opt-in returns Unknown
    let (decision, res_root) =
        resolve_trust(Some(root_path), &cfg_dir, false).expect("resolve trust");
    assert_eq!(decision, TrustDecision::Unknown);
    assert_eq!(res_root.as_ref(), Some(&root));

    // 2. Explicit opt-in persists trust
    let (decision_optin, _) =
        resolve_trust(Some(root_path), &cfg_dir, true).expect("resolve trust opt-in");
    assert_eq!(decision_optin, TrustDecision::Trusted);

    // 3. Next resolution without opt-in uses store and returns Trusted
    let (decision_persisted, _) =
        resolve_trust(Some(root_path), &cfg_dir, false).expect("resolve trust persisted");
    assert_eq!(decision_persisted, TrustDecision::Trusted);

    // 4. Verify symlink alias to project root resolves to same trusted identity
    let symlink_dir = tempfile_dir("symlink-parent");
    let alias_path = symlink_dir.join("alias_proj");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root_path, &alias_path).expect("create symlink alias");
        let (decision_alias, alias_root) =
            resolve_trust(Some(&alias_path), &cfg_dir, false).expect("resolve alias trust");
        assert_eq!(decision_alias, TrustDecision::Trusted);
        assert_eq!(alias_root.as_ref(), Some(&root));
    }

    let _ = fs::remove_dir_all(&cfg_dir);
    let _ = fs::remove_dir_all(&proj_dir);
    let _ = fs::remove_dir_all(&symlink_dir);
}

#[test]
fn test_process_level_pi_trust_project_and_isolated_persistence() {
    let cfg_dir = tempfile_dir("pi-trust-cfg");
    let proj_dir = tempfile_dir("pi-trust-proj");

    let root = CanonicalProjectRoot::new(&proj_dir).expect("canonical root");

    // Explicit opt-in simulating PI_TRUST_PROJECT=1 process env
    let (decision, res_root) =
        resolve_trust(Some(&proj_dir), &cfg_dir, true).expect("resolve trust");
    assert_eq!(decision, TrustDecision::Trusted);
    assert_eq!(res_root.as_ref(), Some(&root));

    // Check store file exists and has private permissions on Unix
    let store_path = TrustStore::store_path(&cfg_dir);
    assert!(store_path.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&store_path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "trust store file must have 0o600 permissions"
        );
    }

    // Verify stored content deserializes with version 1
    let store = TrustStore::load(&cfg_dir);
    assert_eq!(store.version, 1);
    assert!(store.is_trusted(&root));

    let _ = fs::remove_dir_all(&cfg_dir);
    let _ = fs::remove_dir_all(&proj_dir);
}

#[test]
fn test_fail_closed_malformed_unrelated_symlink() {
    let cfg_dir = tempfile_dir("failclosed-cfg");
    let proj_dir = tempfile_dir("failclosed-proj");
    let store_path = TrustStore::store_path(&cfg_dir);

    let root = CanonicalProjectRoot::new(&proj_dir).expect("canonical root");

    // 1. Malformed JSON -> load returns default, resolve returns Unknown
    fs::write(&store_path, "{ malformed json }").expect("write malformed");
    let (decision, _) = resolve_trust(Some(&proj_dir), &cfg_dir, false).expect("resolve");
    assert_eq!(decision, TrustDecision::Unknown);

    // 2. Unsupported schema version -> fail closed
    fs::write(&store_path, r#"{"version": 99, "trusted_roots": []}"#).expect("write bad ver");
    let (decision_v, _) = resolve_trust(Some(&proj_dir), &cfg_dir, false).expect("resolve");
    assert_eq!(decision_v, TrustDecision::Unknown);

    // 3. Trust store path is a symlink -> save fails with error, load fails closed
    let target_file = cfg_dir.join("other.json");
    fs::write(&target_file, "{}").expect("write target");
    let _ = fs::remove_file(&store_path);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target_file, &store_path).expect("create symlink store");
        let store_default = TrustStore::load(&cfg_dir);
        assert!(!store_default.is_trusted(&root));

        let mut s = TrustStore::default();
        s.add_trusted(&root);
        assert!(
            s.save(&cfg_dir).is_err(),
            "saving to symlink store path must fail"
        );
    }

    // 4. Non-directory project root fails canonicalization
    let file_proj = proj_dir.join("not_a_dir.txt");
    fs::write(&file_proj, "content").expect("write file");
    let res = CanonicalProjectRoot::new(&file_proj);
    assert!(res.is_err());
    let (decision_file, root_file) =
        resolve_trust(Some(&file_proj), &cfg_dir, false).expect("resolve");
    assert_eq!(decision_file, TrustDecision::Untrusted);
    assert!(root_file.is_none());

    let _ = fs::remove_dir_all(&cfg_dir);
    let _ = fs::remove_dir_all(&proj_dir);
}

#[test]
fn test_symlink_safe_passive_ancestor_instruction_loader() {
    let parent_dir = tempfile_dir("parent");
    let child_dir = parent_dir.join("child");
    fs::create_dir_all(&child_dir).expect("create child dir");

    let external_dir = tempfile_dir("external");

    // Setup direct instruction files
    fs::write(parent_dir.join("AGENTS.md"), "parent agents instructions\n")
        .expect("write parent AGENTS");
    fs::write(child_dir.join("AGENTS.md"), "child agents instructions\n")
        .expect("write child AGENTS");

    // Setup symlink attempting to escape to external file
    let secret_external_file = external_dir.join("SECRET.md");
    fs::write(&secret_external_file, "SENSITIVE EXTERNAL CONTENT\n").expect("write secret");

    let escaped_symlink = child_dir.join("CLAUDE.md");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&secret_external_file, &escaped_symlink)
        .expect("create escaped symlink");

    // Setup symlinked sub-directory .pi pointing outside
    let escaped_pi_dir = child_dir.join(".pi");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&external_dir, &escaped_pi_dir)
        .expect("create escaped .pi dir symlink");

    let child_root = CanonicalProjectRoot::new(&child_dir).expect("child root");
    let prompt = project::load_project_prompt(&child_root);

    // Verify ordering: child AGENTS first, then parent AGENTS
    let child_pos = prompt
        .find("child agents instructions")
        .expect("find child content");
    let parent_pos = prompt
        .find("parent agents instructions")
        .expect("find parent content");
    assert!(
        child_pos < parent_pos,
        "child prompt must precede parent prompt"
    );

    // Verify symlink escaping file content is completely absent
    assert!(
        !prompt.contains("SENSITIVE EXTERNAL CONTENT"),
        "symlink escaped content must not be loaded"
    );

    let _ = fs::remove_dir_all(&parent_dir);
    let _ = fs::remove_dir_all(&external_dir);
}
