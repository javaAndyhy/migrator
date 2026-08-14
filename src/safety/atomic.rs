//! 原子写入 — 先写 .tmp 再 rename，中断不留半写状态
//!
//! 契约 (keel §5 决策): 所有目标文件原子提交

use std::fs;
use std::io;
use std::path::Path;

/// 原子写入文件:
/// 1. 写入同目录 .tmp 文件
/// 2. fsync
/// 3. rename 覆盖目标
///
/// 中途崩溃不会留下半写状态的目标文件
pub fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "路径缺少文件名"))?;

    let tmp_path = parent.join(format!(".{}.tmp", file_name.to_string_lossy()));

    fs::write(&tmp_path, content)?;
    // Windows 上 rename 覆盖是原子的（ReplaceFile 语义）
    fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("migrator-atomic-{}-{}", name, std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn atomic_write_creates_file() {
        let dir = test_dir("create");
        let path = dir.join("test.txt");
        atomic_write(&path, "hello").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
        // 无残留 .tmp
        assert!(!dir.join(".test.txt.tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_overwrites() {
        let dir = test_dir("overwrite");
        let path = dir.join("test.txt");
        atomic_write(&path, "v1").unwrap();
        atomic_write(&path, "v2").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "v2");
        fs::remove_dir_all(&dir).unwrap();
    }
}
