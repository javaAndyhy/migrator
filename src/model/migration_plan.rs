//! MigrationPlan — 迁移计划（三级 scope + 动作列表）
//!
//! 来源: Qoder 官方迁移工具的三级分列 + claude-codex-sync 的托管块

use serde::{Deserialize, Serialize};

/// 迁移作用域 — 用户级 / 项目共享级 / 项目本地级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanScope {
    /// 用户级 (~/.claude -> ~/.qoder)
    User,
    /// 项目共享级 (.claude/ -> .qoder/ 项目内)
    ProjectShared,
    /// 项目本地级 (.claude/settings.local.json 等)
    ProjectLocal,
}

impl PlanScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlanScope::User => "user",
            PlanScope::ProjectShared => "project-shared",
            PlanScope::ProjectLocal => "project-local",
        }
    }
}

/// 单个迁移动作
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationAction {
    pub surface: String,
    pub source: String,
    pub target: String,
    /// 动作类型: copy / convert / index / skip
    pub action: String,
    /// 状态: planned
    pub status: String,
    /// 备注
    pub notes: Option<String>,
}

/// 迁移计划 — 按 scope 分组的动作集合
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub scope: PlanScope,
    pub actions: Vec<MigrationAction>,
}

impl MigrationPlan {
    pub fn new(scope: PlanScope) -> Self {
        Self {
            scope,
            actions: Vec::new(),
        }
    }

    pub fn add_action(&mut self, action: MigrationAction) {
        self.actions.push(action);
    }
}
