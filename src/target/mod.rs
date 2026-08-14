//! Target 适配器 — 目标平台配置写入
//!
//! 每个目标平台实现 TargetAdapter trait
//! 契约: 写入通过安全写入层（托管块 + 原子写入）

use crate::converter::mapping::ConversionResult;
use std::path::{Path, PathBuf};

pub mod qoder;

pub use qoder::QoderTarget;

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
    fn target_path(&self, surface_key: &str) -> PathBuf;

    /// 写入转换结果（走安全写入层）
    fn write(&self, path: &Path, result: &ConversionResult) -> std::io::Result<TargetWriteResult>;
}
