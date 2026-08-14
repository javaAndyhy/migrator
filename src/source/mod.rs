//! Source 适配器 — 源平台配置读取
//!
//! 每个源平台实现 SourceAdapter trait
//! 契约: 只读，永不修改源文件

use crate::model::ConfigSurface;
use std::path::{Path, PathBuf};

/// 收集 skill 目录下的子目录（references/scripts/assets/agents 等），供迁移时复制
pub(crate) fn collect_skill_subdirs(skill_root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(skill_root) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                dirs.push(p);
            }
        }
    }
    dirs
}

pub mod claude;
pub mod codex;

pub use claude::ClaudeCodeSource;
pub use codex::CodexSource;

/// 源适配器接口
pub trait SourceAdapter {
    /// 源平台名称（如 "claude-code"）
    fn name(&self) -> &'static str;

    /// 扫描源侧存在的配置面（只读）
    fn scan(&self) -> Vec<ConfigSurface>;

    /// 读取指定配置面的原始内容（只读）
    fn read(&self, surface: &ConfigSurface) -> Option<String>;

    /// 返回 skill 目录下的支持子目录（references/scripts/assets 等），供迁移时复制
    ///
    /// source_path 形如 .claude/skills/<name>/SKILL.md 或 ~/.codex/skills/<name>/SKILL.md
    /// 返回空表示无子目录或非 skill
    fn skill_support_dirs(&self, source_path: &str) -> Vec<PathBuf> {
        let _ = source_path;
        Vec::new()
    }
}
