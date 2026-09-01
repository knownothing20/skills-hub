use std::fs;

use super::git_cmd;
use crate::core::git_fetcher::{clone_or_pull, clone_or_pull_sparse, validate_git_source_url};

fn commit_file(repo: &git2::Repository, path: &str, content: &[u8], msg: &str) -> git2::Oid {
    let workdir = repo.workdir().expect("workdir");
    let file_path = workdir.join(path);
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&file_path, content).unwrap();

    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new(path)).unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();

    let sig = git2::Signature::now("t", "t@example.com").unwrap();
    let parents = match repo.head() {
        Ok(head) => vec![repo.find_commit(head.target().unwrap()).unwrap()],
        Err(_) => vec![],
    };
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, parent_refs.as_slice())
        .unwrap()
}

#[test]
fn clone_then_pull_updates_head() {
    let origin_dir = tempfile::tempdir().unwrap();
    let origin = git2::Repository::init(origin_dir.path()).unwrap();
    let _c1 = commit_file(&origin, "a.txt", b"v1", "c1");
    let c2 = commit_file(&origin, "a.txt", b"v2", "c2");

    let dest_dir = tempfile::tempdir().unwrap();
    let dest = dest_dir.path().join("clone");

    let h1 = clone_or_pull(
        origin_dir.path().to_string_lossy().as_ref(),
        &dest,
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(h1, c2.to_string(), "首次 clone 应指向最新提交");

    let c3 = commit_file(&origin, "b.txt", b"v3", "c3");
    let h2 = clone_or_pull(
        origin_dir.path().to_string_lossy().as_ref(),
        &dest,
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(h2, c3.to_string(), "再次调用应更新到最新提交");
}

#[test]
fn sparse_clone_only_materializes_requested_subpath() {
    let origin_dir = tempfile::tempdir().unwrap();
    let origin = git2::Repository::init(origin_dir.path()).unwrap();
    let _ = commit_file(&origin, "skills/a/SKILL.md", b"---\nname: A\n---\n", "c1");
    let _ = commit_file(&origin, "skills/b/SKILL.md", b"---\nname: B\n---\n", "c2");

    let dest_dir = tempfile::tempdir().unwrap();
    let dest = dest_dir.path().join("clone");

    let head = match clone_or_pull_sparse(
        origin_dir.path().to_string_lossy().as_ref(),
        &dest,
        None,
        "skills/a",
        None,
        None,
    ) {
        Ok(head) => head,
        Err(err) if format!("{:#}", err).contains("system git is required") => return,
        Err(err) => panic!("sparse clone failed: {:#}", err),
    };

    assert!(!head.is_empty());
    assert!(dest.join("skills/a/SKILL.md").exists());
    assert!(
        !dest.join("skills/b/SKILL.md").exists(),
        "未请求的子目录不应被检出到工作区"
    );
}

#[test]
fn git_command_injects_configured_proxy() {
    let cmd = git_cmd(Some("http://127.0.0.1:7890"));
    let args = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    assert!(args.contains(&"http.proxy=http://127.0.0.1:7890".to_string()));
    assert!(args.contains(&"https.proxy=http://127.0.0.1:7890".to_string()));
    assert_eq!(
        cmd.get_envs()
            .find(|(key, _)| key.to_string_lossy() == "https_proxy")
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().to_string()),
        Some("http://127.0.0.1:7890".to_string())
    );
    assert_eq!(
        cmd.get_envs()
            .find(|(key, _)| key.to_string_lossy() == "GIT_CONFIG_GLOBAL")
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().to_string()),
        Some("/dev/null".to_string())
    );
    assert_eq!(
        cmd.get_envs()
            .find(|(key, _)| key.to_string_lossy() == "GIT_ALLOW_PROTOCOL")
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().to_string()),
        Some("https:file".to_string())
    );
}

#[test]
fn rejects_http_git_credentials_query_and_fragment_without_echoing_secrets() {
    for unsafe_url in [
        "https://user:super-secret@example.com/repo.git",
        "https://user@example.com/repo.git",
        "https://@example.com/repo.git",
        "https://example.com/repo.git?token=super-secret",
        "https://example.com/repo.git#super-secret",
    ] {
        let err = validate_git_source_url(unsafe_url).unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("UNSAFE_GIT_URL"));
        assert!(!message.contains("super-secret"));
    }
}

#[test]
fn low_level_clone_rejects_unsafe_http_url_before_creating_destination() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("clone");
    let err = match clone_or_pull(
        "https://user:super-secret@example.com/repo.git",
        &destination,
        None,
        None,
        None,
    ) {
        Ok(_) => panic!("expected unsafe Git URL rejection"),
        Err(err) => err,
    };

    let message = format!("{err:#}");
    assert!(message.contains("UNSAFE_GIT_URL"));
    assert!(!message.contains("super-secret"));
    assert!(!destination.exists());
}

#[test]
fn rejects_unsafe_git_transports_and_option_like_sources() {
    for unsafe_source in [
        "http://example.com/repo.git",
        "ssh://git@example.com/repo.git",
        "git://example.com/repo.git",
        "file:///tmp/repo.git",
        "ext::sh -c touch /tmp/unsafe",
        "git@example.com:owner/repo.git",
        "--upload-pack=/tmp/unsafe",
        "relative/repository",
    ] {
        let err = validate_git_source_url(unsafe_source).unwrap_err();
        assert!(format!("{err:#}").contains("UNSAFE_GIT_URL"));
    }

    validate_git_source_url("https://example.com/owner/repo.git").unwrap();
    validate_git_source_url("/tmp/local-repository").unwrap();
}

#[cfg(unix)]
#[test]
fn clone_or_pull_rejects_symlink_destination_without_touching_target() {
    use std::os::unix::fs::symlink;

    let origin_dir = tempfile::tempdir().unwrap();
    let origin = git2::Repository::init(origin_dir.path()).unwrap();
    commit_file(&origin, "origin.txt", b"origin", "origin");

    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    fs::write(external.path().join("sentinel"), b"unchanged").unwrap();
    let destination = root.path().join("clone");
    symlink(external.path(), &destination).unwrap();

    let err = clone_or_pull(
        origin_dir.path().to_string_lossy().as_ref(),
        &destination,
        None,
        None,
        None,
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("UNSAFE_GIT_CACHE"));
    assert_eq!(
        fs::read(external.path().join("sentinel")).unwrap(),
        b"unchanged"
    );
    assert!(!external.path().join("origin.txt").exists());
}

#[cfg(unix)]
#[test]
fn clone_or_pull_rejects_symlink_git_directory_without_touching_target() {
    use std::os::unix::fs::symlink;

    let origin_dir = tempfile::tempdir().unwrap();
    let origin = git2::Repository::init(origin_dir.path()).unwrap();
    commit_file(&origin, "origin.txt", b"origin", "origin");

    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("clone");
    fs::create_dir(&destination).unwrap();
    let external_git = tempfile::tempdir().unwrap();
    fs::write(external_git.path().join("sentinel"), b"unchanged").unwrap();
    symlink(external_git.path(), destination.join(".git")).unwrap();

    let err = clone_or_pull(
        origin_dir.path().to_string_lossy().as_ref(),
        &destination,
        None,
        None,
        None,
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("UNSAFE_GIT_CACHE"));
    assert_eq!(
        fs::read(external_git.path().join("sentinel")).unwrap(),
        b"unchanged"
    );
}
