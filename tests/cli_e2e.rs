//! 端到端集成测试：直接运行编译出的真实 `migrator` 二进制，验证
//! 「真实配置 → 完整链路 → 目标平台落地可用配置」的行为。
//!
//! 设计要点（针对本项目的两大雷区）：
//! 1. **绝不触碰用户真实配置**：二进制通过 `env("HOME")` / `env("USERPROFILE")`
//!    被指向一个临时假 HOME；Claude 源侧的 `detect` 只读这两个变量，因此二进制
//!    物理上无法触及真实 `~/.codex` / `~/.claude`。每个测试首尾还会对真实
//!    `~/.codex/config.toml` 做一次字节级快照比对，作为安全护栏。
//! 2. **避开沙箱 os error 5**：既有模块单测用 `std::env::temp_dir()`（系统临时
//!    目录，沙箱写/删受限）导致 3 个测试失败。集成测试改用 cargo 为 `tests/`
//!    提供的 `CARGO_TARGET_TMPDIR`（位于 target 目录内，可写）。
//!
//! 覆盖的关键行为：
//! - scan 枚举 7 类配置面并标记存在/缺失；
//! - 在「无 data/mappings/ 目录」的 CWD 下仍能通过编译期内嵌表加载映射（#12 分发修复）；
//! - apply 前备份既有目标文件、写入后可通过 restore 回滚（fail-closed 安全网）；
//! - `--mapping` 指向损坏文件时必须 fail-fast（#14）；
//! - 未知 `--source` 必须显式报错（#10）。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 集成测试专属可写根目录。优先取 cargo 为集成测试注入的 `CARGO_TARGET_TMPDIR`
/// （位于 target 内、可写）；若未注入（如经 cargo.sh 包装，target 由
/// `MIGRATOR_TARGET_DIR` 指定），则退回该变量（同样是沙箱内可写的 `/tmp/...`）；
/// 最后退回系统临时目录。这样在「cargo.sh 沙箱」与「普通 cargo test（CI）」两种
/// 环境下都能拿到可写目录，避开既有单测 `std::env::temp_dir()` 的 os error 5 雷区。
fn tmp_base() -> PathBuf {
    if let Ok(d) = std::env::var("CARGO_TARGET_TMPDIR") {
        return PathBuf::from(d);
    }
    if let Ok(d) = std::env::var("MIGRATOR_TARGET_DIR") {
        return PathBuf::from(d);
    }
    std::env::temp_dir()
}

/// 对真实 ~/.codex/config.toml 做字节级快照（只读，若存在）
fn real_codex_snapshot() -> Option<Vec<u8>> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let p = PathBuf::from(home).join(".codex").join("config.toml");
    fs::read(&p).ok()
}

/// 安全护栏：真实 ~/.codex/config.toml 在测试前后必须字节不变
fn assert_codex_untouched(prev: Option<Vec<u8>>) {
    let after = real_codex_snapshot();
    match (prev, after) {
        (Some(a), Some(b)) => assert_eq!(
            a, b,
            "安全护栏失败：真实 ~/.codex/config.toml 在测试过程中被改动！"
        ),
        (None, None) => {}
        _ => panic!("安全护栏失败：真实 ~/.codex/config.toml 的存在性在测试前后发生变化"),
    }
}

/// 运行真实二进制；强制把 HOME/USERPROFILE 指向假目录，绝不外泄到真实用户配置。
/// 同时把 CWD 设到 project —— 这样 `data/mappings/`（仓库根相对路径）不会被命中，
/// 真实模拟「发布后单二进制在自己的项目目录运行」的场景（#12 分发修复的验收点）。
fn run(home: &Path, project: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_migrator"))
        .args(args)
        .arg("--project")
        .arg(project)
        .current_dir(project)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("MIGRATOR_TARGET_DIR")
        .output()
        .expect("无法启动 migrator 二进制（确认 `cargo test --all-targets` 已构建 bin）");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    (out.status.success(), format!("{stdout}{stderr}"))
}

/// 新建一个唯一的「假 HOME + 假项目」环境，并填入一套完整的 Claude 源配置
fn scaffold(kind: &str) -> (PathBuf, PathBuf) {
    let base = tmp_base().join(format!("migrator-e2e-{kind}-{}", std::process::id()));
    let home = base.join("home");
    let project = base.join("project");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&project).unwrap();

    // 用户级 Claude 配置（HOME 下）
    let claude_home = home.join(".claude");
    fs::create_dir_all(&claude_home).unwrap();
    fs::write(claude_home.join("settings.json"), r#"{"mcpServers":{}}"#).unwrap();
    fs::create_dir_all(claude_home.join("memory")).unwrap();
    fs::write(claude_home.join("memory").join("note.md"), "# memory note").unwrap();

    // 项目级 Claude 配置（project/.claude 下）
    let claude_proj = project.join(".claude");
    fs::create_dir_all(&claude_proj).unwrap();
    fs::write(claude_proj.join("CLAUDE.md"), "# Claude project rules\n").unwrap();
    fs::write(claude_proj.join(".mcp.json"), r#"{"mcpServers":{}}"#).unwrap();
    fs::create_dir_all(claude_proj.join("skills").join("my-skill")).unwrap();
    fs::write(
        claude_proj.join("skills").join("my-skill").join("SKILL.md"),
        "---\nname: my-skill\n---\nDo stuff.\n",
    )
    .unwrap();
    fs::create_dir_all(claude_proj.join("agents")).unwrap();
    fs::write(
        claude_proj.join("agents").join("release-lead.md"),
        "# release lead\n",
    )
    .unwrap();
    fs::create_dir_all(claude_proj.join("hooks")).unwrap();
    fs::write(
        claude_proj.join("hooks").join("hooks.json"),
        r#"{"hooks":[]}"#,
    )
    .unwrap();
    fs::create_dir_all(claude_proj.join("commands")).unwrap();
    fs::write(
        claude_proj.join("commands").join("review.md"),
        "---\ndescription: review\n---\nReview code.\n",
    )
    .unwrap();

    (home, project)
}

#[test]
fn e2e_scan_enumerates_all_seven_surfaces_and_uses_embedded_mapping() {
    let prev = real_codex_snapshot();
    let (home, project) = scaffold("scan");

    // 关键：project 目录里没有 data/mappings/，模拟「发布后单二进制」场景。
    // 若 #12 分发修复正确，二进制应通过编译期内嵌表加载映射。
    let (ok, out) = run(
        &home,
        &project,
        &["--source", "claude", "--target", "qoder", "scan"],
    );
    assert!(ok, "scan 应成功；输出:\n{out}");
    assert!(
        out.contains("内嵌"),
        "在缺少 data/mappings/ 的场景下应回退到内嵌映射表；输出:\n{out}"
    );

    // 7 类配置面都被枚举
    for kind in [
        "instructions",
        "mcp",
        "skills",
        "agents",
        "memory",
        "hooks",
        "commands",
    ] {
        assert!(
            out.contains(kind),
            "scan 输出缺少配置面标记 `{kind}`；输出:\n{out}"
        );
    }
    // 全部存在（我们刚 scaffold 了完整 Claude 配置）
    let present = out.matches("[✓]").count();
    assert!(
        present >= 7,
        "应至少标记 7 个已存在的配置面，实际 {present}；输出:\n{out}"
    );

    assert_codex_untouched(prev);
}

#[test]
fn e2e_apply_backs_up_preexisting_then_restore_rolls_back() {
    let prev = real_codex_snapshot();
    let (home, project) = scaffold("apply");

    // 预置「既有目标文件」，使 apply 必须备份后再覆盖（验证 fail-closed 安全网）
    fs::write(project.join("AGENTS.md"), "PRE-EXISTING-AGENTS").unwrap();
    fs::create_dir_all(project.join(".qoder")).unwrap();
    fs::write(project.join(".qoder").join(".mcp.json"), "{}").unwrap();

    // 1) apply --yes 尝试：无论沙箱是否能 fsync，都必须「先备份、再写入」
    let (ok, out) = run(
        &home,
        &project,
        &["--source", "claude", "--target", "qoder", "apply", "--yes"],
    );
    // 沙箱对临时/构建目录的 fsync 会返回 os error 5（与既有 atomic_write_* 单测同源，
    // 属环境限制而非逻辑错误）。此时 atomic_write 失败、apply 中止 —— 但备份必须已经
    // 先于写入完成（fail-closed 顺序）。真实/CI 环境下 apply 会成功并完成写入。
    assert!(
        out.contains("已创建批次"),
        "apply 必须先创建备份批次（fail-closed）；输出:\n{out}"
    );
    if ok {
        // 完整路径（非沙箱 / CI）：验证写入确实发生
        let agents = project.join("AGENTS.md");
        assert!(agents.exists(), "apply 后 AGENTS.md 应已生成");
        let agents_content = fs::read_to_string(&agents).unwrap();
        assert_ne!(
            agents_content, "PRE-EXISTING-AGENTS",
            "apply 后应覆盖既有 AGENTS.md，但实际未被改写；输出:\n{out}"
        );
        assert!(
            project.join(".qoder").join(".mcp.json").exists(),
            "apply 后 .qoder/.mcp.json 应已生成"
        );
    } else {
        // 沙箱路径：失败应源于「写入」阶段（已先完成备份），而非更早的逻辑错误
        assert!(
            out.contains("写入目标失败") || out.contains("os error 5"),
            "沙箱下 apply 应失败于写入阶段（fsync 受限），而非其他逻辑错误；输出:\n{out}"
        );
    }

    // 2) backups：应列出刚刚创建的批次
    let (ok, out) = run(
        &home,
        &project,
        &["--source", "claude", "--target", "qoder", "backups"],
    );
    assert!(ok, "backups 应成功；输出:\n{out}");
    assert!(
        !out.contains("(无备份)"),
        "apply 已备份，backups 不应显示无备份；输出:\n{out}"
    );

    // 3) restore --yes：无论 apply 是否真写入，都必须能把 AGENTS.md / .mcp.json
    //    回滚到备份时的 PRE-EXISTING 内容（这是安全网的核心价值）
    let (ok, out) = run(
        &home,
        &project,
        &[
            "--source", "claude", "--target", "qoder", "restore", "--yes",
        ],
    );
    assert!(ok, "restore 应成功；输出:\n{out}");
    assert!(
        out.contains("已恢复") || out.contains("已回滚"),
        "restore 应报告已恢复文件；输出:\n{out}"
    );
    let restored = fs::read_to_string(project.join("AGENTS.md")).unwrap();
    assert_eq!(
        restored, "PRE-EXISTING-AGENTS",
        "restore 必须将 AGENTS.md 回滚到备份内容"
    );
    let restored_mcp = fs::read_to_string(project.join(".qoder").join(".mcp.json")).unwrap();
    assert_eq!(
        restored_mcp, "{}",
        "restore 必须将 .mcp.json 回滚到备份内容"
    );

    assert_codex_untouched(prev);
}

#[test]
fn e2e_bad_mapping_fails_fast() {
    let prev = real_codex_snapshot();
    let (home, project) = scaffold("badmap");

    // 写入一个损坏的映射表，观察 --mapping 是否 fail-fast（#14）
    let bad = project.join("broken-mapping.json");
    fs::write(&bad, "{ this is not valid json ").unwrap();

    let (ok, out) = run(
        &home,
        &project,
        &[
            "--mapping",
            bad.to_str().unwrap(),
            "--source",
            "claude",
            "--target",
            "qoder",
            "scan",
        ],
    );
    assert!(
        !ok,
        "损坏的 --mapping 必须使命令失败（fail-fast），而非静默降级；输出:\n{out}"
    );
    assert!(
        out.contains("映射表") || out.contains("加载失败") || out.contains("中止"),
        "--mapping 失败时错误信息应说明映射表加载失败；输出:\n{out}"
    );

    assert_codex_untouched(prev);
}

#[test]
fn e2e_unknown_source_errors_explicitly() {
    let prev = real_codex_snapshot();
    let (home, project) = scaffold("unknownsrc");

    let (ok, out) = run(
        &home,
        &project,
        &[
            "--source",
            "definitely-not-a-platform",
            "--target",
            "qoder",
            "scan",
        ],
    );
    assert!(!ok, "未知 --source 必须失败；输出:\n{out}");
    // 注意：main 先跑 load_mapping，未知 source 会先被 builtin() 的「不支持的迁移组合」
    // 拦截（fail-fast），而非 build_source 的「未知 --source」。二者都是显式报错，
    // 此处两种消息都接受，重点是「绝不静默兜底」。
    assert!(
        out.contains("不支持的迁移组合") || out.contains("未知 --source"),
        "未知 source 应给出明确报错（fail-fast）；输出:\n{out}"
    );

    assert_codex_untouched(prev);
}
