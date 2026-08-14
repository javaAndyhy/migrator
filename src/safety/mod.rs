//! 安全写入层 — 托管块 v1 / 原子写入 / 备份回退
//!
//! 来源: claude-codex-sync 的 5 个安全设计
//! 契约 v1 (冻结): 托管块标记格式，变更需走 cutover 策略
//! 回退链: apply 前备份 → restore（默认演练）→ clean 清场

pub mod atomic;
pub mod backup;
pub mod managed_block;
