use super::{
    managed_skill_update_capability, normalize_self_sourced_local_skills,
    record_auto_update_progress, record_auto_update_progress_snapshot, record_auto_update_result,
    record_auto_update_started, record_auto_update_triggered, record_update_error,
    sanitize_unsafe_git_sources, AutoUpdateProgressSnapshot, AutoUpdateRunResult,
    AutoUpdateSkillProgress,
};
use crate::core::auto_update::{
    get_auto_update_config, is_auto_update_due, set_auto_update_config, AutoUpdateConfig,
    AutoUpdateIntervalUnit, AutoUpdateSchedule, AutoUpdateScheduleType,
    AUTO_UPDATE_LAST_CHECKED_KEY, AUTO_UPDATE_LAST_ERROR_KEY, DEFAULT_AUTO_UPDATE_INTERVAL_HOURS,
};
use crate::core::skill_store::{SkillRecord, SkillStore};

fn make_store() -> (tempfile::TempDir, SkillStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("test.db");
    let store = SkillStore::new(db);
    store.ensure_schema().expect("ensure_schema");
    (dir, store)
}

fn make_skill(id: &str, source_type: &str, central_path: &str) -> SkillRecord {
    SkillRecord {
        id: id.to_string(),
        name: id.to_string(),
        description: None,
        source_type: source_type.to_string(),
        source_ref: Some("/tmp/source".to_string()),
        source_subpath: None,
        source_revision: None,
        central_path: central_path.to_string(),
        content_hash: None,
        created_at: 1,
        updated_at: 1,
        last_sync_at: None,
        last_seen_at: 1,
        enabled: true,
        status: "ok".to_string(),
    }
}

fn hourly_schedule(hours: i64) -> AutoUpdateSchedule {
    AutoUpdateSchedule {
        schedule_type: AutoUpdateScheduleType::Interval,
        interval_value: hours,
        interval_unit: AutoUpdateIntervalUnit::Hours,
        daily_time: "03:00".to_string(),
    }
}

fn minute_schedule(minutes: i64) -> AutoUpdateSchedule {
    AutoUpdateSchedule {
        schedule_type: AutoUpdateScheduleType::Interval,
        interval_value: minutes,
        interval_unit: AutoUpdateIntervalUnit::Minutes,
        daily_time: "03:00".to_string(),
    }
}

fn daily_schedule(time: &str) -> AutoUpdateSchedule {
    AutoUpdateSchedule {
        schedule_type: AutoUpdateScheduleType::Daily,
        interval_value: 24,
        interval_unit: AutoUpdateIntervalUnit::Hours,
        daily_time: time.to_string(),
    }
}

#[test]
fn default_config_is_disabled_with_24_hour_interval() {
    let (_dir, store) = make_store();

    let config = get_auto_update_config(&store).unwrap();

    assert!(!config.enabled);
    assert_eq!(config.interval_hours, DEFAULT_AUTO_UPDATE_INTERVAL_HOURS);
    assert_eq!(config.last_run_at, None);
    assert_eq!(config.last_status.as_deref(), None);
}

#[test]
fn config_roundtrips_and_rejects_invalid_interval() {
    let (_dir, store) = make_store();

    let saved = set_auto_update_config(
        &store,
        AutoUpdateConfig {
            enabled: true,
            interval_hours: 12,
            schedule: hourly_schedule(12),
            local_skill_count: 0,
            protected_local_skill_count: 0,
            last_run_at: None,
            last_started_at: None,
            last_finished_at: None,
            last_status: None,
            last_error: None,
            last_checked: 0,
            last_updated: 0,
            last_failed: 0,
            progress: AutoUpdateProgressSnapshot::default(),
        },
    )
    .unwrap();

    assert!(saved.enabled);
    assert_eq!(saved.interval_hours, 12);
    assert_eq!(saved.schedule, hourly_schedule(12));
    assert_eq!(get_auto_update_config(&store).unwrap().interval_hours, 12);

    let err = set_auto_update_config(
        &store,
        AutoUpdateConfig {
            enabled: true,
            interval_hours: 0,
            schedule: minute_schedule(10),
            local_skill_count: 0,
            protected_local_skill_count: 0,
            last_run_at: None,
            last_started_at: None,
            last_finished_at: None,
            last_status: None,
            last_error: None,
            last_checked: 0,
            last_updated: 0,
            last_failed: 0,
            progress: AutoUpdateProgressSnapshot::default(),
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("interval"));
}

#[test]
fn schedule_supports_minutes_and_daily_time() {
    let (_dir, store) = make_store();

    let saved = set_auto_update_config(
        &store,
        AutoUpdateConfig {
            enabled: true,
            interval_hours: 1,
            schedule: minute_schedule(30),
            local_skill_count: 0,
            protected_local_skill_count: 0,
            last_run_at: None,
            last_started_at: None,
            last_finished_at: None,
            last_status: None,
            last_error: None,
            last_checked: 0,
            last_updated: 0,
            last_failed: 0,
            progress: AutoUpdateProgressSnapshot::default(),
        },
    )
    .unwrap();
    assert_eq!(saved.interval_hours, 1);
    assert_eq!(saved.schedule, minute_schedule(30));

    let saved = set_auto_update_config(
        &store,
        AutoUpdateConfig {
            schedule: daily_schedule("23:45"),
            ..saved
        },
    )
    .unwrap();
    assert_eq!(saved.interval_hours, 24);
    assert_eq!(saved.schedule, daily_schedule("23:45"));
}

#[test]
fn due_check_respects_enabled_state_and_interval() {
    let disabled = AutoUpdateConfig {
        enabled: false,
        interval_hours: 24,
        schedule: hourly_schedule(24),
        local_skill_count: 0,
        protected_local_skill_count: 0,
        last_run_at: Some(1_000),
        last_started_at: Some(1_000),
        last_finished_at: Some(1_000),
        last_status: None,
        last_error: None,
        last_checked: 0,
        last_updated: 0,
        last_failed: 0,
        progress: AutoUpdateProgressSnapshot::default(),
    };
    assert!(!is_auto_update_due(&disabled, 1_000 + 48 * 60 * 60 * 1000));

    let enabled_never_run = AutoUpdateConfig {
        enabled: true,
        ..disabled.clone()
    };
    let enabled_never_run = AutoUpdateConfig {
        last_run_at: None,
        ..enabled_never_run
    };
    assert!(is_auto_update_due(&enabled_never_run, 1_000));

    let recent = AutoUpdateConfig {
        enabled: true,
        interval_hours: 24,
        schedule: hourly_schedule(24),
        last_run_at: Some(1_000),
        ..disabled
    };
    assert!(!is_auto_update_due(&recent, 1_000 + 23 * 60 * 60 * 1000));
    assert!(is_auto_update_due(&recent, 1_000 + 24 * 60 * 60 * 1000));

    let minute_interval = AutoUpdateConfig {
        schedule: minute_schedule(30),
        last_run_at: Some(1_000),
        ..recent
    };
    assert!(!is_auto_update_due(
        &minute_interval,
        1_000 + 29 * 60 * 1000
    ));
    assert!(is_auto_update_due(&minute_interval, 1_000 + 30 * 60 * 1000));
}

#[test]
fn eligible_skills_include_git_and_local_sources() {
    let (_dir, store) = make_store();
    store
        .upsert_skill(&make_skill("git-skill", "git", "/tmp/git-skill"))
        .unwrap();
    store
        .upsert_skill(&make_skill("local-skill", "local", "/tmp/local-skill"))
        .unwrap();
    store
        .upsert_skill(&make_skill("other-skill", "generated", "/tmp/other-skill"))
        .unwrap();

    let ids = crate::core::auto_update::list_auto_update_skill_ids(&store).unwrap();

    assert_eq!(
        ids,
        vec!["git-skill".to_string(), "local-skill".to_string()]
    );
}

#[test]
fn config_reports_local_skills_for_permission_hint() {
    let (_dir, store) = make_store();
    store
        .upsert_skill(&make_skill("local-skill", "local", "/tmp/local-skill"))
        .unwrap();
    store
        .upsert_skill(&make_skill("git-skill", "git", "/tmp/git-skill"))
        .unwrap();

    let config = get_auto_update_config(&store).unwrap();

    assert_eq!(config.local_skill_count, 1);
}

#[test]
fn progress_snapshot_is_persisted_while_update_is_running() {
    let (_dir, store) = make_store();

    record_auto_update_progress(
        &store,
        &AutoUpdateRunResult {
            checked: 60,
            updated: 12,
            skipped: 0,
            failed: 3,
            errors: vec!["skill-a: network timeout".to_string()],
            progress: AutoUpdateProgressSnapshot::default(),
        },
    )
    .unwrap();

    let config = get_auto_update_config(&store).unwrap();

    assert_eq!(config.last_status.as_deref(), Some("running"));
    assert_eq!(config.last_checked, 60);
    assert_eq!(config.last_updated, 12);
    assert_eq!(config.last_failed, 3);
    assert_eq!(
        config.last_error.as_deref(),
        Some("skill-a: network timeout")
    );
}

#[test]
fn starting_update_clears_previous_result_and_progress() {
    let (_dir, store) = make_store();
    record_auto_update_progress(
        &store,
        &AutoUpdateRunResult {
            checked: 2,
            updated: 1,
            skipped: 0,
            failed: 1,
            errors: vec!["old-skill: old error".to_string()],
            progress: AutoUpdateProgressSnapshot {
                total: 2,
                succeeded: vec![AutoUpdateSkillProgress {
                    skill_id: "done".to_string(),
                    name: "Done".to_string(),
                    reason: None,
                }],
                skipped: vec![],
                failed: vec![AutoUpdateSkillProgress {
                    skill_id: "bad".to_string(),
                    name: "Bad".to_string(),
                    reason: Some("old error".to_string()),
                }],
                running: None,
                pending: vec![],
            },
        },
    )
    .unwrap();

    record_auto_update_started(&store, 3).unwrap();

    let config = get_auto_update_config(&store).unwrap();

    assert_eq!(config.last_status.as_deref(), Some("running"));
    assert!(config.last_started_at.is_some());
    assert_eq!(config.last_finished_at, None);
    assert_eq!(config.last_checked, 3);
    assert_eq!(config.last_updated, 0);
    assert_eq!(config.last_failed, 0);
    assert_eq!(config.last_error.as_deref(), Some(""));
    assert_eq!(config.progress.total, 3);
    assert!(config.progress.succeeded.is_empty());
    assert!(config.progress.failed.is_empty());
    assert!(config.progress.running.is_none());
    assert!(config.progress.pending.is_empty());
}

#[test]
fn started_and_finished_times_are_recorded_separately() {
    let (_dir, store) = make_store();

    record_auto_update_started(&store, 1).unwrap();
    let running = get_auto_update_config(&store).unwrap();
    assert!(running.last_started_at.is_some());
    assert_eq!(running.last_finished_at, None);

    record_auto_update_result(
        &store,
        &AutoUpdateRunResult {
            checked: 1,
            updated: 1,
            skipped: 0,
            failed: 0,
            errors: vec![],
            progress: AutoUpdateProgressSnapshot::default(),
        },
    )
    .unwrap();

    let finished = get_auto_update_config(&store).unwrap();
    assert!(finished.last_started_at.is_some());
    assert!(finished.last_finished_at.is_some());
    assert!(finished.last_finished_at >= finished.last_started_at);
}

#[test]
fn triggered_update_clears_previous_result_using_current_eligible_count() {
    let (_dir, store) = make_store();
    store
        .upsert_skill(&make_skill("git-skill", "git", "/tmp/git-skill"))
        .unwrap();
    store
        .upsert_skill(&make_skill("local-skill", "local", "/tmp/local-skill"))
        .unwrap();
    store
        .set_setting(AUTO_UPDATE_LAST_ERROR_KEY, "old: failed")
        .unwrap();

    record_auto_update_triggered(&store).unwrap();

    let config = get_auto_update_config(&store).unwrap();

    assert_eq!(config.last_status.as_deref(), Some("running"));
    assert_eq!(config.last_checked, 2);
    assert_eq!(config.last_failed, 0);
    assert_eq!(config.last_error.as_deref(), Some(""));
    assert_eq!(config.progress.total, 2);
    assert_eq!(config.progress.pending.len(), 2);
    assert!(config.progress.failed.is_empty());
}

#[test]
fn structured_progress_snapshot_tracks_success_failure_running_and_pending() {
    let (_dir, store) = make_store();

    record_auto_update_progress_snapshot(
        &store,
        &AutoUpdateProgressSnapshot {
            total: 4,
            succeeded: vec![AutoUpdateSkillProgress {
                skill_id: "done".to_string(),
                name: "Done Skill".to_string(),
                reason: None,
            }],
            skipped: vec![],
            failed: vec![AutoUpdateSkillProgress {
                skill_id: "bad".to_string(),
                name: "Bad Skill".to_string(),
                reason: Some("network timeout".to_string()),
            }],
            running: Some(AutoUpdateSkillProgress {
                skill_id: "now".to_string(),
                name: "Now Skill".to_string(),
                reason: None,
            }),
            pending: vec![AutoUpdateSkillProgress {
                skill_id: "next".to_string(),
                name: "Next Skill".to_string(),
                reason: None,
            }],
        },
    )
    .unwrap();

    let config = get_auto_update_config(&store).unwrap();

    assert_eq!(config.progress.total, 4);
    assert_eq!(config.progress.succeeded[0].name, "Done Skill");
    assert_eq!(
        config.progress.failed[0].reason.as_deref(),
        Some("network timeout")
    );
    assert_eq!(
        config
            .progress
            .running
            .as_ref()
            .map(|item| item.skill_id.as_str()),
        Some("now")
    );
    assert_eq!(config.progress.pending[0].skill_id, "next");
}

#[test]
fn legacy_error_progress_uses_skill_name_when_available() {
    let (_dir, store) = make_store();
    let mut skill = make_skill(
        "64798624-ca2a-4811-8747-00147567facf",
        "local",
        "/tmp/youdaonote",
    );
    skill.name = "有道云笔记".to_string();
    store.upsert_skill(&skill).unwrap();
    store
        .set_setting(AUTO_UPDATE_LAST_CHECKED_KEY, "1")
        .unwrap();
    store
        .set_setting(
            AUTO_UPDATE_LAST_ERROR_KEY,
            "64798624-ca2a-4811-8747-00147567facf: source path not found: \"/Users/may/Downloads/youdaonote\"",
        )
        .unwrap();

    let config = get_auto_update_config(&store).unwrap();

    assert_eq!(config.progress.failed[0].name, "有道云笔记");
    assert_eq!(
        config.progress.failed[0].reason.as_deref(),
        Some("source path not found: \"/Users/may/Downloads/youdaonote\"")
    );
}

#[cfg(unix)]
#[test]
fn real_path_identity_excludes_self_sourced_symlinks_but_keeps_external_local_sources() {
    use std::os::unix::fs::symlink;

    let (dir, store) = make_store();
    let central = dir.path().join("central");
    let source_link = dir.path().join("source-link");
    let external = dir.path().join("pua-source");
    std::fs::create_dir(&central).unwrap();
    std::fs::create_dir(&external).unwrap();
    symlink(&central, &source_link).unwrap();

    let mut self_sourced = make_skill("self-sourced", "local", central.to_str().unwrap());
    self_sourced.source_ref = Some(source_link.to_string_lossy().to_string());
    store.upsert_skill(&self_sourced).unwrap();

    let mut pua = make_skill("pua", "local", central.to_str().unwrap());
    pua.central_path = dir.path().join("pua-central").to_string_lossy().to_string();
    std::fs::create_dir(&pua.central_path).unwrap();
    pua.source_ref = Some(external.to_string_lossy().to_string());
    store.upsert_skill(&pua).unwrap();

    let self_capability = managed_skill_update_capability(&self_sourced);
    assert!(!self_capability.has_external_source);
    assert!(!self_capability.updateable);
    assert!(self_capability.integrity_error.is_none());
    let pua_capability = managed_skill_update_capability(&pua);
    assert!(pua_capability.has_external_source);
    assert!(pua_capability.updateable);

    let ids = crate::core::auto_update::list_auto_update_skill_ids(&store).unwrap();
    assert_eq!(ids, vec!["pua".to_string()]);
}

#[cfg(unix)]
#[test]
fn self_sourced_local_migration_is_identity_based_idempotent_and_preserves_external_sources() {
    use std::os::unix::fs::symlink;

    let (dir, store) = make_store();
    let central = dir.path().join("central");
    let source_link = dir.path().join("source-link");
    let pua_central = dir.path().join("pua-central");
    let pua_source = dir.path().join("pua-source");
    for path in [&central, &pua_central, &pua_source] {
        std::fs::create_dir(path).unwrap();
    }
    symlink(&central, &source_link).unwrap();

    let mut self_sourced = make_skill("self-sourced", "local", central.to_str().unwrap());
    self_sourced.source_ref = Some(source_link.to_string_lossy().to_string());
    self_sourced.source_subpath = Some("legacy".to_string());
    self_sourced.source_revision = Some("legacy-revision".to_string());
    store.upsert_skill(&self_sourced).unwrap();

    let mut pua = make_skill("pua", "local", pua_central.to_str().unwrap());
    pua.source_ref = Some(pua_source.to_string_lossy().to_string());
    store.upsert_skill(&pua).unwrap();

    assert_eq!(normalize_self_sourced_local_skills(&store).unwrap(), 1);
    assert_eq!(normalize_self_sourced_local_skills(&store).unwrap(), 0);

    let migrated = store.get_skill_by_id("self-sourced").unwrap().unwrap();
    assert_eq!(migrated.source_type, "managed");
    assert!(migrated.source_ref.is_none());
    assert!(migrated.source_subpath.is_none());
    assert!(migrated.source_revision.is_none());

    let preserved = store.get_skill_by_id("pua").unwrap().unwrap();
    assert_eq!(preserved.source_type, "local");
    assert_eq!(preserved.source_ref.as_deref(), pua_source.to_str());
}

#[test]
fn missing_local_paths_remain_integrity_errors_and_are_not_silently_filtered() {
    let (dir, store) = make_store();
    let missing_central = dir.path().join("missing-central");
    let missing_source = dir.path().join("missing-source");
    let mut broken = make_skill("broken", "local", missing_central.to_str().unwrap());
    broken.source_ref = Some(missing_source.to_string_lossy().to_string());
    store.upsert_skill(&broken).unwrap();

    let capability = managed_skill_update_capability(&broken);
    assert!(capability.has_external_source);
    assert!(!capability.updateable);
    assert!(capability.integrity_error.is_some());
    assert_eq!(normalize_self_sourced_local_skills(&store).unwrap(), 0);
    assert_eq!(
        crate::core::auto_update::list_auto_update_skill_ids(&store).unwrap(),
        vec!["broken".to_string()]
    );
}

#[test]
fn unsafe_git_sources_are_not_updateable_and_do_not_echo_credentials() {
    let (dir, store) = make_store();
    let central = dir.path().join("git-central");
    std::fs::create_dir(&central).unwrap();
    let mut skill = make_skill("unsafe-git", "git", central.to_str().unwrap());
    skill.source_ref = Some("https://secret-token@example.com/repo.git".to_string());
    skill.source_subpath = Some("skills/private".to_string());
    skill.source_revision = Some("legacy-revision".to_string());
    store.upsert_skill(&skill).unwrap();

    let capability = managed_skill_update_capability(&skill);
    assert!(capability.has_external_source);
    assert!(!capability.updateable);
    assert!(!capability.source_ref_safe_to_expose);
    let reason = capability.integrity_error.unwrap();
    assert!(reason.contains("invalid Git source reference"));
    assert!(!reason.contains("secret-token"));
    assert_eq!(
        crate::core::auto_update::list_auto_update_skill_ids(&store).unwrap(),
        vec!["unsafe-git".to_string()]
    );

    let safe_central = dir.path().join("safe-git-central");
    std::fs::create_dir(&safe_central).unwrap();
    let mut safe = make_skill("safe-git", "git", safe_central.to_str().unwrap());
    safe.source_ref = Some("http://github.com/anthropics/skills".to_string());
    store.upsert_skill(&safe).unwrap();

    assert_eq!(sanitize_unsafe_git_sources(&store).unwrap(), 1);
    assert_eq!(sanitize_unsafe_git_sources(&store).unwrap(), 0);
    let sanitized = store.get_skill_by_id("unsafe-git").unwrap().unwrap();
    assert!(sanitized.source_ref.is_none());
    assert!(sanitized.source_subpath.is_none());
    assert!(sanitized.source_revision.is_none());
    assert_eq!(sanitized.status, "error");
    let preserved = store.get_skill_by_id("safe-git").unwrap().unwrap();
    assert_eq!(
        preserved.source_ref.as_deref(),
        Some("http://github.com/anthropics/skills")
    );
}

#[test]
fn supported_github_source_forms_remain_updateable() {
    let (dir, _store) = make_store();
    let central = dir.path().join("git-central");
    std::fs::create_dir(&central).unwrap();

    let local_repo = dir.path().join("source-repo.git");
    for source in [
        "https://github.com/anthropics/skills".to_string(),
        "anthropics/skills".to_string(),
        "github.com/anthropics/skills".to_string(),
        "http://github.com/anthropics/skills".to_string(),
        "anthropics/skills/tree/main/skills/example".to_string(),
        "https://github.com/anthropics/skills/tree/main/skills".to_string(),
        "https://example.com/repo.git".to_string(),
        local_repo.to_string_lossy().to_string(),
    ] {
        let mut skill = make_skill("supported-git", "git", central.to_str().unwrap());
        skill.source_ref = Some(source.clone());
        let capability = managed_skill_update_capability(&skill);
        assert!(capability.has_external_source, "source={source}");
        assert!(capability.updateable, "source={source}");
        assert!(capability.integrity_error.is_none(), "source={source}");
        assert!(capability.source_ref_safe_to_expose, "source={source}");
    }
}

#[test]
fn unsafe_git_source_forms_remain_rejected_without_echoing_secrets() {
    let (dir, _store) = make_store();
    let central = dir.path().join("git-central");
    std::fs::create_dir(&central).unwrap();

    for source in [
        "http://example.com/repo.git",
        "http://github.com.evil/repo",
        "git@github.com:owner/repo.git",
        "../relative/repo",
        "https://example.com/repo.git?token=secret-value",
    ] {
        let mut skill = make_skill("rejected-git", "git", central.to_str().unwrap());
        skill.source_ref = Some(source.to_string());
        let capability = managed_skill_update_capability(&skill);
        assert!(capability.has_external_source, "source={source}");
        assert!(!capability.updateable, "source={source}");
        assert!(!capability.source_ref_safe_to_expose, "source={source}");
        assert!(!capability
            .integrity_error
            .as_deref()
            .unwrap_or_default()
            .contains("secret-value"));
    }
}

#[test]
fn unsafe_stored_git_subpaths_are_not_updateable() {
    let (dir, _store) = make_store();
    let central = dir.path().join("git-central");
    std::fs::create_dir(&central).unwrap();
    let mut skill = make_skill("unsafe-subpath", "git", central.to_str().unwrap());
    skill.source_ref = Some("anthropics/skills".to_string());
    skill.source_subpath = Some("../private".to_string());

    let capability = managed_skill_update_capability(&skill);
    assert!(capability.has_external_source);
    assert!(!capability.updateable);
    assert!(capability.source_ref_safe_to_expose);
    assert!(capability
        .integrity_error
        .as_deref()
        .is_some_and(|reason| reason.contains("invalid Git source subpath")));
}

#[test]
fn no_external_source_is_recorded_as_skipped_without_failing_the_run() {
    let mut result = AutoUpdateRunResult {
        checked: 1,
        updated: 0,
        skipped: 0,
        failed: 0,
        errors: vec![],
        progress: AutoUpdateProgressSnapshot::default(),
    };
    let mut progress = AutoUpdateProgressSnapshot::default();
    record_update_error(
        &mut result,
        &mut progress,
        AutoUpdateSkillProgress {
            skill_id: "legacy".to_string(),
            name: "Legacy".to_string(),
            reason: None,
        },
        anyhow::anyhow!("NO_EXTERNAL_SOURCE|already managed"),
    );

    assert_eq!(result.skipped, 1);
    assert_eq!(result.failed, 0);
    assert!(result.errors.is_empty());
    assert_eq!(progress.skipped.len(), 1);
    assert_eq!(
        progress.skipped[0].reason.as_deref(),
        Some("already managed")
    );
}

#[test]
fn stored_progress_without_skipped_field_remains_backward_compatible() {
    let progress: AutoUpdateProgressSnapshot = serde_json::from_str(
        r#"{"total":0,"succeeded":[],"failed":[],"running":null,"pending":[]}"#,
    )
    .unwrap();
    assert!(progress.skipped.is_empty());
}
