//! 五阶段管线 — scan / plan / dry-run / apply / validate
//!
//! 来源: migrate-to-codex 的命令集 + claude-codex-sync 的只读/显式确认
//! 契约: 默认只读，写入需显式确认

pub mod pipeline;

pub use pipeline::{dry_run_migration, plan_migration, run_migration, scan_sources, validate_target};
