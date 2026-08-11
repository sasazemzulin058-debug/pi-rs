use std::fs;
use std::path::PathBuf;

#[path = "../src/trust.rs"]
mod trust;

#[path = "../src/project.rs"]
mod project;

#[path = "../src/system_prompt.rs"]
mod system_prompt;

use system_prompt::resources::{
    ExtensionResources, PathMetadata, ResolvedResource, ResourceCatalog, ResourceKind,
    SourceOrigin, SourceScope,
};
use system_prompt::{build_system_prompt_with_sources, BASE_SYSTEM_PROMPT};
use trust::{CanonicalProjectRoot, TrustDecision};

fn tempfile_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "pi-rs-u4-rl-{label}-{}-{}",
        std::process::id(),
        nanos
    ));
    fs::create_dir_all(&dir).expect("create temporary fixture directory");
    dir
}

struct TempDirCleaner(PathBuf);
impl Drop for TempDirCleaner {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn system_prompt_sources() {
    let cfg_dir = tempfile_dir("sys-sources-cfg");
    let _clean_cfg = TempDirCleaner(cfg_dir.clone());
    let proj_dir = tempfile_dir("sys-sources-proj");
    let _clean_proj = TempDirCleaner(proj_dir.clone());

    let proj_root = CanonicalProjectRoot::new(&proj_dir).unwrap();

    // 1. Built-in prompt reports no source
    let res_builtin = build_system_prompt_with_sources(&cfg_dir, &(TrustDecision::Untrusted, None));
    assert_eq!(res_builtin.prompt, BASE_SYSTEM_PROMPT);
    assert_eq!(res_builtin.system_prompt_source, None);
    assert!(res_builtin.append_system_prompt_sources.is_empty());

    // 2. Global SYSTEM.md used when untrusted or no project file
    fs::write(cfg_dir.join("SYSTEM.md"), "global sys prompt").unwrap();
    fs::write(cfg_dir.join("APPEND_SYSTEM.md"), "global append prompt").unwrap();

    let res_untrusted = build_system_prompt_with_sources(
        &cfg_dir,
        &(TrustDecision::Untrusted, Some(proj_root.clone())),
    );
    assert!(res_untrusted.prompt.starts_with("global sys prompt"));
    assert_eq!(
        res_untrusted.system_prompt_source,
        Some(cfg_dir.join("SYSTEM.md"))
    );
    assert_eq!(
        res_untrusted.append_system_prompt_sources,
        vec![cfg_dir.join("APPEND_SYSTEM.md")]
    );

    // 3. Trusted project SYSTEM.md and APPEND_SYSTEM.md override global
    let pi_dir = proj_dir.join(".pi");
    fs::create_dir_all(&pi_dir).unwrap();
    fs::write(pi_dir.join("SYSTEM.md"), "proj sys prompt").unwrap();
    fs::write(pi_dir.join("APPEND_SYSTEM.md"), "proj append prompt").unwrap();
    fs::write(proj_dir.join("AGENTS.md"), "project context instructions").unwrap();

    let res_trusted =
        build_system_prompt_with_sources(&cfg_dir, &(TrustDecision::Trusted, Some(proj_root)));
    assert!(res_trusted.prompt.starts_with("proj sys prompt"));
    assert_eq!(
        res_trusted.system_prompt_source,
        Some(pi_dir.join("SYSTEM.md"))
    );
    assert_eq!(
        res_trusted.append_system_prompt_sources,
        vec![pi_dir.join("APPEND_SYSTEM.md")]
    );

    // Project context precedes append content
    let proj_pos = res_trusted
        .prompt
        .find("project context instructions")
        .unwrap();
    let app_pos = res_trusted.prompt.find("proj append prompt").unwrap();
    assert!(
        proj_pos < app_pos,
        "project context must precede append content"
    );

    // Directory or invalid UTF-8 inputs report no false source
    let bad_cfg = tempfile_dir("bad-cfg");
    let _clean_bad = TempDirCleaner(bad_cfg.clone());
    fs::create_dir_all(bad_cfg.join("SYSTEM.md")).unwrap(); // directory instead of file
    #[cfg(unix)]
    {
        // Invalid bytes must be inside expected filename, not in filename itself.
        fs::write(bad_cfg.join("APPEND_SYSTEM.md"), b"invalid utf8 \xFF").unwrap();
    }
    let res_bad = build_system_prompt_with_sources(&bad_cfg, &(TrustDecision::Untrusted, None));
    assert_eq!(res_bad.system_prompt_source, None);
    assert!(res_bad.append_system_prompt_sources.is_empty());
}

#[test]
fn package_metadata_survives_extension() {
    let cwd = tempfile_dir("metadata-cwd");
    let _clean_cwd = TempDirCleaner(cwd.clone());

    let mut catalog = ResourceCatalog::new(cwd.clone());

    let base_meta = PathMetadata {
        source: "base-pkg".into(),
        scope: SourceScope::User,
        origin: SourceOrigin::Package,
        base_dir: Some(cwd.join("base_pkg")),
    };

    let base_skill = ResolvedResource {
        path: cwd.join("skills/skill1.js"),
        enabled: true,
        metadata: base_meta.clone(),
    };
    let base_prompt = ResolvedResource {
        path: cwd.join("prompts/p1.md"),
        enabled: true,
        metadata: base_meta.clone(),
    };
    let base_theme = ResolvedResource {
        path: cwd.join("themes/t1.json"),
        enabled: true,
        metadata: base_meta.clone(),
    };

    catalog.reload(vec![base_skill], vec![base_prompt], vec![base_theme]);

    let ext_meta = PathMetadata {
        source: "ext-pkg".into(),
        scope: SourceScope::Project,
        origin: SourceOrigin::TopLevel,
        base_dir: Some(cwd.join("ext_pkg")),
    };

    let ext_skill = ResolvedResource {
        path: cwd.join("skills/skill2.js"),
        enabled: true,
        metadata: ext_meta.clone(),
    };

    catalog.extend_resources(ExtensionResources {
        skills: vec![ext_skill],
        prompts: vec![],
        themes: vec![],
    });

    let skill_records = catalog.get_records(ResourceKind::Skill);
    assert_eq!(skill_records.len(), 2);
    let s1 = skill_records
        .iter()
        .find(|r| r.path == catalog.normalize_path(&cwd.join("skills/skill1.js")))
        .unwrap();
    assert_eq!(s1.source_info.path, s1.path);
    assert_eq!(s1.source_info.source, "base-pkg");
    assert_eq!(s1.source_info.scope, SourceScope::User);
    assert_eq!(s1.source_info.origin, SourceOrigin::Package);
    assert_eq!(
        s1.source_info.base_dir,
        Some(catalog.normalize_path(&cwd.join("base_pkg")))
    );

    let s2 = skill_records
        .iter()
        .find(|r| r.path == catalog.normalize_path(&cwd.join("skills/skill2.js")))
        .unwrap();
    assert_eq!(s2.source_info.path, s2.path);
    assert_eq!(s2.source_info.source, "ext-pkg");
    assert_eq!(s2.source_info.scope, SourceScope::Project);
    assert_eq!(s2.source_info.origin, SourceOrigin::TopLevel);
    assert_eq!(
        s2.source_info.base_dir,
        Some(catalog.normalize_path(&cwd.join("ext_pkg")))
    );

    // Extension metadata remains kind-specific and preserves all fields.
    let ext_prompt = ResolvedResource {
        path: cwd.join("prompts/ext.md"),
        enabled: true,
        metadata: ext_meta.clone(),
    };
    let ext_theme = ResolvedResource {
        path: cwd.join("themes/ext.json"),
        enabled: true,
        metadata: ext_meta.clone(),
    };
    catalog.extend_resources(ExtensionResources {
        skills: vec![],
        prompts: vec![ext_prompt],
        themes: vec![ext_theme],
    });
    let prompt_records = catalog.get_records(ResourceKind::Prompt);
    assert_eq!(prompt_records.len(), 2);
    assert_eq!(prompt_records[0].source_info.source, "base-pkg");
    assert_eq!(prompt_records[0].source_info.scope, SourceScope::User);
    assert_eq!(prompt_records[0].source_info.origin, SourceOrigin::Package);
    assert_eq!(
        prompt_records[0].source_info.base_dir,
        Some(catalog.normalize_path(&cwd.join("base_pkg")))
    );
    assert_eq!(prompt_records[1].source_info.source, "ext-pkg");
    assert_eq!(prompt_records[1].source_info.scope, SourceScope::Project);
    assert_eq!(prompt_records[1].source_info.origin, SourceOrigin::TopLevel);
    assert_eq!(
        prompt_records[1].source_info.base_dir,
        Some(catalog.normalize_path(&cwd.join("ext_pkg")))
    );
    let theme_records = catalog.get_records(ResourceKind::Theme);
    assert_eq!(theme_records.len(), 2);
    assert_eq!(theme_records[0].source_info.source, "base-pkg");
    assert_eq!(theme_records[0].source_info.scope, SourceScope::User);
    assert_eq!(theme_records[0].source_info.origin, SourceOrigin::Package);
    assert_eq!(
        theme_records[0].source_info.base_dir,
        Some(catalog.normalize_path(&cwd.join("base_pkg")))
    );
    assert_eq!(theme_records[1].source_info.source, "ext-pkg");
    assert_eq!(theme_records[1].source_info.scope, SourceScope::Project);
    assert_eq!(theme_records[1].source_info.origin, SourceOrigin::TopLevel);
    assert_eq!(
        theme_records[1].source_info.base_dir,
        Some(catalog.normalize_path(&cwd.join("ext_pkg")))
    );

    // Disabled resources remain invisible
    let disabled_res = ResolvedResource {
        path: cwd.join("skills/disabled.js"),
        enabled: false,
        metadata: base_meta.clone(),
    };
    catalog.reload(vec![disabled_res], vec![], vec![]);
    assert!(catalog.get_records(ResourceKind::Skill).is_empty());

    // Stale metadata clearing: extension metadata from previous reload must be cleared on reload
    let old_ext_res = ResolvedResource {
        path: cwd.join("skills/old_ext.js"),
        enabled: true,
        metadata: ext_meta.clone(),
    };
    catalog.extend_resources(ExtensionResources {
        skills: vec![old_ext_res],
        prompts: vec![],
        themes: vec![],
    });
    // Add base metadata for a descendant path of old_ext.js to test fallback if stale extension metadata wasn't cleared
    let child_path = cwd.join("skills/old_ext.js/child");
    let new_base = ResolvedResource {
        path: child_path.clone(),
        enabled: true,
        metadata: base_meta.clone(),
    };
    catalog.reload(vec![new_base], vec![], vec![]);
    let reloaded_records = catalog.get_records(ResourceKind::Skill);
    assert_eq!(reloaded_records.len(), 1);
    assert_eq!(reloaded_records[0].source_info.source, "base-pkg"); // If stale extension_metadata survived, it would have matched "ext-pkg"

    // Duplicate extension paths remain unique in visible paths
    let duplicate_ext = ResolvedResource {
        path: cwd.join("skills/dup.js"),
        enabled: true,
        metadata: base_meta.clone(),
    };
    catalog.extend_resources(ExtensionResources {
        skills: vec![duplicate_ext.clone(), duplicate_ext],
        prompts: vec![],
        themes: vec![],
    });
    assert_eq!(catalog.get_records(ResourceKind::Skill).len(), 2);
}

#[test]
fn component_boundary_and_ancestor_selection() {
    let cwd = tempfile_dir("comp-boundary");
    let _clean_cwd = TempDirCleaner(cwd.clone());

    let mut catalog = ResourceCatalog::new(cwd.clone());

    let pkg_foo_meta = PathMetadata {
        source: "pkg-foo".into(),
        scope: SourceScope::User,
        origin: SourceOrigin::Package,
        base_dir: Some(cwd.join("pkg/foo")),
    };
    let pkg_foo_child_meta = PathMetadata {
        source: "pkg-foo-child".into(),
        scope: SourceScope::User,
        origin: SourceOrigin::Package,
        base_dir: Some(cwd.join("pkg/foo/child")),
    };

    // Register metadata at root /pkg/foo and child /pkg/foo/child
    let res_foo = ResolvedResource {
        path: cwd.join("pkg/foo"),
        enabled: true,
        metadata: pkg_foo_meta,
    };
    let res_foo_child = ResolvedResource {
        path: cwd.join("pkg/foo/child"),
        enabled: true,
        metadata: pkg_foo_child_meta,
    };

    // Visible target 1: /pkg/foobar/deep.js - string prefix match of /pkg/foo, but NOT component match!
    // Visible target 2: /pkg/foo/child/file.js - ancestor match of BOTH /pkg/foo and /pkg/foo/child. Longest ancestor (/pkg/foo/child) must win!
    let res_foobar_target = ResolvedResource {
        path: cwd.join("pkg/foobar/deep.js"),
        enabled: true,
        metadata: PathMetadata {
            source: "unrelated".into(),
            scope: SourceScope::User,
            origin: SourceOrigin::Package,
            base_dir: None,
        },
    };
    let res_child_target = ResolvedResource {
        path: cwd.join("pkg/foo/child/file.js"),
        enabled: true,
        metadata: PathMetadata {
            source: "default-fallback".into(),
            scope: SourceScope::User,
            origin: SourceOrigin::Package,
            base_dir: None,
        },
    };

    catalog.reload(
        vec![res_foo, res_foo_child, res_foobar_target, res_child_target],
        vec![],
        vec![],
    );
    let records = catalog.get_records(ResourceKind::Skill);

    // 1. Component boundary check: /pkg/foobar/deep.js does NOT match ancestor /pkg/foo
    let foobar_record = records
        .iter()
        .find(|r| r.path == catalog.normalize_path(&cwd.join("pkg/foobar/deep.js")))
        .unwrap();
    assert_eq!(foobar_record.source_info.source, "unrelated");

    // 2. Longest ancestor selection: /pkg/foo/child/file.js matches both /pkg/foo and /pkg/foo/child.
    // If we remove res_child_target's exact metadata from map (or look up descendant without exact metadata):
    let child_file_info = catalog
        .resolve_metadata(ResourceKind::Skill, &cwd.join("pkg/foo/child/file.js"))
        .unwrap();
    // /pkg/foo/child/file.js exact match returns "default-fallback".
    assert_eq!(child_file_info.source, "default-fallback");

    // Now test descendant without exact match: /pkg/foo/child/deep/leaf.js
    let leaf_info = catalog
        .resolve_metadata(ResourceKind::Skill, &cwd.join("pkg/foo/child/deep/leaf.js"))
        .unwrap();
    assert_eq!(leaf_info.source, "pkg-foo-child");
    assert_eq!(
        leaf_info.path,
        catalog.normalize_path(&cwd.join("pkg/foo/child/deep/leaf.js"))
    );
}

#[test]
fn linked_worktree_fail_open_matrix() {
    let base_dir = tempfile_dir("worktree-fail-open-matrix");
    let _clean_base = TempDirCleaner(base_dir.clone());

    fn assert_context(base: &std::path::Path, case: &str, root: &std::path::Path) {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join("AGENTS.md"), format!("{case} local")).unwrap();
        fs::write(base.join("AGENTS.md"), format!("{case} ancestor")).unwrap();
        let project_root = CanonicalProjectRoot::new(root).unwrap();
        let prompt = project::load_project_prompt(&project_root);
        assert!(
            prompt.contains(&format!("{case} local")),
            "{case}: local context"
        );
        assert!(
            prompt.contains(&format!("{case} ancestor")),
            "{case}: ancestor context"
        );
    }

    // Ordinary repository: .git directory means no linked-worktree suppression.
    let ordinary = base_dir.join("ordinary");
    fs::create_dir_all(ordinary.join(".git")).unwrap();
    assert_context(&ordinary, "ordinary", &ordinary.join("project"));

    // Sibling worktree: valid metadata, but worktree lies outside main repository root.
    let sibling_case = base_dir.join("sibling");
    let sibling_main = sibling_case.join("main");
    let sibling = sibling_case.join("worktree");
    let sibling_git = sibling_main.join(".git").join("worktrees").join("wt");
    fs::create_dir_all(&sibling_git).unwrap();
    fs::create_dir_all(&sibling).unwrap();
    fs::write(sibling_git.join("commondir"), "../..\n").unwrap();
    fs::write(
        sibling.join(".git"),
        format!("gitdir: {}\n", sibling_git.display()),
    )
    .unwrap();
    assert_context(&sibling_case, "sibling", &sibling);

    // Bare-like layout: common git directory is not main-root/.git.
    let bare_case = base_dir.join("bare-like");
    let bare = bare_case.join("repo.git");
    let bare_worktree = bare_case.join("worktree");
    let bare_gitdir = bare.join("worktrees").join("wt");
    fs::create_dir_all(&bare_gitdir).unwrap();
    fs::create_dir_all(&bare_worktree).unwrap();
    fs::write(bare_gitdir.join("commondir"), "../..\n").unwrap();
    fs::write(
        bare_worktree.join(".git"),
        format!("gitdir: {}\n", bare_gitdir.display()),
    )
    .unwrap();
    assert_context(&bare_case, "bare", &bare_worktree);

    // Missing gitdir target: parser must fail open instead of suppressing context.
    let missing_case = base_dir.join("missing-gitdir");
    let missing = missing_case.join("worktree");
    fs::create_dir_all(&missing).unwrap();
    fs::write(missing.join(".git"), "gitdir: absent-gitdir\n").unwrap();
    assert_context(&missing_case, "missing", &missing);

    // Invalid commondir: gitdir exists, but commondir cannot be canonicalized.
    let invalid_case = base_dir.join("invalid-commondir");
    let invalid = invalid_case.join("worktree");
    let invalid_gitdir = invalid_case.join("gitdir");
    fs::create_dir_all(&invalid_gitdir).unwrap();
    fs::create_dir_all(&invalid).unwrap();
    fs::write(invalid_gitdir.join("commondir"), "does-not-exist\n").unwrap();
    fs::write(
        invalid.join(".git"),
        format!("gitdir: {}\n", invalid_gitdir.display()),
    )
    .unwrap();
    assert_context(&invalid_case, "invalid", &invalid);

    // Malformed gitfile with existing target: strict one-line parsing must fail open.
    let malformed_case = base_dir.join("malformed-gitfile");
    let malformed = malformed_case.join("worktree");
    let malformed_gitdir = malformed_case.join("gitdir");
    fs::create_dir_all(&malformed_gitdir).unwrap();
    fs::create_dir_all(&malformed).unwrap();
    fs::write(malformed_gitdir.join("commondir"), "..\n").unwrap();
    fs::write(
        malformed.join(".git"),
        format!("gitdir: {}\nextra line\n", malformed_gitdir.display()),
    )
    .unwrap();
    assert_context(&malformed_case, "malformed", &malformed);
}

#[test]
fn nested_worktree_context_shadowing() {
    let base_dir = tempfile_dir("worktree-fixtures");
    let _clean_base = TempDirCleaner(base_dir.clone());

    // 1. Main repository fixture (containing linked worktree subfolder inside main repo tree)
    let main_repo = base_dir.join("main_repo");
    let main_git = main_repo.join(".git");
    fs::create_dir_all(&main_git).unwrap();
    fs::write(main_repo.join("AGENTS.md"), "main repo agents").unwrap();
    fs::write(main_repo.join("CLAUDE.md"), "main repo claude").unwrap();

    // 2. Worktree strictly below main repository (main_repo/worktrees/wt1)
    let wt_dir = main_repo.join("worktrees").join("wt1");
    fs::create_dir_all(&wt_dir).unwrap();
    fs::write(wt_dir.join("AGENTS.md"), "worktree agents").unwrap();

    let git_worktrees_wt = main_git.join("worktrees").join("wt1");
    fs::create_dir_all(&git_worktrees_wt).unwrap();
    fs::write(
        git_worktrees_wt.join("gitdir"),
        wt_dir.join(".git").to_str().unwrap(),
    )
    .unwrap();
    fs::write(git_worktrees_wt.join("commondir"), "../..\n").unwrap();

    fs::write(
        wt_dir.join(".git"),
        format!(
            "gitdir: {}\n",
            git_worktrees_wt.canonicalize().unwrap().display()
        ),
    )
    .unwrap();

    // Nested trusted root inside worktree directory
    let wt_nested = wt_dir.join("nested_proj");
    fs::create_dir_all(wt_nested.join(".pi")).unwrap();
    fs::write(
        wt_nested.join(".pi").join("instructions.md"),
        "nested instructions",
    )
    .unwrap();

    let nested_root = CanonicalProjectRoot::new(&wt_nested).unwrap();
    let prompt = project::load_project_prompt(&nested_root);

    // Worktree context (worktree agents) and nested instructions loaded; main repo AGENTS.md shadowed/suppressed
    assert!(prompt.contains("worktree agents"));
    assert!(prompt.contains("nested instructions"));
    assert!(!prompt.contains("main repo agents"));
    assert!(prompt.contains("main repo claude"));

    // 3. Ancestor above main repository remains loaded if present
    let outer_dir = base_dir.join("outer");
    let outer_main = outer_dir.join("main_repo");
    let outer_main_git = outer_main.join(".git");
    fs::create_dir_all(&outer_main_git).unwrap();
    fs::write(outer_dir.join("AGENTS.md"), "outer agents").unwrap();
    fs::write(outer_main.join("AGENTS.md"), "outer main agents").unwrap();

    let outer_wt = outer_main.join("wt");
    fs::create_dir_all(&outer_wt).unwrap();
    fs::write(outer_wt.join("AGENTS.md"), "outer wt agents").unwrap();
    let outer_git_wt = outer_main_git.join("worktrees").join("wt");
    fs::create_dir_all(&outer_git_wt).unwrap();
    fs::write(outer_git_wt.join("commondir"), "../..\n").unwrap();
    fs::write(
        outer_wt.join(".git"),
        format!(
            "gitdir: {}\n",
            outer_git_wt.canonicalize().unwrap().display()
        ),
    )
    .unwrap();

    let outer_wt_root = CanonicalProjectRoot::new(&outer_wt).unwrap();
    let outer_prompt = project::load_project_prompt(&outer_wt_root);
    assert!(outer_prompt.contains("outer wt agents"));
    assert!(!outer_prompt.contains("outer main agents"));
    assert!(outer_prompt.contains("outer agents"));

    // 4. Fail-open matrix fixtures are covered by dedicated test below.

    // e. Missing worktree context file (WT has no AGENTS.md/CLAUDE.md): shadow detection returns None
    let empty_wt = main_repo.join("worktrees").join("empty_wt");
    fs::create_dir_all(&empty_wt).unwrap();
    let git_empty_wt = main_git.join("worktrees").join("empty_wt");
    fs::create_dir_all(&git_empty_wt).unwrap();
    fs::write(git_empty_wt.join("commondir"), "../..\n").unwrap();
    fs::write(
        empty_wt.join(".git"),
        format!(
            "gitdir: {}\n",
            git_empty_wt.canonicalize().unwrap().display()
        ),
    )
    .unwrap();
    let empty_root = CanonicalProjectRoot::new(&empty_wt).unwrap();
    let empty_prompt = project::load_project_prompt(&empty_root);
    // Since worktree has no context, main_repo context is NOT suppressed!
    assert!(empty_prompt.contains("main repo agents"));

    // f. Different context filename (WT has CLAUDE.md, main has AGENTS.md): WT has CLAUDE.md so main AGENTS.md is shadowed
    let claude_wt = main_repo.join("worktrees").join("claude_wt");
    fs::create_dir_all(&claude_wt).unwrap();
    fs::write(claude_wt.join("CLAUDE.md"), "worktree claude").unwrap();
    let git_claude_wt = main_git.join("worktrees").join("claude_wt");
    fs::create_dir_all(&git_claude_wt).unwrap();
    fs::write(git_claude_wt.join("commondir"), "../..\n").unwrap();
    fs::write(
        claude_wt.join(".git"),
        format!(
            "gitdir: {}\n",
            git_claude_wt.canonicalize().unwrap().display()
        ),
    )
    .unwrap();
    let claude_root = CanonicalProjectRoot::new(&claude_wt).unwrap();
    let claude_prompt = project::load_project_prompt(&claude_root);
    assert!(claude_prompt.contains("worktree claude"));
    assert!(!claude_prompt.contains("main repo claude"));

    // g. Submodule-like layout (gitdir points to .git/modules/... where commondir points to .):
    // commondir is "." so common git is .git/modules/sub1, whose parent is .git/modules != main repo .git
    let sub_dir = main_repo.join("submodule");
    fs::create_dir_all(&sub_dir).unwrap();
    fs::write(sub_dir.join("AGENTS.md"), "submodule agents").unwrap();
    let git_sub = main_git.join("modules").join("sub1");
    fs::create_dir_all(&git_sub).unwrap();
    fs::write(git_sub.join("commondir"), ".\n").unwrap();
    fs::write(
        sub_dir.join(".git"),
        format!("gitdir: {}\n", git_sub.canonicalize().unwrap().display()),
    )
    .unwrap();
    let sub_root = CanonicalProjectRoot::new(&sub_dir).unwrap();
    let sub_prompt = project::load_project_prompt(&sub_root);
    // Submodule is not a linked worktree of main repo; fails open, both submodule agents and main repo agents loaded
    assert!(sub_prompt.contains("submodule agents"));
    assert!(sub_prompt.contains("main repo agents"));
}
