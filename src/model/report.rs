//! ReportRow — 迁移报告（三态 + 三级 scope）
//!
//! 来源: migrate-to-codex 的 Added / Check before using / Not Added 三态

use serde::{Deserialize, Serialize};

/// 报告状态 — 三态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReportStatus {
    /// 已添加，无需特殊审查
    Added,
    /// 已添加但语义有变化，需人工检查
    CheckBeforeUsing,
    /// 检测到源配置但未生成目标配置
    NotAdded,
}

impl ReportStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReportStatus::Added => "Added",
            ReportStatus::CheckBeforeUsing => "Check before using",
            ReportStatus::NotAdded => "Not Added",
        }
    }
}

/// 报告行
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportRow {
    /// scope 标签（用户级/项目级/本地级）
    pub scope: String,
    pub status: ReportStatus,
    /// 条目类型 + 名称（如 "Skill release-notes"）
    pub item: String,
    pub notes: String,
}

impl ReportRow {
    pub fn new(scope: impl Into<String>, status: ReportStatus, item: impl Into<String>, notes: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            status,
            item: item.into(),
            notes: notes.into(),
        }
    }
}
