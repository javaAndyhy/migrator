# migrator — 多目标 Agent 配置迁移引擎

> 把 Claude Code / Codex 的配置一键迁移到国产 AI 编程平台（Qoder / Trae / 灵码 / MarsCode...）
> 平台中立 · 安全优先 · 社区可扩展

[![Rust](https://img.shields.io/badge/Rust-1.97+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

---

## 为什么需要它

2026 年，AI 编程工具的"搬家"成为高频场景：Claude Code 的安全争议、阿里禁用 Claude 全系、OpenAI 推出 Codex `/import` 抢人……大量开发者从 Claude Code / Codex 迁移到国产平台。

但现有迁移工具都是**单目标官方迁移**（每个平台自己做搬运工），且普遍缺乏安全机制：

| 工具 | 来源 → 目标 | 安全机制 |
|---|---|---|
| OpenAI Codex `/import` | Claude/Cursor → Codex | ❌ 只进不出，无回退 |
| 阿里 Qoder 迁移 skill | Claude → Qoder | ⚠️ 只复制不删除 |
| **migrator（本项目）** | **Claude / Codex → 任意平台** | ✅ 托管块 + 备份回退 + 原子写入 + 幂等 |

**migrator 的差异化**：平台中立 + 完整安全协议（吸收自社区最佳实践）。

---

## 快速开始

```bash
# 扫描源侧配置（只读）
migrator --source codex scan --project .

# 生成迁移计划（只读）
migrator --source codex plan --project .

# 干跑迁移（只读，不写入）
migrator --source codex dry-run --project .

# 执行迁移（必须显式 --yes，契约: 默认只读）
# 目标平台: qoder | trae | lingma (默认 qoder)
migrator --source codex --target qoder apply --project . --yes
migrator --source claude --target trae apply --project . --yes
migrator --source codex --target lingma apply --project . --yes

# 校验目标
migrator --source codex validate --project .
```

### 目标平台差异

| 平台 | 指令文件 | MCP 配置 | 配置根目录 |
|---|---|---|---|
| Qoder | `AGENTS.md` | `.mcp.json` | `.qoder/` |
| Trae | `AGENTS.md` | `mcp.json` | `.trae/` |
| 通义灵码 | `LINGMA.md` | `mcp-settings.json` | `.lingma/` |

平台间差异集中在"输出布局"，新增平台只需在 `src/target/layout.rs` 定义一个布局 + 写映射表 JSON。

### 安全回退链

```bash
# 列出备份批次
migrator backups --project .

# 恢复（默认演练，--yes 才回滚）
migrator restore --project . --yes

# 清场（删除备份，用户文件保留）
migrator clean --project . --yes
```

---

## 架构

```
Source Adapter → 转换引擎(映射表驱动) → Target Adapter → 安全写入层
      ↓                 ↓                   ↓
   扫描/读取         语义降级            生成/写入
      └──────────→ 安全写入层 ←──────────┘
                (托管块·原子写入·备份回退)
                        ↓
                   报告格式(三态+三级scope)
```

**四层承重管线**（经 keel 架构评审）：
1. **Source Adapter** — 读取源平台配置（只读契约）：`claude` / `codex`
2. **转换引擎** — 映射表驱动，语义降级 → `MANUAL MIGRATION REQUIRED` 块
3. **Target Adapter** — 生成目标平台格式：`qoder`（MCP 格式兼容）
4. **安全写入层** — 托管块 v1、原子写入、备份/restore/clean、幂等

### 五安全契约（v1 冻结）

| 契约 | 实现 |
|---|---|
| 默认只读 | `apply` 必须显式 `--yes` |
| 托管块 | `<!-- BEGIN/END MIGRATOR:<scope> -->`，手写内容保护，标记异常拒绝写入 |
| 原子写入 | `.tmp` + rename，中断不留半写 |
| 备份回退 | 写入前自动备份 → `restore`（演练）→ `clean` |
| 幂等 | 同一迁移重复执行结果一致 |

---

## 支持能力

### 已支持的配置面（ConfigSurface）

| 配置面 | Claude Code 源 | Codex 源 | 转换行为 |
|---|---|---|---|
| instructions | CLAUDE.md | AGENTS.md | → AGENTS.md（托管块包裹） |
| mcp | .mcp.json | config.toml [mcp_servers] | → .mcp.json（TOML→JSON） |
| skills | .claude/skills/ | ~/.codex/skills/ | frontmatter 降级 + 子目录复制 |
| agents | .claude/agents/*.md | ~/.codex/agents/*.toml | YAML/TOML → Qoder 格式 |
| memory | ~/.claude/memory/ | ~/.codex/memories/ | **只读索引**（不复制正文） |

### 语义降级三态报告

```
[Added               ] skills xxx — convert
[Check before using  ] skills yyy — fields downgraded: model, paths
[Not Added           ] hooks     — no target equivalent
```

Partial 转换会在目标文件内嵌 `## MANUAL MIGRATION REQUIRED` 块，指导人工审查。

---

## 社区扩展

**新目标平台 = 写一个 JSON 映射文件**（无需改 Rust 代码）：

```json
// data/mappings/claude-to-qoder.json
{
  "schema_version": 1,
  "source": "claude-code",
  "target": "qoder",
  "entries": [
    { "source": "instructions", "target": "AGENTS.md", "behavior": "convert", "status": "exact" }
  ]
}
```

```bash
migrator --mapping data/mappings/claude-to-qoder.json apply --project . --yes
```

---

## 设计来源（吸收蒸馏）

本项目架构吸收自 2026 年 AI 编程迁移工具的社区最佳实践：

- **migrate-to-codex**（OpenAI 官方）：映射表驱动、三态报告、MANUAL 块
- **claude-codex-sync**（RuntianLee）：托管块、只读记忆索引、备份回退链
- **claude-to-qoder-migration**（阿里官方）：三级 scope、只复制不删除
- **our-codex**（Getty）：frontmatter 剥离、层级排序

并经 **keel**（承重架构评审）修正：报告降级为输出格式、契约机制（托管块 v1 冻结 / 映射表 schema 版本化 / 适配器契约测试）、负路径（原子写入 / 幂等 / 竞态检测）。

---

## 开发

```bash
# 环境注意: WorkBuddy 沙箱下 target 目录一次性，用 cargo.sh 自动分配新目录
./cargo.sh test
./cargo.sh build

# 直接构建（无沙箱环境用默认 target）
cargo build
```

测试：**62 个单元测试**（托管块 / 原子写入 / 备份回退 / 映射表 / 各转换器 / 双源适配器）。

---

## 路线图

- [x] Phase 1: 四层管线骨架 + 托管块 v1 + 原子写入 + 映射表 schema v1
- [x] Phase 2: 语义降级 + MANUAL 块 / 备份回退链 / 映射表 JSON 化 / 记忆只读索引
- [x] 真实环境验证: Codex 源（45 skills + 9 agents + 10 MCP 全链路）
- [ ] Phase 3: Trae / 灵码 / MarsCode 目标适配器
- [ ] 双向迁移（国产 → Claude/Codex）
- [ ] 会话历史迁移

## License

MIT
