use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use git2::{FetchOptions, Repository};
use reqwest::Url;

use super::cancel_token::CancelToken;
use super::network_proxy::validate_proxy_url;
use super::safe_fs::move_internal_to_trash;

/// Validate a Git working-tree destination without following a destination or
/// .git symlink. Callers use this immediately before every operation that
/// lets Git interpret repository-controlled paths.
pub(crate) fn validate_git_worktree_destination(dest: &Path, require_git_dir: bool) -> Result<()> {
    let parent = dest
        .parent()
        .ok_or_else(|| anyhow::anyhow!("UNSAFE_GIT_CACHE|Git destination has no parent"))?;
    let parent_meta = std::fs::symlink_metadata(parent)
        .with_context(|| format!("stat Git destination parent {:?}", parent))?;
    if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
        anyhow::bail!("UNSAFE_GIT_CACHE|Git destination parent must be a real directory");
    }

    match std::fs::symlink_metadata(dest) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                anyhow::bail!("UNSAFE_GIT_CACHE|Git destination must be a real directory");
            }
            let parent_canonical = parent
                .canonicalize()
                .with_context(|| format!("resolve Git destination parent {:?}", parent))?;
            let dest_canonical = dest
                .canonicalize()
                .with_context(|| format!("resolve Git destination {:?}", dest))?;
            if dest_canonical.parent() != Some(parent_canonical.as_path()) {
                anyhow::bail!("UNSAFE_GIT_CACHE|Git destination escapes its parent");
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound && !require_git_dir => return Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("UNSAFE_GIT_CACHE|Git destination is missing")
        }
        Err(err) => return Err(err).with_context(|| format!("stat Git destination {:?}", dest)),
    }

    let git_dir = dest.join(".git");
    match std::fs::symlink_metadata(&git_dir) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            anyhow::bail!("UNSAFE_GIT_CACHE|.git must be a real directory");
        }
        Ok(_) => {
            let dest_canonical = dest
                .canonicalize()
                .with_context(|| format!("resolve Git destination {:?}", dest))?;
            let git_canonical = git_dir
                .canonicalize()
                .with_context(|| format!("resolve Git metadata directory {:?}", git_dir))?;
            if git_canonical.parent() != Some(dest_canonical.as_path()) {
                anyhow::bail!("UNSAFE_GIT_CACHE|.git escapes its working tree");
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound && !require_git_dir => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("UNSAFE_GIT_CACHE|Git destination has no .git directory")
        }
        Err(err) => return Err(err).with_context(|| format!("stat Git metadata {:?}", git_dir)),
    }
    Ok(())
}

/// Validate a Git source before it can reach logs, subprocess arguments, or
/// persistent records. This fork accepts only HTTPS remotes or an
/// explicit absolute local repository path. This excludes remote-helper and
/// SSH schemes that can launch helper commands or consume ambient credentials.
pub fn validate_git_source_url(repo_url: &str) -> Result<()> {
    let value = repo_url.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        anyhow::bail!("INVALID_GIT_URL|Git source is empty or malformed");
    }

    match Url::parse(value) {
        Ok(url) if url.scheme() == "https" => {
            let has_userinfo = !url.username().is_empty()
                || url.password().is_some()
                || http_authority_has_userinfo(value);
            if has_userinfo || url.query().is_some() || url.fragment().is_some() {
                anyhow::bail!(
                    "UNSAFE_GIT_URL|HTTP(S) Git URLs must not contain credentials, query, or fragment"
                );
            }
            if url.host_str().is_none() {
                anyhow::bail!("INVALID_GIT_URL|HTTPS Git URL must include a host");
            }
        }
        Ok(_) => {
            anyhow::bail!("UNSAFE_GIT_URL|Only HTTPS Git URLs are allowed");
        }
        Err(_) if has_http_scheme_prefix(value) => {
            anyhow::bail!("INVALID_GIT_URL|HTTPS Git URL is malformed");
        }
        Err(_) if Path::new(value).is_absolute() => {
            if value.starts_with('-') {
                anyhow::bail!("UNSAFE_GIT_URL|Git source must not begin with an option prefix");
            }
        }
        Err(_) => {
            anyhow::bail!("UNSAFE_GIT_URL|Git source must be an HTTPS URL or absolute local path");
        }
    }

    Ok(())
}

fn has_http_scheme_prefix(value: &str) -> bool {
    value
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
        || value
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
}

fn http_authority_has_userinfo(value: &str) -> bool {
    let Some((_, remainder)) = value.split_once("://") else {
        return false;
    };
    remainder
        .split(['/', '?', '#'])
        .next()
        .is_some_and(|authority| authority.contains('@'))
}

pub fn clone_or_pull(
    repo_url: &str,
    dest: &Path,
    branch: Option<&str>,
    cancel: Option<&CancelToken>,
    proxy_url: Option<&str>,
) -> Result<String> {
    validate_git_source_url(repo_url)?;
    if let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) {
        validate_proxy_url(proxy_url)?;
    }

    // Prefer the system `git` binary if available. It tends to work better on macOS
    // networks because it respects user git config (proxy/certs) and OS trust store.
    if let Some(git_bin) = resolve_git_bin() {
        let started = Instant::now();
        match clone_or_pull_via_git_cli(repo_url, dest, branch, cancel, proxy_url) {
            Ok(head) => {
                log::info!(
                    "[git_fetcher] git-cli ok (bin={}) {}s url={}",
                    git_bin,
                    started.elapsed().as_secs_f32(),
                    repo_url
                );
                return Ok(head);
            }
            Err(err) => {
                if proxy_url.is_some_and(|v| !v.trim().is_empty()) {
                    anyhow::bail!(
                        "git 命令执行失败。已配置 GitHub 代理，为确保访问一定走代理，已停止并不回退到内置 git。\n{:#}",
                        err
                    );
                }
                let allow_fallback = std::env::var("SKILLS_HUB_ALLOW_LIBGIT2_FALLBACK")
                    .ok()
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
                log::warn!(
                    "[git_fetcher] git-cli failed (bin={}) {}s url={} err={:#}",
                    git_bin,
                    started.elapsed().as_secs_f32(),
                    repo_url,
                    err
                );
                if !allow_fallback {
                    anyhow::bail!(
                        "git 命令执行失败（为避免卡死，已停止并不再回退到内置 git）。请检查系统 git/网络/代理；或设置环境变量 SKILLS_HUB_ALLOW_LIBGIT2_FALLBACK=1 允许回退。\n{:#}",
                        err
                    );
                }
                log::warn!(
                    "[git_fetcher] falling back to libgit2 (SKILLS_HUB_ALLOW_LIBGIT2_FALLBACK=1)"
                );
            }
        }
    } else {
        log::info!("[git_fetcher] system git not available; using libgit2");
    }

    let repo = if std::fs::symlink_metadata(dest).is_ok() {
        validate_git_worktree_destination(dest, true)?;
        let repo = Repository::open(dest).with_context(|| format!("open repo at {:?}", dest))?;
        fetch_origin(&repo)?;
        repo
    } else {
        validate_git_worktree_destination(dest, false)?;
        let repo = Repository::clone(repo_url, dest)
            .with_context(|| format!("clone {} into {:?}", repo_url, dest))?;
        validate_git_worktree_destination(dest, true)?;
        repo
    };

    // Best-effort: move working tree HEAD to the fetched remote head (so "pull" actually updates).
    validate_git_worktree_destination(dest, true)?;
    if let Some(branch) = branch {
        if let Ok(obj) = repo.revparse_single(&format!("refs/remotes/origin/{}", branch)) {
            repo.checkout_tree(&obj, None)?;
            repo.set_head_detached(obj.id())?;
        }
    } else {
        let candidates = [
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            "refs/remotes/origin/master",
        ];
        for r in candidates {
            if let Ok(obj) = repo.revparse_single(r) {
                repo.checkout_tree(&obj, None)?;
                repo.set_head_detached(obj.id())?;
                break;
            }
        }
    }

    let head = repo.head()?.target().context("missing HEAD target")?;
    Ok(head.to_string())
}

pub fn clone_or_pull_sparse(
    repo_url: &str,
    dest: &Path,
    branch: Option<&str>,
    subpath: &str,
    cancel: Option<&CancelToken>,
    proxy_url: Option<&str>,
) -> Result<String> {
    validate_git_source_url(repo_url)?;
    if let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) {
        validate_proxy_url(proxy_url)?;
    }

    let clean_subpath = subpath.trim_matches('/');
    if clean_subpath.is_empty() {
        anyhow::bail!("sparse checkout path is empty");
    }

    if resolve_git_bin().is_none() {
        anyhow::bail!("system git is required for sparse checkout");
    }

    // Ensure parent exists so `git clone` can create dest.
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir {:?}", parent))?;
    }

    if std::fs::symlink_metadata(dest).is_ok() {
        validate_git_worktree_destination(dest, true)?;
        let git_dir = dest.join(".git");
        for lock_name in &["index.lock", "shallow.lock", "HEAD.lock"] {
            let lock_path = git_dir.join(lock_name);
            if std::fs::symlink_metadata(&lock_path).is_ok() {
                log::warn!(
                    "[git_fetcher] moving stale lock file to Trash: {:?}",
                    lock_path
                );
                move_internal_to_trash(&git_dir, &lock_path)
                    .with_context(|| format!("move stale git lock to Trash {:?}", lock_path))?;
            }
        }

        let out = run_cmd_with_timeout(
            {
                let mut cmd = git_cmd(proxy_url);
                cmd.arg("-C").arg(dest).args([
                    "sparse-checkout",
                    "set",
                    "--no-cone",
                    "--",
                    clean_subpath,
                ]);
                cmd
            },
            git_fetch_timeout(),
            format!("git sparse-checkout set {} in {:?}", clean_subpath, dest),
            cancel,
        )?;
        if !out.status.success() {
            anyhow::bail!(
                "git sparse-checkout set failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        validate_git_worktree_destination(dest, true)?;
        let out = run_cmd_with_timeout(
            {
                let mut cmd = git_cmd(proxy_url);
                cmd.arg("-C").arg(dest).args(["fetch", "--prune", "origin"]);
                cmd
            },
            git_fetch_timeout(),
            format!("git fetch in {:?}", dest),
            cancel,
        )?;
        if !out.status.success() {
            anyhow::bail!("git fetch failed: {}", String::from_utf8_lossy(&out.stderr));
        }

        if let Some(branch) = branch {
            validate_git_worktree_destination(dest, true)?;
            let out = run_cmd_with_timeout(
                {
                    let mut cmd = git_cmd(proxy_url);
                    cmd.arg("-C").arg(dest).args([
                        "checkout",
                        "-B",
                        branch,
                        &format!("origin/{}", branch),
                    ]);
                    cmd
                },
                git_fetch_timeout(),
                format!("git checkout -B {} in {:?}", branch, dest),
                cancel,
            )?;
            if !out.status.success() {
                anyhow::bail!(
                    "git checkout branch failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        } else {
            validate_git_worktree_destination(dest, true)?;
            let out = run_cmd_with_timeout(
                {
                    let mut cmd = git_cmd(proxy_url);
                    cmd.arg("-C")
                        .arg(dest)
                        .args(["reset", "--hard", "FETCH_HEAD"]);
                    cmd
                },
                git_fetch_timeout(),
                format!("git reset --hard in {:?}", dest),
                cancel,
            )?;
            if !out.status.success() {
                anyhow::bail!(
                    "git reset --hard failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
    } else {
        validate_git_worktree_destination(dest, false)?;
        let mut cmd = git_cmd(proxy_url);
        cmd.arg("clone").args([
            "--depth",
            "1",
            "--filter=blob:none",
            "--sparse",
            "--no-tags",
        ]);
        if let Some(branch) = branch {
            cmd.arg("--branch").arg(branch).arg("--single-branch");
        }
        cmd.arg("--").arg(repo_url).arg(dest);
        let out = run_cmd_with_timeout(
            cmd,
            git_timeout(),
            format!("git clone {} into {:?}", repo_url, dest),
            cancel,
        )?;
        if !out.status.success() {
            anyhow::bail!("git clone failed: {}", String::from_utf8_lossy(&out.stderr));
        }

        validate_git_worktree_destination(dest, true)?;
        let out = run_cmd_with_timeout(
            {
                let mut cmd = git_cmd(proxy_url);
                cmd.arg("-C").arg(dest).args([
                    "sparse-checkout",
                    "set",
                    "--no-cone",
                    "--",
                    clean_subpath,
                ]);
                cmd
            },
            git_fetch_timeout(),
            format!("git sparse-checkout set {} in {:?}", clean_subpath, dest),
            cancel,
        )?;
        if !out.status.success() {
            anyhow::bail!(
                "git sparse-checkout set failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    validate_git_worktree_destination(dest, true)?;
    let out = run_cmd_with_timeout(
        {
            let mut cmd = git_cmd(proxy_url);
            cmd.arg("-C").arg(dest).args(["rev-parse", "HEAD"]);
            cmd
        },
        git_fetch_timeout(),
        format!("git rev-parse HEAD in {:?}", dest),
        cancel,
    )?;
    if !out.status.success() {
        anyhow::bail!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_timeout() -> Duration {
    let secs = std::env::var("SKILLS_HUB_GIT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(300);
    Duration::from_secs(secs)
}

fn git_fetch_timeout() -> Duration {
    let secs = std::env::var("SKILLS_HUB_GIT_FETCH_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(180);
    Duration::from_secs(secs)
}

static GIT_BIN: OnceLock<Option<String>> = OnceLock::new();

fn resolve_git_bin() -> Option<String> {
    GIT_BIN
        .get_or_init(|| {
            // Allow overriding from environment for debugging / enterprise setups.
            for key in ["SKILLS_HUB_GIT_BIN", "SKILLS_HUB_GIT_PATH"] {
                if let Ok(v) = std::env::var(key) {
                    let v = v.trim().to_string();
                    if !v.is_empty() && git_bin_works(&v) {
                        log::info!("[git_fetcher] using git bin from {}: {}", key, v);
                        return Some(v);
                    }
                }
            }

            // Try PATH lookup first (works in dev; sometimes missing in macOS bundles).
            if git_bin_works("git") {
                log::info!("[git_fetcher] using git bin from PATH: git");
                return Some("git".to_string());
            }

            // Common macOS locations (system git and Homebrew).
            for cand in [
                "/usr/bin/git",
                "/opt/homebrew/bin/git",
                "/usr/local/bin/git",
            ] {
                if git_bin_works(cand) {
                    log::info!("[git_fetcher] using git bin: {}", cand);
                    return Some(cand.to_string());
                }
            }

            log::warn!("[git_fetcher] no usable git binary found");
            None
        })
        .clone()
}

fn git_bin_works(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git_cmd(proxy_url: Option<&str>) -> Command {
    let bin = resolve_git_bin().unwrap_or_else(|| "git".to_string());
    let mut cmd = Command::new(bin);
    if let Some(proxy_url) = proxy_url.map(str::trim).filter(|v| !v.is_empty()) {
        cmd.arg("-c")
            .arg(format!("http.proxy={}", proxy_url))
            .arg("-c")
            .arg(format!("https.proxy={}", proxy_url))
            .env("http_proxy", proxy_url)
            .env("https_proxy", proxy_url)
            .env("all_proxy", proxy_url)
            .env("HTTP_PROXY", proxy_url)
            .env("HTTPS_PROXY", proxy_url)
            .env("ALL_PROXY", proxy_url);
    }
    // Ignore ambient Git configuration and credential/transport helpers. A
    // repository may control .gitattributes, so inherited filter commands must
    // not be available while materializing an untrusted checkout.
    if let Ok(count) = std::env::var("GIT_CONFIG_COUNT") {
        if let Ok(count) = count.parse::<usize>() {
            for index in 0..count.min(1024) {
                cmd.env_remove(format!("GIT_CONFIG_KEY_{index}"));
                cmd.env_remove(format!("GIT_CONFIG_VALUE_{index}"));
            }
        }
    }
    for key in [
        "GIT_CONFIG_COUNT",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_SSH",
        "GIT_SSH_COMMAND",
        "GIT_PROXY_COMMAND",
        "SSH_ASKPASS",
    ] {
        cmd.env_remove(key);
    }

    // Never block on interactive auth prompts inside a GUI app.
    cmd.arg("-c")
        .arg("core.hooksPath=/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_ALLOW_PROTOCOL", "https:file")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "echo");
    // Abort stalled HTTPS transfers (helps avoid "spinner forever" on bad networks).
    cmd.env("GIT_HTTP_LOW_SPEED_LIMIT", "1024")
        .env("GIT_HTTP_LOW_SPEED_TIME", "120");
    cmd
}

fn run_cmd_with_timeout(
    mut cmd: Command,
    timeout: Duration,
    context: String,
    cancel: Option<&CancelToken>,
) -> Result<std::process::Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().with_context(|| context.clone())?;
    let start = Instant::now();
    loop {
        if cancel.is_some_and(|c| c.is_cancelled()) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("CANCELLED|操作已被用户取消。");
        }

        if start.elapsed() > timeout {
            let _ = child.kill();
            let stderr = child
                .wait_with_output()
                .map(|out| String::from_utf8_lossy(&out.stderr).to_string())
                .unwrap_or_default();
            anyhow::bail!(
                "git 操作超时（{}s）。请检查网络/代理是否可访问 GitHub；也可设置环境变量 SKILLS_HUB_GIT_TIMEOUT_SECS 增大超时。\n{}",
                timeout.as_secs(),
                stderr.trim()
            );
        }

        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().with_context(|| context.clone()),
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(err) => return Err(err).with_context(|| context.clone()),
        }
    }
}

fn clone_or_pull_via_git_cli(
    repo_url: &str,
    dest: &Path,
    branch: Option<&str>,
    cancel: Option<&CancelToken>,
    proxy_url: Option<&str>,
) -> Result<String> {
    // Ensure parent exists so `git clone` can create dest.
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir {:?}", parent))?;
    }

    if std::fs::symlink_metadata(dest).is_ok() {
        validate_git_worktree_destination(dest, true)?;
        // Remove stale lock files left by a previously crashed git process.
        let git_dir = dest.join(".git");
        for lock_name in &["index.lock", "shallow.lock", "HEAD.lock"] {
            let lock_path = git_dir.join(lock_name);
            if std::fs::symlink_metadata(&lock_path).is_ok() {
                log::warn!(
                    "[git_fetcher] moving stale lock file to Trash: {:?}",
                    lock_path
                );
                move_internal_to_trash(&git_dir, &lock_path)
                    .with_context(|| format!("move stale git lock to Trash {:?}", lock_path))?;
            }
        }

        // Fetch updates.
        validate_git_worktree_destination(dest, true)?;
        let out = run_cmd_with_timeout(
            {
                let mut cmd = git_cmd(proxy_url);
                cmd.arg("-C").arg(dest).args(["fetch", "--prune", "origin"]);
                cmd
            },
            git_fetch_timeout(),
            format!("git fetch in {:?}", dest),
            cancel,
        )?;
        if !out.status.success() {
            anyhow::bail!("git fetch failed: {}", String::from_utf8_lossy(&out.stderr));
        }

        // Move local HEAD to fetched commit.
        if let Some(branch) = branch {
            validate_git_worktree_destination(dest, true)?;
            let out = run_cmd_with_timeout(
                {
                    let mut cmd = git_cmd(proxy_url);
                    cmd.arg("-C").arg(dest).args([
                        "checkout",
                        "-B",
                        branch,
                        &format!("origin/{}", branch),
                    ]);
                    cmd
                },
                git_fetch_timeout(),
                format!("git checkout -B {} in {:?}", branch, dest),
                cancel,
            )?;
            if !out.status.success() {
                anyhow::bail!(
                    "git checkout branch failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        } else {
            validate_git_worktree_destination(dest, true)?;
            let out = run_cmd_with_timeout(
                {
                    let mut cmd = git_cmd(proxy_url);
                    cmd.arg("-C")
                        .arg(dest)
                        .args(["reset", "--hard", "FETCH_HEAD"]);
                    cmd
                },
                git_fetch_timeout(),
                format!("git reset --hard in {:?}", dest),
                cancel,
            )?;
            if !out.status.success() {
                anyhow::bail!(
                    "git reset --hard failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
    } else {
        // Clone.
        validate_git_worktree_destination(dest, false)?;
        let mut cmd = git_cmd(proxy_url);
        cmd.arg("clone")
            .args(["--depth", "1", "--filter=blob:none", "--no-tags"]);
        if let Some(branch) = branch {
            cmd.arg("--branch").arg(branch).arg("--single-branch");
        }
        cmd.arg("--").arg(repo_url).arg(dest);
        let out = run_cmd_with_timeout(
            cmd,
            git_timeout(),
            format!("git clone {} into {:?}", repo_url, dest),
            cancel,
        )?;
        if !out.status.success() {
            anyhow::bail!("git clone failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        validate_git_worktree_destination(dest, true)?;
    }

    // Checkout desired branch if specified (best-effort; shallow clones may already be on it).
    if let Some(branch) = branch {
        validate_git_worktree_destination(dest, true)?;
        let out = run_cmd_with_timeout(
            {
                let mut cmd = git_cmd(proxy_url);
                cmd.arg("-C").arg(dest).args(["checkout", "--", branch]);
                cmd
            },
            git_fetch_timeout(),
            format!("git checkout {} in {:?}", branch, dest),
            cancel,
        )?;
        if !out.status.success() {
            // Don't hard-fail; still return HEAD for caller.
            // But include useful context for debugging.
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.trim().is_empty() {
                eprintln!("[git_fetcher] checkout warning: {}", stderr);
            }
        }
    }

    // Read HEAD revision.
    validate_git_worktree_destination(dest, true)?;
    let out = run_cmd_with_timeout(
        {
            let mut cmd = git_cmd(proxy_url);
            cmd.arg("-C").arg(dest).args(["rev-parse", "HEAD"]);
            cmd
        },
        git_fetch_timeout(),
        format!("git rev-parse HEAD in {:?}", dest),
        cancel,
    )?;
    if !out.status.success() {
        anyhow::bail!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn fetch_origin(repo: &Repository) -> Result<()> {
    let mut remote = repo.find_remote("origin")?;
    let mut opts = FetchOptions::new();
    remote.fetch(
        &["refs/heads/*:refs/remotes/origin/*"],
        Some(&mut opts),
        None,
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "tests/git_fetcher.rs"]
mod tests;
