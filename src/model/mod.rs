//! Canonical 数据模型 — 所有适配器/引擎共享的中间表示
//!
//! 设计来源: 吸收蒸馏自 migrate-to-codex / claude-codex-sync / Qoder 迁移工具
//! 关键抽象: ConfigSurface / MigrationPlan / ReportRow

pub mod config_surface;
pub mod migration_plan;
pub mod options;
pub mod report;

pub use config_surface::{ConfigSurface, SurfaceKind};
pub use migration_plan::{MigrationAction, MigrationPlan, PlanScope};
pub use options::MigrationOptions;
pub use report::{ReportRow, ReportStatus};
