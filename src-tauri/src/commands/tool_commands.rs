use tauri::State;

use crate::core::skill_store::SkillStore;
use crate::core::tool_adapters::{load_tool_config, save_tool_config};

use super::common::{
    format_anyhow_error, runtime_tools, ToolConfigDto, ToolInfoDto, ToolStatusDto,
};

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
