use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::installer::{update_managed_skill_from_source, validate_git_source_reference};
use super::safe_fs::{paths_have_same_identity, validate_relative_subpath};
use super::skill_store::SkillRecord;
use super::skill_store::SkillStore;

pub const AUTO_UPDATE_ENABLED_KEY: &str = "skill_auto_update_enabled";
pub const AUTO_UPDATE_INTERVAL_HOURS_KEY: &str = "skill_auto_update_interval_hours";
pub const AUTO_UPDATE_SCHEDULE_TYPE_KEY: &str = "skill_auto_update_schedule_type";
pub const AUTO_UPDATE_INTERVAL_VALUE_KEY: &str = "skill_auto_update_interval_value";
pub const AUTO_UPDATE_INTERVAL_UNIT_KEY: &str = "skill_auto_update_interval_unit";
pub const AUTO_UPDATE_DAILY_TIME_KEY: &str = "skill_auto_update_daily_time";
pub const AUTO_UPDATE_LAST_RUN_AT_KEY: &str = "skill_auto_update_last_run_at";
pub const AUTO_UPDATE_LAST_STARTED_AT_KEY: &str = "skill_auto_update_last_started_at";
pub const AUTO_UPDATE_LAST_FINISHED_AT_KEY: &str = "skill_auto_update_last_finished_at";
pub const AUTO_UPDATE_LAST_STATUS_KEY: &str = "skill_auto_update_last_status";
pub const AUTO_UPDATE_LAST_ERROR_KEY: &str = "skill_auto_update_last_error";
pub const AUTO_UPDATE_LAST_CHECKED_KEY: &str = "skill_auto_update_last_checked";
pub const AUTO_UPDATE_LAST_UPDATED_KEY: &str = "skill_auto_update_last_updated";
pub const AUTO_UPDATE_LAST_FAILED_KEY: &str = "skill_auto_update_last_failed";
pub const AUTO_UPDATE_PROGRESS_KEY: &str = "skill_auto_update_progress";

pub const DEFAULT_AUTO_UPDATE_INTERVAL_HOURS: i64 = 24;
pub const DEFAULT_AUTO_UPDATE_DAILY_TIME: &str = "03:00";
const MIN_AUTO_UPDATE_INTERVAL_HOURS: i64 = 1;
const MAX_AUTO_UPDATE_INTERVAL_HOURS: i64 = 24 * 30;
const MIN_AUTO_UPDATE_INTERVAL_MINUTES: i64 = 15;
const MAX_AUTO_UPDATE_INTERVAL_MINUTES: i64 = 24 * 30 * 60;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutoUpdateScheduleType {
    Interval,
    Daily,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutoUpdateIntervalUnit {
    Minutes,
    Hours,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutoUpdateSchedule {
    pub schedule_type: AutoUpdateScheduleType,
    pub interval_value: i64,
    pub interval_unit: AutoUpdateIntervalUnit,
    pub daily_time: String,
}

impl AutoUpdateSchedule {
    pub fn interval_minutes(&self) -> i64 {
        match self.interval_unit {
            AutoUpdateIntervalUnit::Minutes => self.interval_value,
            AutoUpdateIntervalUnit::Hours => self.interval_value.saturating_mul(60),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutoUpdateConfig {
    pub enabled: bool,
    pub interval_hours: i64,
    pub schedule: AutoUpdateSchedule,
    pub local_skill_count: usize,
    pub protected_local_skill_count: usize,
    pub last_run_at: Option<i64>,
    pub last_started_at: Option<i64>,
    pub last_finished_at: Option<i64>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_checked: usize,
    pub last_updated: usize,
    pub last_failed: usize,
    pub progress: AutoUpdateProgressSnapshot,
}

#[derive(Clone, Debug, Serialize)]
pub struct AutoUpdateRunResult {
    pub checked: usize,
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
    pub errors: Vec<String>,
    pub progress: AutoUpdateProgressSnapshot,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutoUpdateProgressSnapshot {
    pub total: usize,
    pub succeeded: Vec<AutoUpdateSkillProgress>,
    #[serde(default)]
    pub skipped: Vec<AutoUpdateSkillProgress>,
    pub failed: Vec<AutoUpdateSkillProgress>,
    pub running: Option<AutoUpdateSkillProgress>,
    pub pending: Vec<AutoUpdateSkillProgress>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutoUpdateSkillProgress {
    pub skill_id: String,
    pub name: String,
    pub reason: Option<String>,
}

/// The single backend authority for whether an installed Skill has an
/// independent source that can safely replace its managed copy.
///
/// `updateable` is intentionally false for malformed records (missing managed
/// or local source paths). Those records remain integrity errors and are still
/// included in an automatic run so the failure is visible instead of silently
/// disappearing from the diagnostics panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedSkillUpdateCapability {
    pub has_external_source: bool,
    pub updateable: bool,
    pub integrity_error: Option<String>,
    /// Invalid legacy Git inputs may contain credentials or tokens. Callers
    /// must not serialize the stored reference unless this is true.
    pub source_ref_safe_to_expose: bool,
}

pub fn managed_skill_update_capability(skill: &SkillRecord) -> ManagedSkillUpdateCapability {
    let central_path = Path::new(&skill.central_path);
    let central_error = validate_existing_skill_dir(central_path, "managed central").err();

    match skill.source_type.as_str() {
        "git" => {
            let source_ref = skill
                .source_ref
                .as_deref()
                .map(str::trim)
                .filter(|source| !source.is_empty());
            let has_source = source_ref.is_some();
            let source_validation = source_ref
                .ok_or_else(|| anyhow::anyhow!("missing Git source reference"))
                .and_then(validate_git_source_reference);
            let source_ref_safe_to_expose = source_validation.is_ok();
            let source_error = source_validation
                .err()
                .map(|err| format!("invalid Git source reference: {err:#}"));
            let subpath_error = skill
                .source_subpath
                .as_deref()
                .map(validate_relative_subpath)
                .transpose()
                .err()
                .map(|err| format!("invalid Git source subpath: {err:#}"));
            let integrity_error = central_error.or(source_error).or(subpath_error);
            ManagedSkillUpdateCapability {
                has_external_source: has_source,
                updateable: has_source && integrity_error.is_none(),
                integrity_error,
                source_ref_safe_to_expose,
            }
        }
        "local" => {
            let Some(source_ref) = skill
                .source_ref
                .as_deref()
                .map(str::trim)
                .filter(|source| !source.is_empty())
            else {
                return ManagedSkillUpdateCapability {
                    has_external_source: false,
                    updateable: false,
                    integrity_error: Some("missing local source reference".to_string()),
                    source_ref_safe_to_expose: true,
                };
            };
            let source_path = PathBuf::from(source_ref);
            let source_error = validate_existing_skill_dir(&source_path, "local source").err();
            let integrity_error = central_error.or(source_error);
            if integrity_error.is_some() {
                return ManagedSkillUpdateCapability {
                    // A distinct source is configured, even though it is
                    // currently unavailable and cannot be used for an update.
                    has_external_source: true,
                    updateable: false,
                    integrity_error,
                    source_ref_safe_to_expose: true,
                };
            }

            match paths_have_same_identity(&source_path, central_path) {
                Ok(true) => ManagedSkillUpdateCapability {
                    has_external_source: false,
                    updateable: false,
                    integrity_error: None,
                    source_ref_safe_to_expose: true,
                },
                Ok(false) => ManagedSkillUpdateCapability {
                    has_external_source: true,
                    updateable: true,
                    integrity_error: None,
                    source_ref_safe_to_expose: true,
                },
                Err(err) => ManagedSkillUpdateCapability {
                    has_external_source: true,
                    updateable: false,
                    integrity_error: Some(format!("failed to resolve source identity: {err:#}")),
                    source_ref_safe_to_expose: true,
                },
            }
        }
        _ => ManagedSkillUpdateCapability {
            has_external_source: false,
            updateable: false,
            integrity_error: central_error,
            source_ref_safe_to_expose: true,
        },
    }
}

fn validate_existing_skill_dir(path: &Path, label: &str) -> std::result::Result<(), String> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(format!("{label} path is not a directory: {path:?}")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("{label} path not found: {path:?}"))
        }
        Err(err) => Err(format!("failed to inspect {label} path {path:?}: {err}")),
    }
}

/// Normalize legacy records whose `local` source is merely a symlink (or an
/// alternate spelling) of the managed central directory. This migration is
/// filesystem-identity based, idempotent, and metadata-only: it never mutates
/// either directory. Missing/unresolvable paths are deliberately left intact
/// so they remain visible as integrity errors.
pub fn normalize_self_sourced_local_skills(store: &SkillStore) -> Result<usize> {
    let mut normalized = 0;
    for mut skill in store.list_skills()? {
        if skill.source_type != "local" {
            continue;
        }
        let Some(source_ref) = skill.source_ref.as_deref() else {
            continue;
        };
        let source_path = Path::new(source_ref);
        let central_path = Path::new(&skill.central_path);
        if !source_path.is_dir() || !central_path.is_dir() {
            continue;
        }
        if !paths_have_same_identity(source_path, central_path).unwrap_or(false) {
            continue;
        }

        skill.source_type = "managed".to_string();
        skill.source_ref = None;
        skill.source_subpath = None;
        skill.source_revision = None;
        store.upsert_skill(&skill)?;
        normalized += 1;
    }
    Ok(normalized)
}

/// Remove unsafe Git references persisted by legacy builds before any IPC can
/// expose them. Validation deliberately reuses the install/update parser so
/// supported GitHub shorthands remain intact. This migration is metadata-only
/// and idempotent; it never touches a managed Skill directory.
pub fn sanitize_unsafe_git_sources(store: &SkillStore) -> Result<usize> {
    let mut sanitized = 0;
    for mut skill in store.list_skills()? {
        if skill.source_type != "git" {
            continue;
        }
        let Some(source_ref) = skill.source_ref.as_deref() else {
            continue;
        };
        if validate_git_source_reference(source_ref).is_ok() {
            continue;
        }

        skill.source_ref = None;
        skill.source_subpath = None;
        skill.source_revision = None;
        skill.status = "error".to_string();
        store.upsert_skill(&skill)?;
        sanitized += 1;
    }
    Ok(sanitized)
}

pub fn get_auto_update_config(store: &SkillStore) -> Result<AutoUpdateConfig> {
    let enabled = store
        .get_setting(AUTO_UPDATE_ENABLED_KEY)?
        .map(|v| v == "true")
        .unwrap_or(false);
    let interval_hours = store
        .get_setting(AUTO_UPDATE_INTERVAL_HOURS_KEY)?
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| (MIN_AUTO_UPDATE_INTERVAL_HOURS..=MAX_AUTO_UPDATE_INTERVAL_HOURS).contains(v))
        .unwrap_or(DEFAULT_AUTO_UPDATE_INTERVAL_HOURS);
    let schedule = get_auto_update_schedule(store, interval_hours)?;
    let last_run_at = store
        .get_setting(AUTO_UPDATE_LAST_RUN_AT_KEY)?
        .and_then(|v| v.parse::<i64>().ok());
    let last_started_at = store
        .get_setting(AUTO_UPDATE_LAST_STARTED_AT_KEY)?
        .and_then(|v| v.parse::<i64>().ok())
        .or(last_run_at);
    let last_status = store.get_setting(AUTO_UPDATE_LAST_STATUS_KEY)?;
    let mut last_finished_at = store
        .get_setting(AUTO_UPDATE_LAST_FINISHED_AT_KEY)?
        .and_then(|v| v.parse::<i64>().ok());
    if last_finished_at.is_none() && last_status.as_deref() != Some("running") {
        last_finished_at = last_run_at;
    }
    let last_error = store.get_setting(AUTO_UPDATE_LAST_ERROR_KEY)?;
    let last_checked = parse_usize_setting(store, AUTO_UPDATE_LAST_CHECKED_KEY)?;
    let last_updated = parse_usize_setting(store, AUTO_UPDATE_LAST_UPDATED_KEY)?;
    let last_failed = parse_usize_setting(store, AUTO_UPDATE_LAST_FAILED_KEY)?;
    let mut progress = parse_progress_setting(store)?;
    if progress_is_empty(&progress) {
        progress = legacy_error_progress(store, last_checked, last_error.as_deref())?;
    }
    let (local_skill_count, protected_local_skill_count) = count_local_auto_update_skills(store)?;

    Ok(AutoUpdateConfig {
        enabled,
        interval_hours,
        schedule,
        local_skill_count,
        protected_local_skill_count,
        last_run_at,
        last_started_at,
        last_finished_at,
        last_status,
        last_error,
        last_checked,
        last_updated,
        last_failed,
        progress,
    })
}

pub fn set_auto_update_config(
    store: &SkillStore,
    config: AutoUpdateConfig,
) -> Result<AutoUpdateConfig> {
    validate_schedule(&config.schedule)?;
    let interval_hours = schedule_to_legacy_interval_hours(&config.schedule);
    store.set_setting(
        AUTO_UPDATE_ENABLED_KEY,
        if config.enabled { "true" } else { "false" },
    )?;
    store.set_setting(AUTO_UPDATE_INTERVAL_HOURS_KEY, &interval_hours.to_string())?;
    store.set_setting(
        AUTO_UPDATE_SCHEDULE_TYPE_KEY,
        match config.schedule.schedule_type {
            AutoUpdateScheduleType::Interval => "interval",
            AutoUpdateScheduleType::Daily => "daily",
        },
    )?;
    store.set_setting(
        AUTO_UPDATE_INTERVAL_VALUE_KEY,
        &config.schedule.interval_value.to_string(),
    )?;
    store.set_setting(
        AUTO_UPDATE_INTERVAL_UNIT_KEY,
        match config.schedule.interval_unit {
            AutoUpdateIntervalUnit::Minutes => "minutes",
            AutoUpdateIntervalUnit::Hours => "hours",
        },
    )?;
    store.set_setting(AUTO_UPDATE_DAILY_TIME_KEY, &config.schedule.daily_time)?;
    get_auto_update_config(store)
}

pub fn is_auto_update_due(config: &AutoUpdateConfig, now_ms: i64) -> bool {
    if !config.enabled {
        return false;
    }
    let Some(last_run_at) = config.last_run_at else {
        return true;
    };
    let interval_ms = config
        .schedule
        .interval_minutes()
        .saturating_mul(60)
        .saturating_mul(1000);
    now_ms.saturating_sub(last_run_at) >= interval_ms
}

#[allow(dead_code)]
pub fn list_auto_update_skill_ids(store: &SkillStore) -> Result<Vec<String>> {
    Ok(list_auto_update_skill_entries(store)?
        .into_iter()
        .map(|skill| skill.skill_id)
        .collect())
}

pub fn run_auto_update_now<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
) -> Result<AutoUpdateRunResult> {
    let entries = list_auto_update_skill_entries(store)?;
    let mut progress = AutoUpdateProgressSnapshot {
        total: entries.len(),
        pending: entries.clone(),
        ..AutoUpdateProgressSnapshot::default()
    };
    record_auto_update_started(store, entries.len())?;
    record_auto_update_progress_snapshot(store, &progress)?;
    let mut result = AutoUpdateRunResult {
        checked: entries.len(),
        updated: 0,
        skipped: 0,
        failed: 0,
        errors: Vec::new(),
        progress: progress.clone(),
    };

    for entry in entries {
        let skill_id = entry.skill_id.clone();
        progress.running = Some(entry.clone());
        progress
            .pending
            .retain(|pending| pending.skill_id != skill_id);
        record_auto_update_progress_snapshot(store, &progress)?;

        match update_managed_skill_from_source(app, store, &skill_id) {
            Ok(update) => {
                result.updated += 1;
                progress.succeeded.push(AutoUpdateSkillProgress {
                    skill_id,
                    name: update.name,
                    reason: None,
                });
            }
            Err(err) => record_update_error(&mut result, &mut progress, entry, err),
        }
        progress.running = None;
        result.progress = progress.clone();
        record_auto_update_progress(store, &result)?;
    }

    record_auto_update_result(store, &result)?;
    Ok(result)
}

pub fn record_auto_update_triggered(store: &SkillStore) -> Result<()> {
    let entries = list_auto_update_skill_entries(store)?;
    record_auto_update_started(store, entries.len())?;
    record_auto_update_progress_snapshot(
        store,
        &AutoUpdateProgressSnapshot {
            total: entries.len(),
            pending: entries,
            ..AutoUpdateProgressSnapshot::default()
        },
    )
}

fn record_auto_update_started(store: &SkillStore, checked: usize) -> Result<()> {
    let started_at = now_ms();
    store.set_setting(AUTO_UPDATE_LAST_RUN_AT_KEY, &started_at.to_string())?;
    store.set_setting(AUTO_UPDATE_LAST_STARTED_AT_KEY, &started_at.to_string())?;
    store.set_setting(AUTO_UPDATE_LAST_FINISHED_AT_KEY, "")?;
    store.set_setting(AUTO_UPDATE_LAST_STATUS_KEY, "running")?;
    store.set_setting(AUTO_UPDATE_LAST_CHECKED_KEY, &checked.to_string())?;
    store.set_setting(AUTO_UPDATE_LAST_UPDATED_KEY, "0")?;
    store.set_setting(AUTO_UPDATE_LAST_FAILED_KEY, "0")?;
    store.set_setting(AUTO_UPDATE_LAST_ERROR_KEY, "")?;
    record_auto_update_progress_snapshot(
        store,
        &AutoUpdateProgressSnapshot {
            total: checked,
            ..AutoUpdateProgressSnapshot::default()
        },
    )?;
    Ok(())
}

fn record_auto_update_progress(store: &SkillStore, result: &AutoUpdateRunResult) -> Result<()> {
    store.set_setting(AUTO_UPDATE_LAST_STATUS_KEY, "running")?;
    store.set_setting(AUTO_UPDATE_LAST_CHECKED_KEY, &result.checked.to_string())?;
    store.set_setting(AUTO_UPDATE_LAST_UPDATED_KEY, &result.updated.to_string())?;
    store.set_setting(AUTO_UPDATE_LAST_FAILED_KEY, &result.failed.to_string())?;
    store.set_setting(AUTO_UPDATE_LAST_ERROR_KEY, &result.errors.join("\n"))?;
    record_auto_update_progress_snapshot(store, &result.progress)?;
    Ok(())
}

fn record_auto_update_progress_snapshot(
    store: &SkillStore,
    progress: &AutoUpdateProgressSnapshot,
) -> Result<()> {
    store.set_setting(
        AUTO_UPDATE_PROGRESS_KEY,
        &serde_json::to_string(progress).unwrap_or_else(|_| "{}".to_string()),
    )?;
    Ok(())
}

pub fn run_due_auto_update<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
) -> Result<Option<AutoUpdateRunResult>> {
    let config = get_auto_update_config(store)?;
    if !is_auto_update_due(&config, now_ms()) {
        return Ok(None);
    }
    run_auto_update_now(app, store).map(Some)
}

fn record_auto_update_result(store: &SkillStore, result: &AutoUpdateRunResult) -> Result<()> {
    let status = if result.failed == 0 { "ok" } else { "error" };
    let finished_at = now_ms();
    store.set_setting(AUTO_UPDATE_LAST_RUN_AT_KEY, &finished_at.to_string())?;
    store.set_setting(AUTO_UPDATE_LAST_FINISHED_AT_KEY, &finished_at.to_string())?;
    store.set_setting(AUTO_UPDATE_LAST_STATUS_KEY, status)?;
    store.set_setting(AUTO_UPDATE_LAST_CHECKED_KEY, &result.checked.to_string())?;
    store.set_setting(AUTO_UPDATE_LAST_UPDATED_KEY, &result.updated.to_string())?;
    store.set_setting(AUTO_UPDATE_LAST_FAILED_KEY, &result.failed.to_string())?;
    store.set_setting(AUTO_UPDATE_LAST_ERROR_KEY, &result.errors.join("\n"))?;
    record_auto_update_progress_snapshot(store, &result.progress)?;
    Ok(())
}

fn list_auto_update_skill_entries(store: &SkillStore) -> Result<Vec<AutoUpdateSkillProgress>> {
    let mut skills = store
        .list_skills()?
        .into_iter()
        .filter(|skill| {
            if skill.source_type != "git" && skill.source_type != "local" {
                return false;
            }
            let capability = managed_skill_update_capability(skill);
            capability.updateable || capability.integrity_error.is_some()
        })
        .map(|skill| AutoUpdateSkillProgress {
            skill_id: skill.id,
            name: skill.name,
            reason: None,
        })
        .collect::<Vec<_>>();
    skills.sort_by(|a, b| a.skill_id.cmp(&b.skill_id));
    Ok(skills)
}

fn count_local_auto_update_skills(store: &SkillStore) -> Result<(usize, usize)> {
    let mut local_count = 0;
    let mut protected_count = 0;
    for skill in store.list_skills()? {
        if skill.source_type != "local"
            || !managed_skill_update_capability(&skill).has_external_source
        {
            continue;
        }
        local_count += 1;
        if skill
            .source_ref
            .as_deref()
            .map(is_macos_protected_user_path)
            .unwrap_or(false)
        {
            protected_count += 1;
        }
    }
    Ok((local_count, protected_count))
}

fn is_macos_protected_user_path(path: &str) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let path = std::path::Path::new(path);
    ["Desktop", "Documents", "Downloads"]
        .iter()
        .map(|dir| home.join(dir))
        .any(|protected_dir| path.starts_with(protected_dir))
}

fn parse_usize_setting(store: &SkillStore, key: &str) -> Result<usize> {
    Ok(store
        .get_setting(key)?
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0))
}

fn parse_progress_setting(store: &SkillStore) -> Result<AutoUpdateProgressSnapshot> {
    Ok(store
        .get_setting(AUTO_UPDATE_PROGRESS_KEY)?
        .and_then(|value| serde_json::from_str::<AutoUpdateProgressSnapshot>(&value).ok())
        .unwrap_or_default())
}

fn progress_is_empty(progress: &AutoUpdateProgressSnapshot) -> bool {
    progress.total == 0
        && progress.succeeded.is_empty()
        && progress.skipped.is_empty()
        && progress.failed.is_empty()
        && progress.running.is_none()
        && progress.pending.is_empty()
}

fn record_update_error(
    result: &mut AutoUpdateRunResult,
    progress: &mut AutoUpdateProgressSnapshot,
    entry: AutoUpdateSkillProgress,
    err: anyhow::Error,
) {
    if let Some(reason) = no_external_source_reason(&err) {
        result.skipped += 1;
        progress.skipped.push(AutoUpdateSkillProgress {
            skill_id: entry.skill_id,
            name: entry.name,
            reason: Some(reason),
        });
        return;
    }

    result.failed += 1;
    let reason = format!("{err:#}");
    result
        .errors
        .push(format!("{}: {}", entry.skill_id, reason));
    progress.failed.push(AutoUpdateSkillProgress {
        skill_id: entry.skill_id,
        name: entry.name,
        reason: Some(reason),
    });
}

fn no_external_source_reason(err: &anyhow::Error) -> Option<String> {
    err.chain().find_map(|cause| {
        cause
            .to_string()
            .strip_prefix("NO_EXTERNAL_SOURCE|")
            .map(str::trim)
            .map(str::to_string)
    })
}

fn legacy_error_progress(
    store: &SkillStore,
    total: usize,
    raw_error: Option<&str>,
) -> Result<AutoUpdateProgressSnapshot> {
    let Some(raw_error) = raw_error else {
        return Ok(AutoUpdateProgressSnapshot::default());
    };

    let mut failed = Vec::new();
    for line in raw_error
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Some((skill_id, reason)) = line.split_once(':') else {
            failed.push(AutoUpdateSkillProgress {
                skill_id: line.to_string(),
                name: line.to_string(),
                reason: Some(line.to_string()),
            });
            continue;
        };
        let skill_id = skill_id.trim();
        let name = store
            .get_skill_by_id(skill_id)?
            .map(|skill| skill.name)
            .unwrap_or_else(|| skill_id.to_string());
        failed.push(AutoUpdateSkillProgress {
            skill_id: skill_id.to_string(),
            name,
            reason: Some(reason.trim().to_string()),
        });
    }

    Ok(AutoUpdateProgressSnapshot {
        total,
        failed,
        ..AutoUpdateProgressSnapshot::default()
    })
}

fn get_auto_update_schedule(
    store: &SkillStore,
    legacy_interval_hours: i64,
) -> Result<AutoUpdateSchedule> {
    let schedule_type = match store
        .get_setting(AUTO_UPDATE_SCHEDULE_TYPE_KEY)?
        .as_deref()
        .unwrap_or("interval")
    {
        "daily" => AutoUpdateScheduleType::Daily,
        _ => AutoUpdateScheduleType::Interval,
    };
    let interval_unit = match store
        .get_setting(AUTO_UPDATE_INTERVAL_UNIT_KEY)?
        .as_deref()
        .unwrap_or("hours")
    {
        "minutes" => AutoUpdateIntervalUnit::Minutes,
        _ => AutoUpdateIntervalUnit::Hours,
    };
    let interval_value = store
        .get_setting(AUTO_UPDATE_INTERVAL_VALUE_KEY)?
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(legacy_interval_hours);
    let daily_time = store
        .get_setting(AUTO_UPDATE_DAILY_TIME_KEY)?
        .filter(|time| is_valid_daily_time(time))
        .unwrap_or_else(|| DEFAULT_AUTO_UPDATE_DAILY_TIME.to_string());
    let schedule = AutoUpdateSchedule {
        schedule_type,
        interval_value,
        interval_unit,
        daily_time,
    };
    Ok(if validate_schedule(&schedule).is_ok() {
        schedule
    } else {
        default_schedule()
    })
}

fn schedule_to_legacy_interval_hours(schedule: &AutoUpdateSchedule) -> i64 {
    match schedule.schedule_type {
        AutoUpdateScheduleType::Daily => DEFAULT_AUTO_UPDATE_INTERVAL_HOURS,
        AutoUpdateScheduleType::Interval => {
            let minutes = schedule.interval_minutes();
            minutes.saturating_add(59).saturating_div(60).clamp(
                MIN_AUTO_UPDATE_INTERVAL_HOURS,
                MAX_AUTO_UPDATE_INTERVAL_HOURS,
            )
        }
    }
}

fn validate_schedule(schedule: &AutoUpdateSchedule) -> Result<()> {
    match schedule.schedule_type {
        AutoUpdateScheduleType::Interval => validate_interval_minutes(schedule.interval_minutes()),
        AutoUpdateScheduleType::Daily => {
            if !is_valid_daily_time(&schedule.daily_time) {
                anyhow::bail!("daily time must use HH:mm format");
            }
            Ok(())
        }
    }
}

fn validate_interval_minutes(interval_minutes: i64) -> Result<()> {
    if !(MIN_AUTO_UPDATE_INTERVAL_MINUTES..=MAX_AUTO_UPDATE_INTERVAL_MINUTES)
        .contains(&interval_minutes)
    {
        anyhow::bail!(
            "interval minutes must be between {} and {}",
            MIN_AUTO_UPDATE_INTERVAL_MINUTES,
            MAX_AUTO_UPDATE_INTERVAL_MINUTES
        );
    }
    Ok(())
}

fn is_valid_daily_time(time: &str) -> bool {
    let Some((hour, minute)) = time.split_once(':') else {
        return false;
    };
    if hour.len() != 2 || minute.len() != 2 {
        return false;
    }
    let Ok(hour) = hour.parse::<u8>() else {
        return false;
    };
    let Ok(minute) = minute.parse::<u8>() else {
        return false;
    };
    hour <= 23 && minute <= 59
}

fn default_schedule() -> AutoUpdateSchedule {
    AutoUpdateSchedule {
        schedule_type: AutoUpdateScheduleType::Interval,
        interval_value: DEFAULT_AUTO_UPDATE_INTERVAL_HOURS,
        interval_unit: AutoUpdateIntervalUnit::Hours,
        daily_time: DEFAULT_AUTO_UPDATE_DAILY_TIME.to_string(),
    }
}

fn now_ms() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis() as i64
}

#[cfg(test)]
#[path = "tests/auto_update.rs"]
mod tests;
