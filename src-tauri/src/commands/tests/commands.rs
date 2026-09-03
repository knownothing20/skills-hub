use super::*;
use crate::core::skill_store::SkillRecord;
use crate::core::tool_adapters::{save_tool_config, CustomToolConfig, ToolConfig};

fn make_store() -> (tempfile::TempDir, SkillStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SkillStore::new(dir.path().join("test.db"));
    store.ensure_schema().expect("ensure_schema");
    (dir, store)
}

struct EnableFixture {
    _dir: tempfile::TempDir,
    store: SkillStore,
    db_path: std::path::PathBuf,
    target_paths: Vec<std::path::PathBuf>,
}

fn make_enable_fixture(target_count: usize) -> EnableFixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let store = SkillStore::new(db_path.clone());
    store.ensure_schema().expect("ensure_schema");

    let central_root = dir.path().join("central");
    let managed_skill = central_root.join("atomic-enable");
    std::fs::create_dir_all(&managed_skill).unwrap();
    std::fs::write(
        managed_skill.join("SKILL.md"),
        "---\nname: atomic-enable\n---\n",
    )
    .unwrap();
    store
        .set_setting("central_repo_path", central_root.to_string_lossy().as_ref())
        .unwrap();
    store
        .upsert_skill(&SkillRecord {
            id: "atomic-enable".to_string(),
            name: "atomic-enable".to_string(),
            description: None,
            source_type: "managed".to_string(),
            source_ref: None,
            source_subpath: None,
            source_revision: None,
            central_path: managed_skill.to_string_lossy().to_string(),
            content_hash: None,
            created_at: 1,
            updated_at: 2,
            last_sync_at: None,
            last_seen_at: 1,
            enabled: false,
            status: "ok".to_string(),
        })
        .unwrap();

    let mut custom_tools = Vec::new();
    let mut target_paths = Vec::new();
    for index in 0..target_count {
        let tool_root = dir.path().join(format!("tool-{index}-skills"));
        std::fs::create_dir_all(&tool_root).unwrap();
        let tool_key = format!("custom_{index}");
        custom_tools.push(CustomToolConfig {
            key: tool_key.clone(),
            label: format!("Custom {index}"),
            avatar: None,
            skills_dir: tool_root.to_string_lossy().to_string(),
            project_skills_dir: None,
            sync_mode: SyncMode::Copy,
            enabled: true,
        });
        let target_path = tool_root.join("atomic-enable");
        store
            .upsert_skill_target(&SkillTargetRecord {
                id: format!("target-{index}"),
                skill_id: "atomic-enable".to_string(),
                tool: tool_key,
                scope: "global".to_string(),
                project_path: None,
                target_path: target_path.to_string_lossy().to_string(),
                mode: "copy".to_string(),
                status: "disabled".to_string(),
                last_error: Some("saved-disabled-state".to_string()),
                synced_at: None,
            })
            .unwrap();
        target_paths.push(target_path);
    }
    save_tool_config(
        &store,
        ToolConfig {
            disabled_builtin_tools: Vec::new(),
            custom_tools,
        },
    )
    .unwrap();

    EnableFixture {
        _dir: dir,
        store,
        db_path,
        target_paths,
    }
}

fn assert_enable_fixture_database_rolled_back(fixture: &EnableFixture) {
    let skill = fixture
        .store
        .get_skill_by_id("atomic-enable")
        .unwrap()
        .unwrap();
    assert!(!skill.enabled);
    let targets = fixture.store.list_skill_targets("atomic-enable").unwrap();
    assert!(targets.iter().all(|target| target.status == "disabled"));
    assert!(targets
        .iter()
        .all(|target| target.last_error.as_deref() == Some("saved-disabled-state")));
    assert!(targets.iter().all(|target| target.synced_at.is_none()));
}

fn assert_enable_fixture_rolled_back(fixture: &EnableFixture) {
    assert_enable_fixture_database_rolled_back(fixture);
    assert!(fixture.target_paths.iter().all(|path| !path.exists()));
}

#[test]
fn format_anyhow_error_passthrough_prefixes() {
    for message in [
        "MULTI_SKILLS|abc",
        "TARGET_EXISTS|/tmp/skill",
        "TOOL_NOT_INSTALLED|cursor",
        "TOOL_NOT_WRITABLE|Cursor|/tmp/skills",
    ] {
        assert_eq!(format_anyhow_error(anyhow::anyhow!(message)), message);
    }
}

#[test]
fn format_anyhow_error_redacts_clone_temp_path() {
    let err = anyhow::anyhow!("clone https://example.com/a/b into /tmp/skills-hub-git-123");
    let msg = format_anyhow_error(err);
    assert!(msg.contains("已省略临时目录"));
    assert!(!msg.contains("/tmp/skills-hub-git-123"));
}

#[test]
fn format_anyhow_error_github_hint_auth() {
    let err = anyhow::anyhow!("git clone https://github.com/a/b failed: authentication failed");
    let msg = format_anyhow_error(err);
    assert!(msg.contains("无法访问该仓库"));
}

#[test]
fn expand_home_path_basic() {
    let home = dirs::home_dir().expect("home");
    assert_eq!(expand_home_path("~").unwrap(), home);
    assert_eq!(expand_home_path("~/abc").unwrap(), home.join("abc"));
}

#[test]
fn expand_home_path_empty_is_error() {
    let err = expand_home_path("  ").unwrap_err().to_string();
    assert!(err.contains("storage path is empty"));
}

#[test]
fn saving_custom_tool_config_creates_enabled_skills_dir() {
    let (dir, store) = make_store();
    let existing = dir.path().join("existing-skills");
    std::fs::create_dir_all(&existing).unwrap();
    let created = dir.path().join("created-skills");
    assert!(!created.exists());

    save_tool_config(
        &store,
        ToolConfig {
            disabled_builtin_tools: Vec::new(),
            custom_tools: vec![
                CustomToolConfig {
                    key: "custom_existing".to_string(),
                    label: "Existing".to_string(),
                    avatar: Some("data:image/png;base64,AA==".to_string()),
                    skills_dir: existing.to_string_lossy().to_string(),
                    project_skills_dir: None,
                    sync_mode: SyncMode::Auto,
                    enabled: true,
                },
                CustomToolConfig {
                    key: "custom_created".to_string(),
                    label: "Created".to_string(),
                    avatar: None,
                    skills_dir: created.to_string_lossy().to_string(),
                    project_skills_dir: None,
                    sync_mode: SyncMode::Copy,
                    enabled: true,
                },
            ],
        },
    )
    .unwrap();
    assert!(created.is_dir());

    let tools = runtime_tools(&store, true).unwrap();
    let existing_tool = tools
        .iter()
        .find(|tool| tool.key == "custom_existing")
        .unwrap();
    let created_tool = tools
        .iter()
        .find(|tool| tool.key == "custom_created")
        .unwrap();

    assert!(existing_tool.enabled);
    assert!(existing_tool.installed);
    assert_eq!(
        existing_tool.avatar.as_deref(),
        Some("data:image/png;base64,AA==")
    );
    assert_eq!(existing_tool.sync_mode, SyncMode::Auto);
    assert!(created_tool.enabled);
    assert!(created_tool.installed);
    assert_eq!(created_tool.sync_mode, SyncMode::Copy);
}

#[test]
fn normalize_scope_defaults_to_global_and_rejects_unknown() {
    assert_eq!(normalize_scope(None).unwrap(), "global");
    assert_eq!(normalize_scope(Some("global")).unwrap(), "global");
    assert_eq!(normalize_scope(Some("project")).unwrap(), "project");
    assert!(normalize_scope(Some("workspace")).is_err());
}

#[test]
fn recent_projects_are_deduped_ordered_and_limited() {
    let (_dir, store) = make_store();
    let project_root = tempfile::tempdir().unwrap();
    let mut paths = Vec::new();
    for i in 0..9 {
        let path = project_root.path().join(format!("project-{i}"));
        std::fs::create_dir_all(&path).unwrap();
        paths.push(path);
    }

    for path in &paths {
        save_recent_project_impl(&store, path.to_string_lossy().as_ref()).unwrap();
    }

    let recent = get_recent_projects_impl(&store).unwrap();
    assert_eq!(recent.len(), 8);
    assert_eq!(recent[0], paths[8].to_string_lossy());
    assert_eq!(recent[7], paths[1].to_string_lossy());
    assert!(!recent.contains(&paths[0].to_string_lossy().to_string()));

    save_recent_project_impl(&store, paths[3].to_string_lossy().as_ref()).unwrap();
    let recent = get_recent_projects_impl(&store).unwrap();
    assert_eq!(recent.len(), 8);
    assert_eq!(recent[0], paths[3].to_string_lossy());
    assert_eq!(
        recent
            .iter()
            .filter(|item| *item == &paths[3].to_string_lossy())
            .count(),
        1
    );
}

#[test]
fn save_recent_project_rejects_missing_directory() {
    let (_dir, store) = make_store();
    let missing = tempfile::tempdir().unwrap().path().join("missing-project");
    let err = save_recent_project_impl(&store, missing.to_string_lossy().as_ref())
        .unwrap_err()
        .to_string();
    assert!(err.contains("projectPath must be an existing directory"));
}

#[test]
fn get_managed_skills_impl_maps_targets() {
    let (dir, store) = make_store();
    let skill = SkillRecord {
        id: "s1".to_string(),
        name: "S1".to_string(),
        description: None,
        source_type: "local".to_string(),
        source_ref: Some(
            dir.path()
                .join("missing-source")
                .to_string_lossy()
                .to_string(),
        ),
        source_subpath: None,
        source_revision: None,
        central_path: "/tmp/central".to_string(),
        content_hash: None,
        created_at: 1,
        updated_at: 2,
        last_sync_at: None,
        last_seen_at: 1,
        enabled: true,
        status: "ok".to_string(),
    };
    store.upsert_skill(&skill).unwrap();

    let target = SkillTargetRecord {
        id: "t1".to_string(),
        skill_id: "s1".to_string(),
        tool: "cursor".to_string(),
        scope: "global".to_string(),
        project_path: None,
        target_path: "/tmp/target".to_string(),
        mode: "copy".to_string(),
        status: "error".to_string(),
        last_error: Some("permission denied".to_string()),
        synced_at: None,
    };
    store.upsert_skill_target(&target).unwrap();
    let tag = store.create_tag("Frontend").unwrap();
    store.set_skill_tags("s1", &[tag.id]).unwrap();

    let out = get_managed_skills_impl(&store).unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].enabled);
    assert_eq!(out[0].tags.len(), 1);
    assert_eq!(out[0].tags[0].name, "Frontend");
    assert_eq!(out[0].targets.len(), 1);
    assert_eq!(out[0].targets[0].tool, "cursor");
    assert_eq!(out[0].targets[0].scope, "global");
    assert_eq!(out[0].targets[0].status, "error");
    assert_eq!(
        out[0].targets[0].last_error.as_deref(),
        Some("permission denied")
    );
    assert!(out[0].targets[0].project_path.is_none());
    assert_eq!(out[0].status, "error");
    assert!(out[0].has_external_source);
    assert!(!out[0].updateable);
}

#[test]
fn get_managed_skills_impl_embeds_standard_icon_without_exposing_asset_path() {
    let (dir, store) = make_store();
    let central = dir.path().join("metadata-icon-skill");
    std::fs::create_dir_all(central.join("agents")).unwrap();
    std::fs::create_dir_all(central.join("assets")).unwrap();
    std::fs::write(
        central.join("agents/openai.yaml"),
        "interface:\n  icon_small: \"./assets/icon.svg\"\n  brand_color: \"#3B82F6\"\n",
    )
    .unwrap();
    std::fs::write(
        central.join("assets/icon.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"></svg>",
    )
    .unwrap();
    store
        .upsert_skill(&SkillRecord {
            id: "metadata-icon".to_string(),
            name: "metadata-icon".to_string(),
            description: None,
            source_type: "managed".to_string(),
            source_ref: None,
            source_subpath: None,
            source_revision: None,
            central_path: central.to_string_lossy().to_string(),
            content_hash: None,
            created_at: 1,
            updated_at: 2,
            last_sync_at: None,
            last_seen_at: 1,
            enabled: true,
            status: "ok".to_string(),
        })
        .unwrap();

    let out = get_managed_skills_impl(&store).unwrap();
    assert!(out[0]
        .icon_data_url
        .as_deref()
        .unwrap()
        .starts_with("data:image/svg+xml;base64,"));
    assert_eq!(out[0].brand_color.as_deref(), Some("#3B82F6"));
    let serialized = serde_json::to_string(&out).unwrap();
    assert!(!serialized.contains("assets/icon.svg"));
}

#[test]
fn invalid_icon_metadata_does_not_change_skill_health() {
    let (dir, store) = make_store();
    let central = dir.path().join("invalid-icon-skill");
    std::fs::create_dir_all(central.join("agents")).unwrap();
    std::fs::write(
        central.join("agents/openai.yaml"),
        "interface:\n  icon_small: \"../outside.png\"\n",
    )
    .unwrap();
    store
        .upsert_skill(&SkillRecord {
            id: "invalid-icon".to_string(),
            name: "invalid-icon".to_string(),
            description: None,
            source_type: "managed".to_string(),
            source_ref: None,
            source_subpath: None,
            source_revision: None,
            central_path: central.to_string_lossy().to_string(),
            content_hash: None,
            created_at: 1,
            updated_at: 2,
            last_sync_at: None,
            last_seen_at: 1,
            enabled: true,
            status: "ok".to_string(),
        })
        .unwrap();

    let out = get_managed_skills_impl(&store).unwrap();
    assert!(out[0].icon_data_url.is_none());
    assert_eq!(out[0].status, "ok");
}

#[test]
fn managed_icon_data_url_budget_drops_the_overflow_and_all_later_icons() {
    let mut remaining = 10usize;
    let mut first = Some("123456".to_string());
    apply_managed_icon_data_url_budget(&mut first, &mut remaining);
    assert!(first.is_some());
    assert_eq!(remaining, 4);

    let mut overflow = Some("12345".to_string());
    apply_managed_icon_data_url_budget(&mut overflow, &mut remaining);
    assert!(overflow.is_none());
    assert_eq!(remaining, 0);

    let mut later = Some("1".to_string());
    apply_managed_icon_data_url_budget(&mut later, &mut remaining);
    assert!(later.is_none());
}

#[test]
fn managed_list_budget_keeps_earlier_icon_and_falls_back_later_without_health_error() {
    let (dir, store) = make_store();
    for (id, updated_at) in [("first-icon", 2), ("second-icon", 1)] {
        let central = dir.path().join(id);
        std::fs::create_dir_all(central.join("agents")).unwrap();
        std::fs::create_dir_all(central.join("assets")).unwrap();
        std::fs::write(
            central.join("agents/openai.yaml"),
            "interface:\n  icon_small: './assets/icon.svg'\n",
        )
        .unwrap();
        std::fs::write(
            central.join("assets/icon.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"><path d=\"M0 0\"/></svg>",
        )
        .unwrap();
        store
            .upsert_skill(&SkillRecord {
                id: id.to_string(),
                name: id.to_string(),
                description: None,
                source_type: "managed".to_string(),
                source_ref: None,
                source_subpath: None,
                source_revision: None,
                central_path: central.to_string_lossy().to_string(),
                content_hash: None,
                created_at: 1,
                updated_at,
                last_sync_at: None,
                last_seen_at: 1,
                enabled: true,
                status: "ok".to_string(),
            })
            .unwrap();
    }

    let unlimited = get_managed_skills_impl(&store).unwrap();
    let first_icon_bytes = unlimited[0].icon_data_url.as_ref().unwrap().len();
    let budgeted = get_managed_skills_impl_with_icon_budget(&store, first_icon_bytes).unwrap();
    assert_eq!(budgeted[0].name, "first-icon");
    assert!(budgeted[0].icon_data_url.is_some());
    assert_eq!(budgeted[1].name, "second-icon");
    assert!(budgeted[1].icon_data_url.is_none());
    assert!(budgeted.iter().all(|skill| skill.status == "ok"));
}

#[test]
fn get_managed_skills_impl_redacts_unsafe_git_source_refs() {
    let (dir, store) = make_store();
    let central = dir.path().join("unsafe-git");
    std::fs::create_dir(&central).unwrap();
    let mut skill = SkillRecord {
        id: "unsafe-git".to_string(),
        name: "unsafe-git".to_string(),
        description: None,
        source_type: "git".to_string(),
        source_ref: Some("https://secret-token@example.com/repo.git".to_string()),
        source_subpath: None,
        source_revision: None,
        central_path: central.to_string_lossy().to_string(),
        content_hash: None,
        created_at: 1,
        updated_at: 2,
        last_sync_at: None,
        last_seen_at: 1,
        enabled: true,
        status: "ok".to_string(),
    };
    store.upsert_skill(&skill).unwrap();

    let out = get_managed_skills_impl(&store).unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].source_ref.is_none());
    assert!(!out[0].updateable);
    assert_eq!(out[0].status, "error");
    let serialized = serde_json::to_string(&out).unwrap();
    assert!(!serialized.contains("secret-token"));

    skill.source_ref = Some("anthropics/skills".to_string());
    store.upsert_skill(&skill).unwrap();
    let out = get_managed_skills_impl(&store).unwrap();
    assert_eq!(out[0].source_ref.as_deref(), Some("anthropics/skills"));
    assert!(out[0].updateable);
}

#[test]
fn managed_skill_status_keeps_existing_local_sources_healthy() {
    let source = tempfile::tempdir().unwrap();
    let central = tempfile::tempdir().unwrap();
    let skill = SkillRecord {
        id: "s1".to_string(),
        name: "S1".to_string(),
        description: None,
        source_type: "local".to_string(),
        source_ref: Some(source.path().to_string_lossy().to_string()),
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
    };

    let capability = managed_skill_update_capability(&skill);
    assert_eq!(managed_skill_status(&skill, &capability), "ok");
}

#[test]
fn record_skill_target_failure_persists_error_status() {
    let (_dir, store) = make_store();
    let skill = SkillRecord {
        id: "s1".to_string(),
        name: "S1".to_string(),
        description: None,
        source_type: "local".to_string(),
        source_ref: Some("/tmp/src".to_string()),
        source_subpath: None,
        source_revision: None,
        central_path: "/tmp/central".to_string(),
        content_hash: None,
        created_at: 1,
        updated_at: 2,
        last_sync_at: None,
        last_seen_at: 1,
        enabled: true,
        status: "ok".to_string(),
    };
    store.upsert_skill(&skill).unwrap();

    record_skill_target_failure(
        &store,
        "s1",
        "cursor",
        "global",
        None,
        std::path::Path::new("/tmp/target"),
        SyncMode::Copy,
        "permission denied",
    )
    .unwrap();

    let target = store
        .get_skill_target("s1", "cursor", "global", None)
        .unwrap()
        .unwrap();
    assert_eq!(target.status, "error");
    assert_eq!(target.last_error.as_deref(), Some("permission denied"));
    assert_eq!(target.mode, "copy");
    assert!(target.synced_at.is_none());
}

#[test]
fn atomic_enable_restores_every_target_before_enabling() {
    let fixture = make_enable_fixture(2);
    let app = tauri::test::mock_app();

    let result =
        enable_skill_and_restore_targets_impl(app.handle(), &fixture.store, "atomic-enable")
            .unwrap();

    assert_eq!(result.restored_targets, 2);
    assert!(
        fixture
            .store
            .get_skill_by_id("atomic-enable")
            .unwrap()
            .unwrap()
            .enabled
    );
    let targets = fixture.store.list_skill_targets("atomic-enable").unwrap();
    assert!(targets.iter().all(|target| target.status == "ok"));
    assert!(targets.iter().all(|target| target.last_error.is_none()));
    assert!(targets.iter().all(|target| target.synced_at.is_some()));
    for path in &fixture.target_paths {
        assert_eq!(
            std::fs::read_to_string(path.join("SKILL.md")).unwrap(),
            "---\nname: atomic-enable\n---\n"
        );
    }
}

#[cfg(unix)]
#[test]
fn atomic_enable_stages_and_atomically_publishes_saved_symlink_mode() {
    let fixture = make_enable_fixture(1);
    let app = tauri::test::mock_app();
    let mut target = fixture
        .store
        .list_skill_targets("atomic-enable")
        .unwrap()
        .remove(0);
    target.mode = "symlink".to_string();
    fixture.store.upsert_skill_target(&target).unwrap();

    enable_skill_and_restore_targets_impl(app.handle(), &fixture.store, "atomic-enable").unwrap();

    assert!(std::fs::symlink_metadata(&fixture.target_paths[0])
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        fixture.store.list_skill_targets("atomic-enable").unwrap()[0].mode,
        "symlink"
    );
}

#[test]
fn atomic_enable_rolls_back_when_the_nth_target_restore_fails() {
    let fixture = make_enable_fixture(3);
    let app = tauri::test::mock_app();
    let mut attempt = 0usize;

    let err = enable_skill_and_restore_targets_with(
        app.handle(),
        &fixture.store,
        "atomic-enable",
        |_source, plan| {
            attempt += 1;
            if attempt == 2 {
                anyhow::bail!("injected nth target failure");
            }
            std::fs::create_dir(&plan.path)?;
            std::fs::write(plan.path.join("created"), format!("attempt {attempt}"))?;
            Ok(OwnedEnableTarget {
                root: plan.root.clone(),
                path: plan.path.clone(),
                identity: capture_enable_path_identity(&plan.path)?,
                mode_used: SyncMode::Copy,
            })
        },
    )
    .unwrap_err();

    assert!(format!("{err:#}").contains("injected nth target failure"));
    assert_eq!(attempt, 2);
    assert_enable_fixture_rolled_back(&fixture);
}

#[test]
fn atomic_enable_preserves_competing_copy_target_created_after_preflight() {
    let fixture = make_enable_fixture(1);
    let app = tauri::test::mock_app();

    let err = enable_skill_and_restore_targets_with(
        app.handle(),
        &fixture.store,
        "atomic-enable",
        |source, plan| {
            restore_enable_target_staged_with(source, plan, |plan, _staged| {
                std::fs::create_dir(&plan.path)?;
                std::fs::write(plan.path.join("owner"), "competing-process")?;
                Ok(())
            })
        },
    )
    .unwrap_err();

    assert!(format!("{err:#}").contains("SKILL_EXISTS"));
    assert_eq!(
        std::fs::read_to_string(fixture.target_paths[0].join("owner")).unwrap(),
        "competing-process"
    );
    assert!(!fixture.target_paths[0].join("SKILL.md").exists());
    assert_enable_fixture_database_rolled_back(&fixture);
}

#[test]
fn atomic_enable_rollback_preserves_a_replaced_journal_target() {
    let fixture = make_enable_fixture(2);
    let app = tauri::test::mock_app();
    let first_target = fixture.target_paths[0].clone();
    let displaced_target = first_target
        .parent()
        .unwrap()
        .join("operation-created-displaced");
    let mut attempt = 0usize;

    let err = enable_skill_and_restore_targets_with(
        app.handle(),
        &fixture.store,
        "atomic-enable",
        |_source, plan| {
            attempt += 1;
            if attempt == 2 {
                std::fs::rename(&first_target, &displaced_target)?;
                std::fs::create_dir(&first_target)?;
                std::fs::write(first_target.join("owner"), "competing-process")?;
                anyhow::bail!("injected later target failure");
            }
            std::fs::create_dir(&plan.path)?;
            Ok(OwnedEnableTarget {
                root: plan.root.clone(),
                path: plan.path.clone(),
                identity: capture_enable_path_identity(&plan.path)?,
                mode_used: SyncMode::Copy,
            })
        },
    )
    .unwrap_err();

    let message = format!("{err:#}");
    assert!(message.contains("ROLLBACK_INCOMPLETE"));
    assert!(message.contains("OWNERSHIP_CHANGED"));
    assert_eq!(
        std::fs::read_to_string(first_target.join("owner")).unwrap(),
        "competing-process"
    );
    assert!(displaced_target.is_dir());
    assert_enable_fixture_database_rolled_back(&fixture);
}

#[test]
fn atomic_enable_rolls_back_files_rows_and_enabled_on_db_failure() {
    let fixture = make_enable_fixture(2);
    let app = tauri::test::mock_app();
    let connection = rusqlite::Connection::open(&fixture.db_path).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_second_target_restore
             BEFORE UPDATE OF status ON skill_targets
             WHEN NEW.id = 'target-1' AND NEW.status = 'ok'
             BEGIN
               SELECT RAISE(FAIL, 'injected target row failure');
             END;
             CREATE TRIGGER reject_compensating_target_write
             BEFORE UPDATE OF status ON skill_targets
             WHEN NEW.id = 'target-0' AND NEW.status = 'disabled'
             BEGIN
               SELECT RAISE(FAIL, 'compensating writes are forbidden');
             END;",
        )
        .unwrap();
    drop(connection);

    let err = enable_skill_and_restore_targets_impl(app.handle(), &fixture.store, "atomic-enable")
        .unwrap_err();

    assert!(format!("{err:#}").contains("injected target row failure"));
    assert_enable_fixture_rolled_back(&fixture);
}

