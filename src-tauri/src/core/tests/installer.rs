use std::fs;
use std::path::{Path, PathBuf};

use crate::core::skill_store::{SkillRecord, SkillStore, SkillTargetRecord};
use crate::core::sync_engine::SyncMode;
use crate::core::tool_adapters::{save_tool_config, CustomToolConfig, ToolConfig};

fn make_store() -> (tempfile::TempDir, SkillStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SkillStore::new(dir.path().join("test.db"));
    store.ensure_schema().expect("ensure_schema");
    (dir, store)
}

fn set_central_path(store: &SkillStore, central: &Path) {
    store
        .set_setting("central_repo_path", central.to_string_lossy().as_ref())
        .unwrap();
}

fn init_git_repo(dir: &Path) -> git2::Repository {
    let repo = git2::Repository::init(dir).unwrap();
    let sig = git2::Signature::now("t", "t@example.com").unwrap();

    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    let tree_id = index.write_tree().unwrap();
    {
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }
    repo
}

fn commit_all(repo: &git2::Repository, msg: &str) -> git2::Oid {
    let sig = git2::Signature::now("t", "t@example.com").unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();

    let parent = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok());
    match parent {
        Some(p) => repo
            .commit(Some("HEAD"), &sig, &sig, msg, &tree, &[&p])
            .unwrap(),
        None => repo
            .commit(Some("HEAD"), &sig, &sig, msg, &tree, &[])
            .unwrap(),
    }
}

fn cache_key() -> &'static str {
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
}

#[cfg(unix)]
#[test]
fn git_cache_root_symlink_is_rejected() {
    use std::os::unix::fs::symlink;

    let parent = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let cache_root = parent.path().join("skills-hub-git-cache");
    symlink(external.path(), &cache_root).unwrap();

    let err = super::ensure_real_git_cache_root(&cache_root).unwrap_err();
    assert!(format!("{err:#}").contains("UNSAFE_GIT_CACHE"));
}

#[cfg(unix)]
#[test]
fn git_cache_entry_symlink_is_rejected_without_following_it() {
    use std::os::unix::fs::symlink;

    let cache_root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    fs::write(external.path().join("sentinel"), b"unchanged").unwrap();
    let entry = cache_root.path().join(cache_key());
    symlink(external.path(), &entry).unwrap();

    let err = super::validate_git_cache_entry(cache_root.path(), &entry).unwrap_err();
    assert!(format!("{err:#}").contains("UNSAFE_GIT_CACHE"));
    assert_eq!(
        fs::read(external.path().join("sentinel")).unwrap(),
        b"unchanged"
    );
}

#[cfg(unix)]
#[test]
fn git_cache_git_symlink_is_rejected_without_following_it() {
    use std::os::unix::fs::symlink;

    let cache_root = tempfile::tempdir().unwrap();
    let entry = cache_root.path().join(cache_key());
    fs::create_dir(&entry).unwrap();
    let external_git = tempfile::tempdir().unwrap();
    fs::write(external_git.path().join("sentinel"), b"unchanged").unwrap();
    symlink(external_git.path(), entry.join(".git")).unwrap();

    let err = super::validate_git_cache_entry(cache_root.path(), &entry).unwrap_err();
    assert!(format!("{err:#}").contains("UNSAFE_GIT_CACHE"));
    assert_eq!(
        fs::read(external_git.path().join("sentinel")).unwrap(),
        b"unchanged"
    );
}

#[test]
fn real_direct_git_cache_entry_remains_valid() {
    let cache_root = tempfile::tempdir().unwrap();
    let entry = cache_root.path().join(cache_key());
    fs::create_dir(&entry).unwrap();
    fs::create_dir(entry.join(".git")).unwrap();

    super::validate_git_cache_entry(cache_root.path(), &entry).unwrap();
}

#[test]
fn parses_github_urls() {
    let p = super::parse_github_url("https://github.com/owner/repo").unwrap();
    assert_eq!(p.clone_url, "https://github.com/owner/repo.git");
    assert!(p.branch.is_none());
    assert!(p.subpath.is_none());

    let p = super::parse_github_url("anthropics/skills").unwrap();
    assert_eq!(p.clone_url, "https://github.com/anthropics/skills.git");
    assert!(p.branch.is_none());
    assert!(p.subpath.is_none());

    let p = super::parse_github_url("github.com/owner/repo").unwrap();
    assert_eq!(p.clone_url, "https://github.com/owner/repo.git");
    assert!(p.branch.is_none());
    assert!(p.subpath.is_none());

    let p = super::parse_github_url("https://github.com/owner/repo/tree/main/skills/x").unwrap();
    assert_eq!(p.clone_url, "https://github.com/owner/repo.git");
    assert_eq!(p.branch.as_deref(), Some("main"));
    assert_eq!(p.subpath.as_deref(), Some("skills/x"));

    let p = super::parse_github_url("owner/repo/tree/main/skills/x").unwrap();
    assert_eq!(p.clone_url, "https://github.com/owner/repo.git");
    assert_eq!(p.branch.as_deref(), Some("main"));
    assert_eq!(p.subpath.as_deref(), Some("skills/x"));

    let p = super::parse_github_url("https://github.com/owner/repo/blob/main/skills/x/SKILL.md")
        .unwrap();
    assert_eq!(p.clone_url, "https://github.com/owner/repo.git");
    assert_eq!(p.branch.as_deref(), Some("main"));
    assert_eq!(p.subpath.as_deref(), Some("skills/x"));

    let p = super::parse_github_url("https://github.com/owner/repo/blob/main/SKILL.md").unwrap();
    assert_eq!(p.clone_url, "https://github.com/owner/repo.git");
    assert_eq!(p.branch.as_deref(), Some("main"));
    assert_eq!(p.subpath.as_deref(), Some("."));

    let p = super::parse_github_url("/local/path/to/repo").unwrap();
    assert_eq!(p.clone_url, "/local/path/to/repo");
}

#[test]
fn rejects_http_git_urls_that_could_leak_secrets() {
    for unsafe_url in [
        "https://user:super-secret@github.com/owner/repo.git",
        "https://user@github.com/owner/repo.git",
        "https://@github.com/owner/repo.git",
        "https://github.com/owner/repo.git?token=super-secret",
        "https://github.com/owner/repo.git#super-secret",
    ] {
        let err = match super::parse_github_url(unsafe_url) {
            Ok(_) => panic!("expected unsafe Git URL rejection"),
            Err(err) => err,
        };
        let message = format!("{err:#}");
        assert!(message.contains("UNSAFE_GIT_URL"));
        assert!(!message.contains("super-secret"));
    }
}

#[test]
fn all_git_install_and_list_entries_reject_secret_urls_before_writes() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    set_central_path(&store, central.path());
    let unsafe_url = "https://user:super-secret@github.com/owner/repo.git";

    let install_err = match super::install_git_skill(
        app.handle(),
        &store,
        unsafe_url,
        Some("safe-name".to_string()),
        None,
    ) {
        Ok(_) => panic!("expected unsafe Git install rejection"),
        Err(err) => err,
    };
    let list_err = match super::list_git_skills(app.handle(), &store, unsafe_url) {
        Ok(_) => panic!("expected unsafe Git list rejection"),
        Err(err) => err,
    };
    let selection_err = match super::install_git_skill_from_selection(
        app.handle(),
        &store,
        unsafe_url,
        ".",
        Some("safe-name".to_string()),
    ) {
        Ok(_) => panic!("expected unsafe Git selection rejection"),
        Err(err) => err,
    };

    for err in [install_err, list_err, selection_err] {
        let message = format!("{err:#}");
        assert!(message.contains("UNSAFE_GIT_URL"));
        assert!(!message.contains("super-secret"));
    }
    assert!(store.list_skills().unwrap().is_empty());
    assert_eq!(fs::read_dir(central.path()).unwrap().count(), 0);
}

#[test]
fn update_rejects_and_clears_legacy_secret_git_source() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    set_central_path(&store, central.path());
    let skill_path = central.path().join("legacy-git-source");
    fs::create_dir(&skill_path).unwrap();
    fs::write(skill_path.join("SKILL.md"), "# Keep me").unwrap();
    store
        .upsert_skill(&SkillRecord {
            id: "legacy-git-source-id".to_string(),
            name: "legacy-git-source".to_string(),
            description: None,
            source_type: "git".to_string(),
            source_ref: Some("https://user:super-secret@github.com/owner/repo.git".to_string()),
            source_subpath: None,
            source_revision: None,
            central_path: skill_path.to_string_lossy().to_string(),
            content_hash: None,
            created_at: 1,
            updated_at: 1,
            last_sync_at: None,
            last_seen_at: 1,
            enabled: true,
            status: "ok".to_string(),
        })
        .unwrap();

    let err =
        match super::update_managed_skill_from_source(app.handle(), &store, "legacy-git-source-id")
        {
            Ok(_) => panic!("expected unsafe stored Git URL rejection"),
            Err(err) => err,
        };
    let message = format!("{err:#}");
    assert!(message.contains("UNSAFE_GIT_URL"));
    assert!(!message.contains("super-secret"));
    let sanitized = store
        .get_skill_by_id("legacy-git-source-id")
        .unwrap()
        .unwrap();
    assert!(sanitized.source_ref.is_none());
    assert_eq!(sanitized.status, "error");
    assert_eq!(
        fs::read_to_string(skill_path.join("SKILL.md")).unwrap(),
        "# Keep me"
    );
}

#[test]
fn parses_skill_md_frontmatter() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("SKILL.md");
    fs::write(
        &p,
        r#"---
name: "My Skill"
description: "Desc"
---

body
"#,
    )
    .unwrap();

    let (name, desc) = super::parse_skill_md(&p).unwrap();
    assert_eq!(name, "My Skill");
    assert_eq!(desc.as_deref(), Some("Desc"));
}

#[test]
fn parses_skill_md_frontmatter_literal_description() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("SKILL.md");
    fs::write(
        &p,
        r#"---
name: technical-writer
description: |
  Creates clear documentation, API references, guides, and
  technical content for developers and users.
author: awesome-llm-apps
---

body
"#,
    )
    .unwrap();

    let (name, desc) = super::parse_skill_md(&p).unwrap();
    assert_eq!(name, "technical-writer");
    assert_eq!(
        desc.as_deref(),
        Some("Creates clear documentation, API references, guides, and\ntechnical content for developers and users.")
    );
}

#[test]
fn parses_skill_md_frontmatter_folded_chomp_description() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("SKILL.md");
    fs::write(
        &p,
        r#"---
name: fireworks-tech-graph
description: >-
  Use when the user wants to create any technical diagram - architecture, data
  flow, flowchart, sequence, agent/memory, or concept map - and export as
  SVG+PNG.
---

body
"#,
    )
    .unwrap();

    let (name, desc) = super::parse_skill_md(&p).unwrap();
    assert_eq!(name, "fireworks-tech-graph");
    assert_eq!(
        desc.as_deref(),
        Some(
            "Use when the user wants to create any technical diagram - architecture, data flow, flowchart, sequence, agent/memory, or concept map - and export as SVG+PNG."
        )
    );
}

#[test]
fn backfill_skill_descriptions_replaces_stale_frontmatter_marker() {
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    fs::write(
        central.path().join("SKILL.md"),
        r#"---
name: fireworks-tech-graph
description: >-
  Correct folded description.
---
"#,
    )
    .unwrap();

    store
        .upsert_skill(&SkillRecord {
            id: "fireworks".to_string(),
            name: "fireworks-tech-graph".to_string(),
            description: Some(">-".to_string()),
            source_type: "local".to_string(),
            source_ref: None,
            source_subpath: None,
            source_revision: None,
            central_path: central.path().to_string_lossy().to_string(),
            content_hash: None,
            created_at: 1,
            updated_at: 1,
            last_sync_at: None,
            last_seen_at: 1,
            enabled: true,
            status: "ok".to_string(),
        })
        .unwrap();

    super::backfill_skill_descriptions(&store);

    let skill = store.get_skill_by_id("fireworks").unwrap().unwrap();
    assert_eq!(
        skill.description.as_deref(),
        Some("Correct folded description.")
    );
}

#[test]
fn installs_local_skill_and_updates_from_source() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();

    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let source = tempfile::tempdir().unwrap();
    fs::write(source.path().join("SKILL.md"), b"---\nname: x\n---\n").unwrap();
    fs::write(source.path().join("a.txt"), b"v1").unwrap();

    let res = super::install_local_skill(
        app.handle(),
        &store,
        source.path(),
        Some("local1".to_string()),
    )
    .unwrap();
    assert!(res.central_path.exists());

    let skill = store.get_skill_by_id(&res.skill_id).unwrap().unwrap();
    assert_eq!(skill.name, "local1");

    // Historical targets must remain completely untouched by Safe updates.
    let target_root = tempfile::tempdir().unwrap();
    let target = target_root.path().join("local1");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("a.txt"), b"external-v1").unwrap();
    let t = SkillTargetRecord {
        id: "t1".to_string(),
        skill_id: res.skill_id.clone(),
        tool: "legacy-unknown-tool".to_string(),
        scope: "global".to_string(),
        project_path: None,
        target_path: target.to_string_lossy().to_string(),
        mode: "copy".to_string(),
        status: "ok".to_string(),
        last_error: None,
        synced_at: None,
    };
    store.upsert_skill_target(&t).unwrap();

    fs::write(source.path().join("a.txt"), b"v2").unwrap();
    let up = super::update_managed_skill_from_source(app.handle(), &store, &res.skill_id).unwrap();
    assert_eq!(up.skill_id, res.skill_id);
    assert!(up.updated_targets.is_empty());
    assert!(PathBuf::from(
        store
            .get_skill_by_id(&res.skill_id)
            .unwrap()
            .unwrap()
            .central_path
    )
    .exists());
    assert_eq!(fs::read(target.join("a.txt")).unwrap(), b"external-v1");
    let unchanged_target = store
        .get_skill_target(&res.skill_id, "legacy-unknown-tool", "global", None)
        .unwrap()
        .unwrap();
    assert_eq!(unchanged_target.id, t.id);
    assert_eq!(unchanged_target.skill_id, t.skill_id);
    assert_eq!(unchanged_target.tool, t.tool);
    assert_eq!(unchanged_target.scope, t.scope);
    assert_eq!(unchanged_target.project_path, t.project_path);
    assert_eq!(unchanged_target.target_path, t.target_path);
    assert_eq!(unchanged_target.mode, t.mode);
    assert_eq!(unchanged_target.status, t.status);
    assert_eq!(unchanged_target.last_error, t.last_error);
    assert_eq!(unchanged_target.synced_at, t.synced_at);

    let err = match super::install_local_skill(
        app.handle(),
        &store,
        source.path(),
        Some("local1".to_string()),
    ) {
        Ok(_) => panic!("expected error"),
        Err(e) => e,
    };
    assert!(format!("{:#}", err).contains("skill already exists"));
}

#[test]
fn update_refreshes_only_validated_copy_targets() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let source = tempfile::tempdir().unwrap();
    fs::write(source.path().join("SKILL.md"), b"---\nname: x\n---\n").unwrap();
    fs::write(source.path().join("a.txt"), b"v1").unwrap();
    let installed = super::install_local_skill(
        app.handle(),
        &store,
        source.path(),
        Some("local-sync".to_string()),
    )
    .unwrap();

    let tool_root = tempfile::tempdir().unwrap();
    save_tool_config(
        &store,
        ToolConfig {
            disabled_builtin_tools: Vec::new(),
            custom_tools: vec![CustomToolConfig {
                key: "custom_test".to_string(),
                label: "Test Tool".to_string(),
                avatar: None,
                skills_dir: tool_root.path().to_string_lossy().to_string(),
                project_skills_dir: None,
                sync_mode: SyncMode::Copy,
                enabled: true,
            }],
        },
    )
    .unwrap();

    let target = tool_root.path().join("local-sync");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("a.txt"), b"v1").unwrap();
    store
        .upsert_skill_target(&SkillTargetRecord {
            id: "copy-target".to_string(),
            skill_id: installed.skill_id.clone(),
            tool: "custom_test".to_string(),
            scope: "global".to_string(),
            project_path: None,
            target_path: target.to_string_lossy().to_string(),
            mode: "copy".to_string(),
            status: "ok".to_string(),
            last_error: None,
            synced_at: None,
        })
        .unwrap();

    fs::write(source.path().join("a.txt"), b"v2").unwrap();
    let updated =
        super::update_managed_skill_from_source(app.handle(), &store, &installed.skill_id).unwrap();
    assert_eq!(updated.updated_targets, vec!["custom_test".to_string()]);
    assert_eq!(fs::read(target.join("a.txt")).unwrap(), b"v2");
    let target_record = store
        .get_skill_target(&installed.skill_id, "custom_test", "global", None)
        .unwrap()
        .unwrap();
    assert_eq!(target_record.status, "ok");
    assert!(target_record.synced_at.is_some());
}

#[test]
fn failed_update_marks_skill_error_and_success_clears_it() {
    let app = tauri::test::mock_app();
    let (dir, store) = make_store();
    let central_root = dir.path().join("central");
    fs::create_dir_all(&central_root).unwrap();
    set_central_path(&store, &central_root);
    let central = central_root.join("Source Status");
    fs::create_dir_all(&central).unwrap();
    fs::write(central.join("SKILL.md"), b"---\nname: x\n---\n").unwrap();
    let source = dir.path().join("missing-source");
    let skill = SkillRecord {
        id: "source-status".to_string(),
        name: "Source Status".to_string(),
        description: None,
        source_type: "local".to_string(),
        source_ref: Some(source.to_string_lossy().to_string()),
        source_subpath: None,
        source_revision: None,
        central_path: central.to_string_lossy().to_string(),
        content_hash: None,
        created_at: 1,
        updated_at: 1,
        last_sync_at: None,
        last_seen_at: 1,
        enabled: true,
        status: "ok".to_string(),
    };
    store.upsert_skill(&skill).unwrap();

    let error = match super::update_managed_skill_from_source(app.handle(), &store, &skill.id) {
        Ok(_) => panic!("expected source update failure"),
        Err(err) => err.to_string(),
    };
    assert!(error.contains("source path not found"));
    assert_eq!(
        store.get_skill_by_id(&skill.id).unwrap().unwrap().status,
        "error"
    );

    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), b"---\nname: x\n---\n").unwrap();
    super::update_managed_skill_from_source(app.handle(), &store, &skill.id).unwrap();
    assert_eq!(
        store.get_skill_by_id(&skill.id).unwrap().unwrap().status,
        "ok"
    );

    let blocked_parent = dir.path().join("blocked-parent");
    fs::write(&blocked_parent, b"not a directory").unwrap();
    let target = SkillTargetRecord {
        id: "blocked-target".to_string(),
        skill_id: skill.id.clone(),
        tool: "unknown_tool".to_string(),
        scope: "global".to_string(),
        project_path: None,
        target_path: blocked_parent.join("target").to_string_lossy().to_string(),
        mode: "copy".to_string(),
        status: "ok".to_string(),
        last_error: None,
        synced_at: None,
    };
    store.upsert_skill_target(&target).unwrap();
    super::update_managed_skill_from_source(app.handle(), &store, &skill.id).unwrap();
    assert_eq!(
        store.get_skill_by_id(&skill.id).unwrap().unwrap().status,
        "ok"
    );
    assert_eq!(
        store
            .get_skill_target(&skill.id, "unknown_tool", "global", None)
            .unwrap()
            .unwrap()
            .status,
        "ok"
    );
}

#[test]
fn imports_identical_existing_local_skill_but_rejects_different_content() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let original = tempfile::tempdir().unwrap();
    fs::write(original.path().join("SKILL.md"), b"---\nname: x\n---\n").unwrap();
    fs::write(original.path().join("a.txt"), b"same").unwrap();
    let installed = super::install_local_skill(
        app.handle(),
        &store,
        original.path(),
        Some("local1".to_string()),
    )
    .unwrap();

    let discovered = tempfile::tempdir().unwrap();
    fs::write(discovered.path().join("SKILL.md"), b"---\nname: x\n---\n").unwrap();
    fs::write(discovered.path().join("a.txt"), b"same").unwrap();
    let imported = super::import_existing_local_skill(
        app.handle(),
        &store,
        discovered.path(),
        Some("local1".to_string()),
    )
    .unwrap();
    assert_eq!(imported.skill_id, installed.skill_id);
    assert_eq!(imported.central_path, installed.central_path);

    fs::write(discovered.path().join("a.txt"), b"different").unwrap();
    let err = match super::import_existing_local_skill(
        app.handle(),
        &store,
        discovered.path(),
        Some("local1".to_string()),
    ) {
        Ok(_) => panic!("expected error"),
        Err(err) => err,
    };
    assert!(format!("{err:#}").contains("skill already exists"));
}

#[test]
fn lists_and_installs_git_skills_without_network() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::write(repo_dir.path().join("SKILL.md"), "---\nname: Root\n---\n").unwrap();
    fs::create_dir_all(repo_dir.path().join("skills/a")).unwrap();
    fs::write(
        repo_dir.path().join("skills/a/SKILL.md"),
        "---\nname: A\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add skills");

    let candidates = super::list_git_skills(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
    )
    .unwrap();
    let subpaths: Vec<String> = candidates.into_iter().map(|c| c.subpath).collect();
    assert!(subpaths.contains(&".".to_string()));
    assert!(subpaths.iter().any(|s| s.ends_with("skills/a")));

    let res = super::install_git_skill_from_selection(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        "skills/a",
        None,
    )
    .unwrap();
    assert!(res.central_path.exists());
}

#[test]
fn install_git_skill_errors_on_multi_skills_repo_root() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo_dir.path().join("skills/a")).unwrap();
    fs::create_dir_all(repo_dir.path().join("skills/b")).unwrap();
    fs::write(
        repo_dir.path().join("skills/a/SKILL.md"),
        "---\nname: A\n---\n",
    )
    .unwrap();
    fs::write(
        repo_dir.path().join("skills/b/SKILL.md"),
        "---\nname: B\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "multi skills");

    let err = match super::install_git_skill(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        None,
        None,
    ) {
        Ok(_) => panic!("expected error"),
        Err(e) => e,
    };
    assert!(format!("{:#}", err).contains("MULTI_SKILLS|"));
}

#[test]
fn lists_local_skills_with_invalid_entries() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    fs::create_dir_all(base.join("skills/a")).unwrap();
    fs::create_dir_all(base.join("skills/b")).unwrap();
    fs::create_dir_all(base.join("skills/c")).unwrap();
    fs::create_dir_all(base.join("skills/d")).unwrap();

    fs::write(base.join("skills/a/SKILL.md"), "---\nname: A\n---\n").unwrap();
    fs::write(base.join("skills/c/SKILL.md"), "name: C\n").unwrap();
    fs::write(base.join("skills/d/SKILL.md"), "---\ndescription: D\n---\n").unwrap();

    let list = super::list_local_skills(base).unwrap();

    let find = |subpath: &str| list.iter().find(|c| c.subpath == subpath).cloned();

    let a = find("skills/a").expect("skills/a");
    assert!(a.valid);
    assert_eq!(a.name, "A");

    let b = find("skills/b").expect("skills/b");
    assert!(!b.valid);
    assert_eq!(b.reason.as_deref(), Some("missing_skill_md"));

    let c = find("skills/c").expect("skills/c");
    assert!(!c.valid);
    assert_eq!(c.reason.as_deref(), Some("invalid_frontmatter"));

    let d = find("skills/d").expect("skills/d");
    assert!(!d.valid);
    assert_eq!(d.reason.as_deref(), Some("missing_name"));
}

#[test]
fn install_local_selection_validates_skill_md() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();

    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let base = tempfile::tempdir().unwrap();
    fs::create_dir_all(base.path().join("skills/a")).unwrap();
    fs::create_dir_all(base.path().join("skills/b")).unwrap();
    fs::write(
        base.path().join("skills/a/SKILL.md"),
        "---\nname: Local A\n---\n",
    )
    .unwrap();

    let res = super::install_local_skill_from_selection(
        app.handle(),
        &store,
        base.path(),
        "skills/a",
        None,
    )
    .unwrap();
    assert!(res.central_path.exists());
    let skill = store.get_skill_by_id(&res.skill_id).unwrap().unwrap();
    assert_eq!(skill.name, "Local A");

    let err = match super::install_local_skill_from_selection(
        app.handle(),
        &store,
        base.path(),
        "skills/b",
        None,
    ) {
        Ok(_) => panic!("expected error"),
        Err(e) => e,
    };
    assert!(format!("{:#}", err).contains("SKILL_INVALID|missing_skill_md"));
}

/// Issue #28: when a git subpath is "skills", the derived name should be replaced by the
/// SKILL.md name to avoid path duplication (e.g. `~/.claude/skills/skills/`).
#[test]
fn install_git_skill_uses_skill_md_name_over_subpath_skills() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    // Build a repo with skills/<folder> where the folder is named "skills" (simulating
    // a URL like https://github.com/owner/repo/tree/main/skills).
    let repo_dir = tempfile::tempdir().unwrap();
    let skills_dir = repo_dir.path().join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: my-real-skill\ndescription: A real skill\n---\n",
    )
    .unwrap();
    fs::write(skills_dir.join("helper.txt"), b"data").unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add skill in skills dir");

    // install_git_skill_from_selection with subpath "skills" (no user-provided name)
    let res = super::install_git_skill_from_selection(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        "skills",
        None,
    )
    .unwrap();

    // The name should be "my-real-skill" from SKILL.md, NOT "skills" from the subpath.
    assert_eq!(res.name, "my-real-skill");
    assert!(res.central_path.ends_with("my-real-skill"));
    assert!(res.central_path.join("SKILL.md").exists());

    let skill = store.get_skill_by_id(&res.skill_id).unwrap().unwrap();
    assert_eq!(skill.name, "my-real-skill");
    assert_eq!(skill.description.as_deref(), Some("A real skill"));
}

#[test]
fn install_git_skill_rejects_container_subpath_without_skill_md() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(
        repo_dir
            .path()
            .join("awesome_agent_skills/technical-writer"),
    )
    .unwrap();
    fs::write(
        repo_dir
            .path()
            .join("awesome_agent_skills/technical-writer/SKILL.md"),
        "---\nname: technical-writer\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add container skill");

    let err = match super::install_git_skill_from_selection(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        "awesome_agent_skills",
        None,
    ) {
        Ok(_) => panic!("expected invalid skill path"),
        Err(e) => e,
    };
    assert!(format!("{:#}", err).contains("SKILL_INVALID|missing_skill_md"));
}

#[test]
fn install_git_skill_selection_accepts_specific_child_under_container() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(
        repo_dir
            .path()
            .join("awesome_agent_skills/technical-writer"),
    )
    .unwrap();
    fs::write(
        repo_dir
            .path()
            .join("awesome_agent_skills/technical-writer/SKILL.md"),
        "---\nname: technical-writer\ndescription: docs\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add container skill");

    let res = super::install_git_skill_from_selection(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        "awesome_agent_skills/technical-writer",
        None,
    )
    .unwrap();

    assert_eq!(res.name, "technical-writer");
    assert!(res.central_path.join("SKILL.md").exists());
}

/// Issue #28: when user explicitly provides a name, SKILL.md should NOT override it.
#[test]
fn install_git_skill_respects_user_provided_name() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    let skills_dir = repo_dir.path().join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(skills_dir.join("SKILL.md"), "---\nname: md-name\n---\n").unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add skill");

    let res = super::install_git_skill_from_selection(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        "skills",
        Some("user-custom-name".to_string()),
    )
    .unwrap();

    // User-provided name takes priority.
    assert_eq!(res.name, "user-custom-name");
}

/// Issue #28: install_git_skill (non-selection variant) also uses SKILL.md name.
#[test]
fn install_git_skill_derives_name_from_skill_md() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::write(
        repo_dir.path().join("SKILL.md"),
        "---\nname: proper-name\ndescription: desc\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "init");

    // The repo name (derived from path) will be something like a temp dir name.
    // After install, the name should be "proper-name" from SKILL.md.
    let res = super::install_git_skill(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        None,
        None,
    )
    .unwrap();

    assert_eq!(res.name, "proper-name");
    assert!(res.central_path.ends_with("proper-name"));
}

/// Issue #18: repos with skills in root-level subdirectories (no `skills/` parent)
/// should be detected as multi-skill repos.
#[test]
fn install_git_skill_detects_root_level_multi_skills() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    // Build a repo with skills directly in root subdirectories (no skills/ parent)
    let repo_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo_dir.path().join("skill-a")).unwrap();
    fs::create_dir_all(repo_dir.path().join("skill-b")).unwrap();
    fs::write(
        repo_dir.path().join("skill-a/SKILL.md"),
        "---\nname: Skill A\n---\n",
    )
    .unwrap();
    fs::write(
        repo_dir.path().join("skill-b/SKILL.md"),
        "---\nname: Skill B\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add root-level skills");

    // install_git_skill should detect multiple skills and bail with MULTI_SKILLS
    let err = match super::install_git_skill(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        None,
        None,
    ) {
        Ok(_) => panic!("expected MULTI_SKILLS error"),
        Err(e) => e,
    };
    assert!(format!("{:#}", err).contains("MULTI_SKILLS|"));
}

/// Issue #18: list_git_skills should discover skills in root-level subdirectories.
#[test]
fn list_git_skills_finds_root_level_skills() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo_dir.path().join("my-skill-1")).unwrap();
    fs::create_dir_all(repo_dir.path().join("my-skill-2")).unwrap();
    fs::create_dir_all(repo_dir.path().join("not-a-skill")).unwrap();
    fs::write(
        repo_dir.path().join("my-skill-1/SKILL.md"),
        "---\nname: First\n---\n",
    )
    .unwrap();
    fs::write(
        repo_dir.path().join("my-skill-2/SKILL.md"),
        "---\nname: Second\n---\n",
    )
    .unwrap();
    // not-a-skill has no SKILL.md — should NOT be discovered
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add root-level skills");

    let candidates = super::list_git_skills(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
    )
    .unwrap();

    let names: Vec<String> = candidates.iter().map(|c| c.name.clone()).collect();
    assert!(names.contains(&"First".to_string()), "should find First");
    assert!(names.contains(&"Second".to_string()), "should find Second");
    // "not-a-skill" should NOT appear
    assert!(
        !candidates.iter().any(|c| c.subpath.contains("not-a-skill")),
        "should not find not-a-skill"
    );
}

#[test]
fn list_git_skills_finds_root_skill_container_layout() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo_dir.path().join("custom-agent-skills/technical-writer")).unwrap();
    fs::write(
        repo_dir
            .path()
            .join("custom-agent-skills/technical-writer/SKILL.md"),
        "---\nname: technical-writer\ndescription: docs\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add container skill");

    let candidates = super::list_git_skills(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
    )
    .unwrap();

    let candidate = candidates
        .iter()
        .find(|c| c.name == "technical-writer")
        .expect("technical-writer should be discovered");
    assert_eq!(candidate.subpath, "custom-agent-skills/technical-writer");
    assert_eq!(candidate.description.as_deref(), Some("docs"));
}

#[test]
fn collect_skill_dirs_finds_skills_under_explicit_container() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("technical-writer")).unwrap();
    fs::create_dir_all(dir.path().join("not-a-skill")).unwrap();
    fs::write(
        dir.path().join("technical-writer/SKILL.md"),
        "---\nname: technical-writer\n---\n",
    )
    .unwrap();

    let dirs = super::collect_skill_dirs(dir.path());
    let rels: Vec<String> = dirs
        .iter()
        .map(|p| {
            p.strip_prefix(dir.path())
                .unwrap_or(p)
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert_eq!(rels, vec!["technical-writer".to_string()]);
}

#[test]
fn collect_skill_dirs_finds_multiple_skills_under_explicit_container() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("technical-writer")).unwrap();
    fs::create_dir_all(dir.path().join("python-expert")).unwrap();
    fs::create_dir_all(dir.path().join("not-a-skill")).unwrap();
    fs::write(
        dir.path().join("technical-writer/SKILL.md"),
        "---\nname: technical-writer\n---\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("python-expert/SKILL.md"),
        "---\nname: python-expert\n---\n",
    )
    .unwrap();

    let dirs = super::collect_skill_dirs(dir.path());
    let rels: Vec<String> = dirs
        .iter()
        .map(|p| {
            p.strip_prefix(dir.path())
                .unwrap_or(p)
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert_eq!(
        rels,
        vec!["python-expert".to_string(), "technical-writer".to_string()]
    );
}

#[test]
fn collect_skill_dirs_scans_named_skill_containers_but_not_generic_dirs() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("agent-pack/hidden-skill")).unwrap();
    fs::create_dir_all(dir.path().join("agent-skills/visible-skill")).unwrap();
    fs::write(
        dir.path().join("agent-pack/hidden-skill/SKILL.md"),
        "---\nname: hidden\n---\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("agent-skills/visible-skill/SKILL.md"),
        "---\nname: visible\n---\n",
    )
    .unwrap();

    let dirs = super::collect_skill_dirs(dir.path());
    let rels: Vec<String> = dirs
        .iter()
        .map(|p| {
            p.strip_prefix(dir.path())
                .unwrap_or(p)
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert_eq!(rels, vec!["agent-skills/visible-skill".to_string()]);
}

#[test]
fn collect_skill_dirs_deduplicates_known_root_containers() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("skills/technical-writer")).unwrap();
    fs::write(
        dir.path().join("skills/technical-writer/SKILL.md"),
        "---\nname: technical-writer\n---\n",
    )
    .unwrap();

    let dirs = super::collect_skill_dirs(dir.path());
    assert_eq!(dirs.len(), 1);
    assert!(dirs[0].ends_with("skills/technical-writer"));
}

#[test]
fn import_existing_central_skill_registers_in_place() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    set_central_path(&store, central.path());
    let existing = central.path().join("existing-skill");
    fs::create_dir(&existing).unwrap();
    fs::write(
        existing.join("SKILL.md"),
        "---\nname: existing-skill\ndescription: Existing\n---\n",
    )
    .unwrap();

    let result = super::import_existing_local_skill(
        app.handle(),
        &store,
        &existing,
        Some("existing-skill".to_string()),
    )
    .unwrap();

    assert_eq!(result.central_path, existing);
    assert_eq!(store.list_skills().unwrap().len(), 1);
    let second = super::import_existing_local_skill(
        app.handle(),
        &store,
        &existing,
        Some("existing-skill".to_string()),
    )
    .unwrap();
    assert_eq!(second.skill_id, result.skill_id);
    assert_eq!(store.list_skills().unwrap().len(), 1);
}

#[test]
fn install_local_rejects_unsafe_name_before_creating_any_skill() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    set_central_path(&store, central.path());
    let source = tempfile::tempdir().unwrap();
    fs::write(source.path().join("SKILL.md"), "---\nname: safe\n---\n").unwrap();

    let err = match super::install_local_skill(
        app.handle(),
        &store,
        source.path(),
        Some("../escape".to_string()),
    ) {
        Ok(_) => panic!("expected unsafe name rejection"),
        Err(err) => err,
    };
    assert!(format!("{err:#}").contains("unsafe_name"));
    assert!(!central.path().parent().unwrap().join("escape").exists());
    assert!(store.list_skills().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn install_local_rejects_source_root_and_tree_symlinks() {
    use std::os::unix::fs::symlink;

    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    set_central_path(&store, central.path());

    let real_source = tempfile::tempdir().unwrap();
    fs::write(
        real_source.path().join("SKILL.md"),
        "---\nname: source-link\n---\n",
    )
    .unwrap();
    let source_parent = tempfile::tempdir().unwrap();
    let source_link = source_parent.path().join("source-link");
    symlink(real_source.path(), &source_link).unwrap();

    let root_err = match super::install_local_skill(
        app.handle(),
        &store,
        &source_link,
        Some("source-link".to_string()),
    ) {
        Ok(_) => panic!("expected source-root symlink rejection"),
        Err(err) => err,
    };
    assert!(format!("{root_err:#}").contains("unsafe_source"));

    let tree_source = tempfile::tempdir().unwrap();
    fs::write(
        tree_source.path().join("SKILL.md"),
        "---\nname: tree-link\n---\n",
    )
    .unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    symlink(outside.path(), tree_source.path().join("linked-file")).unwrap();
    let tree_err = match super::install_local_skill(
        app.handle(),
        &store,
        tree_source.path(),
        Some("tree-link".to_string()),
    ) {
        Ok(_) => panic!("expected tree symlink rejection"),
        Err(err) => err,
    };
    assert!(format!("{tree_err:#}").contains("symbolic link"));

    assert!(store.list_skills().unwrap().is_empty());
    assert_eq!(fs::read_dir(central.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn import_existing_resolves_source_root_symlink_but_keeps_strict_tree_validation() {
    use std::os::unix::fs::symlink;

    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    set_central_path(&store, central.path());

    let real_source = tempfile::tempdir().unwrap();
    fs::write(
        real_source.path().join("SKILL.md"),
        "---\nname: source-link\ndescription: Imported through a root link\n---\n",
    )
    .unwrap();
    let source_parent = tempfile::tempdir().unwrap();
    let source_link = source_parent.path().join("source-link");
    symlink(real_source.path(), &source_link).unwrap();

    let imported = super::import_existing_local_skill(
        app.handle(),
        &store,
        &source_link,
        Some("source-link".to_string()),
    )
    .unwrap();

    assert_eq!(imported.central_path, central.path().join("source-link"));
    let record = store.get_skill_by_id(&imported.skill_id).unwrap().unwrap();
    let canonical_source = fs::canonicalize(real_source.path()).unwrap();
    assert_eq!(
        record.source_ref.as_deref(),
        Some(canonical_source.to_string_lossy().as_ref())
    );
    assert!(imported.central_path.join("SKILL.md").is_file());

    let nested_link_source = tempfile::tempdir().unwrap();
    fs::write(
        nested_link_source.path().join("SKILL.md"),
        "---\nname: nested-link\n---\n",
    )
    .unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    symlink(
        outside.path(),
        nested_link_source.path().join("linked-file"),
    )
    .unwrap();
    let nested_parent = tempfile::tempdir().unwrap();
    let nested_root_link = nested_parent.path().join("nested-link");
    symlink(nested_link_source.path(), &nested_root_link).unwrap();

    let err = match super::import_existing_local_skill(
        app.handle(),
        &store,
        &nested_root_link,
        Some("nested-link".to_string()),
    ) {
        Ok(_) => panic!("nested symlink must remain forbidden"),
        Err(err) => err,
    };
    assert!(format!("{err:#}").contains("symbolic link"));
}

#[cfg(unix)]
#[test]
fn install_local_requires_real_regular_skill_md_and_rejects_special_nodes() {
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;

    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    set_central_path(&store, central.path());

    let linked_md_source = tempfile::tempdir().unwrap();
    let real_md = tempfile::NamedTempFile::new().unwrap();
    fs::write(real_md.path(), "---\nname: linked-md\n---\n").unwrap();
    symlink(real_md.path(), linked_md_source.path().join("SKILL.md")).unwrap();
    let linked_md_err = match super::install_local_skill(
        app.handle(),
        &store,
        linked_md_source.path(),
        Some("linked-md".to_string()),
    ) {
        Ok(_) => panic!("expected linked SKILL.md rejection"),
        Err(err) => err,
    };
    assert!(format!("{linked_md_err:#}").contains("symbolic link"));

    let special_source = tempfile::tempdir().unwrap();
    fs::write(
        special_source.path().join("SKILL.md"),
        "---\nname: special-node\n---\n",
    )
    .unwrap();
    let _socket = UnixListener::bind(special_source.path().join("socket")).unwrap();
    let special_err = match super::install_local_skill(
        app.handle(),
        &store,
        special_source.path(),
        Some("special-node".to_string()),
    ) {
        Ok(_) => panic!("expected special-node rejection"),
        Err(err) => err,
    };
    assert!(format!("{special_err:#}").contains("special node"));

    assert!(store.list_skills().unwrap().is_empty());
    assert_eq!(fs::read_dir(central.path()).unwrap().count(), 0);
}

#[test]
fn staged_copy_failure_moves_hidden_partial_to_test_trash() {
    let central = tempfile::tempdir().unwrap();
    let err = match super::stage_skill_with(central.path(), "install", |staging| {
        fs::create_dir(staging).unwrap();
        fs::write(staging.join("partial"), b"incomplete").unwrap();
        anyhow::bail!("injected copy failure")
    }) {
        Ok(_) => panic!("expected injected staging failure"),
        Err(err) => err,
    };
    assert!(format!("{err:#}").contains("injected copy failure"));

    let visible = fs::read_dir(central.path())
        .unwrap()
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .count();
    assert_eq!(visible, 0);
    let trash = central.path().join(".skills-hub-test-trash");
    let moved = fs::read_dir(&trash)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(fs::read(moved.join("partial")).unwrap(), b"incomplete");
}

#[test]
fn concurrent_same_name_local_installs_publish_exactly_one_complete_skill() {
    use std::sync::{Arc, Barrier};

    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    set_central_path(&store, central.path());

    let source_one = tempfile::tempdir().unwrap();
    fs::write(
        source_one.path().join("SKILL.md"),
        "---\nname: first-source\n---\n",
    )
    .unwrap();
    fs::write(source_one.path().join("winner"), b"first").unwrap();
    let source_two = tempfile::tempdir().unwrap();
    fs::write(
        source_two.path().join("SKILL.md"),
        "---\nname: second-source\n---\n",
    )
    .unwrap();
    fs::write(source_two.path().join("winner"), b"second").unwrap();

    let barrier = Arc::new(Barrier::new(2));
    let first_app = app.handle().clone();
    let second_app = app.handle().clone();
    let first_store = store.clone();
    let second_store = store.clone();
    let first_source = source_one.path().to_path_buf();
    let second_source = source_two.path().to_path_buf();
    let first_barrier = barrier.clone();
    let second_barrier = barrier.clone();
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(move || {
            first_barrier.wait();
            super::install_local_skill(
                &first_app,
                &first_store,
                &first_source,
                Some("same-name".to_string()),
            )
        });
        let second = scope.spawn(move || {
            second_barrier.wait();
            super::install_local_skill(
                &second_app,
                &second_store,
                &second_source,
                Some("same-name".to_string()),
            )
        });
        (first.join().unwrap(), second.join().unwrap())
    });

    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let failed = match (first, second) {
        (Err(err), Ok(_)) | (Ok(_), Err(err)) => err,
        _ => panic!("expected one successful install and one collision"),
    };
    assert!(format!("{failed:#}").contains("skill already exists"));
    let live = central.path().join("same-name");
    assert!(live.join("SKILL.md").is_file());
    let winner = fs::read(live.join("winner")).unwrap();
    assert!(winner == b"first" || winner == b"second");
    assert_eq!(store.list_skills().unwrap().len(), 1);
    let visible = fs::read_dir(central.path())
        .unwrap()
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .count();
    assert_eq!(visible, 1);
}

#[test]
fn git_frontmatter_unsafe_name_is_rejected_before_staging() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    set_central_path(&store, central.path());
    let repo_dir = tempfile::tempdir().unwrap();
    fs::write(
        repo_dir.path().join("SKILL.md"),
        "---\nname: ../escape\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "unsafe frontmatter");

    let err = match super::install_git_skill(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        None,
        None,
    ) {
        Ok(_) => panic!("expected unsafe frontmatter rejection"),
        Err(err) => err,
    };
    assert!(format!("{err:#}").contains("unsafe_name"));
    assert!(!central.path().parent().unwrap().join("escape").exists());
    assert_eq!(fs::read_dir(central.path()).unwrap().count(), 0);
    assert!(store.list_skills().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn adopts_only_real_first_level_central_skills_idempotently() {
    use std::os::unix::fs::symlink;

    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    set_central_path(&store, central.path());

    let good = central.path().join("good-skill");
    fs::create_dir(&good).unwrap();
    fs::write(
        good.join("SKILL.md"),
        "---\nname: ignored-frontmatter-name\ndescription: Good\n---\n",
    )
    .unwrap();
    let hidden = central.path().join(".hidden-skill");
    fs::create_dir(&hidden).unwrap();
    fs::write(hidden.join("SKILL.md"), "---\nname: hidden\n---\n").unwrap();
    fs::create_dir(central.path().join("missing-md")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(
        outside.path().join("SKILL.md"),
        "---\nname: external\n---\n",
    )
    .unwrap();
    symlink(outside.path(), central.path().join("linked-skill")).unwrap();
    fs::write(central.path().join("ordinary-file"), b"x").unwrap();

    let first = super::adopt_existing_central_skills(app.handle(), &store).unwrap();
    assert_eq!(first.adopted, 1);
    assert_eq!(first.skipped_invalid_name, 1);
    assert_eq!(first.skipped_symlink, 1);
    assert_eq!(first.skipped_missing_skill_md, 1);
    assert_eq!(first.skipped_other, 1);
    let skills = store.list_skills().unwrap();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "good-skill");
    assert_eq!(PathBuf::from(&skills[0].central_path), good);
    assert_eq!(skills[0].source_type, "managed");
    assert!(skills[0].source_ref.is_none());

    let second = super::adopt_existing_central_skills(app.handle(), &store).unwrap();
    assert_eq!(second.adopted, 0);
    assert_eq!(second.already_registered, 1);
    assert_eq!(store.list_skills().unwrap().len(), 1);
}

#[test]
fn local_update_rejects_central_path_as_its_own_source() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    set_central_path(&store, central.path());
    let skill_path = central.path().join("self-source");
    fs::create_dir(&skill_path).unwrap();
    fs::write(skill_path.join("SKILL.md"), "# Keep me").unwrap();
    store
        .upsert_skill(&SkillRecord {
            id: "self-source-id".to_string(),
            name: "self-source".to_string(),
            description: None,
            source_type: "local".to_string(),
            source_ref: Some(skill_path.to_string_lossy().to_string()),
            source_subpath: None,
            source_revision: None,
            central_path: skill_path.to_string_lossy().to_string(),
            content_hash: None,
            created_at: 1,
            updated_at: 1,
            last_sync_at: None,
            last_seen_at: 1,
            enabled: true,
            status: "ok".to_string(),
        })
        .unwrap();

    let err = match super::update_managed_skill_from_source(app.handle(), &store, "self-source-id")
    {
        Ok(_) => panic!("expected self-source update rejection"),
        Err(err) => err,
    };
    assert!(format!("{err:#}").contains("NO_EXTERNAL_SOURCE"));
    assert_eq!(
        fs::read_to_string(skill_path.join("SKILL.md")).unwrap(),
        "# Keep me"
    );
    assert!(!central.path().join(".skills-hub-test-trash").exists());
}
