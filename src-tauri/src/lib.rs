mod commands;
mod core;

use std::sync::Arc;

use core::cancel_token::CancelToken;
use core::onboarding::{
    get_discovery_scan_settings, save_discovery_scan_config, DiscoveryScanConfig,
};
use core::skill_store::{default_db_path, SkillStore};
use core::tool_adapters::{default_tool_adapters, load_tool_config, save_tool_config, ToolConfig};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use tauri_plugin_log::{Target, TargetKind};

fn open_folder_in_explorer(path: &std::path::Path) {
    if !path.exists() {
        let _ = std::fs::create_dir_all(path);
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

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

    const DISCOVERY_SCAN_INIT_KEY: &str = "discovery_scan_init_v2";
    if store.get_setting(DISCOVERY_SCAN_INIT_KEY)?.is_none() {
        save_discovery_scan_config(
            store,
            DiscoveryScanConfig {
                disabled_source_keys: Vec::new(),
            },
        )?;
        store.set_setting(DISCOVERY_SCAN_INIT_KEY, "true")?;
    }

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

            let handle_clone = app.handle().clone();
            let store_clone = store.clone();
            tauri::async_runtime::spawn_blocking(move || {
                match core::installer::adopt_existing_central_skills(&handle_clone, &store_clone) {
                    Ok(adopted) => {
                        log::info!(
                            "safe central adoption: adopted={}, existing={}, invalid={}, symlink={}, missing_skill_md={}, other={}",
                            adopted.adopted,
                            adopted.already_registered,
                            adopted.skipped_invalid_name,
                            adopted.skipped_symlink,
                            adopted.skipped_missing_skill_md,
                            adopted.skipped_other
                        );
                    }
                    Err(err) => {
                        log::warn!("background central adoption warning: {err:#}");
                    }
                }
                core::installer::backfill_skill_descriptions(&store_clone);

                // 如果启用了自动备份，启动时异步静默更新备份
                let config = core::backup::get_backup_config(&store_clone);
                if config.enabled {
                    log::info!("running initial silent auto-backup to {:?}", config.backup_dir);
                    let _ = core::backup::perform_backup(&handle_clone, &store_clone, None);
                }
            });

            // 建立系统托盘菜单与托盘图标
            let show_item = MenuItem::with_id(app, "show", "显示 Skills Hub", true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let central_item = MenuItem::with_id(
                app,
                "open_central",
                "📂 打开技能中心库 (~/.agents/skills)",
                true,
                None::<&str>,
            )?;
            let backup_item = MenuItem::with_id(
                app,
                "open_backup",
                "📂 打开自动备份目录 (D:\\GitHub\\skill-hub\\backup)",
                true,
                None::<&str>,
            )?;
            let antigravity_item = MenuItem::with_id(
                app,
                "open_antigravity",
                "📂 打开 Antigravity 技能目录",
                true,
                None::<&str>,
            )?;
            let cursor_item = MenuItem::with_id(
                app,
                "open_cursor",
                "📂 打开 Cursor 技能目录",
                true,
                None::<&str>,
            )?;
            let claude_item = MenuItem::with_id(
                app,
                "open_claude",
                "📂 打开 Claude 技能目录",
                true,
                None::<&str>,
            )?;
            let repo_item = MenuItem::with_id(
                app,
                "open_repo_folder",
                "📂 打开项目源码目录 (D:\\GitHub\\skill-hub)",
                true,
                None::<&str>,
            )?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let sync_item = MenuItem::with_id(
                app,
                "sync_now",
                "🔄 立即检查并同步技能",
                true,
                None::<&str>,
            )?;
            let sep3 = PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "quit", "❌ 退出 Skills Hub", true, None::<&str>)?;

            let tray_menu = Menu::with_items(
                app,
                &[
                    &show_item,
                    &sep1,
                    &central_item,
                    &backup_item,
                    &antigravity_item,
                    &cursor_item,
                    &claude_item,
                    &repo_item,
                    &sep2,
                    &sync_item,
                    &sep3,
                    &quit_item,
                ],
            )?;

            if let Some(app_icon) = app.default_window_icon() {
                let _tray = TrayIconBuilder::new()
                    .icon(app_icon.clone())
                    .tooltip("Skills Hub - Agent 技能管家")
                    .menu(&tray_menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(move |app_handle, event| match event.id.as_ref() {
                        "show" => {
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                        "open_central" => {
                            if let Some(home) = dirs::home_dir() {
                                open_folder_in_explorer(&home.join(".agents/skills"));
                            }
                        }
                        "open_backup" => {
                            let store = app_handle.state::<SkillStore>().inner().clone();
                            let config = core::backup::get_backup_config(&store);
                            open_folder_in_explorer(std::path::Path::new(&config.backup_dir));
                        }
                        "open_antigravity" => {
                            if let Some(home) = dirs::home_dir() {
                                open_folder_in_explorer(&home.join(".gemini/config/skills"));
                            }
                        }
                        "open_cursor" => {
                            if let Some(home) = dirs::home_dir() {
                                open_folder_in_explorer(&home.join(".cursor/skills"));
                            }
                        }
                        "open_claude" => {
                            if let Some(home) = dirs::home_dir() {
                                open_folder_in_explorer(&home.join(".claude/skills"));
                            }
                        }
                        "open_repo_folder" => {
                            open_folder_in_explorer(std::path::Path::new("d:\\GitHub\\skill-hub"));
                        }
                        "sync_now" => {
                            let handle = app_handle.clone();
                            let store = app_handle.state::<SkillStore>().inner().clone();
                            tauri::async_runtime::spawn(async move {
                                log::info!("manual sync triggered from system tray");
                                let _ = core::auto_update::run_auto_update_now(&handle, &store);
                            });
                        }
                        "quit" => {
                            app_handle.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                if window.is_visible().unwrap_or(false) {
                                    let _ = window.hide();
                                } else {
                                    let _ = window.show();
                                    let _ = window.unminimize();
                                    let _ = window.set_focus();
                                }
                            }
                        }
                    })
                    .build(app)?;
            }

            // 检查启动参数是否包含最小化启动
            let start_minimized = std::env::args().any(|arg| arg == "--minimized" || arg == "-m");
            if start_minimized {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

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
            commands::get_skill_ignore_config,
            commands::save_skill_ignore_config,
            commands::reconcile_all_skill_targets,
            commands::cancel_current_operation,
            commands::get_skill_target_comparisons,
            commands::promote_target_to_central,
            commands::get_backup_config,
            commands::save_backup_config,
            commands::create_backup_now,
            commands::restore_backup_now
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 点击右上角 X 关闭时，阻止退出并隐藏至托盘
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, _event| {});
}

