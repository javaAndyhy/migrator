//! MigrationOptions — 迁移选项

use crate::converter::mapping::MappingTable;
use crate::model::PlanScope;

/// 迁移选项
#[derive(Debug, Clone)]
pub struct MigrationOptions {
    pub scope: PlanScope,
    /// 是否执行写入（apply 阶段用）
    pub confirm_write: bool,
    /// 映射表（source×target）
    pub mapping: MappingTable,
}

impl Default for MigrationOptions {
    fn default() -> Self {
        Self {
            scope: PlanScope::ProjectShared,
            confirm_write: false,
            mapping: MappingTable::new("claude-code", "qoder"),
        }
    }
}
