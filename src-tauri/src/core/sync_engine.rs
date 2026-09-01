use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::safe_fs::{
    move_skill_to_trash, path_entry_exists, restore_skill_from_trash, validate_direct_skill_path,
    TrashReceipt,
};

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncMode {
    #[default]
    Auto,
    Symlink,
    Junction,
    Copy,
}

#[derive(Clone, Debug)]
pub struct SyncOutcome {
    pub mode_used: SyncMode,
    pub target_path: PathBuf,
    pub replaced: bool,
}

pub fn sync_dir_hybrid(source: &Path, target: &Path) -> Result<SyncOutcome> {
    ensure_parent_dir(target)?;
    validate_sync_target(target)?;
    if path_entry_exists(target)? {
        if is_same_link(target, source) {
            return Ok(SyncOutcome {
                mode_used: SyncMode::Symlink,
                target_path: target.to_path_buf(),
                replaced: false,
            });
        }
        anyhow::bail!("target already exists: {:?}", target);
    }

    if try_link_dir(source, target).is_ok() {
        return Ok(SyncOutcome {
            mode_used: SyncMode::Symlink,
            target_path: target.to_path_buf(),
            replaced: false,
        });
    }

    #[cfg(windows)]
    if try_junction(source, target).is_ok() {
        return Ok(SyncOutcome {
            mode_used: SyncMode::Junction,
            target_path: target.to_path_buf(),
            replaced: false,
        });
    }

    copy_dir_recursive(source, target)?;
    Ok(SyncOutcome {
        mode_used: SyncMode::Copy,
        target_path: target.to_path_buf(),
        replaced: false,
    })
}

pub fn sync_dir_hybrid_with_overwrite(
    source: &Path,
    target: &Path,
    overwrite: bool,
) -> Result<SyncOutcome> {
    let mut trashed_previous = None;
    if path_entry_exists(target)? {
        if is_same_link(target, source) {
            return Ok(SyncOutcome {
                mode_used: SyncMode::Symlink,
                target_path: target.to_path_buf(),
                replaced: false,
            });
        }

        if overwrite {
            trashed_previous = move_sync_target_to_trash(target)
                .with_context(|| format!("move existing target to Trash {:?}", target))?;
        } else {
            anyhow::bail!("target already exists: {:?}", target);
        }
    }

    match sync_dir_hybrid(source, target) {
        Ok(mut out) => {
            out.replaced = trashed_previous.is_some();
            Ok(out)
        }
        Err(err) => rollback_sync_failure(target, trashed_previous.as_ref(), err),
    }
}

pub fn sync_dir_copy_with_overwrite(
    source: &Path,
    target: &Path,
    overwrite: bool,
) -> Result<SyncOutcome> {
    let mut trashed_previous = None;
    if path_entry_exists(target)? {
        if overwrite {
            trashed_previous = move_sync_target_to_trash(target)
                .with_context(|| format!("move existing target to Trash {:?}", target))?;
        } else {
            anyhow::bail!("target already exists: {:?}", target);
        }
    }

    ensure_parent_dir(target)?;
    if let Err(err) = copy_dir_recursive(source, target) {
        return rollback_sync_failure(target, trashed_previous.as_ref(), err);
    }

    Ok(SyncOutcome {
        mode_used: SyncMode::Copy,
        target_path: target.to_path_buf(),
        replaced: trashed_previous.is_some(),
    })
}

pub fn sync_dir_with_mode_with_overwrite(
    mode: SyncMode,
    source: &Path,
    target: &Path,
    overwrite: bool,
) -> Result<SyncOutcome> {
    match mode {
        SyncMode::Auto => sync_dir_hybrid_with_overwrite(source, target, overwrite),
        SyncMode::Copy => sync_dir_copy_with_overwrite(source, target, overwrite),
        SyncMode::Symlink | SyncMode::Junction => {
            sync_dir_link_with_overwrite(mode, source, target, overwrite)
        }
    }
}

fn sync_dir_link_with_overwrite(
    mode: SyncMode,
    source: &Path,
    target: &Path,
    overwrite: bool,
) -> Result<SyncOutcome> {
    let mut trashed_previous = None;
    if path_entry_exists(target)? {
        if is_same_link(target, source) {
            return Ok(SyncOutcome {
                mode_used: mode,
                target_path: target.to_path_buf(),
                replaced: false,
            });
        }
        if overwrite {
            trashed_previous = move_sync_target_to_trash(target)
                .with_context(|| format!("move existing target to Trash {:?}", target))?;
        } else {
            anyhow::bail!("target already exists: {:?}", target);
        }
    }

    ensure_parent_dir(target)?;
    let create_result = match mode {
        SyncMode::Symlink => try_link_dir(source, target),
        SyncMode::Junction => try_junction(source, target),
        SyncMode::Auto | SyncMode::Copy => unreachable!("link mode required"),
    };
    if let Err(err) = create_result {
        return rollback_sync_failure(target, trashed_previous.as_ref(), err);
    }

    Ok(SyncOutcome {
        mode_used: mode,
        target_path: target.to_path_buf(),
        replaced: trashed_previous.is_some(),
    })
}

pub fn sync_dir_for_tool_with_overwrite(
    tool_key: &str,
    source: &Path,
    target: &Path,
    overwrite: bool,
) -> Result<SyncOutcome> {
    // Cursor 目前不支持软链/junction：强制使用 copy，避免同步后在 Cursor 内不可用。
    if tool_key.eq_ignore_ascii_case("cursor") {
        return sync_dir_copy_with_overwrite(source, target, overwrite);
    }
    sync_dir_hybrid_with_overwrite(source, target, overwrite)
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create dir {:?}", parent))?;
    }
    Ok(())
}

fn validate_sync_target(path: &Path) -> Result<()> {
    let root = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Sync target has no parent"))?;
    validate_direct_skill_path(root, path)
}

fn move_sync_target_to_trash(path: &Path) -> Result<Option<TrashReceipt>> {
    let root = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Sync target has no parent"))?;
    move_skill_to_trash(root, path)
}

fn rollback_sync_failure<T>(
    target: &Path,
    trashed_previous: Option<&TrashReceipt>,
    original: anyhow::Error,
) -> Result<T> {
    if let Some(backup) = trashed_previous {
        let root = target
            .parent()
            .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Sync target has no parent"))?;
        restore_skill_from_trash(root, target, backup).with_context(|| {
            format!("sync failed ({original:#}) and the previous target could not be restored")
        })?;
    }
    Err(original)
}

fn is_same_link(link_path: &Path, target: &Path) -> bool {
    if let Ok(existing) = std::fs::read_link(link_path) {
        return existing == target;
    }
    false
}

fn try_link_dir(source: &Path, target: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, target)
            .with_context(|| format!("symlink {:?} -> {:?}", target, source))?;
        Ok(())
    }

    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(source, target)
            .with_context(|| format!("symlink {:?} -> {:?}", target, source))?;
        return Ok(());
    }

    #[cfg(not(any(unix, windows)))]
    anyhow::bail!("symlink not supported on this platform");
}

#[cfg(windows)]
fn try_junction(source: &Path, target: &Path) -> Result<()> {
    junction::create(source, target)
        .with_context(|| format!("junction {:?} -> {:?}", target, source))?;
    Ok(())
}

#[cfg(not(windows))]
fn try_junction(_source: &Path, _target: &Path) -> Result<()> {
    anyhow::bail!("junction not supported on this platform");
}

fn should_skip_copy(entry: &walkdir::DirEntry) -> bool {
    entry.file_name() == ".git"
}

pub fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    let profile = std::env::var("SKILLS_HUB_PROFILE_IO")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let started = std::time::Instant::now();
    let mut copied_files: u64 = 0;
    let mut copied_bytes: u64 = 0;

    for entry in walkdir::WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !should_skip_copy(entry))
    {
        let entry = entry?;
        if should_skip_copy(&entry) {
            continue;
        }
        let relative = entry.path().strip_prefix(source)?;
        let target_path = target.join(relative);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target_path)
                .with_context(|| format!("create dir {:?}", target_path))?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let bytes = std::fs::copy(entry.path(), &target_path)
                .with_context(|| format!("copy file {:?} -> {:?}", entry.path(), target_path))?;
            if profile {
                copied_files += 1;
                copied_bytes = copied_bytes.saturating_add(bytes);
            }
        }
    }
    if profile {
        log::info!(
            "[sync_engine] copy_dir_recursive {} files, {} bytes in {}s (src={:?} dst={:?})",
            copied_files,
            copied_bytes,
            started.elapsed().as_secs_f32(),
            source,
            target
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/sync_engine.rs"]
mod tests;
