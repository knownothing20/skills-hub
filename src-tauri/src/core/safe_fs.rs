//! Filesystem safety primitives for managed Skills.
//!
//! The app writes to several user-controlled tool roots.  Every destructive
//! operation must therefore prove that the selected path is one, non-hidden,
//! direct child of the expected root before moving it to the operating
//! system's recoverable Trash / Recycle Bin.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use anyhow::{Context, Result};
use uuid::Uuid;

static CENTRAL_MUTATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// An opaque, operation-specific handle to exactly one item moved to Trash.
///
/// The unique holding path prevents rollback from selecting an older,
/// same-named item already present in the system Trash.
#[derive(Debug)]
pub struct TrashReceipt {
    restore_path: PathBuf,
    backend: TrashReceiptBackend,
}

#[derive(Debug)]
enum TrashReceiptBackend {
    /// Unit tests use a concrete, uniquely named temporary Trash entry.
    #[cfg(test)]
    LocalPath(PathBuf),
    /// macOS returns the exact, potentially volume-local Trash URL. It cannot
    /// be assumed to live below the home-directory Trash.
    #[cfg(target_os = "macos")]
    MacSystemPath(PathBuf),
    /// Windows and Freedesktop Trash restore by the unique pre-trash holding
    /// path recorded as the system item's original path.
    #[cfg(any(
        windows,
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    ))]
    SystemOriginalPath {
        path: PathBuf,
        /// Set only after the system Trash API has successfully restored the
        /// exact receipt to its unique holding path. A failed final publish can
        /// then be retried without trusting a replacement at that path.
        restored_identity: OnceLock<SystemHoldingIdentity>,
    },
}

#[cfg(test)]
impl TrashReceipt {
    fn test_local_path(&self) -> &Path {
        match &self.backend {
            TrashReceiptBackend::LocalPath(path) => path,
            #[cfg(target_os = "macos")]
            TrashReceiptBackend::MacSystemPath(_) => {
                unreachable!("unit tests use an injected temporary Trash backend")
            }
            #[cfg(any(
                windows,
                all(
                    unix,
                    not(target_os = "macos"),
                    not(target_os = "ios"),
                    not(target_os = "android")
                )
            ))]
            TrashReceiptBackend::SystemOriginalPath { .. } => {
                unreachable!("unit tests never access the real system Trash")
            }
        }
    }
}

#[cfg(any(
    windows,
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
#[derive(Debug)]
struct SystemHoldingIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    handle: same_file::Handle,
    #[cfg(windows)]
    attributes: u32,
    #[cfg(windows)]
    creation_time: u64,
    #[cfg(windows)]
    file_size: u64,
}

#[cfg(any(
    windows,
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
impl SystemHoldingIdentity {
    fn capture(path: &Path) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            let metadata = std::fs::symlink_metadata(path)
                .with_context(|| format!("capture restored Trash identity {path:?}"))?;
            Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }

        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;

            let handle = same_file::Handle::from_path(path)
                .with_context(|| format!("open restored Trash identity {path:?}"))?;
            let metadata = std::fs::symlink_metadata(path)
                .with_context(|| format!("capture restored Trash fingerprint {path:?}"))?;
            Ok(Self {
                handle,
                attributes: metadata.file_attributes(),
                creation_time: metadata.creation_time(),
                file_size: metadata.file_size(),
            })
        }
    }

    fn matches(&self, path: &Path) -> Result<bool> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("verify restored Trash identity {path:?}"));
            }
        };

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            Ok(self.device == metadata.dev() && self.inode == metadata.ino())
        }

        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;

            let handle = same_file::Handle::from_path(path)
                .with_context(|| format!("open current restored Trash identity {path:?}"))?;
            Ok(self.handle == handle
                && self.attributes == metadata.file_attributes()
                && self.creation_time == metadata.creation_time()
                && self.file_size == metadata.file_size())
        }
    }
}

/// Process-wide serialization for every mutation of the central Skill root or
/// a managed target. Holding this guard keeps check/stage/publish/DB rollback
/// sequences from interleaving with another command in this process.
pub struct CentralMutationGuard {
    _guard: MutexGuard<'static, ()>,
}

pub fn lock_central_mutation() -> Result<CentralMutationGuard> {
    let guard = CENTRAL_MUTATION_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| {
            anyhow::anyhow!("MUTATION_LOCK_POISONED|Central mutation lock is unavailable")
        })?;
    Ok(CentralMutationGuard { _guard: guard })
}

/// Validate a name before it is ever passed to `Path::join`.
pub fn validate_skill_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("SKILL_INVALID|unsafe_name|Skill name must not be empty");
    }
    if name.trim() != name {
        anyhow::bail!("SKILL_INVALID|unsafe_name|Skill name must not have surrounding whitespace");
    }
    if name.starts_with('.') {
        anyhow::bail!("SKILL_INVALID|unsafe_name|Hidden Skill names are not allowed");
    }
    if name
        .chars()
        .any(|ch| ch == '/' || ch == '\\' || ch.is_control())
    {
        anyhow::bail!(
            "SKILL_INVALID|unsafe_name|Skill name contains a path separator or control character"
        );
    }

    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(segment)), None) if segment == OsStr::new(name) => Ok(()),
        _ => anyhow::bail!("SKILL_INVALID|unsafe_name|Skill name must be a single path segment"),
    }
}

/// Validate a relative source subpath used when selecting a Skill from a repo.
pub fn validate_relative_subpath(subpath: &str) -> Result<()> {
    if subpath == "." {
        return Ok(());
    }
    if subpath.is_empty()
        || subpath.trim() != subpath
        || subpath.contains('\\')
        || subpath.chars().any(char::is_control)
    {
        anyhow::bail!("SKILL_INVALID|unsafe_subpath|Invalid Skill source subpath");
    }
    let path = Path::new(subpath);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        anyhow::bail!(
            "SKILL_INVALID|unsafe_subpath|Skill source subpath must stay inside its repository"
        );
    }
    Ok(())
}

/// Construct a direct Skill child only after validating its name and root.
pub fn direct_skill_child(root: &Path, name: &str) -> Result<PathBuf> {
    validate_skill_name(name)?;
    let root_canonical = root
        .canonicalize()
        .with_context(|| format!("resolve Skill root {:?}", root))?;
    let candidate = root.join(name);
    let parent = candidate
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Skill target has no parent: {:?}", candidate))?;
    let parent_canonical = parent
        .canonicalize()
        .with_context(|| format!("resolve Skill target parent {:?}", parent))?;
    if parent_canonical != root_canonical {
        anyhow::bail!("UNSAFE_PATH|Skill target escapes its configured root");
    }
    Ok(candidate)
}

/// Prove that an existing (or dangling symlink) path is a direct child of root.
/// Only the parent is canonicalized so a symlink target is never followed.
pub fn validate_direct_skill_path(root: &Path, path: &Path) -> Result<()> {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Skill target has no UTF-8 file name"))?;
    validate_skill_name(name)?;

    let root_canonical = root
        .canonicalize()
        .with_context(|| format!("resolve Skill root {:?}", root))?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Skill target has no parent"))?;
    let parent_canonical = parent
        .canonicalize()
        .with_context(|| format!("resolve Skill target parent {:?}", parent))?;
    if parent_canonical != root_canonical {
        anyhow::bail!("UNSAFE_PATH|Skill target is not a direct child of its configured root");
    }
    if path == root || name == ".system" {
        anyhow::bail!("UNSAFE_PATH|Refusing to operate on a protected Skill path");
    }
    Ok(())
}

/// Compare paths by identity. Both paths must exist; symlinks are resolved.
pub fn paths_have_same_identity(left: &Path, right: &Path) -> Result<bool> {
    let left = left
        .canonicalize()
        .with_context(|| format!("resolve source path {:?}", left))?;
    let right = right
        .canonicalize()
        .with_context(|| format!("resolve managed central path {:?}", right))?;
    Ok(left == right)
}

pub fn ensure_distinct_roots(left: &Path, right: &Path) -> Result<()> {
    let left = left
        .canonicalize()
        .with_context(|| format!("resolve root {:?}", left))?;
    let right = right
        .canonicalize()
        .with_context(|| format!("resolve protected root {:?}", right))?;
    if left == right {
        anyhow::bail!("UNSAFE_PATH|The central Skill source cannot also be a sync target");
    }
    Ok(())
}

/// Like `Path::exists`, but sees dangling symlinks and never treats an I/O
/// error as absence.
pub fn path_entry_exists(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("stat path entry {:?}", path)),
    }
}

/// Atomically publish a fully validated hidden sibling staging directory. The
/// platform primitive must fail if `live` already exists; a plain Unix rename
/// is deliberately insufficient because it can replace the destination.
pub fn publish_staged_skill_no_replace(root: &Path, staged: &Path, live: &Path) -> Result<()> {
    let staged_meta = std::fs::symlink_metadata(staged)
        .with_context(|| format!("stat staged Skill {:?}", staged))?;
    if staged_meta.file_type().is_symlink() || !staged_meta.is_dir() {
        anyhow::bail!("INSTALL_PUBLISH_FAILED|Staged Skill must be a real directory");
    }
    publish_staged_entry_no_replace(root, staged, live)
}

/// Atomically publish an app-owned hidden sibling that is either a real
/// directory or a directory symlink/junction. Ordinary files and dangling or
/// file-targeting links are rejected. The destination is never replaced.
pub fn publish_staged_entry_no_replace(root: &Path, staged: &Path, live: &Path) -> Result<()> {
    validate_internal_direct_child(root, staged)?;
    validate_direct_skill_path(root, live)?;
    let staged_name = staged
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Staged entry has no UTF-8 file name"))?;
    if !staged_name.starts_with(".skills-hub-") {
        anyhow::bail!("UNSAFE_PATH|Staged entry must be a hidden app-owned sibling");
    }
    let staged_meta = std::fs::symlink_metadata(staged)
        .with_context(|| format!("stat staged entry {:?}", staged))?;
    if staged_meta.file_type().is_symlink() {
        let target_meta = std::fs::metadata(staged)
            .with_context(|| format!("resolve staged directory link {:?}", staged))?;
        if !target_meta.is_dir() {
            anyhow::bail!("INSTALL_PUBLISH_FAILED|Staged link must target a directory");
        }
    } else if !staged_meta.is_dir() {
        anyhow::bail!("INSTALL_PUBLISH_FAILED|Staged entry must be a directory or directory link");
    }

    rename_no_replace(staged, live).map_err(|err| {
        if matches!(
            err.kind(),
            std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::PermissionDenied
        ) && std::fs::symlink_metadata(live).is_ok()
        {
            anyhow::anyhow!("SKILL_EXISTS|A Skill with this name already exists")
        } else {
            anyhow::Error::new(err).context("INSTALL_PUBLISH_FAILED|Could not publish staged entry")
        }
    })
}

#[cfg(target_os = "macos")]
fn rename_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int};
    use std::os::unix::ffi::OsStrExt;

    const RENAME_EXCL: u32 = 0x0000_0004;
    extern "C" {
        fn renamex_np(from: *const c_char, to: *const c_char, flags: u32) -> c_int;
    }

    let source = CString::new(source.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid source path")
    })?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid destination path")
    })?;
    // SAFETY: both pointers are valid NUL-terminated path strings for the
    // duration of the call and flags uses the documented macOS RENAME_EXCL.
    let result = unsafe { renamex_np(source.as_ptr(), destination.as_ptr(), RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn rename_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int};
    use std::os::unix::ffi::OsStrExt;

    const AT_FDCWD: c_int = -100;
    const RENAME_NOREPLACE: u32 = 1;
    extern "C" {
        fn renameat2(
            olddirfd: c_int,
            oldpath: *const c_char,
            newdirfd: c_int,
            newpath: *const c_char,
            flags: u32,
        ) -> c_int;
    }

    let source = CString::new(source.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid source path")
    })?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid destination path")
    })?;
    // SAFETY: both pointers are valid NUL-terminated path strings for the
    // duration of the call and RENAME_NOREPLACE is supported by renameat2.
    let result = unsafe {
        renameat2(
            AT_FDCWD,
            source.as_ptr(),
            AT_FDCWD,
            destination.as_ptr(),
            RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn rename_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(destination).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "destination exists",
        ));
    }
    // Windows rename fails rather than replacing an existing directory. The
    // explicit check also gives a stable AlreadyExists classification.
    std::fs::rename(source, destination)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn rename_no_replace(_source: &Path, _destination: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic no-replace rename is unsupported on this platform",
    ))
}

fn validate_internal_direct_child(root: &Path, path: &Path) -> Result<()> {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Internal path has no UTF-8 file name"))?;
    if name.is_empty()
        || name == "."
        || name == ".."
        || name == ".system"
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control)
    {
        anyhow::bail!("UNSAFE_PATH|Invalid protected/internal path");
    }
    let root_canonical = root
        .canonicalize()
        .with_context(|| format!("resolve root {:?}", root))?;
    let parent_canonical = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("UNSAFE_PATH|Internal path has no parent"))?
        .canonicalize()
        .with_context(|| format!("resolve parent for {:?}", path))?;
    if parent_canonical != root_canonical || path == root {
        anyhow::bail!("UNSAFE_PATH|Internal path escapes its root");
    }
    Ok(())
}

// Unit tests must never touch the user's real Trash. The managed root is always
// a tempfile in tests, so the hidden test Trash is removed by tempfile itself.
#[cfg(test)]
fn default_local_trash_root(managed_root: &Path) -> Result<PathBuf> {
    Ok(managed_root.join(".skills-hub-test-trash"))
}

#[cfg(test)]
fn unique_local_trash_destination(trash_root: &Path, source: &Path) -> Result<PathBuf> {
    match std::fs::symlink_metadata(trash_root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            anyhow::bail!("TRASH_UNAVAILABLE|Trash root must be a real directory");
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(trash_root)
                .with_context(|| format!("create Trash directory {:?}", trash_root))?;
            let metadata = std::fs::symlink_metadata(trash_root)
                .with_context(|| format!("verify Trash directory {:?}", trash_root))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                anyhow::bail!("TRASH_UNAVAILABLE|Trash root must be a real directory");
            }
        }
        Err(err) => return Err(err).with_context(|| format!("stat Trash root {:?}", trash_root)),
    }
    let original = source
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("skill");
    let short: String = original.chars().take(80).collect();
    for _ in 0..8 {
        let candidate = trash_root.join(format!("{}.SkillsHub-{}", short, Uuid::new_v4().simple()));
        if std::fs::symlink_metadata(&candidate).is_err() {
            return Ok(candidate);
        }
    }
    anyhow::bail!("TRASH_COLLISION|Could not allocate a unique Trash destination")
}

#[cfg(test)]
fn move_checked_to_local_trash_at(
    root: &Path,
    path: &Path,
    trash_root: &Path,
    internal: bool,
) -> Result<Option<TrashReceipt>> {
    if internal {
        validate_internal_direct_child(root, path)?;
    } else {
        validate_direct_skill_path(root, path)?;
    }

    match std::fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("stat Skill path {:?}", path)),
    }

    let destination = unique_local_trash_destination(trash_root, path)?;
    // Rename is intentionally the only operation. EXDEV or any other error is
    // returned; there is no copy+delete fallback.
    std::fs::rename(path, &destination).with_context(|| {
        format!(
            "TRASH_MOVE_FAILED|move {:?} to {:?}; original was left in place",
            path, destination
        )
    })?;
    Ok(Some(TrashReceipt {
        restore_path: path.to_path_buf(),
        backend: TrashReceiptBackend::LocalPath(destination),
    }))
}

#[cfg(target_os = "macos")]
fn move_checked_to_macos_trash_with<F>(
    root: &Path,
    path: &Path,
    internal: bool,
    trash_item: F,
) -> Result<Option<TrashReceipt>>
where
    F: FnOnce(&Path, &std::fs::Metadata) -> Result<PathBuf>,
{
    if internal {
        validate_internal_direct_child(root, path)?;
    } else {
        validate_direct_skill_path(root, path)?;
    }
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("stat Trash source {:?}", path)),
    };

    let trashed_path = match trash_item(path, &metadata) {
        Ok(trashed_path) => trashed_path,
        Err(err) if path_entry_exists(path)? => {
            return Err(err).context("TRASH_MOVE_FAILED|macOS left the original item in place")
        }
        Err(err) => {
            return Err(err).context(
                "TRASH_STATE_UNCERTAIN|macOS moved the item without returning a Trash receipt",
            )
        }
    };
    if trashed_path == path {
        anyhow::bail!("TRASH_STATE_UNCERTAIN|macOS returned the original path as its receipt");
    }
    if !path_entry_exists(&trashed_path)? {
        anyhow::bail!("TRASH_STATE_UNCERTAIN|macOS returned a missing Trash receipt path");
    }
    Ok(Some(TrashReceipt {
        restore_path: path.to_path_buf(),
        backend: TrashReceiptBackend::MacSystemPath(trashed_path),
    }))
}

#[cfg(all(not(test), target_os = "macos"))]
fn move_checked_to_macos_trash(
    root: &Path,
    path: &Path,
    internal: bool,
) -> Result<Option<TrashReceipt>> {
    move_checked_to_macos_trash_with(root, path, internal, macos_trash_item)
}

#[cfg(all(not(test), target_os = "macos"))]
fn macos_trash_item(path: &Path, metadata: &std::fs::Metadata) -> Result<PathBuf> {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::ptr::NonNull;

    use objc2_foundation::{NSFileManager, NSURL};

    let path_bytes = path.as_os_str().as_bytes();
    let path_c = CString::new(path_bytes)
        .map_err(|_| anyhow::anyhow!("TRASH_MOVE_FAILED|macOS path contains a NUL byte"))?;
    let path_ptr = NonNull::new(path_c.as_ptr() as *mut c_char)
        .ok_or_else(|| anyhow::anyhow!("TRASH_MOVE_FAILED|macOS path pointer is null"))?;
    // SAFETY: path_ptr references the live NUL-terminated CString for the
    // duration of this call.
    let source_url = unsafe {
        NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
            path_ptr,
            metadata.is_dir(),
            None,
        )
    };
    let manager = NSFileManager::defaultManager();
    let mut resulting_url = None;
    let operation =
        manager.trashItemAtURL_resultingItemURL_error(&source_url, Some(&mut resulting_url));
    if let Err(error) = operation {
        if path_entry_exists(path)? {
            return Err(anyhow::anyhow!(
                "TRASH_MOVE_FAILED|macOS NSFileManager rejected the item: {error}"
            ));
        }
        if resulting_url.is_none() {
            return Err(anyhow::anyhow!(
                "TRASH_STATE_UNCERTAIN|macOS NSFileManager moved the item but returned no URL: {error}"
            ));
        }
        log::warn!(
            "[safe_fs] macOS Trash reported an error but returned a recoverable item URL: {}",
            error
        );
    }

    let resulting_url = resulting_url
        .context("TRASH_STATE_UNCERTAIN|macOS returned no resulting Trash item URL")?;
    if !resulting_url.isFileURL() {
        anyhow::bail!("TRASH_STATE_UNCERTAIN|macOS returned a non-file Trash URL");
    }
    let result_ptr = resulting_url.fileSystemRepresentation();
    // SAFETY: NSURL guarantees fileSystemRepresentation is a valid
    // NUL-terminated pointer for the lifetime of resulting_url.
    let result_bytes = unsafe { CStr::from_ptr(result_ptr.as_ptr()) }
        .to_bytes()
        .to_vec();
    Ok(PathBuf::from(std::ffi::OsString::from_vec(result_bytes)))
}

#[cfg(all(
    not(test),
    any(
        windows,
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    )
))]
fn unique_system_trash_holding_path(root: &Path) -> Result<PathBuf> {
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("resolve managed root {:?}", root))?;
    for _ in 0..8 {
        let candidate =
            canonical_root.join(format!(".skills-hub-trash-{}", Uuid::new_v4().simple()));
        validate_internal_direct_child(&canonical_root, &candidate)?;
        if !path_entry_exists(&candidate)? {
            return Ok(candidate);
        }
    }
    anyhow::bail!("TRASH_COLLISION|Could not allocate a unique Trash receipt path")
}

#[cfg(all(
    not(test),
    any(
        windows,
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    )
))]
fn move_checked_to_system_trash(
    root: &Path,
    path: &Path,
    internal: bool,
) -> Result<Option<TrashReceipt>> {
    if internal {
        validate_internal_direct_child(root, path)?;
    } else {
        validate_direct_skill_path(root, path)?;
    }
    match std::fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("stat Trash source {:?}", path)),
    }

    // Every operation gets a unique original path before the system Trash API
    // runs. Windows and Freedesktop record this exact path for precise restore.
    let holding = unique_system_trash_holding_path(root)?;
    rename_no_replace(path, &holding)
        .with_context(|| format!("TRASH_MOVE_FAILED|stage {:?} as {:?}", path, holding))?;
    if let Err(trash_err) = trash::delete(&holding) {
        if path_entry_exists(&holding)? {
            rename_no_replace(&holding, path).with_context(|| {
                format!(
                    "TRASH_ROLLBACK_FAILED|system Trash failed ({trash_err}); restore {:?}",
                    path
                )
            })?;
            return Err(anyhow::Error::new(trash_err))
                .context("TRASH_MOVE_FAILED|System Trash rejected the item; original restored");
        }
        return Err(anyhow::Error::new(trash_err)).context(format!(
            "TRASH_STATE_UNCERTAIN|System Trash reported failure after {:?} moved",
            holding
        ));
    }
    Ok(Some(TrashReceipt {
        restore_path: path.to_path_buf(),
        backend: TrashReceiptBackend::SystemOriginalPath {
            path: holding,
            restored_identity: OnceLock::new(),
        },
    }))
}

#[cfg(test)]
fn move_checked_to_trash(root: &Path, path: &Path, internal: bool) -> Result<Option<TrashReceipt>> {
    let trash_root = default_local_trash_root(root)?;
    move_checked_to_local_trash_at(root, path, &trash_root, internal)
}

#[cfg(all(not(test), target_os = "macos"))]
fn move_checked_to_trash(root: &Path, path: &Path, internal: bool) -> Result<Option<TrashReceipt>> {
    move_checked_to_macos_trash(root, path, internal)
}

#[cfg(all(
    not(test),
    any(
        windows,
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    )
))]
fn move_checked_to_trash(root: &Path, path: &Path, internal: bool) -> Result<Option<TrashReceipt>> {
    move_checked_to_system_trash(root, path, internal)
}

#[cfg(all(
    not(test),
    not(any(
        target_os = "macos",
        windows,
        all(unix, not(target_os = "ios"), not(target_os = "android"))
    ))
))]
fn move_checked_to_trash(
    _root: &Path,
    _path: &Path,
    _internal: bool,
) -> Result<Option<TrashReceipt>> {
    anyhow::bail!("TRASH_UNAVAILABLE|System Trash is unsupported on this platform")
}

pub fn move_skill_to_trash(root: &Path, path: &Path) -> Result<Option<TrashReceipt>> {
    move_checked_to_trash(root, path, false)
}

/// Move a non-Skill app-owned artifact (cache entry, lock, or scheduler file)
/// to Trash with the same direct-child containment and fail-closed semantics.
pub(crate) fn move_internal_to_trash(root: &Path, path: &Path) -> Result<Option<TrashReceipt>> {
    move_checked_to_trash(root, path, true)
}

/// Replace a live Skill with an already-built sibling staging directory.
/// On every failure after the first rename, the original live Skill is restored.
pub fn replace_skill_with_staged(root: &Path, live: &Path, staged: &Path) -> Result<TrashReceipt> {
    replace_skill_with_staged_impl(root, live, staged, None)
}

/// Restore the old version returned by replace_skill_with_staged if a later
/// commit step (for example the database update) fails.
pub fn rollback_replaced_skill(
    root: &Path,
    live: &Path,
    trashed_backup: &TrashReceipt,
) -> Result<()> {
    restore_skill_from_trash(root, live, trashed_backup)
}

/// Restore a previously trashed Skill to its configured direct-child path.
/// If a partial/new target exists, it is moved to Trash first and restored if
/// the old-version rename itself fails.
pub(crate) fn restore_skill_from_trash(
    root: &Path,
    live: &Path,
    trashed_backup: &TrashReceipt,
) -> Result<()> {
    validate_direct_skill_path(root, live)?;
    if trashed_backup.restore_path != live {
        anyhow::bail!("UPDATE_ROLLBACK_FAILED|Trash receipt does not match restore target");
    }

    let displaced = if path_entry_exists(live)? {
        move_skill_to_trash(root, live)?
    } else {
        None
    };
    if let Err(restore_err) = restore_receipt_to_empty_path(root, live, trashed_backup) {
        if let Some(displaced) = displaced {
            if let Err(displaced_err) = restore_receipt_to_empty_path(root, live, &displaced) {
                return Err(displaced_err).context(format!(
                    "UPDATE_ROLLBACK_FAILED|old restore failed ({restore_err:#}); current item also could not be restored"
                ));
            }
        }
        return Err(restore_err)
            .context("UPDATE_ROLLBACK_FAILED|Could not restore exact Trash item");
    }
    Ok(())
}

/// Atomically restore one exact Trash receipt only if `live` is still empty.
/// Unlike `restore_skill_from_trash`, this never displaces a current entry:
/// the final filesystem operation is a platform no-replace rename, so an
/// external creator racing after validation is preserved intact.
pub(crate) fn restore_skill_from_trash_no_displace(
    root: &Path,
    live: &Path,
    trashed_backup: &TrashReceipt,
) -> Result<()> {
    validate_direct_skill_path(root, live)?;
    if trashed_backup.restore_path != live {
        anyhow::bail!("UPDATE_ROLLBACK_FAILED|Trash receipt does not match restore target");
    }
    restore_receipt_to_empty_path(root, live, trashed_backup).context(
        "UPDATE_ROLLBACK_FAILED|No-displace restore failed; the current entry was preserved and the previous item remains recoverable",
    )
}

#[cfg(test)]
thread_local! {
    static TEST_RESTORE_COLLISION_AFTER_PRECHECK: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_test_restore_collision_after_precheck(destination: Option<&Path>) {
    TEST_RESTORE_COLLISION_AFTER_PRECHECK.with(|slot| {
        *slot.borrow_mut() = destination.map(Path::to_path_buf);
    });
}

#[cfg(test)]
fn maybe_create_restore_collision_after_precheck_for_test(destination: &Path) -> Result<()> {
    TEST_RESTORE_COLLISION_AFTER_PRECHECK.with(|slot| {
        let should_create = slot.borrow().as_deref() == Some(destination);
        if !should_create {
            return Ok(());
        }
        slot.borrow_mut().take();
        std::fs::create_dir(destination).with_context(|| {
            format!("TEST_RESTORE_RACE_FAILED|create competitor {destination:?}")
        })?;
        std::fs::write(destination.join("COMPETITOR_SENTINEL"), "external owner")
            .context("TEST_RESTORE_RACE_FAILED|write competitor sentinel")?;
        Ok(())
    })
}

#[cfg(not(test))]
fn maybe_create_restore_collision_after_precheck_for_test(_destination: &Path) -> Result<()> {
    Ok(())
}

fn publish_restored_item_no_replace(
    source: &Path,
    destination: &Path,
    error_context: &'static str,
) -> Result<()> {
    rename_no_replace(source, destination).map_err(|err| {
        if matches!(
            err.kind(),
            std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::PermissionDenied
        ) && std::fs::symlink_metadata(destination).is_ok()
        {
            anyhow::anyhow!(
                "RESTORE_COLLISION|Restore destination became occupied; the current entry was preserved and the previous item remains recoverable at {source:?}"
            )
        } else {
            anyhow::Error::new(err).context(error_context)
        }
    })
}

fn restore_receipt_to_empty_path(
    root: &Path,
    destination: &Path,
    receipt: &TrashReceipt,
) -> Result<()> {
    #[cfg(all(not(test), target_os = "macos"))]
    let _ = root;
    if path_entry_exists(destination)? {
        anyhow::bail!("RESTORE_COLLISION|Restore destination already exists");
    }
    if receipt.restore_path != destination {
        anyhow::bail!("UPDATE_ROLLBACK_FAILED|Trash receipt target mismatch");
    }
    maybe_create_restore_collision_after_precheck_for_test(destination)?;
    match &receipt.backend {
        #[cfg(test)]
        TrashReceiptBackend::LocalPath(trashed_path) => {
            let trash_root = default_local_trash_root(root)?;
            validate_internal_direct_child(&trash_root, trashed_path)?;
            if !path_entry_exists(trashed_path)? {
                anyhow::bail!("UPDATE_ROLLBACK_FAILED|Trashed item is missing");
            }
            publish_restored_item_no_replace(
                trashed_path,
                destination,
                "UPDATE_ROLLBACK_FAILED|Could not restore local Trash item",
            )?;
            Ok(())
        }
        #[cfg(target_os = "macos")]
        TrashReceiptBackend::MacSystemPath(trashed_path) => {
            if !path_entry_exists(trashed_path)? {
                anyhow::bail!("UPDATE_ROLLBACK_FAILED|macOS Trash receipt is missing");
            }
            publish_restored_item_no_replace(
                trashed_path,
                destination,
                "UPDATE_ROLLBACK_FAILED|Could not restore macOS Trash item",
            )?;
            Ok(())
        }
        #[cfg(any(
            windows,
            all(
                unix,
                not(target_os = "macos"),
                not(target_os = "ios"),
                not(target_os = "android")
            )
        ))]
        TrashReceiptBackend::SystemOriginalPath {
            path: system_original,
            restored_identity,
        } => {
            validate_internal_direct_child(root, system_original)?;
            if path_entry_exists(system_original)? {
                let Some(identity) = restored_identity.get() else {
                    anyhow::bail!(
                        "RESTORE_COLLISION|Trash holding path is occupied by an unverified entry: {system_original:?}"
                    );
                };
                if !identity.matches(system_original)? {
                    anyhow::bail!(
                        "RESTORE_COLLISION|Preserved a replacement at the Trash holding path; the expected restored item was not moved"
                    );
                }
                publish_restored_item_no_replace(
                    system_original,
                    destination,
                    "UPDATE_ROLLBACK_FAILED|Could not publish previously restored Trash item",
                )?;
                return Ok(());
            }
            let mut matches = trash::os_limited::list()
                .context("UPDATE_ROLLBACK_FAILED|Could not list system Trash")?
                .into_iter()
                .filter(|item| system_trash_paths_match(&item.original_path(), system_original))
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                anyhow::bail!(
                    "UPDATE_ROLLBACK_FAILED|Expected one exact Trash item, found {}",
                    matches.len()
                );
            }
            if let Err(err) = trash::os_limited::restore_all(matches.drain(..)) {
                if path_entry_exists(system_original)? {
                    return Err(anyhow::Error::new(err)).context(format!(
                        "TRASH_STATE_UNCERTAIN|System Trash reported failure; an item remains recoverable at {system_original:?} and was not moved"
                    ));
                }
                return Err(anyhow::Error::new(err)).context(
                    "UPDATE_ROLLBACK_FAILED|System Trash restore failed; the exact receipt remains in system Trash",
                );
            }
            if !path_entry_exists(system_original)? {
                anyhow::bail!("UPDATE_ROLLBACK_FAILED|System Trash restore produced no item");
            }
            let identity = SystemHoldingIdentity::capture(system_original).with_context(|| {
                format!(
                    "TRASH_STATE_UNCERTAIN|Restored item remains recoverable at {system_original:?} but its identity could not be journaled"
                )
            })?;
            restored_identity.set(identity).map_err(|_| {
                anyhow::anyhow!(
                    "TRASH_STATE_UNCERTAIN|Restored item identity was already journaled unexpectedly; item remains at {system_original:?}"
                )
            })?;
            publish_restored_item_no_replace(
                system_original,
                destination,
                "UPDATE_ROLLBACK_FAILED|Could not publish restored Trash item",
            )?;
            Ok(())
        }
    }
}

#[cfg(windows)]
fn system_trash_paths_match(left: &Path, right: &Path) -> bool {
    fn normalized(path: &Path) -> String {
        let value = path.to_string_lossy().replace('/', "\\");
        value.strip_prefix(r"\\?\").unwrap_or(&value).to_lowercase()
    }
    normalized(left) == normalized(right)
}

#[cfg(all(
    unix,
    not(target_os = "macos"),
    not(target_os = "ios"),
    not(target_os = "android")
))]
fn system_trash_paths_match(left: &Path, right: &Path) -> bool {
    left == right
}

fn replace_skill_with_staged_impl(
    root: &Path,
    live: &Path,
    staged: &Path,
    local_trash_override: Option<&Path>,
) -> Result<TrashReceipt> {
    validate_direct_skill_path(root, live)?;
    validate_internal_direct_child(root, staged)?;
    if std::fs::symlink_metadata(live).is_err() {
        anyhow::bail!("UPDATE_SWAP_FAILED|Live Skill is missing: {:?}", live);
    }
    if std::fs::symlink_metadata(staged).is_err() {
        anyhow::bail!("UPDATE_SWAP_FAILED|Staged Skill is missing: {:?}", staged);
    }

    let backup = root.join(format!(".skills-hub-backup-{}", Uuid::new_v4().simple()));
    validate_internal_direct_child(root, &backup)?;
    std::fs::rename(live, &backup)
        .with_context(|| format!("UPDATE_SWAP_FAILED|backup {:?} -> {:?}", live, backup))?;

    if let Err(swap_err) = std::fs::rename(staged, live) {
        std::fs::rename(&backup, live).with_context(|| {
            format!(
                "UPDATE_ROLLBACK_FAILED|swap failed ({swap_err}); restore {:?} -> {:?}",
                backup, live
            )
        })?;
        return Err(swap_err).context("UPDATE_SWAP_FAILED|Original Skill restored");
    }

    let trash_result = if let Some(trash_root) = local_trash_override {
        #[cfg(test)]
        {
            move_checked_to_local_trash_at(root, &backup, trash_root, true)
        }
        #[cfg(not(test))]
        {
            let _ = trash_root;
            anyhow::bail!("TRASH_UNAVAILABLE|Local Trash override is test-only")
        }
    } else {
        move_internal_to_trash(root, &backup)
    };
    match trash_result {
        Ok(Some(mut receipt)) => {
            receipt.restore_path = live.to_path_buf();
            Ok(receipt)
        }
        Ok(None) => anyhow::bail!("UPDATE_SWAP_FAILED|Backup disappeared before Trash move"),
        Err(trash_err) => {
            let failed_new = root.join(format!(
                ".skills-hub-failed-update-{}",
                Uuid::new_v4().simple()
            ));
            validate_internal_direct_child(root, &failed_new)?;
            std::fs::rename(live, &failed_new).with_context(|| {
                format!(
                    "UPDATE_ROLLBACK_FAILED|Trash failed ({trash_err:#}); could not preserve new content"
                )
            })?;
            if let Err(restore_err) = std::fs::rename(&backup, live) {
                // Best effort to put the new version back at the live path. This
                // never deletes either version and preserves evidence for repair.
                let _ = std::fs::rename(&failed_new, live);
                return Err(restore_err).context(format!(
                    "UPDATE_ROLLBACK_FAILED|Trash failed ({trash_err:#}); old backup remains at {:?}",
                    backup
                ));
            }
            Err(trash_err).context(format!(
                "UPDATE_SWAP_FAILED|Original Skill restored; new content preserved at {:?}",
                failed_new
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malicious_and_ambiguous_names() {
        for name in [
            "",
            ".",
            "..",
            ".system",
            ".hidden",
            "a/b",
            "a\\b",
            " leading",
            "trailing ",
            "line\nbreak",
        ] {
            assert!(validate_skill_name(name).is_err(), "accepted {name:?}");
        }
        for name in ["normal-skill", "技能管理", "Skill 2026", "a.b"] {
            validate_skill_name(name).unwrap();
        }
    }

    #[test]
    fn direct_child_rejects_escape_and_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        assert!(direct_skill_child(root.path(), "../escape").is_err());
        assert!(validate_direct_skill_path(root.path(), root.path()).is_err());
        assert!(
            validate_direct_skill_path(root.path(), &outside.path().join("safe-name")).is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn trashing_symlink_moves_link_only() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let trash = tempfile::tempdir().unwrap();
        let real = tempfile::tempdir().unwrap();
        std::fs::write(real.path().join("keep"), b"keep").unwrap();
        let link = root.path().join("linked-skill");
        symlink(real.path(), &link).unwrap();

        let moved = move_checked_to_local_trash_at(root.path(), &link, trash.path(), false)
            .unwrap()
            .unwrap();
        assert!(std::fs::symlink_metadata(&link).is_err());
        assert!(std::fs::symlink_metadata(moved.test_local_path())
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(real.path().join("keep").exists());
    }

    #[test]
    fn trash_move_is_recoverable_and_unique() {
        let root = tempfile::tempdir().unwrap();
        let trash = tempfile::tempdir().unwrap();
        let skill = root.path().join("same-name");
        std::fs::create_dir(&skill).unwrap();
        let first = move_checked_to_local_trash_at(root.path(), &skill, trash.path(), false)
            .unwrap()
            .unwrap();
        std::fs::create_dir(&skill).unwrap();
        let second = move_checked_to_local_trash_at(root.path(), &skill, trash.path(), false)
            .unwrap()
            .unwrap();
        assert_ne!(first.test_local_path(), second.test_local_path());
        assert!(first.test_local_path().exists());
        assert!(second.test_local_path().exists());
        assert!(first
            .test_local_path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains(".SkillsHub-"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_backend_accepts_volume_local_receipt_and_restores_exact_path() {
        let root = tempfile::tempdir().unwrap();
        let simulated_volume_trash = tempfile::tempdir().unwrap();
        let skill = root.path().join("external-volume-skill");
        std::fs::create_dir(&skill).unwrap();
        std::fs::write(skill.join("version"), b"before").unwrap();
        let simulated_receipt = simulated_volume_trash.path().join("system-generated-name");

        let receipt =
            move_checked_to_macos_trash_with(root.path(), &skill, false, |source, _metadata| {
                std::fs::rename(source, &simulated_receipt)?;
                Ok(simulated_receipt.clone())
            })
            .unwrap()
            .unwrap();
        assert!(!skill.exists());
        match &receipt.backend {
            TrashReceiptBackend::MacSystemPath(path) => {
                assert_eq!(path, &simulated_receipt);
            }
            _ => panic!("expected macOS system receipt"),
        }

        restore_skill_from_trash(root.path(), &skill, &receipt).unwrap();
        assert_eq!(std::fs::read(skill.join("version")).unwrap(), b"before");
        assert!(!simulated_receipt.exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_backend_failure_leaves_original_untouched() {
        let root = tempfile::tempdir().unwrap();
        let skill = root.path().join("keep-on-failure");
        std::fs::create_dir(&skill).unwrap();
        std::fs::write(skill.join("version"), b"keep").unwrap();

        let err =
            move_checked_to_macos_trash_with(root.path(), &skill, false, |_source, _metadata| {
                anyhow::bail!("simulated system Trash denial")
            })
            .unwrap_err();
        assert!(format!("{err:#}").contains("original item in place"));
        assert_eq!(std::fs::read(skill.join("version")).unwrap(), b"keep");
    }

    #[test]
    fn receipt_restores_exact_item_not_older_same_named_trash_entry() {
        let root = tempfile::tempdir().unwrap();
        let skill = root.path().join("same-name");
        std::fs::create_dir(&skill).unwrap();
        std::fs::write(skill.join("version"), b"oldest").unwrap();
        let older = move_skill_to_trash(root.path(), &skill).unwrap().unwrap();

        std::fs::create_dir(&skill).unwrap();
        std::fs::write(skill.join("version"), b"newer").unwrap();
        let newer = move_skill_to_trash(root.path(), &skill).unwrap().unwrap();

        restore_skill_from_trash(root.path(), &skill, &newer).unwrap();
        assert_eq!(std::fs::read(skill.join("version")).unwrap(), b"newer");
        assert!(older.test_local_path().exists());
        assert!(!newer.test_local_path().exists());
    }

    #[test]
    fn direct_restore_collision_leaves_both_items_recoverable() {
        let root = tempfile::tempdir().unwrap();
        let skill = root.path().join("collision");
        std::fs::create_dir(&skill).unwrap();
        std::fs::write(skill.join("version"), b"trashed").unwrap();
        let receipt = move_skill_to_trash(root.path(), &skill).unwrap().unwrap();

        std::fs::create_dir(&skill).unwrap();
        std::fs::write(skill.join("version"), b"current").unwrap();
        let err = restore_receipt_to_empty_path(root.path(), &skill, &receipt).unwrap_err();
        assert!(format!("{err:#}").contains("RESTORE_COLLISION"));
        assert_eq!(std::fs::read(skill.join("version")).unwrap(), b"current");
        assert!(receipt.test_local_path().exists());
    }

    #[test]
    fn no_displace_restore_preserves_competitor_created_after_precheck() {
        let root = tempfile::tempdir().unwrap();
        let skill = root.path().join("restore-race");
        std::fs::create_dir(&skill).unwrap();
        std::fs::write(skill.join("version"), b"previous").unwrap();
        let receipt = move_skill_to_trash(root.path(), &skill).unwrap().unwrap();

        set_test_restore_collision_after_precheck(Some(&skill));
        let err = restore_skill_from_trash_no_displace(root.path(), &skill, &receipt).unwrap_err();
        assert!(format!("{err:#}").contains("RESTORE_COLLISION"));
        assert_eq!(
            std::fs::read_to_string(skill.join("COMPETITOR_SENTINEL")).unwrap(),
            "external owner"
        );
        assert!(!skill.join("version").exists());
        assert_eq!(
            std::fs::read(receipt.test_local_path().join("version")).unwrap(),
            b"previous"
        );
    }

    #[test]
    fn no_replace_publish_preserves_existing_live_skill() {
        let root = tempfile::tempdir().unwrap();
        let live = root.path().join("same-name");
        let staged = root.path().join(".skills-hub-install-test");
        std::fs::create_dir(&live).unwrap();
        std::fs::write(live.join("version"), b"existing").unwrap();
        std::fs::create_dir(&staged).unwrap();
        std::fs::write(staged.join("version"), b"staged").unwrap();

        let err = publish_staged_skill_no_replace(root.path(), &staged, &live).unwrap_err();
        assert!(format!("{err:#}").contains("SKILL_EXISTS"));
        assert_eq!(std::fs::read(live.join("version")).unwrap(), b"existing");
        assert_eq!(std::fs::read(staged.join("version")).unwrap(), b"staged");
    }

    #[test]
    fn publish_rejects_non_hidden_staging_directory() {
        let root = tempfile::tempdir().unwrap();
        let staged = root.path().join("visible-staging");
        let live = root.path().join("live-name");
        std::fs::create_dir(&staged).unwrap();

        let err = publish_staged_skill_no_replace(root.path(), &staged, &live).unwrap_err();
        assert!(format!("{err:#}").contains("hidden app-owned sibling"));
        assert!(staged.is_dir());
        assert!(!live.exists());
    }

    #[cfg(unix)]
    #[test]
    fn generic_publish_atomically_publishes_directory_symlink() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("version"), b"linked").unwrap();
        let staged = root.path().join(".skills-hub-link-test");
        let live = root.path().join("linked-skill");
        symlink(source.path(), &staged).unwrap();

        publish_staged_entry_no_replace(root.path(), &staged, &live).unwrap();
        assert!(std::fs::symlink_metadata(&staged).is_err());
        assert!(std::fs::symlink_metadata(&live)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(live.join("version")).unwrap(), b"linked");
    }

    #[cfg(unix)]
    #[test]
    fn generic_publish_preserves_staged_link_and_competing_destination() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let staged = root.path().join(".skills-hub-link-race");
        let live = root.path().join("linked-skill");
        symlink(source.path(), &staged).unwrap();
        std::fs::create_dir(&live).unwrap();
        std::fs::write(live.join("version"), b"competitor").unwrap();

        let err = publish_staged_entry_no_replace(root.path(), &staged, &live).unwrap_err();
        assert!(format!("{err:#}").contains("SKILL_EXISTS"));
        assert!(std::fs::symlink_metadata(&staged)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(live.join("version")).unwrap(), b"competitor");
    }

    #[test]
    fn generic_publish_rejects_ordinary_file() {
        let root = tempfile::tempdir().unwrap();
        let staged = root.path().join(".skills-hub-file-test");
        let live = root.path().join("live-name");
        std::fs::write(&staged, b"not a directory").unwrap();

        let err = publish_staged_entry_no_replace(root.path(), &staged, &live).unwrap_err();
        assert!(format!("{err:#}").contains("directory or directory link"));
        assert_eq!(std::fs::read(&staged).unwrap(), b"not a directory");
        assert!(!live.exists());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_trash_root() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let real_trash = tempfile::tempdir().unwrap();
        let trash_link = root.path().join("trash-link");
        symlink(real_trash.path(), &trash_link).unwrap();
        let skill = root.path().join("keep-skill");
        std::fs::create_dir(&skill).unwrap();

        let err =
            move_checked_to_local_trash_at(root.path(), &skill, &trash_link, false).unwrap_err();
        assert!(format!("{err:#}").contains("Trash root must be a real directory"));
        assert!(skill.is_dir());
    }

    #[test]
    fn failed_trash_after_swap_restores_original() {
        let root = tempfile::tempdir().unwrap();
        let live = root.path().join("managed-skill");
        let staged = root.path().join(".skills-hub-update-test");
        std::fs::create_dir(&live).unwrap();
        std::fs::write(live.join("version"), b"old").unwrap();
        std::fs::create_dir(&staged).unwrap();
        std::fs::write(staged.join("version"), b"new").unwrap();
        let trash_is_file = root.path().join("trash-file");
        std::fs::write(&trash_is_file, b"not a directory").unwrap();

        let err = replace_skill_with_staged_impl(root.path(), &live, &staged, Some(&trash_is_file))
            .unwrap_err();
        assert!(format!("{err:#}").contains("Original Skill restored"));
        assert_eq!(std::fs::read(live.join("version")).unwrap(), b"old");
        assert!(!staged.exists());
        assert!(std::fs::read_dir(root.path())
            .unwrap()
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".skills-hub-failed-update-")));
    }

    #[test]
    fn post_swap_rollback_restores_trashed_old_version() {
        let root = tempfile::tempdir().unwrap();
        let live = root.path().join("managed-skill");
        let staged = root.path().join(".skills-hub-update-test");
        std::fs::create_dir(&live).unwrap();
        std::fs::write(live.join("version"), b"old").unwrap();
        std::fs::create_dir(&staged).unwrap();
        std::fs::write(staged.join("version"), b"new").unwrap();

        let old_in_trash = replace_skill_with_staged(root.path(), &live, &staged).unwrap();
        assert_eq!(std::fs::read(live.join("version")).unwrap(), b"new");
        rollback_replaced_skill(root.path(), &live, &old_in_trash).unwrap();
        assert_eq!(std::fs::read(live.join("version")).unwrap(), b"old");
        assert!(!old_in_trash.test_local_path().exists());
    }
}
