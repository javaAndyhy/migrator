//! migrator — 多目标 Agent 配置迁移引擎
//!
//! 架构（keel 评审后）:
//!   Source Adapter → 转换引擎(映射表驱动) → Target Adapter → 安全写入层
//!   报告 = 转换引擎输出格式（非独立层）
//!
//! 契约（v1 冻结）:
//!   1. 托管块标记格式 v1（见 safety::managed_block）
//!   2. 映射表 schema v1（见 converter::mapping）
//!   3. 原子写入 + 幂等重跑（见 safety::atomic）

pub mod converter;
pub mod model;
pub mod pipeline;
pub mod safety;
pub mod source;
pub mod target;
