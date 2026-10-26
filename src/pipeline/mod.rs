//! 五阶段管线 — scan / plan / dry-run / apply / validate
//!
//! 来源: migrate-to-codex 的命令集 + claude-codex-sync 的只读/显式确认
//! 契约: 默认只读，写入需显式确认

// `pipeline::pipeline` 的重名是有意为之：模块路径与领域名词一致，
// 外部引用读作 `migrator::pipeline::run_migration`，重命名只会让路径更绕。
#[allow(clippy::module_inception)]
pub mod pipeline;

pub use pipeline::{
    dry_run_migration, plan_migration, run_migration, scan_sources, validate_target,
    workbuddy_rules_path,
};
