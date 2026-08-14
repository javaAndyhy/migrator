use clap::{Parser, Subcommand};
use migrator::converter::mapping::MappingTable;
use migrator::model::{MigrationOptions, PlanScope};
use migrator::pipeline::{dry_run_migration, plan_migration, run_migration, scan_sources, validate_target};
use migrator::source::{ClaudeCodeSource, CodexSource, SourceAdapter};
use migrator::target::{QoderTarget, TargetAdapter};
use std::path::PathBuf;

/// migrator — 多目标 Agent 配置迁移引擎
#[derive(Parser)]
#[command(name = "migrator", version, about)]
struct Cli {
    /// 映射表 JSON 文件路径 (默认内置 claude-to-qoder)
    #[arg(long, global = true)]
    mapping: Option<PathBuf>,
    /// 源平台: claude | codex (默认 claude)
    #[arg(long, global = true, default_value = "claude")]
    source: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 扫描源侧配置面 (只读)
    Scan {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// 生成迁移计划 (只读)
    Plan {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// 干跑迁移 (只读，不写入)
    DryRun {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// 执行迁移 (需 --yes 确认)
    Apply {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// 确认写入 (必须显式提供)
        #[arg(long)]
        yes: bool,
    },
    /// 校验已迁移的目标
    Validate {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// 列出备份批次 (只读)
    Backups {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// 恢复备份 (默认演练；--yes 才执行回滚)
    Restore {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// 指定批次 ID (默认最新)
        #[arg(long)]
        batch: Option<String>,
        /// 确认执行回滚
        #[arg(long)]
        yes: bool,
    },
    /// 清场：删除全部备份 (用户文件不受影响)
    Clean {
        /// 项目目录 (默认当前目录)
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// 确认清场
        #[arg(long)]
        yes: bool,
    },
}

/// 加载映射表：优先外部 JSON 文件（--mapping），否则内置默认
fn load_mapping(mapping_path: Option<&std::path::Path>) -> MappingTable {
    match mapping_path {
        Some(path) => match MappingTable::load_from_json(path) {
            Ok(table) => {
                println!("[映射表] 已加载: {} → {}", table.source, table.target);
                table
            }
            Err(e) => {
                eprintln!("[警告] 映射表加载失败 ({e})，使用内置默认");
                MappingTable::builtin_claude_to_qoder()
            }
        },
        None => MappingTable::builtin_claude_to_qoder(),
    }
}

/// 选择源适配器: claude | codex
fn build_source(source_name: &str, project: &PathBuf) -> Box<dyn SourceAdapter> {
    match source_name {
        "codex" => Box::new(CodexSource::detect()),
        _ => Box::new(ClaudeCodeSource::detect(project)),
    }
}

fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();
    let mapping = load_mapping(cli.mapping.as_deref());
    let source_name = cli.source.clone();

    match cli.command {
        Commands::Scan { project } => {
            let source = build_source(&source_name, &project);
            let surfaces = scan_sources(&*source);
            println!("=== 扫描结果 (source: {}) ===", source.name());
            for s in &surfaces {
                let status = if s.present { "[✓]" } else { "[ ]" };
                println!("{status} {:10} {}", s.kind.as_str(), s.source_path);
            }
        }

        Commands::Plan { project } => {
            let source = build_source(&source_name, &project);
            let surfaces = scan_sources(&*source);
            let options = MigrationOptions {
                scope: PlanScope::ProjectShared,
                confirm_write: false,
                mapping: mapping.clone(),
            };
            let plan = plan_migration(&*source, &surfaces, &options);
            println!("=== 迁移计划 (scope: {}) ===", plan.scope.as_str());
            for a in &plan.actions {
                println!("  {} {} -> {}", a.action, a.source, a.target);
            }
        }

        Commands::DryRun { project } => {
            let source = build_source(&source_name, &project);
            let surfaces = scan_sources(&*source);
            let target = QoderTarget::detect(&project);
            let options = MigrationOptions {
                scope: PlanScope::ProjectShared,
                confirm_write: false,
                mapping: mapping.clone(),
            };
            let reports = dry_run_migration(&*source, &target, &surfaces, &options)?;
            print_reports(&reports, "dry-run (未写入)");
        }

        Commands::Apply { project, yes } => {
            if !yes {
                println!("错误: 必须显式提供 --yes 才执行写入 (契约: 默认只读)");
                std::process::exit(1);
            }
            let source = build_source(&source_name, &project);
            let surfaces = scan_sources(&*source);
            let target = QoderTarget::detect(&project);
            let options = MigrationOptions {
                scope: PlanScope::ProjectShared,
                confirm_write: true,
                mapping: mapping.clone(),
            };
            // 写入前备份已存在的目标文件
            let backup_mgr = migrator::safety::backup::BackupManager::new(&project);
            let target_paths: Vec<std::path::PathBuf> = surfaces
                .iter()
                .filter(|s| s.present)
                .map(|s| target.target_path(&s.source_path))
                .collect();
            match backup_mgr.create_batch(&target_paths) {
                Ok(Some(batch_id)) => println!("[备份] 已创建批次 {batch_id}"),
                Ok(None) => println!("[备份] 无已存在目标文件，跳过备份"),
                Err(e) => eprintln!("[警告] 备份失败: {e}"),
            }
            let reports = run_migration(&*source, &target, &surfaces, &options)?;
            print_reports(&reports, "apply (已写入)");
        }

        Commands::Validate { project } => {
            let source = build_source(&source_name, &project);
            let surfaces = scan_sources(&*source);
            let target = QoderTarget::detect(&project);
            let reports = validate_target(&target, &surfaces);
            print_reports(&reports, "validate");
        }

        Commands::Backups { project } => {
            let backup_mgr = migrator::safety::backup::BackupManager::new(&project);
            match backup_mgr.list_batches() {
                Ok(batches) => {
                    println!("=== 备份批次 ({}) ===", batches.len());
                    for b in &batches {
                        let files = backup_mgr.plan_restore(Some(b)).unwrap_or_default();
                        println!("  {b} ({} 文件)", files.len());
                    }
                    if batches.is_empty() {
                        println!("  (无备份)");
                    }
                }
                Err(e) => eprintln!("错误: {e}"),
            }
        }

        Commands::Restore { project, batch, yes } => {
            let backup_mgr = migrator::safety::backup::BackupManager::new(&project);
            if !yes {
                // 演练
                match backup_mgr.plan_restore(batch.as_deref()) {
                    Ok(plan) => {
                        println!("=== restore 演练 (未写入) ===");
                        for (orig, bak) in &plan {
                            println!("  将恢复: {orig} <- {bak}");
                        }
                        if plan.is_empty() {
                            println!("  (无备份)");
                        }
                        println!("提示: 加 --yes 执行回滚");
                    }
                    Err(e) => eprintln!("错误: {e}"),
                }
            } else {
                match backup_mgr.restore(batch.as_deref()) {
                    Ok(restored) => {
                        println!("=== restore (已回滚) ===");
                        for r in &restored {
                            println!("  已恢复: {r}");
                        }
                    }
                    Err(e) => eprintln!("错误: {e}"),
                }
            }
        }

        Commands::Clean { project, yes } => {
            if !yes {
                println!("错误: 必须显式提供 --yes 才执行清场 (契约: 默认只读)");
                std::process::exit(1);
            }
            let backup_mgr = migrator::safety::backup::BackupManager::new(&project);
            match backup_mgr.clean() {
                Ok(removed) => {
                    println!("=== clean (已清场) ===");
                    println!("  删除 {} 个备份批次，用户文件保留", removed.len());
                }
                Err(e) => eprintln!("错误: {e}"),
            }
        }
    }

    Ok(())
}

fn print_reports(reports: &[migrator::model::ReportRow], title: &str) {
    println!("=== {title} ===");
    if reports.is_empty() {
        println!("  (无)")
    }
    for r in reports {
        println!("  [{:20}] {} — {}", r.status.as_str(), r.item, r.notes);
    }
}
