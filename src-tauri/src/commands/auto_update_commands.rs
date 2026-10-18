use anyhow::Context;
use serde::Serialize;
use tauri::State;

use crate::core::auto_update::{
    get_auto_update_config as get_auto_update_config_core, record_auto_update_triggered,
    run_auto_update_now as run_auto_update_now_core,
    set_auto_update_config as set_auto_update_config_core, AutoUpdateConfig,
    AutoUpdateIntervalUnit, AutoUpdateProgressSnapshot, AutoUpdateRunResult, AutoUpdateSchedule,
    AutoUpdateScheduleType,
};
use crate::core::skill_store::SkillStore;
use crate::core::system_scheduler::{
    current_scheduler_config, get_auto_update_task_status, install_auto_update_task,
    trigger_auto_update_task_now, uninstall_auto_update_task,
};

use super::common::format_anyhow_error;

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

pub fn to_auto_update_config_dto(config: AutoUpdateConfig) -> AutoUpdateConfigDto {
    let task_status = if config.enabled {
        get_auto_update_task_status()
    } else {
        crate::core::system_scheduler::SchedulerTaskStatus {
            registered: false,
            detail: "off".to_string(),
        }
    };
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

pub fn to_auto_update_run_result_dto(result: AutoUpdateRunResult) -> AutoUpdateRunResultDto {
    AutoUpdateRunResultDto {
        checked: result.checked,
        updated: result.updated,
        skipped: result.skipped,
        failed: result.failed,
        errors: result.errors,
        progress: result.progress,
    }
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
