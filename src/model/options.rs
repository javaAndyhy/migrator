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
    /// 只处理这些配置面（`--only instructions,mcp`）；None = 全部
    ///
    /// 存在的理由：MCP 面往往牵连客户端重载连接器（真实客户端会弹授权、
    /// 甚至刷新当前会话的连接），而用户常常只想同步规则。没有这个开关时，
    /// 想改一次 instructions 就必须连带写 MCP，只能放弃迁移。
    pub only: Option<Vec<String>>,
}

impl MigrationOptions {
    /// 该配置面是否被 `--only` 选中（None 表示全部选中）
    pub fn wants(&self, kind: &str) -> bool {
        match &self.only {
            None => true,
            Some(list) => list.iter().any(|k| k == kind),
        }
    }
}

impl Default for MigrationOptions {
    fn default() -> Self {
        Self {
            scope: PlanScope::ProjectShared,
            confirm_write: false,
            mapping: MappingTable::new("claude-code", "qoder"),
            only: None,
        }
    }
}
