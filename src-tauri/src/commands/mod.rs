use anyhow::Context;
use serde::{Deserialize, Serialize};
use tauri::State;

use std::sync::Arc;

use crate::core::auto_update::{
    get_auto_update_config as get_auto_update_config_core, managed_skill_update_capability,
    record_auto_update_triggered, run_auto_update_now as run_auto_update_now_core,
    set_auto_update_config as set_auto_update_config_core, AutoUpdateConfig,
    AutoUpdateIntervalUnit, AutoUpdateProgressSnapshot, AutoUpdateRunResult, AutoUpdateSchedule,
    AutoUpdateScheduleType, ManagedSkillUpdateCapability,
};
use crate::core::cache_cleanup::{
    cleanup_git_cache_dirs, get_git_cache_ttl_secs as get_git_cache_ttl_secs_core,
    set_git_cache_ttl_secs as set_git_cache_ttl_secs_core,
};
use crate::core::cancel_token::CancelToken;
use crate::core::central_repo::{
    ensure_central_repo, fixed_central_repo_path, resolve_central_repo_path,
};
use crate::core::content_hash::hash_dir;
use crate::core::featured_skills::{fetch_featured_skills, FeaturedSkill};
use crate::core::github_search::{search_github_repos, RepoSummary};
use crate::core::installer::{
    import_existing_local_skill, install_git_skill, install_git_skill_from_selection,
    install_local_skill, install_local_skill_from_selection, list_git_skills, list_local_skills,
    update_managed_skill_from_source, GitSkillCandidate, InstallResult, LocalSkillCandidate,
};
use crate::core::network_proxy::{
    get_github_proxy_config as get_github_proxy_config_core,
    get_github_proxy_url as get_github_proxy_url_core,
    set_github_proxy_config as set_github_proxy_config_core, GithubProxyConfig,
};
use crate::core::onboarding::{
    build_onboarding_plan, get_discovery_scan_settings as get_discovery_scan_settings_core,
    save_discovery_scan_config, DiscoveryScanConfig, DiscoveryScanSettings, OnboardingPlan,
};
use crate::core::safe_fs::{
    direct_skill_child, ensure_distinct_roots, lock_central_mutation, move_internal_to_trash,
    move_skill_to_trash, path_entry_exists, paths_have_same_identity,
    publish_staged_entry_no_replace, restore_skill_from_trash, validate_direct_skill_path,
    validate_skill_name, TrashReceipt,
};
use crate::core::skill_metadata::read_skill_ui_metadata;
use crate::core::skill_store::{SkillRecord, SkillStore, SkillTargetRecord};
use crate::core::skills_search::{
    search_skills_online as search_skills_online_core, OnlineSkillResult,
};
use crate::core::sync_engine::{
    copy_dir_recursive, sync_dir_for_tool_with_overwrite, sync_dir_with_mode_with_overwrite,
    SyncMode,
};
use crate::core::system_scheduler::{
    current_scheduler_config, get_auto_update_task_status, install_auto_update_task,
    trigger_auto_update_task_now, uninstall_auto_update_task,
};
use crate::core::tool_adapters::{
    adapter_by_key, adapters_sharing_project_skills_dir, is_builtin_tool_enabled,
    is_tool_installed, load_tool_config, project_relative_skills_dir, resolve_default_path,
    save_tool_config, supports_project_scope, CustomToolConfig, ToolConfig,
};
use uuid::Uuid;

const RECENT_PROJECTS_SETTING: &str = "recent_projects_v1";
const MAX_MANAGED_ICON_DATA_URL_BYTES: usize = 12 * 1024 * 1024;

fn format_anyhow_error(err: anyhow::Error) -> String {
    let first = err.to_string();
    // Frontend relies on these prefixes for special flows.
    if first.starts_with("MULTI_SKILLS|")
        || first.starts_with("TARGET_EXISTS|")
        || first.starts_with("TOOL_NOT_INSTALLED|")
        || first.starts_with("TOOL_NOT_WRITABLE|")
    {
        return first;
    }

    // Include the full error chain (causes), not just the top context.
    let mut full = format!("{:#}", err);

    // Redact noisy temp paths from clone context (we care about the cause, not the dest).
    // Example: `clone https://... into "/Users/.../skills-hub-git-<uuid>"`
    if let Some(head) = full.lines().next() {
        if head.starts_with("clone ") {
            if let Some(pos) = head.find(" into ") {
                let head_redacted = format!("{} (已省略临时目录)", &head[..pos]);
                let rest: String = full.lines().skip(1).collect::<Vec<_>>().join("\n");
                full = if rest.is_empty() {
                    head_redacted
                } else {
                    format!("{}\n{}", head_redacted, rest)
                };
            }
        }
    }

    let root = err.root_cause().to_string();
    let lower = full.to_lowercase();

    // Heuristic-friendly messaging for GitHub clone failures.
    if lower.contains("github.com")
        && (lower.contains("clone ") || lower.contains("remote") || lower.contains("fetch"))
    {
        if lower.contains("securetransport") {
            return format!(
        "无法从 GitHub 拉取仓库：TLS/证书校验失败（macOS SecureTransport）。\n\n建议：\n- 检查网络/代理是否拦截 HTTPS\n- 如在公司网络，可能需要安装公司根证书或使用可信代理\n- 也可在终端确认 `git clone {}` 是否可用\n\n详细：{}",
        "https://github.com/<owner>/<repo>",
        root
      );
        }
        let hint = if lower.contains("authentication")
            || lower.contains("permission denied")
            || lower.contains("credentials")
        {
            "无法访问该仓库：可能是私有仓库/权限不足/需要鉴权。"
        } else if lower.contains("not found") {
            "仓库不存在或无权限访问（GitHub 返回 not found）。"
        } else if lower.contains("failed to resolve")
            || lower.contains("could not resolve")
            || lower.contains("dns")
        {
            "无法解析 GitHub 域名（DNS）。请检查网络/代理。"
        } else if lower.contains("timed out") || lower.contains("timeout") {
            "连接 GitHub 超时。请检查网络/代理。"
        } else if lower.contains("connection refused") || lower.contains("connection reset") {
            "连接 GitHub 失败（连接被拒绝/重置）。请检查网络/代理。"
        } else {
            "无法从 GitHub 拉取仓库。请检查网络/代理，或稍后重试。"
        };

        return format!("{}\n\n详细：{}", hint, root);
    }

    full
}

#[derive(Debug, Serialize)]
pub struct ToolInfoDto {
    pub key: String,
    pub label: String,
    pub avatar: Option<String>,
    pub installed: bool,
    pub enabled: bool,
    pub is_custom: bool,
    pub skills_dir: String,
    pub project_skills_dir: String,
    pub supports_project_scope: bool,
    pub sync_mode: SyncMode,
}

#[derive(Debug, Serialize)]
pub struct ToolStatusDto {
    pub tools: Vec<ToolInfoDto>,
    pub installed: Vec<String>,
    pub newly_installed: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolConfigDto {
    pub disabled_builtin_tools: Vec<String>,
    pub custom_tools: Vec<CustomToolConfigDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomToolConfigDto {
    pub key: String,
    pub label: String,
    pub avatar: Option<String>,
    pub skills_dir: String,
    pub project_skills_dir: Option<String>,
    pub sync_mode: SyncMode,
    pub enabled: bool,
}

impl From<ToolConfig> for ToolConfigDto {
    fn from(config: ToolConfig) -> Self {
        Self {
            disabled_builtin_tools: config.disabled_builtin_tools,
            custom_tools: config
                .custom_tools
                .into_iter()
                .map(|tool| CustomToolConfigDto {
                    key: tool.key,
                    label: tool.label,
                    avatar: tool.avatar,
                    skills_dir: tool.skills_dir,
                    project_skills_dir: tool.project_skills_dir,
                    sync_mode: tool.sync_mode,
                    enabled: tool.enabled,
                })
                .collect(),
        }
    }
}

impl From<ToolConfigDto> for ToolConfig {
    fn from(config: ToolConfigDto) -> Self {
        Self {
            disabled_builtin_tools: config.disabled_builtin_tools,
            custom_tools: config
                .custom_tools
                .into_iter()
                .map(|tool| CustomToolConfig {
                    key: tool.key,
                    label: tool.label,
                    avatar: tool.avatar,
                    skills_dir: tool.skills_dir,
                    project_skills_dir: tool.project_skills_dir,
                    sync_mode: tool.sync_mode,
                    enabled: tool.enabled,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
struct RuntimeTool {
    key: String,
    label: String,
    avatar: Option<String>,
    installed: bool,
    enabled: bool,
    is_custom: bool,
    skills_dir: std::path::PathBuf,
    project_skills_dir: String,
    supports_project_scope: bool,
    sync_mode: SyncMode,
}

fn runtime_tools(store: &SkillStore, include_disabled: bool) -> anyhow::Result<Vec<RuntimeTool>> {
    let config = load_tool_config(store)?;
    let mut tools = Vec::new();

    for adapter in crate::core::tool_adapters::default_tool_adapters() {
        let enabled = is_builtin_tool_enabled(&config, adapter.id.as_key());
        if !include_disabled && !enabled {
            continue;
        }
        let detected = is_tool_installed(&adapter)?;
        tools.push(RuntimeTool {
            key: adapter.id.as_key().to_string(),
            label: adapter.display_name.to_string(),
            avatar: None,
            installed: enabled && detected,
            enabled,
            is_custom: false,
            skills_dir: resolve_default_path(&adapter)?,
            project_skills_dir: project_relative_skills_dir(&adapter).to_string(),
            supports_project_scope: supports_project_scope(&adapter),
            sync_mode: SyncMode::Auto,
        });
    }

    for custom in config.custom_tools {
        if !include_disabled && !custom.enabled {
            continue;
        }
        let skills_dir = expand_home_path(&custom.skills_dir)?;
        let supports_project_scope = custom.project_skills_dir.is_some();
        let detected = skills_dir.is_dir();
        tools.push(RuntimeTool {
            key: custom.key,
            label: custom.label,
            avatar: custom.avatar,
            installed: custom.enabled && detected,
            enabled: custom.enabled,
            is_custom: true,
            skills_dir,
            project_skills_dir: custom.project_skills_dir.unwrap_or_default(),
            supports_project_scope,
            sync_mode: custom.sync_mode,
        });
    }

    Ok(tools)
}

fn runtime_tool_by_key(store: &SkillStore, key: &str) -> anyhow::Result<RuntimeTool> {
    runtime_tools(store, false)?
        .into_iter()
        .find(|tool| tool.key == key)
        .ok_or_else(|| anyhow::anyhow!("TOOL_NOT_INSTALLED|{}", key))
}

fn runtime_tools_sharing_dir(
    store: &SkillStore,
    selected: &RuntimeTool,
    scope: &str,
) -> anyhow::Result<Vec<RuntimeTool>> {
    let tools = runtime_tools(store, false)?;
    let shared = tools
        .into_iter()
        .filter(|tool| {
            tool.installed
                && if scope == "project" {
                    tool.project_skills_dir == selected.project_skills_dir
                } else {
                    tool.skills_dir == selected.skills_dir
                }
        })
        .collect::<Vec<_>>();
    Ok(shared)
}

fn resolve_runtime_tool_root(
    tool: &RuntimeTool,
    project_root: Option<&std::path::Path>,
) -> anyhow::Result<std::path::PathBuf> {
    if let Some(project_root) = project_root {
        if !tool.supports_project_scope {
            anyhow::bail!("PROJECT_SCOPE_UNSUPPORTED|{}", tool.key);
        }
        return Ok(project_root.join(&tool.project_skills_dir));
    }
    Ok(tool.skills_dir.clone())
}

#[tauri::command]
pub async fn get_tool_config(store: State<'_, SkillStore>) -> Result<ToolConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || load_tool_config(&store).map(ToolConfigDto::from))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn set_tool_config(
    store: State<'_, SkillStore>,
    config: ToolConfigDto,
) -> Result<ToolConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        save_tool_config(&store, config.into()).map(ToolConfigDto::from)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_tool_status(store: State<'_, SkillStore>) -> Result<ToolStatusDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut tools: Vec<ToolInfoDto> = Vec::new();
        let mut installed: Vec<String> = Vec::new();

        for tool in runtime_tools(&store, true)? {
            tools.push(ToolInfoDto {
                key: tool.key.clone(),
                label: tool.label,
                avatar: tool.avatar,
                installed: tool.installed,
                enabled: tool.enabled,
                is_custom: tool.is_custom,
                skills_dir: tool.skills_dir.to_string_lossy().to_string(),
                project_skills_dir: tool.project_skills_dir,
                supports_project_scope: tool.supports_project_scope,
                sync_mode: tool.sync_mode,
            });
            if tool.installed {
                installed.push(tool.key);
            }
        }

        installed.dedup();

        let prev: Vec<String> = store
            .get_setting("installed_tools_v1")?
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .unwrap_or_default();

        let prev_set: std::collections::HashSet<String> = prev.into_iter().collect();
        let newly_installed: Vec<String> = installed
            .iter()
            .filter(|k| !prev_set.contains(*k))
            .cloned()
            .collect();

        // Persist current set (best effort).
        let _ = store.set_setting(
            "installed_tools_v1",
            &serde_json::to_string(&installed).unwrap_or_else(|_| "[]".to_string()),
        );

        Ok::<_, anyhow::Error>(ToolStatusDto {
            tools,
            installed,
            newly_installed,
        })
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

pub fn auto_reconcile_skill_targets(
    store: &SkillStore,
    filter_skill_id: Option<&str>,
) -> anyhow::Result<usize> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Ok(0),
    };
    let adapters = crate::core::tool_adapters::default_tool_adapters();
    let skills = store.list_skills()?;
    let now = now_ms();
    let mut reconciled_count = 0;

    for skill in &skills {
        if let Some(fid) = filter_skill_id {
            if skill.id != fid {
                continue;
            }
        }
        let central_path = std::path::PathBuf::from(&skill.central_path);
        if !central_path.is_dir() {
            continue;
        }

        let central_ignore_path = central_path.join(".skillignore");
        let central_ignore_content = std::fs::read_to_string(&central_ignore_path).ok();

        for adapter in &adapters {
            // 快速短路检查：如果数据库中已经记录该目标且状态为 ok，直接 0 耗时跳过！
            let existing = store.get_skill_target(&skill.id, adapter.id.as_key(), "global", None)?;
            if let Some(ref rec) = existing {
                if rec.status == "ok" {
                    continue;
                }
            }

            let tool_dir = home.join(adapter.relative_skills_dir);
            if !tool_dir.is_dir() {
                continue;
            }
            let target_path = tool_dir.join(&skill.name);
            if !target_path.exists() {
                continue;
            }

            // 如果中心库有 .skillignore，而工具目录下还没有，同步一份规则过去以保持比对准则一致
            if let Some(ref ignore_str) = central_ignore_content {
                let target_ignore_path = target_path.join(".skillignore");
                if !target_ignore_path.exists() {
                    let _ = std::fs::write(&target_ignore_path, ignore_str);
                }
            }

            if target_has_same_content(&central_path, &target_path) {
                let mode = if target_path.is_symlink() {
                    "junction".to_string()
                } else {
                    "copy".to_string()
                };

                let record = SkillTargetRecord {
                    id: existing.map(|t| t.id).unwrap_or_else(|| Uuid::new_v4().to_string()),
                    skill_id: skill.id.clone(),
                    tool: adapter.id.as_key().to_string(),
                    scope: "global".to_string(),
                    project_path: None,
                    target_path: target_path.to_string_lossy().to_string(),
                    mode,
                    status: "ok".to_string(),
                    last_error: None,
                    synced_at: Some(now),
                };
                store.upsert_skill_target(&record)?;
                reconciled_count += 1;
            }
        }
    }

    Ok(reconciled_count)
}

#[tauri::command]
pub async fn get_onboarding_plan(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
) -> Result<OnboardingPlan, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = auto_reconcile_skill_targets(&store, None);
        build_onboarding_plan(&app, &store)
    })
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_discovery_scan_settings(
    store: State<'_, SkillStore>,
) -> Result<DiscoveryScanSettings, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || get_discovery_scan_settings_core(&store))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn set_discovery_scan_config(
    store: State<'_, SkillStore>,
    config: DiscoveryScanConfig,
) -> Result<DiscoveryScanSettings, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        save_discovery_scan_config(&store, config)?;
        get_discovery_scan_settings_core(&store)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn clear_git_cache_now(app: tauri::AppHandle) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || {
        cleanup_git_cache_dirs(&app, std::time::Duration::from_secs(0))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_git_cache_ttl_secs(store: State<'_, SkillStore>) -> Result<i64, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        Ok::<_, anyhow::Error>(get_git_cache_ttl_secs_core(&store))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn set_git_cache_ttl_secs(
    store: State<'_, SkillStore>,
    secs: i64,
) -> Result<i64, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || set_git_cache_ttl_secs_core(&store, secs))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
pub struct AutoUpdateConfigDto {
    pub enabled: bool,
    pub interval_hours: i64,
    pub schedule_type: String,
    pub interval_value: i64,
    pub interval_unit: String,
    pub daily_time: String,
    pub local_skill_count: usize,
    pub protected_local_skill_count: usize,
    pub task_registered: bool,
    pub task_status_detail: String,
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

#[derive(Debug, Serialize)]
pub struct AutoUpdateRunResultDto {
    pub checked: usize,
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
    pub errors: Vec<String>,
    pub progress: AutoUpdateProgressSnapshot,
}

#[derive(Debug, Serialize)]
pub struct GithubProxyConfigDto {
    pub enabled: bool,
    pub port: u16,
    pub url: String,
    pub auto_detected: bool,
}

#[tauri::command]
pub async fn get_auto_update_config(
    store: State<'_, SkillStore>,
) -> Result<AutoUpdateConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        get_auto_update_config_core(&store).map(to_auto_update_config_dto)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn set_auto_update_config(
    store: State<'_, SkillStore>,
    enabled: bool,
    intervalHours: i64,
    scheduleType: Option<String>,
    intervalValue: Option<i64>,
    intervalUnit: Option<String>,
    dailyTime: Option<String>,
) -> Result<AutoUpdateConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let schedule = build_auto_update_schedule(
            intervalHours,
            scheduleType.as_deref(),
            intervalValue,
            intervalUnit.as_deref(),
            dailyTime.as_deref(),
        )?;
        if enabled {
            let scheduler_config = current_scheduler_config(schedule.clone())?;
            install_auto_update_task(&scheduler_config)?;
        } else {
            uninstall_auto_update_task()?;
        }

        let existing = get_auto_update_config_core(&store)?;
        let saved = set_auto_update_config_core(
            &store,
            AutoUpdateConfig {
                enabled,
                interval_hours: intervalHours,
                schedule,
                local_skill_count: existing.local_skill_count,
                protected_local_skill_count: existing.protected_local_skill_count,
                last_run_at: existing.last_run_at,
                last_started_at: existing.last_started_at,
                last_finished_at: existing.last_finished_at,
                last_status: existing.last_status,
                last_error: existing.last_error,
                last_checked: existing.last_checked,
                last_updated: existing.last_updated,
                last_failed: existing.last_failed,
                progress: existing.progress,
            },
        )?;
        Ok::<_, anyhow::Error>(to_auto_update_config_dto(saved))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

fn build_auto_update_schedule(
    legacy_interval_hours: i64,
    schedule_type: Option<&str>,
    interval_value: Option<i64>,
    interval_unit: Option<&str>,
    daily_time: Option<&str>,
) -> anyhow::Result<AutoUpdateSchedule> {
    let schedule_type = match schedule_type.unwrap_or("interval") {
        "daily" => AutoUpdateScheduleType::Daily,
        "interval" => AutoUpdateScheduleType::Interval,
        other => anyhow::bail!("unsupported auto update schedule type: {other}"),
    };
    let interval_unit = match interval_unit.unwrap_or("hours") {
        "minutes" => AutoUpdateIntervalUnit::Minutes,
        "hours" => AutoUpdateIntervalUnit::Hours,
        other => anyhow::bail!("unsupported auto update interval unit: {other}"),
    };
    let schedule = AutoUpdateSchedule {
        schedule_type,
        interval_value: interval_value.unwrap_or(legacy_interval_hours),
        interval_unit,
        daily_time: daily_time.unwrap_or("03:00").to_string(),
    };
    match schedule.schedule_type {
        AutoUpdateScheduleType::Interval => {
            let minutes = schedule.interval_minutes();
            if !(15..=24 * 30 * 60).contains(&minutes) {
                anyhow::bail!("interval minutes must be between 15 and 43200");
            }
        }
        AutoUpdateScheduleType::Daily => {
            let Some((hour, minute)) = schedule.daily_time.split_once(':') else {
                anyhow::bail!("daily time must use HH:mm format");
            };
            if hour.len() != 2 || minute.len() != 2 {
                anyhow::bail!("daily time must use HH:mm format");
            }
            let hour = hour.parse::<u8>().context("parse daily schedule hour")?;
            let minute = minute
                .parse::<u8>()
                .context("parse daily schedule minute")?;
            if hour > 23 || minute > 59 {
                anyhow::bail!("daily time must use HH:mm format");
            }
        }
    }
    Ok(schedule)
}

#[tauri::command]
pub async fn run_auto_update_now(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
) -> Result<AutoUpdateRunResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_auto_update_now_core(&app, &store).map(to_auto_update_run_result_dto)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn trigger_auto_update_task_now_cmd(store: State<'_, SkillStore>) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let config = get_auto_update_config_core(&store)?;
        let scheduler_config = current_scheduler_config(config.schedule)?;
        install_auto_update_task(&scheduler_config)?;
        record_auto_update_triggered(&store)?;
        trigger_auto_update_task_now()
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
pub struct InstallResultDto {
    pub skill_id: String,
    pub name: String,
    pub central_path: String,
    pub content_hash: Option<String>,
}

fn expand_home_path(input: &str) -> Result<std::path::PathBuf, anyhow::Error> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        anyhow::bail!("storage path is empty");
    }
    if trimmed == "~" {
        let home = dirs::home_dir().context("failed to resolve home directory")?;
        return Ok(home);
    }
    if let Some(stripped) = trimmed.strip_prefix("~/") {
        let home = dirs::home_dir().context("failed to resolve home directory")?;
        return Ok(home.join(stripped));
    }
    Ok(std::path::PathBuf::from(trimmed))
}

fn normalize_scope(scope: Option<&str>) -> Result<&'static str, anyhow::Error> {
    match scope.unwrap_or("global") {
        "global" => Ok("global"),
        "project" => Ok("project"),
        other => anyhow::bail!("invalid scope: {}", other),
    }
}

#[tauri::command]
pub async fn get_recent_projects(store: State<'_, SkillStore>) -> Result<Vec<String>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || get_recent_projects_impl(&store))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn save_recent_project(
    store: State<'_, SkillStore>,
    projectPath: String,
) -> Result<Vec<String>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || save_recent_project_impl(&store, &projectPath))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

fn get_recent_projects_impl(store: &SkillStore) -> Result<Vec<String>, anyhow::Error> {
    let projects = store
        .get_setting(RECENT_PROJECTS_SETTING)?
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default();
    Ok(projects)
}

fn save_recent_project_impl(
    store: &SkillStore,
    project_path: &str,
) -> Result<Vec<String>, anyhow::Error> {
    let path = expand_home_path(project_path)?;
    if !path.is_dir() {
        anyhow::bail!("projectPath must be an existing directory: {:?}", path);
    }
    let normalized = path.to_string_lossy().to_string();
    let mut projects = get_recent_projects_impl(store)?;
    projects.retain(|item| item != &normalized);
    projects.insert(0, normalized);
    projects.truncate(8);
    store.set_setting(
        RECENT_PROJECTS_SETTING,
        &serde_json::to_string(&projects).unwrap_or_else(|_| "[]".to_string()),
    )?;
    Ok(projects)
}

#[tauri::command]
pub async fn get_central_repo_path(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
) -> Result<String, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = resolve_central_repo_path(&app, &store)?;
        ensure_central_repo(&path)?;
        Ok::<_, anyhow::Error>(path.to_string_lossy().to_string())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn install_local(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    sourcePath: String,
    name: Option<String>,
) -> Result<InstallResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = install_local_skill(&app, &store, sourcePath.as_ref(), name)?;
        Ok::<_, anyhow::Error>(to_install_dto(result))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn list_local_skills_cmd(basePath: String) -> Result<Vec<LocalSkillCandidate>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = std::path::PathBuf::from(basePath);
        list_local_skills(&path)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn install_local_selection(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    basePath: String,
    subpath: String,
    name: Option<String>,
) -> Result<InstallResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let base = std::path::PathBuf::from(basePath);
        let result =
            install_local_skill_from_selection(&app, &store, base.as_ref(), &subpath, name)?;
        Ok::<_, anyhow::Error>(to_install_dto(result))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn install_git(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    cancel: State<'_, Arc<CancelToken>>,
    repoUrl: String,
    name: Option<String>,
) -> Result<InstallResultDto, String> {
    let store = store.inner().clone();
    cancel.reset();
    let cancel_token = Arc::clone(cancel.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let result = install_git_skill(&app, &store, &repoUrl, name, Some(&cancel_token))?;
        Ok::<_, anyhow::Error>(to_install_dto(result))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn list_git_skills_cmd(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    repoUrl: String,
) -> Result<Vec<GitSkillCandidate>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || list_git_skills(&app, &store, &repoUrl))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn install_git_selection(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    repoUrl: String,
    subpath: String,
    name: Option<String>,
) -> Result<InstallResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = install_git_skill_from_selection(&app, &store, &repoUrl, &subpath, name)?;
        Ok::<_, anyhow::Error>(to_install_dto(result))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
pub struct SyncResultDto {
    pub mode_used: String,
    pub target_path: String,
}

fn sync_mode_name(mode: SyncMode) -> &'static str {
    match mode {
        SyncMode::Auto => "auto",
        SyncMode::Symlink => "symlink",
        SyncMode::Junction => "junction",
        SyncMode::Copy => "copy",
    }
}

#[allow(clippy::too_many_arguments)]
fn record_skill_target_failure(
    store: &SkillStore,
    skill_id: &str,
    tool: &str,
    scope: &str,
    project_path: Option<&str>,
    target_path: &std::path::Path,
    requested_mode: SyncMode,
    error: &str,
) -> anyhow::Result<()> {
    let existing = store.get_skill_target(skill_id, tool, scope, project_path)?;
    let record = SkillTargetRecord {
        id: existing
            .as_ref()
            .map(|target| target.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        skill_id: skill_id.to_string(),
        tool: tool.to_string(),
        scope: scope.to_string(),
        project_path: project_path.map(str::to_string),
        target_path: target_path.to_string_lossy().to_string(),
        mode: existing
            .as_ref()
            .map(|target| target.mode.clone())
            .unwrap_or_else(|| sync_mode_name(requested_mode).to_string()),
        status: "error".to_string(),
        last_error: Some(error.to_string()),
        synced_at: existing.and_then(|target| target.synced_at),
    };
    store.upsert_skill_target(&record)
}

#[tauri::command]
pub async fn sync_skill_dir(
    _source_path: String,
    _target_path: String,
) -> Result<SyncResultDto, String> {
    Err("SAFE_POLICY|External tool writes are disabled in Skills Hub".to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
#[allow(clippy::too_many_arguments)]
pub async fn sync_skill_to_tool(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    sourcePath: String,
    skillId: String,
    tool: String,
    name: String,
    overwrite: Option<bool>,
    overwriteIfSameContent: Option<bool>,
    scope: Option<String>,
    projectPath: Option<String>,
) -> Result<SyncResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _mutation_guard = lock_central_mutation()?;
        let skill = store
            .get_skill_by_id(&skillId)?
            .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
        validate_skill_name(&skill.name)?;
        validate_skill_name(&name)?;
        if name != skill.name {
            anyhow::bail!("UNSAFE_PATH|Requested Skill name does not match managed record");
        }
        let central_root = resolve_central_repo_path(&app, &store)?;
        ensure_central_repo(&central_root)?;
        let managed_source = std::path::PathBuf::from(&skill.central_path);
        validate_direct_skill_path(&central_root, &managed_source)?;
        let expected_source = direct_skill_child(&central_root, &skill.name)?;
        if !paths_have_same_identity(&managed_source, &expected_source)?
            || !paths_have_same_identity(std::path::Path::new(&sourcePath), &managed_source)?
        {
            anyhow::bail!(
                "UNSAFE_PATH|sourcePath must exactly match the managed central Skill path"
            );
        }

        let runtime_tool = runtime_tool_by_key(&store, &tool)?;
        let scope = normalize_scope(scope.as_deref())?;
        if scope == "project" && !runtime_tool.supports_project_scope {
            anyhow::bail!("PROJECT_SCOPE_UNSUPPORTED|{}", runtime_tool.key);
        }
        let project_root = if scope == "project" {
            let raw = projectPath
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("projectPath is required for project scope"))?;
            let path = expand_home_path(raw)?;
            if !path.is_dir() {
                anyhow::bail!("projectPath must be an existing directory: {:?}", path);
            }
            Some(path)
        } else {
            None
        };

        let tool_root = resolve_runtime_tool_root(&runtime_tool, project_root.as_deref())?;
        std::fs::create_dir_all(&tool_root)
            .with_context(|| format!("create configured Skill root {:?}", tool_root))?;
        ensure_distinct_roots(&tool_root, &central_root)?;
        let target = direct_skill_child(&tool_root, &name)?;
        let project_path_for_record = project_root
            .as_ref()
            .map(|path| path.to_string_lossy().to_string());
        if scope == "global" && !runtime_tool.installed {
            let error = format!("TOOL_NOT_INSTALLED|{}", runtime_tool.key);
            record_skill_target_failure(
                &store,
                &skillId,
                &tool,
                scope,
                project_path_for_record.as_deref(),
                &target,
                runtime_tool.sync_mode,
                &error,
            )?;
            anyhow::bail!(error);
        }
        // Pre-check: ensure the skills directory is writable (fixes #20 — Windows OS error 5).
        if let Err(err) = std::fs::create_dir_all(&tool_root) {
            let error = if err.kind() == std::io::ErrorKind::PermissionDenied {
                format!(
                    "TOOL_NOT_WRITABLE|{}|{}",
                    runtime_tool.label,
                    tool_root.to_string_lossy()
                )
            } else {
                format!("failed to create skills dir {:?}: {}", tool_root, err)
            };
            record_skill_target_failure(
                &store,
                &skillId,
                &tool,
                scope,
                project_path_for_record.as_deref(),
                &target,
                runtime_tool.sync_mode,
                &error,
            )?;
            anyhow::bail!(error);
        }
        if let Some(existing) =
            store.get_skill_target(&skillId, &tool, scope, project_path_for_record.as_deref())?
        {
            if existing.status == "ok"
                && existing.target_path == target.to_string_lossy()
                && target.exists()
            {
                return Ok::<_, anyhow::Error>(SyncResultDto {
                    mode_used: existing.mode,
                    target_path: existing.target_path,
                });
            }
        }
        let overwrite = overwrite.unwrap_or(false)
            || (overwriteIfSameContent.unwrap_or(false)
                && target_has_same_content(&managed_source, &target));
        let result = if runtime_tool.is_custom {
            sync_dir_with_mode_with_overwrite(
                runtime_tool.sync_mode,
                &managed_source,
                &target,
                overwrite,
            )
        } else {
            sync_dir_for_tool_with_overwrite(&tool, &managed_source, &target, overwrite)
        };
        let result = match result {
            Ok(result) => result,
            Err(err) => {
                let msg = err.to_string();
                let error = if msg.contains("target already exists") {
                    format!("TARGET_EXISTS|{}", target.to_string_lossy())
                } else if msg.contains("os error 5")
                    || msg.contains("Access is denied")
                    || msg.contains("Permission denied")
                {
                    format!(
                        "TOOL_NOT_WRITABLE|{}|{}",
                        runtime_tool.label,
                        tool_root.to_string_lossy()
                    )
                } else {
                    msg
                };
                record_skill_target_failure(
                    &store,
                    &skillId,
                    &tool,
                    scope,
                    project_path_for_record.as_deref(),
                    &target,
                    runtime_tool.sync_mode,
                    &error,
                )?;
                anyhow::bail!(error);
            }
        };

        // Some tools share the same skills directory; keep DB records consistent across them.
        let group = runtime_tools_sharing_dir(&store, &runtime_tool, scope)?;
        for a in group {
            let record = SkillTargetRecord {
                id: Uuid::new_v4().to_string(),
                skill_id: skillId.clone(),
                tool: a.key,
                scope: scope.to_string(),
                project_path: project_path_for_record.clone(),
                target_path: result.target_path.to_string_lossy().to_string(),
                mode: match result.mode_used {
                    SyncMode::Auto => "auto",
                    SyncMode::Symlink => "symlink",
                    SyncMode::Junction => "junction",
                    SyncMode::Copy => "copy",
                }
                .to_string(),
                status: "ok".to_string(),
                last_error: None,
                synced_at: Some(now_ms()),
            };
            store.upsert_skill_target(&record)?;
        }

        Ok::<_, anyhow::Error>(SyncResultDto {
            mode_used: match result.mode_used {
                SyncMode::Auto => "auto",
                SyncMode::Symlink => "symlink",
                SyncMode::Junction => "junction",
                SyncMode::Copy => "copy",
            }
            .to_string(),
            target_path: result.target_path.to_string_lossy().to_string(),
        })
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

fn target_has_same_content(source: &std::path::Path, target: &std::path::Path) -> bool {
    if !source.is_dir() || !target.is_dir() {
        return false;
    }
    match (hash_dir(source), hash_dir(target)) {
        (Ok(source_hash), Ok(target_hash)) => source_hash == target_hash,
        _ => false,
    }
}

fn validated_managed_target(
    store: &SkillStore,
    target: &SkillTargetRecord,
    expected_name: &str,
) -> anyhow::Result<(std::path::PathBuf, std::path::PathBuf)> {
    validate_skill_name(expected_name)?;
    let runtime_tool = runtime_tools(store, true)?
        .into_iter()
        .find(|candidate| candidate.key == target.tool)
        .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Unknown configured tool {}", target.tool))?;
    let project_root = if target.scope == "project" {
        let raw = target
            .project_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Project target has no project root"))?;
        let root = expand_home_path(raw)?;
        if !root.is_dir() {
            anyhow::bail!(
                "UNSAFE_PATH|Project target root no longer exists: {:?}",
                root
            );
        }
        Some(root)
    } else if target.scope == "global" {
        None
    } else {
        anyhow::bail!("UNSAFE_PATH|Unknown target scope {}", target.scope);
    };
    let root = resolve_runtime_tool_root(&runtime_tool, project_root.as_deref())?;
    let fixed_central = fixed_central_repo_path()?;
    if fixed_central.is_dir() {
        ensure_distinct_roots(&root, &fixed_central)?;
    }
    let path = std::path::PathBuf::from(&target.target_path);
    let actual_name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Managed target has no UTF-8 name"))?;
    if actual_name != expected_name {
        anyhow::bail!("UNSAFE_PATH|Managed target name does not match the Skill record");
    }
    validate_direct_skill_path(&root, &path)?;
    Ok((root, path))
}

#[derive(Debug)]
struct TrashedManagedPath {
    root: std::path::PathBuf,
    original: std::path::PathBuf,
    trash_receipt: TrashReceipt,
}

fn move_managed_target_to_trash(
    store: &SkillStore,
    target: &SkillTargetRecord,
    expected_name: &str,
) -> anyhow::Result<Option<TrashedManagedPath>> {
    let (root, path) = validated_managed_target(store, target, expected_name)?;
    Ok(
        move_skill_to_trash(&root, &path)?.map(|trash_receipt| TrashedManagedPath {
            root,
            original: path,
            trash_receipt,
        }),
    )
}

fn rollback_trashed_managed_paths(moved: &[TrashedManagedPath]) -> anyhow::Result<()> {
    let mut errors = Vec::new();
    for item in moved.iter().rev() {
        if let Err(err) = restore_skill_from_trash(&item.root, &item.original, &item.trash_receipt)
        {
            errors.push(format!("{}: {err:#}", item.original.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("ROLLBACK_INCOMPLETE|{}", errors.join(" | "))
    }
}

fn move_managed_targets_to_trash(
    store: &SkillStore,
    targets: &[SkillTargetRecord],
    expected_name: &str,
) -> anyhow::Result<Vec<TrashedManagedPath>> {
    let mut moved = Vec::new();
    let mut moved_paths = std::collections::HashSet::new();
    for target in targets {
        if !moved_paths.insert(target.target_path.clone()) {
            continue;
        }
        match move_managed_target_to_trash(store, target, expected_name) {
            Ok(Some(item)) => moved.push(item),
            Ok(None) => {}
            Err(err) => {
                rollback_trashed_managed_paths(&moved).with_context(|| {
                    format!("target move failed ({err:#}) and rollback was incomplete")
                })?;
                return Err(err).context("target move failed; earlier moves were restored");
            }
        }
    }
    Ok(moved)
}

#[derive(Clone, Debug)]
struct EnableTargetPlan {
    root: std::path::PathBuf,
    path: std::path::PathBuf,
    mode: SyncMode,
}

#[cfg(unix)]
#[derive(Debug, Eq, PartialEq)]
struct EnablePathIdentity {
    device: u64,
    inode: u64,
    mode: u32,
}

#[cfg(windows)]
#[derive(Debug, Eq, PartialEq)]
struct EnablePathIdentity {
    handle: same_file::Handle,
    file_attributes: u32,
    created_at: u64,
}

#[cfg(not(any(unix, windows)))]
#[derive(Debug, Eq, PartialEq)]
struct EnablePathIdentity;

#[derive(Debug)]
struct OwnedEnableTarget {
    root: std::path::PathBuf,
    path: std::path::PathBuf,
    identity: EnablePathIdentity,
    mode_used: SyncMode,
}

#[derive(Debug, Serialize)]
pub struct EnableSkillResultDto {
    pub restored_targets: usize,
}

fn saved_sync_mode(mode: &str) -> anyhow::Result<SyncMode> {
    match mode {
        "auto" => Ok(SyncMode::Auto),
        "symlink" => Ok(SyncMode::Symlink),
        "junction" => Ok(SyncMode::Junction),
        "copy" => Ok(SyncMode::Copy),
        _ => anyhow::bail!("UNSAFE_PATH|Unknown saved sync mode {mode}"),
    }
}

fn preflight_enable_targets<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
    skill: &SkillRecord,
) -> anyhow::Result<(Vec<SkillTargetRecord>, Vec<EnableTargetPlan>)> {
    validate_skill_name(&skill.name)?;
    let central_root = resolve_central_repo_path(app, store)?;
    let managed_source = std::path::PathBuf::from(&skill.central_path);
    validate_direct_skill_path(&central_root, &managed_source)?;
    let expected_source = direct_skill_child(&central_root, &skill.name)?;
    let metadata = std::fs::symlink_metadata(&managed_source)
        .with_context(|| format!("stat managed central Skill {:?}", managed_source))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        anyhow::bail!("UNSAFE_PATH|Managed central Skill must be a real directory");
    }
    if !paths_have_same_identity(&managed_source, &expected_source)? {
        anyhow::bail!("UNSAFE_PATH|Managed central Skill path does not match its record");
    }

    let original_targets = store.list_skill_targets(&skill.id)?;
    let mut plans_by_path =
        std::collections::BTreeMap::<std::path::PathBuf, EnableTargetPlan>::new();

    // Validate every saved row before creating even the first target.
    for target in &original_targets {
        if target.status != "disabled" {
            anyhow::bail!(
                "INCONSISTENT_TARGET_STATE|Saved target {} is not disabled",
                target.tool
            );
        }
        let runtime_tool = runtime_tool_by_key(store, &target.tool)?;
        if target.scope == "global" && !runtime_tool.installed {
            anyhow::bail!("TOOL_NOT_INSTALLED|{}", runtime_tool.key);
        }
        if target.scope == "project" && !runtime_tool.supports_project_scope {
            anyhow::bail!("PROJECT_SCOPE_UNSUPPORTED|{}", runtime_tool.key);
        }

        let (root, path) = validated_managed_target(store, target, &skill.name)?;
        ensure_distinct_roots(&root, &central_root)?;
        if path_entry_exists(&path)? {
            anyhow::bail!("TARGET_EXISTS|{}", path.to_string_lossy());
        }
        let mode = saved_sync_mode(&target.mode)?;

        if let Some(existing) = plans_by_path.get_mut(&path) {
            if existing.root != root || existing.mode != mode {
                anyhow::bail!(
                    "INCONSISTENT_TARGET_STATE|Shared target rows disagree on root or sync mode"
                );
            }
        } else {
            plans_by_path.insert(path.clone(), EnableTargetPlan { root, path, mode });
        }
    }

    Ok((original_targets, plans_by_path.into_values().collect()))
}

#[cfg(unix)]
fn capture_enable_path_identity(path: &std::path::Path) -> anyhow::Result<EnablePathIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("stat operation-owned target {:?}", path))?;
    Ok(EnablePathIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
    })
}

#[cfg(windows)]
fn capture_enable_path_identity(path: &std::path::Path) -> anyhow::Result<EnablePathIdentity> {
    use std::os::windows::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("stat operation-owned target {:?}", path))?;
    let handle = same_file::Handle::from_path(path)
        .with_context(|| format!("open operation-owned target {:?}", path))?;
    Ok(EnablePathIdentity {
        handle,
        file_attributes: metadata.file_attributes(),
        created_at: metadata.creation_time(),
    })
}

#[cfg(not(any(unix, windows)))]
fn capture_enable_path_identity(_path: &std::path::Path) -> anyhow::Result<EnablePathIdentity> {
    anyhow::bail!("UNSUPPORTED_PLATFORM|Filesystem identity is unavailable")
}

fn enable_path_identity_matches(
    path: &std::path::Path,
    expected: &EnablePathIdentity,
) -> anyhow::Result<bool> {
    if !path_entry_exists(path)? {
        return Ok(false);
    }
    Ok(capture_enable_path_identity(path)? == *expected)
}

fn move_owned_enable_target_to_trash(
    target: &OwnedEnableTarget,
    internal: bool,
) -> anyhow::Result<()> {
    if !path_entry_exists(&target.path)? {
        return Ok(());
    }
    if !enable_path_identity_matches(&target.path, &target.identity)? {
        anyhow::bail!(
            "OWNERSHIP_CHANGED|Refusing to move a replaced target: {}",
            target.path.display()
        );
    }
    if internal {
        move_internal_to_trash(&target.root, &target.path)?;
    } else {
        move_skill_to_trash(&target.root, &target.path)?;
    }
    Ok(())
}

fn rollback_restored_enable_targets(targets: &[OwnedEnableTarget]) -> anyhow::Result<()> {
    let mut errors = Vec::new();
    for target in targets.iter().rev() {
        if let Err(err) = move_owned_enable_target_to_trash(target, false) {
            errors.push(format!("{}: {err:#}", target.path.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("ROLLBACK_INCOMPLETE|{}", errors.join(" | "))
    }
}

fn unique_enable_staging_path(root: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    for _ in 0..8 {
        let candidate = root.join(format!(".skills-hub-enable-{}", Uuid::new_v4().simple()));
        if !path_entry_exists(&candidate)? {
            return Ok(candidate);
        }
    }
    anyhow::bail!("STAGING_COLLISION|Could not allocate an enable staging path")
}

fn stage_enable_copy(
    source: &std::path::Path,
    root: &std::path::Path,
) -> anyhow::Result<OwnedEnableTarget> {
    for _ in 0..8 {
        let staged = unique_enable_staging_path(root)?;
        match std::fs::create_dir(&staged) {
            Ok(()) => {
                let identity = capture_enable_path_identity(&staged)?;
                let owned = OwnedEnableTarget {
                    root: root.to_path_buf(),
                    path: staged,
                    identity,
                    mode_used: SyncMode::Copy,
                };
                if let Err(copy_err) = copy_dir_recursive(source, &owned.path) {
                    if let Err(rollback_err) = move_owned_enable_target_to_trash(&owned, true) {
                        anyhow::bail!(
                            "ROLLBACK_INCOMPLETE|enable staging copy failed ({copy_err:#}); {rollback_err:#}"
                        );
                    }
                    return Err(copy_err)
                        .context("enable staging copy failed; staging was moved to Trash");
                }
                return Ok(owned);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(anyhow::Error::new(err))
                    .with_context(|| format!("create enable staging directory {:?}", staged));
            }
        }
    }
    anyhow::bail!("STAGING_COLLISION|Could not claim an enable staging path")
}

#[cfg(unix)]
fn create_enable_staged_symlink(
    source: &std::path::Path,
    staged: &std::path::Path,
) -> anyhow::Result<()> {
    std::os::unix::fs::symlink(source, staged)
        .with_context(|| format!("create enable staging symlink {:?}", staged))
}

#[cfg(windows)]
fn create_enable_staged_symlink(
    source: &std::path::Path,
    staged: &std::path::Path,
) -> anyhow::Result<()> {
    std::os::windows::fs::symlink_dir(source, staged)
        .with_context(|| format!("create enable staging symlink {:?}", staged))
}

#[cfg(not(any(unix, windows)))]
fn create_enable_staged_symlink(
    _source: &std::path::Path,
    _staged: &std::path::Path,
) -> anyhow::Result<()> {
    anyhow::bail!("UNSUPPORTED_PLATFORM|Directory symlinks are unavailable")
}

#[cfg(windows)]
fn create_enable_staged_junction(
    source: &std::path::Path,
    staged: &std::path::Path,
) -> anyhow::Result<()> {
    junction::create(source, staged)
        .with_context(|| format!("create enable staging junction {:?}", staged))
}

#[cfg(not(windows))]
fn create_enable_staged_junction(
    _source: &std::path::Path,
    _staged: &std::path::Path,
) -> anyhow::Result<()> {
    anyhow::bail!("UNSUPPORTED_PLATFORM|Directory junctions are only available on Windows")
}

fn stage_enable_target(
    source: &std::path::Path,
    plan: &EnableTargetPlan,
) -> anyhow::Result<OwnedEnableTarget> {
    if matches!(plan.mode, SyncMode::Auto | SyncMode::Copy) {
        // Auto deliberately selects its safe Copy fallback here. The staged
        // directory is claimed before copying, so a competing process can
        // neither receive our files nor be mistaken for our partial output.
        return stage_enable_copy(source, &plan.root);
    }

    let staged = unique_enable_staging_path(&plan.root)?;
    match plan.mode {
        SyncMode::Symlink => create_enable_staged_symlink(source, &staged)?,
        SyncMode::Junction => create_enable_staged_junction(source, &staged)?,
        SyncMode::Auto | SyncMode::Copy => unreachable!("copy modes returned above"),
    }
    Ok(OwnedEnableTarget {
        root: plan.root.clone(),
        path: staged.clone(),
        identity: capture_enable_path_identity(&staged)?,
        mode_used: plan.mode,
    })
}

fn restore_enable_target_staged_with<B>(
    source: &std::path::Path,
    plan: &EnableTargetPlan,
    before_publish: B,
) -> anyhow::Result<OwnedEnableTarget>
where
    B: FnOnce(&EnableTargetPlan, &std::path::Path) -> anyhow::Result<()>,
{
    let mut staged = stage_enable_target(source, plan)?;
    if let Err(hook_err) = before_publish(plan, &staged.path) {
        if let Err(rollback_err) = move_owned_enable_target_to_trash(&staged, true) {
            anyhow::bail!(
                "ROLLBACK_INCOMPLETE|enable pre-publish step failed ({hook_err:#}); {rollback_err:#}"
            );
        }
        return Err(hook_err).context("enable pre-publish step failed; staging was rolled back");
    }
    if let Err(publish_err) = publish_staged_entry_no_replace(&plan.root, &staged.path, &plan.path)
    {
        if let Err(rollback_err) = move_owned_enable_target_to_trash(&staged, true) {
            anyhow::bail!(
                "ROLLBACK_INCOMPLETE|enable publish failed ({publish_err:#}); {rollback_err:#}"
            );
        }
        return Err(publish_err)
            .context("enable publish failed; operation-owned staging was rolled back");
    }

    staged.path = plan.path.clone();
    staged.root = plan.root.clone();
    Ok(staged)
}

fn enable_skill_and_restore_targets_with<R, F>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
    skill_id: &str,
    mut restore_target: F,
) -> anyhow::Result<EnableSkillResultDto>
where
    R: tauri::Runtime,
    F: FnMut(&std::path::Path, &EnableTargetPlan) -> anyhow::Result<OwnedEnableTarget>,
{
    let _mutation_guard = lock_central_mutation()?;
    let skill = store
        .get_skill_by_id(skill_id)?
        .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
    if skill.enabled {
        return Ok(EnableSkillResultDto {
            restored_targets: 0,
        });
    }

    let (original_targets, plans) = preflight_enable_targets(app, store, &skill)?;
    let managed_source = std::path::PathBuf::from(&skill.central_path);
    let mut restored_journal = Vec::new();
    let mut mode_by_path = std::collections::BTreeMap::new();

    for plan in &plans {
        let restored = match restore_target(&managed_source, plan) {
            Ok(restored) => {
                let ownership_check = if restored.root == plan.root && restored.path == plan.path {
                    enable_path_identity_matches(&restored.path, &restored.identity)
                } else {
                    Ok(false)
                };
                match ownership_check {
                    Ok(true) => restored,
                    Ok(false) => {
                        rollback_restored_enable_targets(&restored_journal)?;
                        anyhow::bail!(
                            "OWNERSHIP_CHANGED|Restore operation returned an unowned or replaced target"
                        );
                    }
                    Err(identity_err) => {
                        if let Err(rollback_err) =
                            rollback_restored_enable_targets(&restored_journal)
                        {
                            anyhow::bail!(
                                "ROLLBACK_INCOMPLETE|target identity check failed ({identity_err:#}); {rollback_err:#}"
                            );
                        }
                        return Err(identity_err).context(
                            "enable target identity check failed; earlier targets were rolled back",
                        );
                    }
                }
            }
            Err(restore_err) => {
                // The failed current plan is intentionally absent from the
                // journal. Only a successful restore can return an identity
                // proving that its live path belongs to this operation.
                if let Err(rollback_err) = rollback_restored_enable_targets(&restored_journal) {
                    anyhow::bail!(
                        "ROLLBACK_INCOMPLETE|target restore failed ({restore_err:#}); {rollback_err:#}"
                    );
                }
                return Err(restore_err)
                    .context("enable target restore failed; restored targets were rolled back");
            }
        };
        mode_by_path.insert(
            plan.path.clone(),
            sync_mode_name(restored.mode_used).to_string(),
        );
        restored_journal.push(restored);
    }

    let synced_at = now_ms();
    let restored_records_result = original_targets
        .iter()
        .map(|target| {
            let mut restored = target.clone();
            restored.mode = mode_by_path
                .get(std::path::Path::new(&target.target_path))
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("INCONSISTENT_TARGET_STATE|Missing restored target outcome")
                })?;
            restored.status = "ok".to_string();
            restored.last_error = None;
            restored.synced_at = Some(synced_at);
            Ok(restored)
        })
        .collect::<anyhow::Result<Vec<_>>>();
    let restored_records = match restored_records_result {
        Ok(targets) => targets,
        Err(state_err) => {
            if let Err(rollback_err) = rollback_restored_enable_targets(&restored_journal) {
                anyhow::bail!(
                    "ROLLBACK_INCOMPLETE|enable state preparation failed ({state_err:#}); {rollback_err:#}"
                );
            }
            return Err(state_err)
                .context("enable state preparation failed; restored targets were rolled back");
        }
    };

    if let Err(db_err) = store.enable_skill_with_targets_atomically(skill_id, &restored_records) {
        if let Err(rollback_err) = rollback_restored_enable_targets(&restored_journal) {
            anyhow::bail!(
                "ROLLBACK_INCOMPLETE|enable DB update failed ({db_err:#}); {rollback_err:#}"
            );
        }
        return Err(db_err)
            .context("enable DB transaction failed; database rolled back and files restored");
    }

    Ok(EnableSkillResultDto {
        restored_targets: original_targets.len(),
    })
}

fn enable_skill_and_restore_targets_impl<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
    skill_id: &str,
) -> anyhow::Result<EnableSkillResultDto> {
    enable_skill_and_restore_targets_with(app, store, skill_id, |source, plan| {
        restore_enable_target_staged_with(source, plan, |_plan, _staged| Ok(()))
    })
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn enable_skill_and_restore_targets(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<EnableSkillResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        enable_skill_and_restore_targets_impl(&app, &store, &skillId)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn unsync_skill_from_tool(
    store: State<'_, SkillStore>,
    skillId: String,
    tool: String,
    scope: Option<String>,
    projectPath: Option<String>,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _mutation_guard = lock_central_mutation()?;
        let skill = store
            .get_skill_by_id(&skillId)?
            .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
        validate_skill_name(&skill.name)?;
        let scope = normalize_scope(scope.as_deref())?;
        let project_path = if scope == "project" {
            let raw = projectPath
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("projectPath is required for project scope"))?;
            Some(expand_home_path(raw)?.to_string_lossy().to_string())
        } else {
            None
        };

        // Some tools share the same skills directory; unsync should update all of them.
        let group_tool_keys: Vec<String> =
            if let Ok(runtime_tool) = runtime_tool_by_key(&store, &tool) {
                runtime_tools_sharing_dir(&store, &runtime_tool, scope)?
                    .into_iter()
                    .map(|tool| tool.key)
                    .collect()
            } else if let Some(adapter) = adapter_by_key(&tool) {
                let group = if scope == "project" {
                    adapters_sharing_project_skills_dir(&adapter)
                } else {
                    crate::core::tool_adapters::adapters_sharing_skills_dir(&adapter)
                };
                // If none of the group tools are installed, do nothing (treat as already not effective).
                if scope == "global" {
                    let mut any_installed = false;
                    for a in &group {
                        if is_tool_installed(a)? {
                            any_installed = true;
                            break;
                        }
                    }
                    if !any_installed {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                group
                    .into_iter()
                    .map(|a| a.id.as_key().to_string())
                    .collect()
            } else {
                vec![tool.clone()]
            };

        // Validate every stored path before mutating any path or DB record.
        let fixed_central = fixed_central_repo_path().ok();
        let mut selected_targets = Vec::new();
        let mut central_coinciding_targets = Vec::new();
        for k in &group_tool_keys {
            if let Some(target) =
                store.get_skill_target(&skillId, k, scope, project_path.as_deref())?
            {
                let is_central_target = if let Some(ref central) = fixed_central {
                    let p = std::path::Path::new(&target.target_path);
                    p == std::path::Path::new(&skill.central_path)
                        || (p.starts_with(central) && target.scope == "global")
                } else {
                    false
                };

                if is_central_target {
                    central_coinciding_targets.push(target);
                } else {
                    validated_managed_target(&store, &target, &skill.name)?;
                    selected_targets.push(target);
                }
            }
        }

        // 清理与中心母版重合的虚假 target 记录（不执行物理文件删除）
        for target in &central_coinciding_targets {
            let _ = store.delete_skill_target(
                &skillId,
                &target.tool,
                &target.scope,
                target.project_path.as_deref(),
            );
        }

        let moved = move_managed_targets_to_trash(&store, &selected_targets, &skill.name)?;
        for target in &selected_targets {
            if let Err(db_err) = store.delete_skill_target(
                &skillId,
                &target.tool,
                &target.scope,
                target.project_path.as_deref(),
            ) {
                let mut recovery_errors = Vec::new();
                for original in &selected_targets {
                    if let Err(err) = store.upsert_skill_target(original) {
                        recovery_errors.push(format!("restore DB target: {err:#}"));
                    }
                }
                if let Err(err) = rollback_trashed_managed_paths(&moved) {
                    recovery_errors.push(format!("restore files: {err:#}"));
                }
                if recovery_errors.is_empty() {
                    return Err(db_err)
                        .context("unsync DB update failed; files and records restored");
                }
                anyhow::bail!(
                    "ROLLBACK_INCOMPLETE|unsync DB update failed ({db_err:#}); {}",
                    recovery_errors.join(" | ")
                );
            }
        }

        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn set_skill_enabled(
    store: State<'_, SkillStore>,
    skillId: String,
    enabled: bool,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _mutation_guard = lock_central_mutation()?;
        if !enabled {
            let skill = store
                .get_skill_by_id(&skillId)?
                .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
            validate_skill_name(&skill.name)?;
            let targets = store.list_skill_targets(&skillId)?;
            let active_targets: Vec<_> = targets
                .iter()
                .filter(|target| target.status != "disabled")
                .collect();
            for target in &active_targets {
                validated_managed_target(&store, target, &skill.name)?;
            }
            let active_targets = active_targets.into_iter().cloned().collect::<Vec<_>>();
            let moved = move_managed_targets_to_trash(&store, &active_targets, &skill.name)?;
            let persist_result = (|| -> anyhow::Result<()> {
                for target in &targets {
                    store.update_skill_target_status(
                        &skillId,
                        &target.tool,
                        &target.scope,
                        target.project_path.as_deref(),
                        "disabled",
                    )?;
                }
                store.set_skill_enabled(&skillId, false)?;
                Ok(())
            })();
            if let Err(db_err) = persist_result {
                let mut recovery_errors = Vec::new();
                for original in &targets {
                    if let Err(err) = store.upsert_skill_target(original) {
                        recovery_errors.push(format!("restore DB target: {err:#}"));
                    }
                }
                if let Err(err) = store.set_skill_enabled(&skillId, skill.enabled) {
                    recovery_errors.push(format!("restore Skill state: {err:#}"));
                }
                if let Err(err) = rollback_trashed_managed_paths(&moved) {
                    recovery_errors.push(format!("restore files: {err:#}"));
                }
                if recovery_errors.is_empty() {
                    return Err(db_err)
                        .context("disable DB update failed; files and records restored");
                }
                anyhow::bail!(
                    "ROLLBACK_INCOMPLETE|disable DB update failed ({db_err:#}); {}",
                    recovery_errors.join(" | ")
                );
            }
            return Ok::<_, anyhow::Error>(());
        }

        anyhow::bail!(
            "USE_ATOMIC_ENABLE|Use enable_skill_and_restore_targets when enabling a Skill"
        )
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
pub struct UpdateResultDto {
    pub skill_id: String,
    pub name: String,
    pub content_hash: Option<String>,
    pub source_revision: Option<String>,
    pub updated_targets: Vec<String>,
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn update_managed_skill(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<UpdateResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let res = update_managed_skill_from_source(&app, &store, &skillId)?;
        Ok::<_, anyhow::Error>(UpdateResultDto {
            skill_id: res.skill_id,
            name: res.name,
            content_hash: res.content_hash,
            source_revision: res.source_revision,
            updated_targets: res.updated_targets,
        })
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn search_github(
    store: State<'_, SkillStore>,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<RepoSummary>, String> {
    let store = store.inner().clone();
    let limit = limit.unwrap_or(10) as usize;
    tauri::async_runtime::spawn_blocking(move || {
        let proxy_url = get_github_proxy_url_core(&store)?;
        search_github_repos(&query, limit, None, &proxy_url)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_github_proxy_config(
    store: State<'_, SkillStore>,
) -> Result<GithubProxyConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        get_github_proxy_config_core(&store).map(to_github_proxy_config_dto)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn set_github_proxy_config(
    store: State<'_, SkillStore>,
    enabled: bool,
    port: u16,
) -> Result<GithubProxyConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        set_github_proxy_config_core(&store, enabled, port).map(to_github_proxy_config_dto)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn import_existing_skill(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    sourcePath: String,
    name: Option<String>,
) -> Result<InstallResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = std::path::Path::new(&sourcePath);
        // Validate SKILL.md exists before importing (fixes #8: prevents importing
        // directories that were "discovered" but lack a valid SKILL.md).
        if !source.join("SKILL.md").exists() {
            anyhow::bail!("SKILL_INVALID|missing_skill_md");
        }

        let skill_name = name.clone().unwrap_or_else(|| {
            source
                .file_name()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or_default()
        });

        // 优先检查：如果中心库中已经存在同名技能，直接复用该母版，绝不抛出任何冲突报错！
        if let Ok(skills) = store.list_skills() {
            if let Some(existing) = skills.into_iter().find(|s| s.name == skill_name) {
                return Ok(InstallResultDto {
                    skill_id: existing.id,
                    name: existing.name,
                    central_path: existing.central_path,
                    content_hash: existing.content_hash,
                });
            }
        }

        let result = import_existing_local_skill(&app, &store, source, name)?;
        Ok::<_, anyhow::Error>(to_install_dto(result))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
pub struct ManagedSkillDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_data_url: Option<String>,
    pub brand_color: Option<String>,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub central_path: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_sync_at: Option<i64>,
    pub enabled: bool,
    pub has_external_source: bool,
    pub updateable: bool,
    pub status: String,
    pub tags: Vec<TagDto>,
    pub targets: Vec<SkillTargetDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagDto {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct TagWithCountDto {
    pub id: i64,
    pub name: String,
    pub skill_count: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
pub struct SkillTargetDto {
    pub tool: String,
    pub scope: String,
    pub project_path: Option<String>,
    pub mode: String,
    pub status: String,
    pub last_error: Option<String>,
    pub target_path: String,
    pub synced_at: Option<i64>,
}

#[tauri::command]
pub fn get_managed_skills(store: State<'_, SkillStore>) -> Result<Vec<ManagedSkillDto>, String> {
    get_managed_skills_impl(store.inner())
}

#[tauri::command]
pub fn get_tags(store: State<'_, SkillStore>) -> Result<Vec<TagWithCountDto>, String> {
    store
        .list_tags_with_counts()
        .map(|tags| {
            tags.into_iter()
                .map(|tag| TagWithCountDto {
                    id: tag.id,
                    name: tag.name,
                    skill_count: tag.skill_count,
                    updated_at: tag.updated_at,
                })
                .collect()
        })
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn create_tag(store: State<'_, SkillStore>, name: String) -> Result<TagDto, String> {
    store
        .create_tag(&name)
        .map(|tag| TagDto {
            id: tag.id,
            name: tag.name,
        })
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn rename_tag(
    store: State<'_, SkillStore>,
    tagId: i64,
    name: String,
) -> Result<TagDto, String> {
    store
        .rename_tag(tagId, &name)
        .map(|tag| TagDto {
            id: tag.id,
            name: tag.name,
        })
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn delete_tag(store: State<'_, SkillStore>, tagId: i64) -> Result<(), String> {
    store.delete_tag(tagId).map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn get_skill_tags(
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<Vec<TagDto>, String> {
    store
        .get_skill_tags(&skillId)
        .map(|tags| {
            tags.into_iter()
                .map(|tag| TagDto {
                    id: tag.id,
                    name: tag.name,
                })
                .collect()
        })
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn set_skill_tags(
    store: State<'_, SkillStore>,
    skillId: String,
    tagIds: Vec<i64>,
) -> Result<(), String> {
    store
        .set_skill_tags(&skillId, &tagIds)
        .map_err(format_anyhow_error)
}

#[tauri::command]
pub fn get_untagged_skill_ids(store: State<'_, SkillStore>) -> Result<Vec<String>, String> {
    store.list_untagged_skill_ids().map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn delete_managed_skill(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _mutation_guard = lock_central_mutation()?;
        // 便于排查“按钮点了没反应”：确认前端确实触发了命令
        println!("[delete_managed_skill] skillId={}", skillId);

        let skill = store
            .get_skill_by_id(&skillId)?
            .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
        validate_skill_name(&skill.name)?;
        let central_root = resolve_central_repo_path(&app, &store)?;
        ensure_central_repo(&central_root)?;
        let central_path = std::path::PathBuf::from(&skill.central_path);
        validate_direct_skill_path(&central_root, &central_path)?;
        let expected_central = direct_skill_child(&central_root, &skill.name)?;
        if !paths_have_same_identity(&central_path, &expected_central)? {
            anyhow::bail!("UNSAFE_PATH|Stored central path does not match managed Skill name");
        }

        // Validate every target before moving anything. This prevents a corrupt
        // database row from turning a delete into an arbitrary filesystem move.
        let targets = store.list_skill_targets(&skillId)?;
        for target in &targets {
            validated_managed_target(&store, target, &skill.name)?;
        }

        let moved_targets = move_managed_targets_to_trash(&store, &targets, &skill.name)?;
        let trashed_central = match move_skill_to_trash(&central_root, &central_path) {
            Ok(Some(path)) => path,
            Ok(None) => {
                rollback_trashed_managed_paths(&moved_targets)?;
                anyhow::bail!("DELETE_FAILED|Central Skill disappeared before Trash move");
            }
            Err(err) => {
                rollback_trashed_managed_paths(&moved_targets).with_context(|| {
                    format!("central move failed ({err:#}) and target rollback was incomplete")
                })?;
                return Err(err).context("central move failed; targets were restored");
            }
        };
        if let Err(db_err) = store.delete_skill(&skillId) {
            let mut recovery_errors = Vec::new();
            if let Err(err) =
                restore_skill_from_trash(&central_root, &central_path, &trashed_central)
            {
                recovery_errors.push(format!("restore central Skill: {err:#}"));
            }
            if let Err(err) = rollback_trashed_managed_paths(&moved_targets) {
                recovery_errors.push(format!("restore targets: {err:#}"));
            }
            if recovery_errors.is_empty() {
                return Err(db_err).context("delete DB update failed; files were restored");
            }
            anyhow::bail!(
                "ROLLBACK_INCOMPLETE|delete DB update failed ({db_err:#}); {}",
                recovery_errors.join(" | ")
            );
        }

        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

fn to_install_dto(result: InstallResult) -> InstallResultDto {
    InstallResultDto {
        skill_id: result.skill_id,
        name: result.name,
        central_path: result.central_path.to_string_lossy().to_string(),
        content_hash: result.content_hash,
    }
}

fn to_auto_update_config_dto(config: AutoUpdateConfig) -> AutoUpdateConfigDto {
    let task_status = get_auto_update_task_status();
    AutoUpdateConfigDto {
        enabled: config.enabled,
        interval_hours: config.interval_hours,
        schedule_type: match config.schedule.schedule_type {
            AutoUpdateScheduleType::Interval => "interval".to_string(),
            AutoUpdateScheduleType::Daily => "daily".to_string(),
        },
        interval_value: config.schedule.interval_value,
        interval_unit: match config.schedule.interval_unit {
            AutoUpdateIntervalUnit::Minutes => "minutes".to_string(),
            AutoUpdateIntervalUnit::Hours => "hours".to_string(),
        },
        daily_time: config.schedule.daily_time,
        local_skill_count: config.local_skill_count,
        protected_local_skill_count: config.protected_local_skill_count,
        task_registered: task_status.registered,
        task_status_detail: task_status.detail,
        last_run_at: config.last_run_at,
        last_started_at: config.last_started_at,
        last_finished_at: config.last_finished_at,
        last_status: config.last_status,
        last_error: config.last_error,
        last_checked: config.last_checked,
        last_updated: config.last_updated,
        last_failed: config.last_failed,
        progress: config.progress,
    }
}

fn to_auto_update_run_result_dto(result: AutoUpdateRunResult) -> AutoUpdateRunResultDto {
    AutoUpdateRunResultDto {
        checked: result.checked,
        updated: result.updated,
        skipped: result.skipped,
        failed: result.failed,
        errors: result.errors,
        progress: result.progress,
    }
}

fn to_github_proxy_config_dto(config: GithubProxyConfig) -> GithubProxyConfigDto {
    GithubProxyConfigDto {
        enabled: config.enabled,
        port: config.port,
        url: config.url,
        auto_detected: config.auto_detected,
    }
}

fn now_ms() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis() as i64
}

fn managed_skill_status(
    skill: &SkillRecord,
    update_capability: &ManagedSkillUpdateCapability,
) -> String {
    if skill.status != "ok" {
        return skill.status.clone();
    }
    if update_capability.integrity_error.is_some() {
        "error".to_string()
    } else {
        skill.status.clone()
    }
}

fn apply_managed_icon_data_url_budget(
    icon_data_url: &mut Option<String>,
    remaining_bytes: &mut usize,
) {
    let Some(encoded_icon) = icon_data_url.as_ref() else {
        return;
    };
    let encoded_bytes = encoded_icon.len();
    if encoded_bytes > *remaining_bytes {
        *icon_data_url = None;
        *remaining_bytes = 0;
        return;
    }
    *remaining_bytes -= encoded_bytes;
}

fn get_managed_skills_impl(store: &SkillStore) -> Result<Vec<ManagedSkillDto>, String> {
    get_managed_skills_impl_with_icon_budget(store, MAX_MANAGED_ICON_DATA_URL_BYTES)
}

fn get_managed_skills_impl_with_icon_budget(
    store: &SkillStore,
    icon_data_url_budget: usize,
) -> Result<Vec<ManagedSkillDto>, String> {
    let skills = store.list_skills().map_err(|err| err.to_string())?;
    let mut remaining_icon_bytes = icon_data_url_budget;
    Ok(skills
        .into_iter()
        .map(|skill| {
            let mut ui_metadata = if remaining_icon_bytes == 0 {
                Default::default()
            } else {
                read_skill_ui_metadata(std::path::Path::new(&skill.central_path))
            };
            apply_managed_icon_data_url_budget(
                &mut ui_metadata.icon_data_url,
                &mut remaining_icon_bytes,
            );
            let update_capability = managed_skill_update_capability(&skill);
            let status = managed_skill_status(&skill, &update_capability);
            let targets = store
                .list_skill_targets(&skill.id)
                .unwrap_or_default()
                .into_iter()
                .map(|target| SkillTargetDto {
                    tool: target.tool,
                    scope: target.scope,
                    project_path: target.project_path,
                    mode: target.mode,
                    status: target.status,
                    last_error: target.last_error,
                    target_path: target.target_path,
                    synced_at: target.synced_at,
                })
                .collect();
            let tags = store
                .get_skill_tags(&skill.id)
                .unwrap_or_default()
                .into_iter()
                .map(|tag| TagDto {
                    id: tag.id,
                    name: tag.name,
                })
                .collect();

            ManagedSkillDto {
                id: skill.id,
                name: skill.name,
                description: skill.description,
                icon_data_url: ui_metadata.icon_data_url,
                brand_color: ui_metadata.brand_color,
                source_type: skill.source_type,
                source_ref: skill
                    .source_ref
                    .filter(|_| update_capability.source_ref_safe_to_expose),
                central_path: skill.central_path,
                created_at: skill.created_at,
                updated_at: skill.updated_at,
                last_sync_at: skill.last_sync_at,
                enabled: skill.enabled,
                has_external_source: update_capability.has_external_source,
                updateable: update_capability.updateable,
                status,
                tags,
                targets,
            }
        })
        .collect())
}

#[derive(Debug, Serialize)]
pub struct FeaturedSkillDto {
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub downloads: u64,
    pub stars: u64,
    pub source_url: String,
}

impl From<FeaturedSkill> for FeaturedSkillDto {
    fn from(s: FeaturedSkill) -> Self {
        Self {
            slug: s.slug,
            name: s.name,
            summary: s.summary,
            downloads: s.downloads,
            stars: s.stars,
            source_url: s.source_url,
        }
    }
}

#[tauri::command]
pub async fn get_featured_skills(
    store: State<'_, SkillStore>,
) -> Result<Vec<FeaturedSkillDto>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skills = fetch_featured_skills(&store)?;
        Ok::<_, anyhow::Error>(skills.into_iter().map(FeaturedSkillDto::from).collect())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
pub struct OnlineSkillDto {
    pub name: String,
    pub installs: u64,
    pub source: String,
    pub source_url: String,
}

impl From<OnlineSkillResult> for OnlineSkillDto {
    fn from(r: OnlineSkillResult) -> Self {
        Self {
            name: r.name,
            installs: r.installs,
            source: r.source,
            source_url: r.source_url,
        }
    }
}

#[tauri::command]
pub async fn search_skills_online(
    store: State<'_, SkillStore>,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<OnlineSkillDto>, String> {
    let store = store.inner().clone();
    let limit = limit.unwrap_or(20) as usize;
    tauri::async_runtime::spawn_blocking(move || {
        let proxy_url = get_github_proxy_url_core(&store)?;
        let results = search_skills_online_core(&query, limit, &proxy_url)?;
        Ok::<_, anyhow::Error>(results.into_iter().map(OnlineSkillDto::from).collect())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFileEntry {
    pub path: String,
    pub size: u64,
}

fn validated_skill_content_root<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
    skill_id: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let skill = store
        .get_skill_by_id(skill_id)?
        .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
    validate_skill_name(&skill.name)?;
    let central_root = resolve_central_repo_path(app, store)?;
    ensure_central_repo(&central_root)?;
    let stored = std::path::PathBuf::from(&skill.central_path);
    validate_direct_skill_path(&central_root, &stored)?;
    let expected = direct_skill_child(&central_root, &skill.name)?;
    if !paths_have_same_identity(&stored, &expected)? {
        anyhow::bail!("UNSAFE_PATH|Stored central path does not match managed Skill name");
    }
    Ok(stored)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn list_skill_files(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: Option<String>,
    skill_id: Option<String>,
    centralPath: Option<String>,
) -> Result<Vec<SkillFileEntry>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = if let Some(ref cp) = centralPath {
            let p = std::path::PathBuf::from(cp);
            if p.is_dir() {
                p
            } else {
                let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
                validated_skill_content_root(&app, &store, sid)?
            }
        } else {
            let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
            validated_skill_content_root(&app, &store, sid)?
        };
        let entries = crate::core::skill_files::list_files(&path)?;
        Ok::<_, anyhow::Error>(
            entries
                .into_iter()
                .map(|e| SkillFileEntry {
                    path: e.path,
                    size: e.size,
                })
                .collect(),
        )
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn read_skill_file(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: Option<String>,
    skill_id: Option<String>,
    centralPath: Option<String>,
    filePath: Option<String>,
    file_path: Option<String>,
) -> Result<String, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let base = if let Some(ref cp) = centralPath {
            let p = std::path::PathBuf::from(cp);
            if p.is_dir() {
                p
            } else {
                let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
                validated_skill_content_root(&app, &store, sid)?
            }
        } else {
            let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
            validated_skill_content_root(&app, &store, sid)?
        };
        let fp = filePath.as_deref().or(file_path.as_deref()).unwrap_or("");
        crate::core::skill_files::read_file(&base, fp)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SkillIgnoreItemDto {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_ignored: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SkillIgnoreConfigDto {
    pub rules: Vec<String>,
    pub items: Vec<SkillIgnoreItemDto>,
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn get_skill_ignore_config(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: Option<String>,
    skill_id: Option<String>,
    centralPath: Option<String>,
) -> Result<SkillIgnoreConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = if let Some(ref cp) = centralPath {
            let p = std::path::PathBuf::from(cp);
            if p.is_dir() {
                p
            } else {
                let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
                validated_skill_content_root(&app, &store, sid)?
            }
        } else {
            let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
            validated_skill_content_root(&app, &store, sid)?
        };

        let skillignore_path = root.join(".skillignore");
        let mut rules = Vec::new();
        if let Ok(content) = std::fs::read_to_string(&skillignore_path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    rules.push(trimmed.to_string());
                }
            }
        }

        let mut items = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&root) {
            for entry in read_dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == ".git" || name == ".skillignore" {
                    continue;
                }
                let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                let clean_name = name.trim_matches('/').to_string();
                let is_ignored = rules.iter().any(|r| {
                    let clean_rule = r.replace('\\', "/").trim_matches('/').to_string();
                    clean_rule == clean_name
                        || (clean_rule.starts_with('*') && clean_name.ends_with(&clean_rule[1..]))
                        || (clean_rule.ends_with('*') && clean_name.starts_with(&clean_rule[..clean_rule.len().saturating_sub(1)]))
                });
                items.push(SkillIgnoreItemDto {
                    name,
                    path: if is_dir { format!("{}/", clean_name) } else { clean_name },
                    is_dir,
                    is_ignored,
                });
            }
        }
        items.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                b.is_dir.cmp(&a.is_dir)
            } else {
                a.name.cmp(&b.name)
            }
        });

        Ok::<_, anyhow::Error>(SkillIgnoreConfigDto { rules, items })
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn save_skill_ignore_config(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: Option<String>,
    skill_id: Option<String>,
    centralPath: Option<String>,
    rules: Vec<String>,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = if let Some(ref cp) = centralPath {
            let p = std::path::PathBuf::from(cp);
            if p.is_dir() {
                p
            } else {
                let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
                validated_skill_content_root(&app, &store, sid)?
            }
        } else {
            let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
            validated_skill_content_root(&app, &store, sid)?
        };

        let skillignore_path = root.join(".skillignore");
        
        let mut text = String::from("# Managed by Skills Hub\n");
        for rule in &rules {
            let trimmed = rule.trim();
            if !trimmed.is_empty() {
                text.push_str(trimmed);
                text.push('\n');
            }
        }
        std::fs::write(&skillignore_path, text)
            .with_context(|| format!("write .skillignore to {:?}", skillignore_path))?;

        // 重新计算哈希并更新 SQLite 记录
        if let Ok(new_hash) = crate::core::content_hash::hash_dir(&root) {
            let sid = skillId.as_deref().or(skill_id.as_deref()).unwrap_or("");
            if let Ok(Some(mut record)) = store.get_skill_by_id(sid) {
                record.content_hash = Some(new_hash);
                record.updated_at = now_ms();
                let _ = store.upsert_skill(&record);
            }
            let _ = auto_reconcile_skill_targets(&store, Some(sid));
        }

        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn reconcile_all_skill_targets(
    store: State<'_, SkillStore>,
) -> Result<usize, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        auto_reconcile_skill_targets(&store, None).map_err(format_anyhow_error)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn cancel_current_operation(cancel: State<'_, Arc<CancelToken>>) -> Result<(), String> {
    cancel.cancel();
    Ok(())
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SkillTargetComparisonDto {
    pub tool: String,
    pub tool_label: String,
    pub target_path: String,
    pub central_path: String,
    pub central_mtime: u64,
    pub tool_mtime: u64,
    pub central_file_count: usize,
    pub tool_file_count: usize,
    pub status: String,
    pub diff_description: String,
}

fn scan_dir_latest_mtime(dir: &std::path::Path) -> (u64, usize) {
    if !dir.is_dir() {
        return (0, 0);
    }
    let mut max_mtime = 0u64;
    let mut file_count = 0usize;
    let rules = crate::core::content_hash::load_ignore_patterns(dir);
    let walker = walkdir::WalkDir::new(dir).follow_links(false).into_iter();
    for entry in walker.filter_entry(|e| !crate::core::content_hash::is_entry_ignored(dir, e, &rules)) {
        if let Ok(entry) = entry {
            if entry.file_type().is_file() {
                file_count += 1;
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        let ms = modified
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        if ms > max_mtime {
                            max_mtime = ms;
                        }
                    }
                }
            }
        }
    }
    (max_mtime, file_count)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn get_skill_target_comparisons(
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<Vec<SkillTargetComparisonDto>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skillId)?
            .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
        let central_path = std::path::PathBuf::from(&skill.central_path);
        let (central_mtime, central_files) = scan_dir_latest_mtime(&central_path);

        let targets = store.list_skill_targets(&skillId)?;
        let mut results = Vec::new();

        for target in targets {
            let target_path = std::path::PathBuf::from(&target.target_path);
            if !target_path.exists() {
                results.push(SkillTargetComparisonDto {
                    tool: target.tool.clone(),
                    tool_label: target.tool.clone(),
                    target_path: target.target_path.clone(),
                    central_path: skill.central_path.clone(),
                    central_mtime,
                    tool_mtime: 0,
                    central_file_count: central_files,
                    tool_file_count: 0,
                    status: "missing".to_string(),
                    diff_description: "目标目录不存在".to_string(),
                });
                continue;
            }

            let (tool_mtime, tool_files) = scan_dir_latest_mtime(&target_path);

            let status = if tool_mtime > central_mtime + 2000 {
                "tool_newer".to_string()
            } else if central_mtime > tool_mtime + 2000 {
                "central_newer".to_string()
            } else {
                "synced".to_string()
            };

            let diff_description = match status.as_str() {
                "tool_newer" => "检测到此工具修改较新，可设为母版",
                "central_newer" => "母版较新，可更新此软件",
                _ => "内容与母版完全一致",
            }
            .to_string();

            results.push(SkillTargetComparisonDto {
                tool: target.tool.clone(),
                tool_label: target.tool.clone(),
                target_path: target.target_path.clone(),
                central_path: skill.central_path.clone(),
                central_mtime,
                tool_mtime,
                central_file_count: central_files,
                tool_file_count: tool_files,
                status,
                diff_description,
            });
        }

        Ok::<_, anyhow::Error>(results)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn promote_target_to_central(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: String,
    tool: String,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock_central_mutation()?;
        let skill = store
            .get_skill_by_id(&skillId)?
            .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
        let target = store
            .get_skill_target(&skillId, &tool, "global", None)?
            .ok_or_else(|| anyhow::anyhow!("target tool not found for skill"))?;

        let src_dir = std::path::PathBuf::from(&target.target_path);
        let dst_dir = std::path::PathBuf::from(&skill.central_path);

        if !src_dir.is_dir() {
            anyhow::bail!("Source tool directory {:?} does not exist", src_dir);
        }

        let ignore_rules = crate::core::content_hash::load_ignore_patterns(&dst_dir);

        // 递归反向拷贝文件：目标软件 -> 中心母版 (跳过受保护的忽略规则，如 local/)
        for entry in walkdir::WalkDir::new(&src_dir).into_iter().filter_map(|e| e.ok()) {
            if crate::core::content_hash::is_entry_ignored(&src_dir, &entry, &ignore_rules) {
                continue;
            }

            let rel_path = match entry.path().strip_prefix(&src_dir) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let dst_file = dst_dir.join(rel_path);
            if entry.file_type().is_dir() {
                let _ = std::fs::create_dir_all(&dst_file);
            } else if entry.file_type().is_file() {
                if let Some(parent) = dst_file.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::copy(entry.path(), &dst_file);
            }
        }

        // 重新计算母版 content_hash 并更新 SQLite 记录
        if let Ok(new_hash) = crate::core::content_hash::hash_dir(&dst_dir) {
            if let Ok(Some(mut record)) = store.get_skill_by_id(&skillId) {
                record.content_hash = Some(new_hash);
                record.updated_at = now_ms();
                let _ = store.upsert_skill(&record);
            }
        }

        let _ = auto_reconcile_skill_targets(&store, Some(&skillId));

        // 设为母版成功后，若开启了自动备份，异步触发一次后台增量备份
        let app_clone = app.clone();
        let store_clone = store.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let config = crate::core::backup::get_backup_config(&store_clone);
            if config.enabled {
                let _ = crate::core::backup::perform_backup(&app_clone, &store_clone, None);
            }
        });

        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_backup_config(store: State<'_, SkillStore>) -> Result<crate::core::backup::BackupConfig, String> {
    let store = store.inner().clone();
    Ok(crate::core::backup::get_backup_config(&store))
}

#[allow(non_snake_case)]
#[tauri::command]
pub async fn save_backup_config(
    store: State<'_, SkillStore>,
    config: crate::core::backup::BackupConfig,
) -> Result<(), String> {
    let store = store.inner().clone();
    crate::core::backup::save_backup_config(&store, &config).map_err(format_anyhow_error)
}

#[allow(non_snake_case)]
#[tauri::command]
pub async fn create_backup_now(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    customDir: Option<String>,
) -> Result<crate::core::backup::BackupManifest, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::core::backup::perform_backup(&app, &store, customDir)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(format_anyhow_error)
}

#[allow(non_snake_case)]
#[tauri::command]
pub async fn restore_backup_now(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    sourceDir: Option<String>,
) -> Result<usize, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::core::backup::perform_restore(&app, &store, sourceDir)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(format_anyhow_error)
}

#[cfg(test)]
#[path = "tests/commands.rs"]
mod tests;
