use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use super::auto_update::{AutoUpdateIntervalUnit, AutoUpdateSchedule, AutoUpdateScheduleType};
use super::safe_fs::move_internal_to_trash;

pub const TASK_LABEL: &str = "com.skillshub.autoupdate";
const BACKGROUND_TASK_ARGS: [&str; 3] = ["--background-task", "update-skills", "--force"];

#[derive(Clone, Debug)]
pub struct SchedulerTaskStatus {
    pub registered: bool,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct SchedulerConfig {
    pub executable: PathBuf,
    pub schedule: AutoUpdateSchedule,
}

pub fn current_scheduler_config(schedule: AutoUpdateSchedule) -> Result<SchedulerConfig> {
    let current_exe = std::env::current_exe().context("resolve current executable")?;
    Ok(SchedulerConfig {
        executable: scheduler_executable_for_current_exe(&current_exe)?,
        schedule,
    })
}

pub fn scheduler_executable_for_current_exe(current_exe: &Path) -> Result<PathBuf> {
    #[cfg(debug_assertions)]
    {
        let path = current_exe.to_string_lossy();
        if path.contains("/target/debug/") {
            let runner = current_exe.with_file_name("skills-hub-autoupdate-runner");
            std::fs::copy(current_exe, &runner)
                .with_context(|| format!("copy auto update runner to {:?}", runner))?;
            return Ok(runner);
        }
    }

    Ok(current_exe.to_path_buf())
}

pub fn install_auto_update_task(config: &SchedulerConfig) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        install_macos_launch_agent(config)
    }
    #[cfg(target_os = "windows")]
    {
        install_windows_task(config)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        install_linux_systemd_timer(config)
    }
}

/// Repair an enabled macOS LaunchAgent only when it is missing or stale.
///
/// Startup repair is deliberately limited to app bundles installed in the
/// system or current user's Applications directory. This keeps a copy opened
/// from a DMG, App Translocation, or a build directory from replacing the
/// persistent background-task executable.
pub fn ensure_installed_auto_update_task(config: &SchedulerConfig) -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().context("resolve home dir")?;
        let mut installed_config = config.clone();
        installed_config.executable = std::fs::canonicalize(&config.executable)
            .with_context(|| format!("resolve scheduler executable {:?}", config.executable))?;
        if !macos_scheduler_executable_is_stable(&installed_config.executable, &home) {
            return Ok(false);
        }

        let plist = home
            .join("Library/LaunchAgents")
            .join(format!("{TASK_LABEL}.plist"));
        let existing = match std::fs::read_to_string(&plist) {
            Ok(contents) => Some(contents),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(err).with_context(|| format!("read LaunchAgent {:?}", plist));
            }
        };
        let registered = get_macos_launch_agent_status().registered;
        if !macos_auto_update_task_needs_refresh(existing.as_deref(), &installed_config, registered)
        {
            return Ok(false);
        }

        install_macos_launch_agent(&installed_config)?;
        Ok(true)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = config;
        Ok(false)
    }
}

pub fn uninstall_auto_update_task() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        uninstall_macos_launch_agent()
    }
    #[cfg(target_os = "windows")]
    {
        uninstall_windows_task()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        uninstall_linux_systemd_timer()
    }
}

pub fn get_auto_update_task_status() -> SchedulerTaskStatus {
    #[cfg(target_os = "macos")]
    {
        get_macos_launch_agent_status()
    }
    #[cfg(target_os = "windows")]
    {
        get_windows_task_status()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        get_linux_systemd_timer_status()
    }
}

pub fn trigger_auto_update_task_now() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        trigger_macos_launch_agent_now()
    }
    #[cfg(target_os = "windows")]
    {
        trigger_windows_task_now()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        trigger_linux_systemd_service_now()
    }
}

#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
pub fn build_launch_agent_plist(config: &SchedulerConfig) -> String {
    let exe = xml_escape(&config.executable.to_string_lossy());
    let schedule = match config.schedule.schedule_type {
        AutoUpdateScheduleType::Interval => {
            let interval_secs = config
                .schedule
                .interval_minutes()
                .saturating_mul(60)
                .max(60);
            format!("  <key>StartInterval</key>\n  <integer>{interval_secs}</integer>")
        }
        AutoUpdateScheduleType::Daily => {
            let (hour, minute) = split_daily_time(&config.schedule.daily_time);
            format!(
                "  <key>StartCalendarInterval</key>\n  <dict>\n    <key>Hour</key>\n    <integer>{hour}</integer>\n    <key>Minute</key>\n    <integer>{minute}</integer>\n  </dict>"
            )
        }
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{TASK_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>{}</string>
    <string>{}</string>
    <string>{}</string>
  </array>
{schedule}
  <key>RunAtLoad</key>
  <false/>
  <key>StandardOutPath</key>
  <string>/tmp/skills-hub-auto-update.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/skills-hub-auto-update.err</string>
</dict>
</plist>
"#,
        BACKGROUND_TASK_ARGS[0], BACKGROUND_TASK_ARGS[1], BACKGROUND_TASK_ARGS[2],
    )
}

pub fn macos_scheduler_executable_is_stable(executable: &Path, home: &Path) -> bool {
    let Some(app_bundle) = executable.ancestors().find(|ancestor| {
        ancestor
            .extension()
            .map(|extension| extension.to_string_lossy().eq_ignore_ascii_case("app"))
            .unwrap_or(false)
    }) else {
        return false;
    };

    let is_bundle_executable = executable
        .parent()
        .and_then(|parent| parent.strip_prefix(app_bundle).ok())
        .map(|relative| relative == Path::new("Contents/MacOS"))
        .unwrap_or(false);
    if !is_bundle_executable {
        return false;
    }

    if app_bundle
        .components()
        .any(|component| component.as_os_str() == "target")
    {
        return false;
    }

    app_bundle.starts_with(Path::new("/Applications"))
        || app_bundle.starts_with(home.join("Applications"))
}

pub fn macos_launch_agent_matches_config(existing_plist: &str, config: &SchedulerConfig) -> bool {
    let Ok(value) = plist::Value::from_reader_xml(existing_plist.as_bytes()) else {
        return false;
    };
    let Some(dictionary) = value.as_dictionary() else {
        return false;
    };

    if dictionary.get("Label").and_then(plist::Value::as_string) != Some(TASK_LABEL) {
        return false;
    }

    let Some(arguments) = dictionary
        .get("ProgramArguments")
        .and_then(plist::Value::as_array)
    else {
        return false;
    };
    let executable = config.executable.to_string_lossy();
    let expected_arguments = [
        executable.as_ref(),
        BACKGROUND_TASK_ARGS[0],
        BACKGROUND_TASK_ARGS[1],
        BACKGROUND_TASK_ARGS[2],
    ];
    if arguments.len() != expected_arguments.len()
        || arguments
            .iter()
            .zip(expected_arguments)
            .any(|(actual, expected)| actual.as_string() != Some(expected))
    {
        return false;
    }

    match config.schedule.schedule_type {
        AutoUpdateScheduleType::Interval => {
            let expected_seconds = config
                .schedule
                .interval_minutes()
                .saturating_mul(60)
                .max(60);
            dictionary.get("StartCalendarInterval").is_none()
                && plist_integer(dictionary.get("StartInterval")) == Some(expected_seconds)
        }
        AutoUpdateScheduleType::Daily => {
            let Some(calendar) = dictionary
                .get("StartCalendarInterval")
                .and_then(plist::Value::as_dictionary)
            else {
                return false;
            };
            let (hour, minute) = split_daily_time(&config.schedule.daily_time);
            dictionary.get("StartInterval").is_none()
                && plist_integer(calendar.get("Hour")) == Some(i64::from(hour))
                && plist_integer(calendar.get("Minute")) == Some(i64::from(minute))
        }
    }
}

pub fn macos_auto_update_task_needs_refresh(
    existing_plist: Option<&str>,
    config: &SchedulerConfig,
    registered: bool,
) -> bool {
    !registered
        || !existing_plist
            .map(|existing| macos_launch_agent_matches_config(existing, config))
            .unwrap_or(false)
}

fn plist_integer(value: Option<&plist::Value>) -> Option<i64> {
    value.and_then(|value| {
        value.as_signed_integer().or_else(|| {
            value
                .as_unsigned_integer()
                .and_then(|number| i64::try_from(number).ok())
        })
    })
}

#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
pub fn summarize_launchctl_status(output: &str) -> String {
    let mut parts = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("state =") || trimmed.starts_with("last exit code =") {
            parts.push(trimmed.to_string());
        }
    }
    if parts.is_empty() {
        "launch agent loaded".to_string()
    } else {
        parts.join("; ")
    }
}

#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
pub fn launchctl_kickstart_args(uid: u32) -> Vec<String> {
    vec![
        "kickstart".to_string(),
        "-k".to_string(),
        format!("gui/{uid}/{TASK_LABEL}"),
    ]
}

#[cfg_attr(not(any(test, target_os = "windows")), allow(dead_code))]
pub fn windows_schtasks_args(config: &SchedulerConfig) -> Result<Vec<String>> {
    let mut args = vec![
        "/Create".to_string(),
        "/F".to_string(),
        "/TN".to_string(),
        TASK_LABEL.to_string(),
    ];
    match config.schedule.schedule_type {
        AutoUpdateScheduleType::Interval => {
            args.push("/SC".to_string());
            match config.schedule.interval_unit {
                AutoUpdateIntervalUnit::Minutes
                    if config.schedule.interval_value <= 1_439 =>
                {
                    args.push("MINUTE".to_string());
                    args.push("/MO".to_string());
                    args.push(config.schedule.interval_value.to_string());
                }
                AutoUpdateIntervalUnit::Hours if config.schedule.interval_value <= 23 => {
                    args.push("HOURLY".to_string());
                    args.push("/MO".to_string());
                    args.push(config.schedule.interval_value.to_string());
                }
                AutoUpdateIntervalUnit::Minutes
                    if config.schedule.interval_value % (24 * 60) == 0 =>
                {
                    args.push("DAILY".to_string());
                    args.push("/MO".to_string());
                    args.push((config.schedule.interval_value / (24 * 60)).to_string());
                }
                AutoUpdateIntervalUnit::Hours
                    if config.schedule.interval_value % 24 == 0 =>
                {
                    args.push("DAILY".to_string());
                    args.push("/MO".to_string());
                    args.push((config.schedule.interval_value / 24).to_string());
                }
                _ => anyhow::bail!(
                    "Windows scheduled tasks support intervals up to 1439 minutes or 23 hours; longer intervals must be whole days"
                ),
            }
        }
        AutoUpdateScheduleType::Daily => {
            args.push("/SC".to_string());
            args.push("DAILY".to_string());
            args.push("/ST".to_string());
            args.push(config.schedule.daily_time.clone());
        }
    }
    args.push("/TR".to_string());
    args.push(format!(
        "\"{}\" {} {} {}",
        config.executable.to_string_lossy(),
        BACKGROUND_TASK_ARGS[0],
        BACKGROUND_TASK_ARGS[1],
        BACKGROUND_TASK_ARGS[2]
    ));
    Ok(args)
}

#[cfg_attr(not(any(test, target_os = "windows")), allow(dead_code))]
pub fn windows_schtasks_run_args() -> Vec<String> {
    vec![
        "/Run".to_string(),
        "/TN".to_string(),
        TASK_LABEL.to_string(),
    ]
}

#[cfg_attr(not(any(test, all(unix, not(target_os = "macos")))), allow(dead_code))]
pub fn build_systemd_service(config: &SchedulerConfig) -> String {
    format!(
        "[Unit]\nDescription=Skills Hub automatic skill update\n\n[Service]\nType=oneshot\nExecStart={} {} {} {}\n",
        systemd_escape_path(&config.executable),
        BACKGROUND_TASK_ARGS[0],
        BACKGROUND_TASK_ARGS[1],
        BACKGROUND_TASK_ARGS[2]
    )
}

#[cfg_attr(not(any(test, all(unix, not(target_os = "macos")))), allow(dead_code))]
pub fn build_systemd_timer(config: &SchedulerConfig) -> String {
    let schedule = match config.schedule.schedule_type {
        AutoUpdateScheduleType::Interval => {
            format!("OnUnitActiveSec={}min", config.schedule.interval_minutes())
        }
        AutoUpdateScheduleType::Daily => {
            format!("OnCalendar=*-*-* {}:00", config.schedule.daily_time)
        }
    };
    format!(
        "[Unit]\nDescription=Run Skills Hub automatic skill update\n\n[Timer]\nOnBootSec=5min\n{schedule}\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
    )
}

#[cfg_attr(not(any(test, all(unix, not(target_os = "macos")))), allow(dead_code))]
pub fn systemd_start_args() -> Vec<String> {
    vec![
        "--user".to_string(),
        "start".to_string(),
        format!("{TASK_LABEL}.service"),
    ]
}

#[cfg(target_os = "macos")]
fn install_macos_launch_agent(config: &SchedulerConfig) -> Result<()> {
    let dir = dirs::home_dir()
        .context("resolve home dir")?
        .join("Library/LaunchAgents");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {:?}", dir))?;
    let plist = dir.join(format!("{TASK_LABEL}.plist"));
    let _ = Command::new("launchctl").arg("unload").arg(&plist).output();
    std::fs::write(&plist, build_launch_agent_plist(config))
        .with_context(|| format!("write {:?}", plist))?;
    let out = Command::new("launchctl")
        .arg("load")
        .arg(&plist)
        .output()
        .context("launchctl load")?;
    if !out.status.success() {
        anyhow::bail!(
            "launchctl load failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall_macos_launch_agent() -> Result<()> {
    let plist = dirs::home_dir()
        .context("resolve home dir")?
        .join("Library/LaunchAgents")
        .join(format!("{TASK_LABEL}.plist"));
    let _ = Command::new("launchctl").arg("unload").arg(&plist).output();
    if plist.exists() {
        let root = plist.parent().context("LaunchAgent has no parent")?;
        move_internal_to_trash(root, &plist)
            .with_context(|| format!("move LaunchAgent to Trash {:?}", plist))?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn get_macos_launch_agent_status() -> SchedulerTaskStatus {
    let plist = match dirs::home_dir() {
        Some(home) => home
            .join("Library/LaunchAgents")
            .join(format!("{TASK_LABEL}.plist")),
        None => {
            return SchedulerTaskStatus {
                registered: false,
                detail: "home dir not found".to_string(),
            };
        }
    };
    if !plist.exists() {
        return SchedulerTaskStatus {
            registered: false,
            detail: format!("missing {}", plist.to_string_lossy()),
        };
    }
    let uid = std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        });
    let Some(uid) = uid else {
        return SchedulerTaskStatus {
            registered: true,
            detail: format!("plist exists: {}", plist.to_string_lossy()),
        };
    };
    let out = Command::new("launchctl")
        .args(["print", &format!("gui/{uid}/{TASK_LABEL}")])
        .output();
    match out {
        Ok(out) if out.status.success() => SchedulerTaskStatus {
            registered: true,
            detail: summarize_launchctl_status(&String::from_utf8_lossy(&out.stdout)),
        },
        Ok(out) => SchedulerTaskStatus {
            registered: false,
            detail: format!(
                "plist exists but launchctl cannot find loaded job: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        },
        Err(err) => SchedulerTaskStatus {
            registered: true,
            detail: format!("plist exists; launchctl check failed: {err}"),
        },
    }
}

#[cfg(target_os = "macos")]
fn trigger_macos_launch_agent_now() -> Result<()> {
    let status = get_macos_launch_agent_status();
    if !status.registered {
        anyhow::bail!("auto update task is not ready: {}", status.detail);
    }
    let uid = current_uid()?;
    let out = Command::new("launchctl")
        .args(launchctl_kickstart_args(uid))
        .output()
        .context("launchctl kickstart")?;
    if !out.status.success() {
        anyhow::bail!(
            "launchctl kickstart failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn current_uid() -> Result<u32> {
    let out = Command::new("id").arg("-u").output().context("id -u")?;
    if !out.status.success() {
        anyhow::bail!("id -u failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    raw.trim()
        .parse::<u32>()
        .with_context(|| format!("parse uid from {}", raw.trim()))
}

#[cfg(target_os = "windows")]
fn install_windows_task(config: &SchedulerConfig) -> Result<()> {
    let out = Command::new("schtasks")
        .args(windows_schtasks_args(config)?)
        .output()
        .context("schtasks create")?;
    if !out.status.success() {
        anyhow::bail!(
            "schtasks create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn uninstall_windows_task() -> Result<()> {
    let out = Command::new("schtasks")
        .args(["/Delete", "/F", "/TN", TASK_LABEL])
        .output()
        .context("schtasks delete")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !stderr.contains("cannot find") {
            anyhow::bail!("schtasks delete failed: {}", stderr);
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn get_windows_task_status() -> SchedulerTaskStatus {
    let out = Command::new("schtasks")
        .args(["/Query", "/TN", TASK_LABEL])
        .output();
    match out {
        Ok(out) if out.status.success() => SchedulerTaskStatus {
            registered: true,
            detail: "scheduled task registered".to_string(),
        },
        Ok(out) => SchedulerTaskStatus {
            registered: false,
            detail: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        },
        Err(err) => SchedulerTaskStatus {
            registered: false,
            detail: format!("schtasks query failed: {err}"),
        },
    }
}

#[cfg(target_os = "windows")]
fn trigger_windows_task_now() -> Result<()> {
    let status = get_windows_task_status();
    if !status.registered {
        anyhow::bail!("auto update task is not ready: {}", status.detail);
    }
    let out = Command::new("schtasks")
        .args(windows_schtasks_run_args())
        .output()
        .context("schtasks run")?;
    if !out.status.success() {
        anyhow::bail!(
            "schtasks run failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn install_linux_systemd_timer(config: &SchedulerConfig) -> Result<()> {
    let dir = dirs::home_dir()
        .context("resolve home dir")?
        .join(".config/systemd/user");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {:?}", dir))?;
    let service = dir.join(format!("{TASK_LABEL}.service"));
    let timer = dir.join(format!("{TASK_LABEL}.timer"));
    std::fs::write(&service, build_systemd_service(config))
        .with_context(|| format!("write {:?}", service))?;
    std::fs::write(&timer, build_systemd_timer(config))
        .with_context(|| format!("write {:?}", timer))?;
    run_systemctl_user(&["daemon-reload"])?;
    run_systemctl_user(&["enable", "--now", &format!("{TASK_LABEL}.timer")])?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn uninstall_linux_systemd_timer() -> Result<()> {
    let _ = run_systemctl_user(&["disable", "--now", &format!("{TASK_LABEL}.timer")]);
    let dir = dirs::home_dir()
        .context("resolve home dir")?
        .join(".config/systemd/user");
    for ext in ["service", "timer"] {
        let path = dir.join(format!("{TASK_LABEL}.{ext}"));
        if path.exists() {
            move_internal_to_trash(&dir, &path)
                .with_context(|| format!("move systemd unit to Trash {:?}", path))?;
        }
    }
    let _ = run_systemctl_user(&["daemon-reload"]);
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn get_linux_systemd_timer_status() -> SchedulerTaskStatus {
    let unit = format!("{TASK_LABEL}.timer");
    let enabled = Command::new("systemctl")
        .arg("--user")
        .args(["is-enabled", &unit])
        .output();
    let active = Command::new("systemctl")
        .arg("--user")
        .args(["is-active", &unit])
        .output();
    let enabled_ok = enabled
        .as_ref()
        .map(|out| out.status.success())
        .unwrap_or(false);
    let active_ok = active
        .as_ref()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if enabled_ok && active_ok {
        return SchedulerTaskStatus {
            registered: true,
            detail: "systemd timer enabled and active".to_string(),
        };
    }
    let detail = match (enabled, active) {
        (Ok(enabled), Ok(active)) => format!(
            "enabled={}, active={}",
            String::from_utf8_lossy(&enabled.stdout).trim(),
            String::from_utf8_lossy(&active.stdout).trim()
        ),
        (Err(err), _) | (_, Err(err)) => format!("systemctl check failed: {err}"),
    };
    SchedulerTaskStatus {
        registered: false,
        detail,
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn trigger_linux_systemd_service_now() -> Result<()> {
    let status = get_linux_systemd_timer_status();
    if !status.registered {
        anyhow::bail!("auto update task is not ready: {}", status.detail);
    }
    let args = systemd_start_args();
    let out = Command::new("systemctl")
        .args(args.iter().map(String::as_str))
        .output()
        .context("systemctl --user start auto update service")?;
    if !out.status.success() {
        anyhow::bail!(
            "systemctl start failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run_systemctl_user(args: &[&str]) -> Result<()> {
    let out = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .with_context(|| format!("systemctl --user {}", args.join(" ")))?;
    if !out.status.success() {
        anyhow::bail!(
            "systemctl --user {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn systemd_escape_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if raw.contains(' ') {
        format!("\"{}\"", raw.replace('"', "\\\""))
    } else {
        raw.to_string()
    }
}

fn split_daily_time(time: &str) -> (u8, u8) {
    let Some((hour, minute)) = time.split_once(':') else {
        return (3, 0);
    };
    (
        hour.parse::<u8>().unwrap_or(3),
        minute.parse::<u8>().unwrap_or(0),
    )
}

#[cfg(test)]
#[path = "tests/system_scheduler.rs"]
mod tests;
