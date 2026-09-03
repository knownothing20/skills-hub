use std::fs;

use crate::core::content_hash::hash_dir;

#[test]
fn hash_changes_with_content_and_ignores_git_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("a.txt"), b"hello").unwrap();
    fs::write(root.join("sub/b.txt"), b"world").unwrap();

    let h1 = hash_dir(root).unwrap();

    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".git/ignored"), b"ignored").unwrap();
    let h2 = hash_dir(root).unwrap();
    assert_eq!(h1, h2, "应忽略 .git 内容");

    fs::write(root.join("a.txt"), b"hello2").unwrap();
    let h3 = hash_dir(root).unwrap();
    assert_ne!(h2, h3);
}

#[test]
fn hash_respects_custom_skillignore() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    fs::create_dir_all(root.join("local")).unwrap();
    fs::write(root.join("SKILL.md"), b"# skill").unwrap();
    fs::write(root.join(".skillignore"), b"local/\n*.key\n").unwrap();
    fs::write(root.join("local/connections.json"), b"key1").unwrap();

    let h1 = hash_dir(root).unwrap();

    // 修改 local/connections.json 或新增 key 文件，哈希应该依然一致（被忽略）
    fs::write(root.join("local/connections.json"), b"key2_different").unwrap();
    fs::write(root.join("secret.key"), b"private_secret").unwrap();
    let h2 = hash_dir(root).unwrap();
    assert_eq!(h1, h2, "应忽略 .skillignore 中指定的 local/ 与 *.key");

    // 修改被追踪的真实代码文件，哈希应该改变
    fs::write(root.join("SKILL.md"), b"# skill updated").unwrap();
    let h3 = hash_dir(root).unwrap();
    assert_ne!(h2, h3, "修改未被忽略的真实代码文件，哈希必须改变");
}
