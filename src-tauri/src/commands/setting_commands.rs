use std::sync::Arc;
use serde::Serialize;
use tauri::State;

use crate::core::backup::{
    get_backup_config as get_backup_config_core, perform_backup, perform_restore,
    save_backup_config as save_backup_config_core, BackupConfig, BackupManifest,
};
use crate::core::cache_cleanup::{
    cleanup_git_cache_dirs, get_git_cache_ttl_secs as get_git_cache_ttl_secs_core,
    set_git_cache_ttl_secs as set_git_cache_ttl_secs_core,
};
use crate::core::cancel_token::CancelToken;
use crate::core::central_repo::{ensure_central_repo, resolve_central_repo_path};
use crate::core::network_proxy::{
    get_github_proxy_config as get_github_proxy_config_core,
    set_github_proxy_config as set_github_proxy_config_core, GithubProxyConfig,
};
use crate::core::skill_store::SkillStore;

use super::common::{expand_home_path, format_anyhow_error, RECENT_PROJECTS_SETTING};

#[derive(Debug, Serialize)]
pub struct GithubProxyConfigDto {
    pub enabled: bool,
    pub port: u16,
    pub url: String,
    pub auto_detected: bool,
}

pub fn to_github_proxy_config_dto(config: GithubProxyConfig) -> GithubProxyConfigDto {
    GithubProxyConfigDto {
        enabled: config.enabled,
        port: config.port,
        url: config.url,
        auto_detected: config.auto_detected,
    }
}

pub fn get_recent_projects_impl(store: &SkillStore) -> Result<Vec<String>, anyhow::Error> {
    let projects = store
        .get_setting(RECENT_PROJECTS_SETTING)?
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default();
    Ok(projects)
}

pub fn save_recent_project_impl(
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
pub async fn get_default_sync_mode(store: State<'_, SkillStore>) -> Result<String, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mode = store
            .get_setting("default_sync_mode")?
            .unwrap_or_else(|| "copy".to_string());
        Ok::<_, anyhow::Error>(mode)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn set_default_sync_mode(
    store: State<'_, SkillStore>,
    mode: String,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if mode != "copy" && mode != "junction" {
            anyhow::bail!("Invalid sync mode: must be 'copy' or 'junction'");
        }
        store.set_setting("default_sync_mode", &mode)?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_backup_config(store: State<'_, SkillStore>) -> Result<BackupConfig, String> {
    let store = store.inner().clone();
    Ok(get_backup_config_core(&store))
}

#[allow(non_snake_case)]
#[tauri::command]
pub async fn save_backup_config(
    store: State<'_, SkillStore>,
    config: BackupConfig,
) -> Result<(), String> {
    let store = store.inner().clone();
    save_backup_config_core(&store, &config).map_err(format_anyhow_error)
}

#[allow(non_snake_case)]
#[tauri::command]
pub async fn create_backup_now(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    customDir: Option<String>,
) -> Result<BackupManifest, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        perform_backup(&app, &store, customDir)
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
        perform_restore(&app, &store, sourceDir)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub fn cancel_current_operation(cancel: State<'_, Arc<CancelToken>>) -> Result<(), String> {
    cancel.cancel();
    Ok(())
}
