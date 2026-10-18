use tauri::State;

use crate::core::installer::import_existing_local_skill;
use crate::core::onboarding::{
    build_onboarding_plan, get_discovery_scan_settings as get_discovery_scan_settings_core,
    save_discovery_scan_config, DiscoveryScanConfig, DiscoveryScanSettings, OnboardingPlan,
};
use crate::core::skill_store::SkillStore;

use super::common::format_anyhow_error;
use super::install_commands::{to_install_dto, InstallResultDto};
use super::sync_commands::auto_reconcile_skill_targets;

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
