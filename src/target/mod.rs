//! Target 适配器 — 目标平台配置写入
//!
//! 每个目标平台实现 TargetAdapter trait
//! 契约: 写入通过安全写入层（托管块 + 原子写入）
//! 平台差异 = TargetLayout（布局配置），新增平台只需定义 Layout

use crate::converter::mapping::ConversionResult;
use crate::model::SurfaceKind;
use std::path::{Path, PathBuf};

pub mod layout;
pub mod qoder;

pub use layout::{McpFormat, TargetLayout};
pub use qoder::{LayoutTarget, QoderTarget};

/// 目标写入结果
#[derive(Debug, Clone)]
pub struct TargetWriteResult {
    /// 写入的目标路径
    pub path: PathBuf,
    /// 是否新增（首次写入）
    pub created: bool,
}

/// 目标适配器接口
pub trait TargetAdapter {
    /// 目标平台名称（如 "qoder"）
    fn name(&self) -> &'static str;

    /// 计算目标文件路径
    ///
    /// 契约（v0.3 起）: 必须按 `SurfaceKind` 分派，**禁止**从路径字符串反推配置类型。
    /// 历史缺陷: 旧签名只收路径，用 `contains("mcp")` / `contains("memor")` 猜类型，
    /// 导致名字含 "mcp" 的 skill 被写成 MCP 文件、WorkBuddy 的 `MEMORY.md`
    /// 被当成记忆索引 —— 均为静默数据丢失。
    fn target_path(&self, kind: SurfaceKind, source_path: &str) -> PathBuf;

    /// 用户级配置根目录（如 ~/.workbuddy），默认空
    fn user_dir(&self) -> PathBuf {
        PathBuf::new()
    }

    /// MCP 目标格式 —— 决定写入时用哪种无损合并算法（JSON / TOML / cordis YAML 插件表）
    ///
    /// 契约: 由布局显式声明；**禁止**在写入路径上按扩展名或文件名猜测。
    fn mcp_format(&self) -> McpFormat {
        McpFormat::Json
    }

    /// 是否为「手动迁移包」目标（豆包）
    ///
    /// 目标平台无文件级配置契约时，产物写入项目内某目录供**人**复制粘贴，
    /// 因此不做托管块包裹（避免把标记一起粘进客户端 UI）。
    fn is_manual_bundle(&self) -> bool {
        false
    }

    /// 手动迁移包目录（仅 `is_manual_bundle()` 为真时有值）
    fn manual_bundle_dir(&self) -> Option<PathBuf> {
        None
    }

    /// 写入转换结果（走安全写入层）
    fn write(&self, path: &Path, result: &ConversionResult) -> std::io::Result<TargetWriteResult>;
}
