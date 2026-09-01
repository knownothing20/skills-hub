mod commands;
mod core;

use std::sync::Arc;

use core::cancel_token::CancelToken;
use core::onboarding::{
    get_discovery_scan_settings, save_discovery_scan_config, DiscoveryScanConfig,
};
use core::skill_store::{default_db_path, SkillStore};
use core::tool_adapters::{default_tool_adapters, load_tool_config, save_tool_config, ToolConfig};
use tauri::Manager;
use tauri_plugin_log::{Target, TargetKind};

fn init_store<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> anyhow::Result<SkillStore> {
    let db_path = default_db_path(app)?;
    let store = SkillStore::new(db_path);
    store.ensure_schema()?;
    ensure_safe_defaults(&store)?;
    let normalized = core::auto_update::normalize_self_sourced_local_skills(&store)?;
    if normalized > 0 {
        log::info!(
            "normalized {} legacy self-sourced local Skill records",
            normalized
        );
    }
    let sanitized = core::auto_update::sanitize_unsafe_git_sources(&store)?;
    if sanitized > 0 {
        log::warn!(
            "cleared {} unsafe legacy Git source references before UI startup",
            sanitized
        );
    }
    Ok(store)
}

fn ensure_safe_defaults(store: &SkillStore) -> anyhow::Result<()> {
    const FULL_MANAGER_MIGRATION_KEY: &str = "safe_full_manager_enabled_v1";
    if store.get_setting(FULL_MANAGER_MIGRATION_KEY)?.is_none() {
        let config = load_tool_config(store)?;
        let all_builtin_keys = default_tool_adapters()
            .into_iter()
            .map(|adapter| adapter.id.as_key().to_string())
            .collect::<std::collections::HashSet<_>>();
        let disabled_keys = config
            .disabled_builtin_tools
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        if config.custom_tools.is_empty() && disabled_keys == all_builtin_keys {
            save_tool_config(store, ToolConfig::default())?;
        }
        store.set_setting(FULL_MANAGER_MIGRATION_KEY, "true")?;
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("failed to resolve home"))?;
    let allowed = [home.join(".agents/skills"), home.join(".codex/skills")];
    let settings = get_discovery_scan_settings(store)?;
    let disabled_source_keys = settings
        .sources
        .into_iter()
        .filter(|source| !allowed.iter().any(|path| path == &source.path))
        .map(|source| source.key)
        .collect();
    save_discovery_scan_config(
        store,
        DiscoveryScanConfig {
            disabled_source_keys,
        },
    )?;

    if store
        .get_setting(core::cache_cleanup::GIT_CACHE_CLEANUP_DAYS_KEY)?
        .is_none()
    {
        store.set_setting(core::cache_cleanup::GIT_CACHE_CLEANUP_DAYS_KEY, "0")?;
    }
    Ok(())
}

fn refresh_enabled_auto_update_task(store: &SkillStore) -> anyhow::Result<()> {
    let config = core::auto_update::get_auto_update_config(store)?;
    if !config.enabled {
        return Ok(());
    }

    let scheduler = core::system_scheduler::current_scheduler_config(config.schedule)?;
    if core::system_scheduler::ensure_installed_auto_update_task(&scheduler)? {
        log::info!("repaired automatic update task registration");
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .targets([
                        Target::new(TargetKind::LogDir { file_name: None }),
                        #[cfg(desktop)]
                        Target::new(TargetKind::Stdout),
                    ])
                    .build(),
            )?;

            let store = init_store(app.handle()).map_err(tauri::Error::from)?;

            let is_background_update = std::env::args()
                .collect::<Vec<_>>()
                .windows(2)
                .any(|pair| pair[0] == "--background-task" && pair[1] == "update-skills");
            let force_background_update = std::env::args().any(|arg| arg == "--force");

            if is_background_update {
                #[cfg(target_os = "macos")]
                {
                    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
                let run_result = if force_background_update {
                    core::auto_update::run_auto_update_now(app.handle(), &store).map(Some)
                } else {
                    core::auto_update::run_due_auto_update(app.handle(), &store)
                };
                match run_result {
                    Ok(Some(result)) => {
                        log::info!(
                            "auto update finished: checked={}, updated={}, skipped={}, failed={}",
                            result.checked,
                            result.updated,
                            result.skipped,
                            result.failed
                        );
                        app.handle().exit(if result.failed == 0 { 0 } else { 2 });
                    }
                    Ok(None) => app.handle().exit(0),
                    Err(err) => {
                        eprintln!("auto update failed: {err:#}");
                        app.handle().exit(1);
                    }
                }
                return Ok(());
            }

            // Keep an enabled background task bound to the current installed
            // executable after the app is renamed or moved. A registration
            // repair must never prevent the foreground manager from opening.
            if let Err(err) = refresh_enabled_auto_update_task(&store) {
                log::warn!("failed to refresh automatic update task: {err:#}");
            }

            app.manage(store.clone());
            app.manage(Arc::new(CancelToken::new()));

            let adopted = core::installer::adopt_existing_central_skills(app.handle(), &store)
                .map_err(tauri::Error::from)?;
            log::info!(
                "safe central adoption: adopted={}, existing={}, invalid={}, symlink={}, missing_skill_md={}, other={}",
                adopted.adopted,
                adopted.already_registered,
                adopted.skipped_invalid_name,
                adopted.skipped_symlink,
                adopted.skipped_missing_skill_md,
                adopted.skipped_other
            );

            // Backfill description for skills that were installed before V2 schema.
            core::installer::backfill_skill_descriptions(&store);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_central_repo_path,
            commands::get_recent_projects,
            commands::save_recent_project,
            commands::get_tool_status,
            commands::get_tool_config,
            commands::set_tool_config,
            commands::get_git_cache_ttl_secs,
            commands::set_git_cache_ttl_secs,
            commands::clear_git_cache_now,
            commands::get_auto_update_config,
            commands::set_auto_update_config,
            commands::run_auto_update_now,
            commands::trigger_auto_update_task_now_cmd,
            commands::get_onboarding_plan,
            commands::get_discovery_scan_settings,
            commands::set_discovery_scan_config,
            commands::install_local,
            commands::list_local_skills_cmd,
            commands::install_local_selection,
            commands::install_git,
            commands::list_git_skills_cmd,
            commands::install_git_selection,
            commands::sync_skill_dir,
            commands::sync_skill_to_tool,
            commands::unsync_skill_from_tool,
            commands::set_skill_enabled,
            commands::enable_skill_and_restore_targets,
            commands::update_managed_skill,
            commands::search_github,
            commands::get_github_proxy_config,
            commands::set_github_proxy_config,
            commands::import_existing_skill,
            commands::get_managed_skills,
            commands::get_tags,
            commands::create_tag,
            commands::rename_tag,
            commands::delete_tag,
            commands::get_skill_tags,
            commands::set_skill_tags,
            commands::get_untagged_skill_ids,
            commands::delete_managed_skill,
            commands::get_featured_skills,
            commands::search_skills_online,
            commands::list_skill_files,
            commands::read_skill_file,
            commands::cancel_current_operation
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                window.app_handle().exit(0);
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, _event| {});
}
