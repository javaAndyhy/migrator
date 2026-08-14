//! 备份管理器 — 修改前先备份，支持 restore / clean 回退链
//!
//! 来源: claude-codex-sync 的"每一步都能退回去"设计
//! 契约:
//!   - apply 写入前对已存在的目标文件做备份
//!   - restore 默认演练（不写盘），--yes 才执行回滚
//!   - clean 清场：删除生成物（备份目录），用户手写保留
//!   - 备份目录: <project>/.migrator-backups/<batch-id>/

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 备份目录名 — 契约
pub const BACKUP_DIR_NAME: &str = ".migrator-backups";

/// 单个文件备份记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileBackup {
    /// 原始文件路径（相对项目根）
    pub original: String,
    /// 备份文件路径（相对备份批次目录）
    pub backup: String,
}

/// 备份批次清单
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub batch_id: String,
    pub created_at: String,
    pub files: Vec<FileBackup>,
}

/// 备份管理器
#[derive(Debug, Clone)]
pub struct BackupManager {
    /// 备份根目录（项目根/.migrator-backups）
    root: PathBuf,
}

impl BackupManager {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            root: project_root.into().join(BACKUP_DIR_NAME),
        }
    }

    /// 创建一个备份批次：复制所有已存在的目标文件到新批次目录
    ///
    /// 返回批次 ID（时间戳）。无文件时返回 None（不创建空批次）。
    pub fn create_batch(&self, target_files: &[PathBuf]) -> std::io::Result<Option<String>> {
        // 过滤出已存在的文件
        let existing: Vec<&PathBuf> = target_files.iter().filter(|p| p.exists()).collect();
        if existing.is_empty() {
            return Ok(None);
        }

        let batch_id = Self::timestamp();
        let batch_dir = self.root.join(&batch_id);
        let files_dir = batch_dir.join("files");
        fs::create_dir_all(&files_dir)?;

        let mut manifest = BackupManifest {
            batch_id: batch_id.clone(),
            created_at: batch_id.clone(),
            files: Vec::new(),
        };

        for (i, file) in existing.iter().enumerate() {
            let backup_name = format!("{:03}-{}", i, file.file_name().unwrap().to_string_lossy());
            let backup_rel = format!("files/{backup_name}");
            let backup_path = files_dir.join(&backup_name);
            fs::copy(file, &backup_path)?;
            manifest.files.push(FileBackup {
                original: file.to_string_lossy().to_string(),
                backup: backup_rel,
            });
        }

        // 写 manifest
        let manifest_path = batch_dir.join("manifest.json");
        let json = serde_json::to_string_pretty(&manifest)?;
        fs::write(manifest_path, json)?;

        Ok(Some(batch_id))
    }

    /// 列出所有备份批次（按时间倒序）
    pub fn list_batches(&self) -> std::io::Result<Vec<String>> {
        if !self.root.exists() {
            return Ok(vec![]);
        }
        let mut batches: Vec<String> = fs::read_dir(&self.root)?
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        batches.sort();
        batches.reverse();
        Ok(batches)
    }

    /// 读取批次 manifest
    fn read_manifest(&self, batch_id: &str) -> std::io::Result<BackupManifest> {
        let path = self.root.join(batch_id).join("manifest.json");
        let json = fs::read_to_string(path)?;
        let manifest: BackupManifest = serde_json::from_str(&json)?;
        Ok(manifest)
    }

    /// 演练 restore：打印将恢复的文件映射（不写盘）
    ///
    /// batch_id 为 None 时用最新批次
    pub fn plan_restore(&self, batch_id: Option<&str>) -> std::io::Result<Vec<(String, String)>> {
        let id = self.resolve_batch(batch_id)?;
        let manifest = self.read_manifest(&id)?;
        Ok(manifest
            .files
            .iter()
            .map(|f| (f.original.clone(), f.backup.clone()))
            .collect())
    }

    /// 执行 restore：从备份恢复文件（覆盖目标）
    pub fn restore(&self, batch_id: Option<&str>) -> std::io::Result<Vec<String>> {
        let id = self.resolve_batch(batch_id)?;
        let manifest = self.read_manifest(&id)?;
        let mut restored = Vec::new();
        for file in &manifest.files {
            let backup_path = self.root.join(&id).join(&file.backup);
            let original_path = PathBuf::from(&file.original);
            if let Some(parent) = original_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&backup_path, &original_path)?;
            restored.push(file.original.clone());
        }
        Ok(restored)
    }

    /// 清场：删除全部备份目录（用户文件不受影响）
    pub fn clean(&self) -> std::io::Result<Vec<String>> {
        let batches = self.list_batches()?;
        for batch in &batches {
            let dir = self.root.join(batch);
            fs::remove_dir_all(&dir)?;
        }
        // 若根目录空了则删除
        if self.root.exists() {
            let _ = fs::remove_dir(&self.root);
        }
        Ok(batches)
    }

    /// 解析批次：None → 最新；Some → 指定
    fn resolve_batch(&self, batch_id: Option<&str>) -> std::io::Result<String> {
        match batch_id {
            Some(id) => {
                if !self.root.join(id).is_dir() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("备份批次不存在: {id}"),
                    ));
                }
                Ok(id.to_string())
            }
            None => {
                let batches = self.list_batches()?;
                batches
                    .first()
                    .cloned()
                    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无备份批次"))
            }
        }
    }

    fn timestamp() -> String {
        // 用进程内毫秒时间戳（避免依赖 chrono）
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        format!("{}-{:03}", now.as_secs(), now.subsec_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("migrator-bk-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn create_batch_backs_up_existing_files() {
        let root = test_dir("create");
        // 模拟目标文件
        let target = root.join("AGENTS.md");
        fs::write(&target, "v1").unwrap();
        let other = root.join("NOT_EXISTS.md"); // 不存在，应跳过

        let mgr = BackupManager::new(&root);
        let batch = mgr.create_batch(&[target.clone(), other]).unwrap().unwrap();
        assert!(!batch.is_empty());

        // 备份目录有 manifest + files
        let batch_dir = root.join(BACKUP_DIR_NAME).join(&batch);
        assert!(batch_dir.join("manifest.json").exists());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn no_existing_files_returns_none() {
        let root = test_dir("empty");
        let mgr = BackupManager::new(&root);
        let batch = mgr
            .create_batch(&[root.join("missing.md")])
            .unwrap();
        assert!(batch.is_none());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn restore_recovers_original_content() {
        let root = test_dir("restore");
        let target = root.join("AGENTS.md");
        fs::write(&target, "original").unwrap();

        let mgr = BackupManager::new(&root);
        let batch = mgr.create_batch(&[target.clone()]).unwrap().unwrap();

        // 修改目标文件（模拟 apply 覆盖）
        fs::write(&target, "modified").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "modified");

        // restore 恢复
        let restored = mgr.restore(Some(&batch)).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn restore_latest_batch_when_none() {
        let root = test_dir("latest");
        let target = root.join("AGENTS.md");
        fs::write(&target, "v1").unwrap();
        let mgr = BackupManager::new(&root);
        mgr.create_batch(&[target.clone()]).unwrap();

        fs::write(&target, "v2").unwrap();
        let restored = mgr.restore(None).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(fs::read_to_string(&target).unwrap(), "v1");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn plan_restore_does_not_write() {
        let root = test_dir("plan");
        let target = root.join("AGENTS.md");
        fs::write(&target, "v1").unwrap();
        let mgr = BackupManager::new(&root);
        mgr.create_batch(&[target.clone()]).unwrap();

        fs::write(&target, "v2").unwrap();
        let plan = mgr.plan_restore(None).unwrap();
        assert_eq!(plan.len(), 1);
        // 演练不写盘
        assert_eq!(fs::read_to_string(&target).unwrap(), "v2");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn clean_removes_all_batches() {
        let root = test_dir("clean");
        let target = root.join("AGENTS.md");
        fs::write(&target, "v1").unwrap();
        let mgr = BackupManager::new(&root);
        mgr.create_batch(&[target.clone()]).unwrap();

        let removed = mgr.clean().unwrap();
        assert_eq!(removed.len(), 1);
        assert!(!root.join(BACKUP_DIR_NAME).exists());
        // 用户文件保留
        assert!(target.exists());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn list_batches_sorted_newest_first() {
        let root = test_dir("list");
        let target = root.join("AGENTS.md");
        fs::write(&target, "v1").unwrap();
        let mgr = BackupManager::new(&root);

        // 手动创建两个批次（时间戳不同）
        let b1 = mgr.create_batch(&[target.clone()]).unwrap().unwrap();
        fs::write(&target, "v2").unwrap();
        let b2 = mgr.create_batch(&[target.clone()]).unwrap().unwrap();

        let batches = mgr.list_batches().unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0], b2); // 最新在前
        assert_eq!(batches[1], b1);

        fs::remove_dir_all(&root).unwrap();
    }
}
